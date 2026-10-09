//! Component preparation in process (RoboCAD's `component_jobs.py` and
//! `component_service.py`): what the Components dock calls, with the names
//! its client had. A start runs the operation (`sim_cad::component_edit`)
//! on a staged edit of the shown snapshot on one `Pool::Dedicated` job;
//! when it is ready, [`commit`] publishes it as one undo step through
//! `local::start` (refused by name when the document moved meanwhile:
//! "The document changed during preparation…"). One component job runs at
//! a time; cancelling stops it before it publishes.
use crate::cad::document::{CadDocument, EditDone};
use crate::cad::sync::LocalSnapshot;
use crate::jobs::{Job, Pool};
use serde_json::Value;
use crate::cad::types::{CadError, ComponentCatalogue, ComponentJobState, ComponentJobStatus, ComponentLibrary, ComponentOperation, ComponentRecipes, ComponentStamp, ComponentStarted};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// A staged result: the edit to publish and the operation's answer.
struct Prepared {
    edit: sim_cad::Edit,
    result: Value,
}

struct Entry {
    status: ComponentJobStatus,
    job: Option<Job<Prepared>>,
    prepared: Option<Prepared>,
    /// The archive identity of the snapshot it was prepared on.
    identity: String,
    label: String,
}

/// Every component job of this process, by id.
static JOBS: Mutex<BTreeMap<String, Entry>> = Mutex::new(BTreeMap::new());

fn err(e: impl Into<String>) -> CadError {
    CadError::local(e)
}

fn typed<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, CadError> {
    serde_json::from_value(v).map_err(|e| err(e.to_string()))
}

/// What a dock job uses: the shown snapshot, its revision and document.
#[derive(Clone)]
pub(crate) struct Service {
    snapshot: Arc<LocalSnapshot>,
    revision: u64,
    document_id: String,
}

/// The service for the shown document, or why there is none.
pub(crate) fn service(doc: &CadDocument) -> Result<Service, String> {
    let snapshot = doc.local.clone().ok_or("Open a CAD document first")?;
    let document_id = snapshot.archive.manifest["document_id"].as_str().unwrap_or_default().to_string();
    Ok(Service { snapshot, revision: doc.shown_revision(), document_id })
}

/// Refresh one entry's status from its job (progress, readiness, failure).
fn refresh(entry: &mut Entry) {
    let Some(job) = &entry.job else { return };
    let p = job.progress();
    if !p.message.is_empty() {
        entry.status.stage = p.message.clone();
    }
    if let Some((done, total)) = p.steps {
        entry.status.done = done;
        entry.status.total = total;
    }
    match job.poll() {
        None => {}
        Some(Ok(prepared)) => {
            entry.status.stage = "Ready to apply".into();
            entry.status.state = ComponentJobState::Ready;
            entry.prepared = Some(prepared);
            entry.job = None;
        }
        Some(Err(e)) => {
            entry.status.state = if e.contains("cancelled") { ComponentJobState::Cancelled } else { ComponentJobState::Failed };
            entry.status.error = Some(e);
            entry.job = None;
        }
    }
}

impl Service {
    pub fn components(&self) -> Result<ComponentCatalogue, CadError> {
        typed(sim_cad::component_edit::catalogue(&self.snapshot.archive.manifest))
    }
    pub fn component_recipes(&self) -> Result<ComponentRecipes, CadError> {
        typed(sim_cad::component_edit::recipes())
    }
    pub fn component_library(&self, path: Option<&str>) -> Result<ComponentLibrary, CadError> {
        typed(sim_cad::component_edit::library(path).map_err(err)?)
    }
    /// Start `operation` on the shown snapshot (refused when the stamp's
    /// document or revision is not the snapshot's, or another job runs).
    pub fn start_component(&self, operation: &ComponentOperation, stamp: &ComponentStamp) -> Result<ComponentStarted, CadError> {
        if stamp.document_id != self.document_id {
            return Err(err("Component start document changed; your draft is preserved."));
        }
        if stamp.expected_revision != self.revision {
            return Err(err("Component start revision changed; your draft is preserved. Refresh and retry."));
        }
        let mut jobs = JOBS.lock().map_err(|_| err("component jobs poisoned"))?;
        for e in jobs.values_mut() {
            refresh(e);
        }
        if jobs.values().any(|e| matches!(e.status.state, ComponentJobState::Pending | ComponentJobState::Running | ComponentJobState::Ready)) {
            return Err(err("A component rebuild is already in progress"));
        }
        let id = format!("{}{}", sim_cad::edit::new_id(), sim_cad::edit::new_id());
        let name = operation.op_name();
        let kwargs = operation.kwargs();
        let snapshot = self.snapshot.clone();
        let job = Job::spawn(Pool::Dedicated, 0, format!("component {name}"), move |ctx| {
            ctx.message("Preparing the component change");
            let cancelled = || ctx.cancelled();
            let mut edit = sim_cad::Edit::of(&snapshot.archive);
            let result = {
                let geometry = &snapshot.geometry;
                let centroid = |nid: &str| geometry.iter().find(|b| b.node_id == nid).map(|b| b.properties.centroid_mm);
                let mut cx = sim_cad::ops::Ctx { doc: &snapshot.archive, stamps: &snapshot.pin_stamps, edit: &mut edit, centroid: &centroid, cancelled: &cancelled };
                sim_cad::component_edit::run(&mut cx, name, &kwargs)?
            };
            if ctx.cancelled() {
                return Err("Component preparation cancelled".into());
            }
            ctx.message("Ready to apply");
            Ok(Prepared { edit, result })
        });
        let status = ComponentJobStatus {
            id: id.clone(),
            operation: name.to_string(),
            state: ComponentJobState::Running,
            stage: "Preparing".into(),
            done: 0,
            total: 0,
            error: None,
            log_path: None,
            result: Value::Null,
            document_id: self.document_id.clone(),
            revision: self.revision,
        };
        let label = name.replace('_', " ");
        let mut chars = label.chars();
        let label = chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect());
        jobs.insert(id, Entry { status: status.clone(), job: Some(job), prepared: None, identity: self.snapshot.archive.identity().to_string(), label });
        Ok(ComponentStarted { job: status })
    }
    pub fn component_job(&self, id: &str) -> Result<ComponentJobStatus, CadError> {
        let mut jobs = JOBS.lock().map_err(|_| err("component jobs poisoned"))?;
        let e = jobs.get_mut(id).ok_or_else(|| CadError { method: "local", route: String::new(), status: Some(404), message: format!("no component job {id}") })?;
        refresh(e);
        Ok(e.status.clone())
    }
    pub fn component_jobs(&self) -> Result<Vec<ComponentJobStatus>, CadError> {
        let mut jobs = JOBS.lock().map_err(|_| err("component jobs poisoned"))?;
        Ok(jobs.values_mut().map(|e| {
            refresh(e);
            e.status.clone()
        }).collect())
    }
    /// Cancel: a preparing job stops; a ready one is never published.
    pub fn cancel_component_job(&self, id: &str) -> Result<ComponentJobStatus, CadError> {
        let mut jobs = JOBS.lock().map_err(|_| err("component jobs poisoned"))?;
        let e = jobs.get_mut(id).ok_or_else(|| CadError { method: "local", route: String::new(), status: Some(404), message: format!("no component job {id}") })?;
        if !e.status.state.terminal() {
            if let Some(job) = e.job.take() {
                job.cancel();
            }
            e.prepared = None;
            e.status.state = ComponentJobState::Cancelled;
        }
        Ok(e.status.clone())
    }
}

/// The system composition reads (RoboCAD's `/system`, `/experiments/catalogue`,
/// `/component-recipes` and a check's imported components).
impl Service {
    pub fn composition(&self) -> Result<crate::cad::types::composition::GraphSnapshot, CadError> {
        let graph = self.snapshot.archive.manifest.get("component_graph").filter(|g| g.is_object()).cloned().unwrap_or_else(sim_cad::component_graph::empty);
        typed(serde_json::json!({"revision": self.revision, "document_id": self.document_id, "graph": graph}))
    }
    /// The registered component types (the experiment runner's catalogue).
    pub fn system_types(&self) -> Result<Vec<crate::cad::types::composition::SystemType>, CadError> {
        use sim_cad::experiments::Simulator;
        let catalogue = crate::cad::lab::Simulate.catalogue();
        if let Some(e) = catalogue.get("metadata_error") {
            return Err(err(format!("the component catalogue could not be read: {e}")));
        }
        typed(catalogue)
    }
    pub fn geometry_recipes(&self) -> Result<BTreeMap<String, crate::cad::types::composition::Recipe>, CadError> {
        typed(sim_cad::component_edit::recipes()["recipes"].clone())
    }
    /// A completed check run's imported components, with whether its
    /// results and its structural metadata still describe this document.
    pub fn system_imports(&self, check: &str) -> Result<crate::cad::types::composition::ImportedSnapshot, CadError> {
        let lab = crate::cad::lab::lab();
        let record = lab.get(check).map_err(err)?;
        let folder = lab.root.join(check);
        let imported = sim_cad::experiments::read_json(&folder.join("imported_components.json")).map_err(|_| err("This check has not produced an imported component list; inspect its diagnostics"))?;
        let resolved = sim_cad::experiments::read_json(&folder.join("resolved_components.json")).ok();
        let current = sim_cad::experiments::capture::capture(&self.snapshot.archive, self.revision).map_err(err)?;
        let p = &record["provenance"];
        let (recorded, now) = match p["cad_derivation_hash"].as_str() {
            Some(h) => (Some(h), current.cad_derivation_hash.as_str()),
            None => (p["physical_hash"].as_str(), current.physical_hash.as_str()),
        };
        let identity = record["document_id"].as_str().or(p["document_id"].as_str());
        let fresh = record["state"] == "completed" && identity == Some(current.document_id.as_str()) && recorded == Some(now);
        typed(serde_json::json!({
            "run_id": check, "revision": record["revision"], "state": record["state"], "error": record["error"],
            "stale": p["physical_hash"].as_str() != Some(current.physical_hash.as_str()),
            "metadata_stale": !fresh, "imported": imported, "resolved": resolved,
            "guard_document_id": current.document_id, "guard_revision": current.revision,
        }))
    }
}

/// Publish a ready job as one undo step (on the UI thread). Refused by name
/// when the document moved since it was prepared; kept ready while another
/// edit is in flight (tried again next frame).
pub(crate) fn commit(doc: &mut CadDocument, id: &str) -> Result<(), String> {
    let mut jobs = JOBS.lock().map_err(|_| "component jobs poisoned")?;
    let Some(e) = jobs.get_mut(id) else { return Ok(()) };
    refresh(e);
    if e.status.state != ComponentJobState::Ready || doc.edit.is_some() {
        return Ok(());
    }
    let current = doc.local.as_ref().map(|l| l.archive.identity().to_string());
    if current.as_deref() != Some(e.identity.as_str()) {
        e.status.state = ComponentJobState::Failed;
        e.status.error = Some("The document changed during preparation. Your edits are preserved; retry the component operation.".into());
        e.prepared = None;
        return Ok(());
    }
    let Some(Prepared { edit, result }) = e.prepared.take() else { return Ok(()) };
    let unchanged = edit.entries.is_empty() && edit.manifest == doc.local.as_ref().map_or(Value::Null, |l| l.archive.manifest.clone());
    if !unchanged {
        let label = e.label.clone();
        let answer = result.clone();
        if let Err(why) = crate::cad::local::start(doc, label.clone(), false, false, move |ws| {
            ws.edit = edit;
            Ok(EditDone { message: format!("{label}: component updated"), result: answer })
        }) {
            e.status.state = ComponentJobState::Failed;
            e.status.error = Some(why);
            return Ok(());
        }
    }
    e.status.state = ComponentJobState::Applied;
    e.status.result = result;
    e.status.stage = "Applied".into();
    Ok(())
}

/// A service over an empty document at revision 7 (tests).
#[cfg(test)]
pub(crate) fn test_service() -> Service {
    let empty = sim_cad::ArchiveDocument::from_bytes(std::path::Path::new("/tmp/components-test.rcad"), sim_cad::edit::empty_archive(None).unwrap(), &|| false, &|_| {}).unwrap();
    let snapshot = crate::cad::sync::snapshot_of(Arc::new(empty), &|| false, &|_| {}).unwrap();
    let document_id = snapshot.archive.manifest["document_id"].as_str().unwrap_or_default().to_string();
    Service { snapshot: Arc::new(snapshot), revision: 7, document_id }
}

//! The experiment service in process (`sim_cad::experiments`): what the
//! Experiments dock and captured review call, with the names RoboCAD's
//! client had. A [`Service`] is taken from the document on the UI thread
//! (the shown snapshot and its revision) and used on jobs.
//!
//! - Runs execute on one `Pool::Dedicated` job each ([`RUNS`] keeps the
//!   handles; cancelling the run cancels its job, which stops between
//!   samples). The simulator is `sim_script` resolution and
//!   `sim_phenomena::experiment::run_cancellable` ([`Simulate`]).
//! - Records live in `runs/experiments` (`sim_cad::experiments::service::default_root`),
//!   candidates in its `candidates/` folder.
//! - Source changes (accepting a candidate, restoring captured inputs) are
//!   CAD edits through `local::start` (`local_client`), never here.
use crate::cad::document::CadDocument;
use crate::cad::sync::LocalSnapshot;
use crate::jobs::{Job, Pool};
use serde_json::{Value, json};
use sim_cad::experiments::Simulator;
use sim_cad::experiments::candidates::Candidates;
use sim_cad::experiments::service::Lab;
use crate::cad::types::candidates::{CandidateRecord, CandidateRequest};
use crate::cad::types::experiments::{CapturedGeometry, ExperimentRecord, ExperimentRequest};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

/// `sim-experiment` in this process.
pub(crate) struct Simulate;
impl Simulator for Simulate {
    fn resolve(&self, composition: &Value) -> Result<Value, String> {
        let spec: sim_phenomena::experiment::Specification = serde_json::from_value(composition.clone()).map_err(|e| e.to_string())?;
        let plan = sim_script::evaluate_seeded(&spec.system, &sim_phenomena::world::registry(), sim_script::parameter_map(&spec.parameters).map_err(|e| e.to_string())?, spec.seed).map_err(|e| e.to_string())?;
        serde_json::to_value(plan).map_err(|e| e.to_string())
    }
    fn run(&self, specification: &Value, output: &Path, progress: &dyn Fn(&Value), cancelled: &dyn Fn() -> bool) -> Result<Value, String> {
        let spec: sim_phenomena::experiment::Specification = serde_json::from_value(specification.clone()).map_err(|e| e.to_string())?;
        sim_phenomena::experiment::run_cancellable(spec, output, |event| progress(&event), cancelled)
    }
    fn catalogue(&self) -> Value {
        sim_script::catalogue(&sim_phenomena::world::registry())
    }
    fn identity(&self) -> Value {
        json!({"runner": "sim-spatial (in process)", "runner_version": env!("CARGO_PKG_VERSION"), "source_identity": sim_cad::production_source_identity()})
    }
}

/// The one experiment service of this process.
pub(crate) fn lab() -> &'static Arc<Lab> {
    static LAB: OnceLock<Arc<Lab>> = OnceLock::new();
    LAB.get_or_init(|| Arc::new(Lab::new(sim_cad::experiments::service::default_root())))
}

/// The jobs executing runs, by run id (a finished one is dropped at the next start).
static RUNS: Mutex<Option<HashMap<String, Job<Value>>>> = Mutex::new(None);

fn execute(id: String) {
    let job = Job::spawn(Pool::Dedicated, 0, format!("experiment {}", &id[..8.min(id.len())]), {
        let id = id.clone();
        move |ctx| lab().execute(&id, &Simulate, &|| ctx.cancelled())
    });
    if let Ok(mut runs) = RUNS.lock() {
        let runs = runs.get_or_insert_with(HashMap::new);
        runs.retain(|_, j| j.poll().is_none());
        runs.insert(id, job);
    }
}

fn candidates() -> Candidates {
    Candidates::new(lab().root.join("candidates"))
}

/// What a dock or review job uses: the shown snapshot, its revision and document.
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

fn typed<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| e.to_string())
}

impl Service {
    fn guard(&self, document_id: &str) -> Result<(), String> {
        if !document_id.is_empty() && document_id != self.document_id {
            return Err("Document replaced; the request belongs to another document".into());
        }
        Ok(())
    }
    pub fn experiments(&self) -> Result<Vec<ExperimentRecord>, String> {
        lab().list(Some(&self.document_id))?.into_iter().map(typed).collect()
    }
    pub fn experiment_catalogue(&self) -> Result<Value, String> {
        Ok(lab().catalogue(&Simulate))
    }
    /// Capture a run of the shown document and start it on its job.
    pub fn start_experiment(&self, r: &ExperimentRequest) -> Result<ExperimentRecord, String> {
        self.guard(&r.document_id)?;
        let (snapshot, graph) = sim_cad::experiments::service::snapshot_of(&self.snapshot.archive, self.revision)?;
        let record = lab().create(&request_json(r)?, Some(&snapshot), &graph, &Simulate)?;
        execute(record["id"].as_str().unwrap_or_default().to_string());
        typed(record)
    }
    /// Capture a run of a candidate's staged archive.
    pub fn candidate_experiment(&self, id: &str, r: &ExperimentRequest) -> Result<ExperimentRecord, String> {
        self.guard(&r.document_id)?;
        let c = candidates();
        let snapshot = c.snapshot(id, &self.document_id)?;
        let staged = c.document(id, &self.document_id)?;
        let graph = staged.manifest.get("component_graph").filter(|g| !g.is_null()).cloned().unwrap_or_else(sim_cad::experiments::service::empty_graph);
        let mut request = request_json(r)?;
        request["candidate_id"] = json!(id);
        let record = lab().create(&request, Some(&snapshot), &graph, &Simulate)?;
        execute(record["id"].as_str().unwrap_or_default().to_string());
        typed(record)
    }
    pub fn experiment(&self, id: &str) -> Result<ExperimentRecord, String> {
        typed(lab().get(id)?)
    }
    /// Cancel a run: its job stops between samples.
    pub fn cancel_experiment(&self, id: &str) -> Result<ExperimentRecord, String> {
        let record = lab().cancel(id)?;
        if let Ok(runs) = RUNS.lock()
            && let Some(job) = runs.as_ref().and_then(|r| r.get(id))
        {
            job.cancel();
        }
        typed(record)
    }
    pub fn experiment_result(&self, id: &str) -> Result<Value, String> {
        let (snapshot, graph) = sim_cad::experiments::service::snapshot_of(&self.snapshot.archive, self.revision)?;
        lab().result(id, Some((&snapshot, &graph)))
    }
    pub fn experiment_inputs(&self, id: &str) -> Result<Value, String> {
        lab().inputs(id)
    }
    pub fn experiment_diagnostics(&self, id: &str) -> Result<Value, String> {
        lab().diagnostics(id)
    }
    pub fn experiment_sources(&self, id: &str) -> Result<Value, String> {
        lab().source_bundles(id)
    }
    pub fn experiment_partial(&self, id: &str) -> Result<Value, String> {
        lab().partial(id)
    }
    pub fn experiment_compare(&self, baseline: &str, id: &str) -> Result<Value, String> {
        lab().compare(baseline, id)
    }
    pub fn experiment_sample(&self, id: &str, time: f64, flex_scale: f64) -> Result<Value, String> {
        let record = lab().get(id)?;
        let result = lab().result(id, None).or_else(|_| lab().partial(id))?;
        sim_cad::experiments::review::sample(&result, &record, time, flex_scale)
    }
    pub fn experiment_geometry(&self, id: &str) -> Result<CapturedGeometry, String> {
        let record = lab().get(id)?;
        let doc = lab().captured_document(id)?;
        typed(sim_cad::experiments::review::geometry(doc.as_ref(), &record, "experiment", &|| false)?)
    }
    /// Read-only capture of a linked `.rhai` entry and its sibling modules.
    pub fn experiment_linked_sources(&self, path: &str) -> Result<Value, String> {
        let p = std::fs::canonicalize(shellexpand(path)).map_err(|e| format!("{path}: {e}"))?;
        if p.extension().and_then(|e| e.to_str()) != Some("rhai") || !p.is_file() {
            return Err("Link an existing .rhai entry".into());
        }
        let dir = p.parent().ok_or("the entry has no folder")?;
        let mut files = serde_json::Map::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))?.flatten() {
                let path = e.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().and_then(|e| e.to_str()) == Some("rhai") {
                    let rel = path.strip_prefix(dir).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/");
                    files.insert(rel, json!(std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?));
                }
            }
        }
        let entry = p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        sim_cad::experiments::service::sources(Some(&json!({"entry": entry, "files": files})), &entry, "")
    }
    pub fn candidates(&self) -> Result<Vec<CandidateRecord>, String> {
        candidates().list(&self.document_id).into_iter().map(typed).collect()
    }
    pub fn candidate(&self, id: &str) -> Result<CandidateRecord, String> {
        typed(candidates().get(id, &self.document_id)?)
    }
    /// Stage a batch on the shown snapshot and keep it as a draft candidate.
    pub fn create_candidate(&self, r: &CandidateRequest) -> Result<CandidateRecord, String> {
        let request = serde_json::to_value(r).map_err(|e| e.to_string())?;
        typed(candidates().create(&self.snapshot.archive, self.revision, &request, &|| false)?)
    }
    pub fn discard_candidate(&self, id: &str) -> Result<CandidateRecord, String> {
        typed(candidates().discard(id, &self.document_id)?)
    }
    pub fn candidate_geometry(&self, id: &str) -> Result<CapturedGeometry, String> {
        let c = candidates();
        let record = c.get(id, &self.document_id)?;
        let doc = c.document(id, &self.document_id)?;
        typed(sim_cad::experiments::review::geometry(Some(&doc), &record, "candidate", &|| false)?)
    }
}

/// `~` at the start of a path is the home folder.
fn shellexpand(path: &str) -> std::path::PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => std::path::PathBuf::from(home).join(rest),
        _ => std::path::PathBuf::from(path),
    }
}

/// The request as the service validates it (document_id dropped; empty fields left out).
fn request_json(r: &ExperimentRequest) -> Result<Value, String> {
    let mut v = serde_json::to_value(r).map_err(|e| e.to_string())?;
    let m = v.as_object_mut().ok_or("the request is not an object")?;
    m.remove("document_id");
    for key in ["system", "controller", "parent_run"] {
        if m.get(key).is_some_and(Value::is_null) {
            m.remove(key);
        }
    }
    if m.get("profile").is_some_and(|p| p == "") {
        m.remove("profile");
    }
    if m.get("label").is_some_and(|p| p == "") {
        m.remove("label");
    }
    Ok(v)
}

/// A candidate accepted onto `doc`'s current archive: its record and the
/// edit that publishes it (one undo step through `local::start`).
pub(crate) fn accept(current: &sim_cad::ArchiveDocument, revision: u64, id: &str, expected_revision: u64) -> Result<(Value, sim_cad::Edit), String> {
    candidates().accept(id, current, revision, expected_revision)
}

/// The receipt of an accepted candidate (written once its edit started).
pub(crate) fn mark_accepted(id: &str, document_id: &str, revision: u64) -> Result<Value, String> {
    candidates().mark_accepted(id, document_id, revision)
}

/// A service over an empty document at revision 7 named "doc" (tests).
#[cfg(test)]
pub(crate) fn test_service() -> Service {
    let empty = sim_cad::ArchiveDocument::from_bytes(Path::new("/tmp/lab-test.rcad"), sim_cad::edit::empty_archive(None).unwrap(), &|| false, &|_| {}).unwrap();
    let snapshot = crate::cad::sync::snapshot_of(Arc::new(empty), &|| false, &|_| {}).unwrap();
    Service { snapshot: Arc::new(snapshot), revision: 7, document_id: "doc".into() }
}

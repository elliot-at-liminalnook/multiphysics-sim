//! Local CAD loading. Native handles live entirely inside the jobs-owned
//! call; only archive bytes and numeric snapshots reach Bevy. No service.
use super::document::{CadDocument, Connection, EditDone};
use super::selection::{Shared, View};
use super::CadTarget;
use crate::document::DocumentRegistry;
use crate::jobs::{Job, Pool};
use crate::selection::Selection;
use bevy::prelude::*;
use serde_json::{Value, json};
use sim_runtime::cad_client::{CadClient, DocState, MeshData, NodeSummary};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

pub(crate) struct LocalSnapshot {
    pub archive: Arc<sim_cad::ArchiveDocument>,
    pub tree: DocState,
    pub geometry: Vec<sim_cad::geometry::BodyGeometry>,
    pub meshes: HashMap<String, Arc<MeshData>>,
    pub masses: sim_cad::mass::MassResults,
    /// Every geometry node's fingerprint (`sim_cad::geometry::fingerprint`):
    /// comment pins' stamps, and what an edit's rebuild reuses bodies by.
    pub fingerprints: HashMap<String, String>,
    /// The current stamps of the nodes that carry comment pins (RoboCAD's
    /// exact stamp, `sim_cad::stamp`): what pins are checked against.
    pub pin_stamps: sim_cad::annotations::Stamps,
}
pub(crate) struct LocalLoad {
    pub sequence: u64,
    pub source_generation: u64,
    pub source_revision: u64,
    pub target: PathBuf,
    pub job: Job<LocalSnapshot>,
}

/// Archive summaries are display projections, never the source owner.
pub(crate) fn tree(archive: &sim_cad::ArchiveDocument) -> Result<DocState, String> {
    let manifest = &archive.manifest;
    let mut nodes = Vec::new();
    for raw in manifest["nodes"].as_array().ok_or_else(|| format!("{}: manifest nodes must be an array", archive.path.display()))? {
        let id = raw["id"].as_str().unwrap_or("<missing id>");
        let node: NodeSummary = serde_json::from_value(archive.node(id).cloned().unwrap_or_else(|| raw.clone())).map_err(|e| format!("{}: node {id}: display metadata: {e}", archive.path.display()))?;
        nodes.push(node);
    }
    let flags: HashMap<String, (Option<String>, bool)> = nodes.iter().map(|n| (n.id.clone(), (n.parent.clone(), n.visible && !n.disabled))).collect();
    for node in &mut nodes {
        let mut at = Some(node.id.clone());
        let mut seen = HashSet::new();
        let mut visible = true;
        while let Some(id) = at {
            if !seen.insert(id.clone()) { return Err(format!("{}: node {}: cyclic parent {id}", archive.path.display(), node.id)); }
            let Some((parent, own)) = flags.get(&id) else { return Err(format!("{}: node {}: missing parent {id}", archive.path.display(), node.id)); };
            visible &= own;
            at = parent.clone();
        }
        node.effective_visible = visible;
    }
    // Document.walk order follows roots/children, not manifest insertion order.
    let by_id: HashMap<String, NodeSummary> = nodes.into_iter().map(|n| (n.id.clone(), n)).collect();
    let mut ordered = Vec::new();
    let mut visited = HashSet::new();
    let mut pending: Vec<String> = manifest["roots"].as_array().into_iter().flatten().filter_map(|n| n.as_str().map(str::to_owned)).collect();
    pending.reverse();
    while let Some(id) = pending.pop() {
        if !visited.insert(id.clone()) { return Err(format!("{}: manifest tree repeats node {id}", archive.path.display())); }
        let n = by_id.get(&id).ok_or_else(|| format!("{}: manifest tree names missing node {id}", archive.path.display()))?;
        for child in n.children.iter().rev() { pending.push(child.clone()); }
        ordered.push(n.clone());
    }
    // Orphan archive metadata remains owned; the reference walk does not display it.
    let nodes = ordered;
    let mut value = manifest.clone();
    value["nodes"] = serde_json::to_value(nodes).map_err(|e| e.to_string())?;
    value["path"] = json!(archive.path);
    value["dirty"] = json!(false);
    serde_json::from_value(value).map_err(|e| format!("{}: manifest display projection: {e}", archive.path.display()))
}

pub(crate) fn request_load(doc: &mut CadDocument, path: PathBuf) -> u64 {
    if let Some(old) = doc.local_load.take() {
        doc.load_outcomes.insert(old.sequence, Err("Local CAD open superseded; current document preserved".into()));
    }
    doc.load_sequence += 1;
    let sequence = doc.load_sequence;
    let source_generation = doc.generation;
    let source_revision = doc.shown_revision();
    let input = path.clone();
    let job = Job::spawn(Pool::Dedicated, source_generation, "local-cad-open", move |ctx| {
        ctx.message("Reading compatible archive");
        let archive = Arc::new(sim_cad::ArchiveDocument::open_with(&input, &|| ctx.cancelled(), &|stage| ctx.message(stage))?);
        if ctx.cancelled() { return Err("Local open cancelled after archive read".into()); }
        let tree = tree(&archive)?;
        let geometry = sim_cad::geometry::load_geometry(&archive, &|| ctx.cancelled(), &|stage| ctx.message(stage))?;
        if ctx.cancelled() { return Err("Local open cancelled after geometry".into()); }
        ctx.message("Deriving exact body and assembly mass properties");
        let masses = sim_cad::mass::derive_document_with(&archive, &geometry, &|| ctx.cancelled(), &|stage| ctx.message(stage))?;
        if ctx.cancelled() { return Err("Local open cancelled after mass derivation".into()); }
        ctx.message("Preparing local display snapshot");
        let meshes = geometry.iter().map(|body| (body.node_id.clone(), super::local::mesh(body))).collect();
        let fingerprints = sim_cad::geometry::fingerprints(&archive);
        ctx.message("Checking comment pins");
        let pin_stamps = sim_cad::annotations::pinned_stamps(&archive);
        Ok(LocalSnapshot { archive, tree, geometry, meshes, masses, fingerprints, pin_stamps })
    });
    doc.local_load = Some(LocalLoad { sequence, source_generation, source_revision, target: path, job });
    doc.show(Ok("Opening locally; current document retained until success (Escape cancels)".into()));
    sequence
}

pub(crate) fn cancel_load(doc: &mut CadDocument, sequence: u64) {
    if doc.local_load.as_ref().is_some_and(|load| load.sequence == sequence) {
        doc.local_load = None;
        let message = "Local CAD open cancelled; current document preserved".to_string();
        doc.load_outcomes.insert(sequence, Err(message.clone()));
        doc.show(Err(message));
    }
}
pub(crate) fn start(doc: &mut CadDocument) {
    match &doc.target {
        CadTarget::File(path) => { request_load(doc, path.clone()); }
        CadTarget::Service(_) => { doc.connection = Connection::Lost { error: "CAD service attachment awaiting Rust migration; open a local .rcad archive".into(), since: Instant::now() }; doc.touch(); }
    }
}

/// Startup and mode entry submit the same typed open that picker and REST use.
pub(crate) fn enter(mut commands: Commands, doc: Option<Res<CadDocument>>, registry: Option<Res<DocumentRegistry>>, mut actions: MessageWriter<crate::app::actions::Act<super::CadAction>>) {
    let target = doc.as_deref().map(|d| d.target.clone()).or_else(|| registry.as_deref().and_then(|r| r.source(crate::app::ViewerMode::Cad)).and_then(crate::app::switch::sources::cad_target));
    let target = target.unwrap_or_else(|| CadTarget::Service(String::new()));
    if doc.is_none() { commands.insert_resource(CadDocument::new(target.clone())); }
    let (path, url) = match target { CadTarget::File(path) => (Some(path), None), CadTarget::Service(url) => (None, Some(url)) };
    actions.write(crate::app::actions::Act::quiet(super::CadAction::CadOpen { path, url }));
}

/// Sole snapshot writer in CadSet::Results. Pending work is accepted only
/// against its captured source lifetime; failure never swaps the document.
pub(crate) fn receive(doc: Option<ResMut<CadDocument>>, mut selection: ResMut<Selection>, mut registry: ResMut<DocumentRegistry>, redraw: Option<MessageWriter<bevy::window::RequestRedraw>>) {
    let Some(mut doc) = doc else { return };
    super::selection::ensure_registered(&mut registry, &mut selection, &doc.target);
    super::selection::publish_changes(&mut doc, View { selection: &selection, registry: &registry });
    if doc.edit.is_some() {
        finish_edit(&mut doc, &mut Shared { selection: &mut selection, registry: &mut registry });
    }
    if let Some(load) = &doc.local_load {
        if let Some(result) = load.job.poll() {
            let load = doc.local_load.take().expect("load still owned in this system");
            if load.source_generation != doc.generation || load.source_revision != doc.shown_revision() {
                let error = "Local open result stale; source lifetime changed; current document preserved".to_string();
                doc.load_outcomes.insert(load.sequence, Err(error.clone()));
                crate::jobs::drop_off_thread(result, "stale local CAD replacement");
                doc.show(Err(error));
                return;
            }
            let blockers = doc.switch_blockers();
            if !blockers.is_empty() {
                let error = format!("Local open result refused: {}; current document preserved", blockers.join("; "));
                doc.load_outcomes.insert(load.sequence, Err(error.clone()));
                crate::jobs::drop_off_thread(result, "refused local CAD replacement");
                doc.show(Err(error));
                return;
            }
            match result {
                Ok(snapshot) => {
                    let target = CadTarget::File(load.target);
                    doc.generation = super::document::next_generation();
                    doc.target = target.clone();
                    doc.doc_key = Some((snapshot.tree.document_id.clone(), snapshot.tree.revision));
                    doc.doc = Some(snapshot.tree.clone());
                    doc.physical = Some((snapshot.tree.revision, Ok(super::local::physical_json(&snapshot))));
                    doc.history = super::local::History::opened(snapshot.archive.identity());
                    let old = doc.local.replace(Arc::new(snapshot));
                    crate::jobs::drop_off_thread(old, "the previous local CAD snapshot");
                    doc.connection = Connection::Connected;
                    doc.stale = None;
                    doc.detail = None;
                    super::selection::reopen(&mut registry, &mut selection, &target);
                    let tree = doc.doc.as_ref().expect("accepted tree");
                    super::selection::follow_tree(&mut Shared { selection: &mut selection, registry: &mut registry }, tree.revision, tree, true);
                    let answer = json!({"opened": target.json(), "generation": doc.generation, "message": "Local archive ready; exact OCCT mass properties"});
                    doc.load_outcomes.insert(load.sequence, Ok(answer));
                    doc.show(Ok("Local archive ready; exact OCCT mass properties".into()));
                }
                Err(error) => {
                    doc.load_outcomes.insert(load.sequence, Err(error.clone()));
                    if doc.local.is_none() { doc.connection = Connection::Lost { error: error.clone(), since: Instant::now() }; }
                    doc.show(Err(format!("Local open failed: {error}; current document preserved")));
                }
            }
        } else {
            let message = load.job.progress().message;
            if doc.status.as_ref().and_then(|s| s.as_ref().ok()) != Some(&message) { doc.show(Ok(message)); }
            if let Some(mut redraw) = redraw { redraw.write(bevy::window::RequestRedraw); }
        }
    }
}

pub(crate) fn refresh(doc: &mut CadDocument, _after_edit: bool) { start(doc); }
pub(crate) fn fetch_physical(doc: &mut CadDocument) -> Result<(), String> {
    let local = doc.local.as_ref().ok_or("Local CAD archive is not loaded")?;
    doc.physical = Some((doc.shown_revision(), Ok(super::local::physical_json(local))));
    doc.touch(); Ok(())
}
/// A finished edit lands (ported from the service-era `finish_edit`): its
/// next snapshot is shown (the previous one goes on the undo stack), the
/// parts that tracked it hear its answer, and a REST caller gets it.
fn finish_edit(doc: &mut CadDocument, shared: &mut Shared) {
    let Some(edit) = &doc.edit else { return };
    let Some(result) = edit.job.poll() else { return };
    let generation = edit.job.generation();
    let Some(edit) = doc.edit.take() else { return };
    let seq = doc.edit_seq;
    let next = super::local::take_pending(doc);
    if generation != doc.generation {
        crate::jobs::drop_off_thread(next, "a replaced document's edit");
        return;
    }
    let answer = result.map(|EditDone { message, result }| (message, result));
    if let (Ok(_), Some(next)) = (&answer, next) {
        super::local::landed(doc, edit.label.clone(), next);
        let tree = doc.doc.clone().expect("installed tree");
        super::selection::follow_tree(shared, tree.revision, &tree, true);
    }
    // cad-print / cad-organize: the parts that track an edit hear its answer.
    crate::cad::print::edit_answered(doc, seq, answer.as_ref().ok().map(|(_, r)| r));
    crate::cad::threads::edit_answered(doc, seq, answer.as_ref().map(|(_, r)| r).map_err(Clone::clone));
    crate::cad::references::edit_answered(doc, seq, answer.as_ref().map(|(_, r)| r));
    doc.status = Some(answer.as_ref().map(|(m, _)| m.clone()).map_err(Clone::clone));
    if answer.is_ok()
        && let Some(path) = edit.retarget
    {
        doc.target = CadTarget::File(path);
    }
    if std::mem::take(&mut doc.edit_waited) {
        doc.edit_results.retain(|s, _| *s + 8 > seq);
        doc.edit_results.insert(seq, answer.map(|(message, result)| json!({"message": message, "result": result, "revision": doc.shown_revision()})));
    }
    doc.touch();
}
pub(crate) fn value<T: serde::Serialize>(answer: &T) -> Value { serde_json::to_value(answer).unwrap_or_else(|e| json!({"serialization_error": e.to_string()})) }
pub(crate) fn on_exit(mut exits: MessageReader<AppExit>, doc: Option<ResMut<CadDocument>>) {
    if exits.read().next().is_some() { if let Some(mut doc) = doc { doc.local_load = None; } }
}

//! `cad_state` and the REST snapshot (Present): the document as this
//! window shows it, with cad-views-export's display, saved views and file
//! jobs (split from `actions` to keep it under the size cap).
use super::document::{CadDocument, CadTarget, Connection};
use super::mesh::CadMeshes;
use super::selection::CadSelection;
use super::sketch::CadActivePlane;
use super::sync::value;
use crate::app::ViewerMode;
use bevy::prelude::*;
use serde_json::{Value, json};
use sim_runtime::cad_client::SelectionItem;

/// cad-views-export's parts of `cad_state` (None without CAD mode's window).
#[derive(Clone, Copy, Default)]
pub(in crate::cad) struct Parts<'a> {
    defaults: Option<&'a crate::app::settings::CadDefaults>,
    display: Option<&'a super::display::CadDisplay>,
    views: Option<&'a super::views::CadViews>,
    files: Option<&'a super::files::CadFiles>,
    components: Option<&'a super::components::ComponentsState>,
    composition: Option<&'a super::composition::CadCompositionState>,
    experiments: Option<&'a super::experiments::ExperimentsState>,
    review: Option<&'a super::experiment_review::ReviewState>,
    motion: Option<&'a super::motion::MotionState>,
}
impl<'a> Parts<'a> {
    pub(in crate::cad) fn defaults(mut self, defaults: &'a crate::app::settings::CadDefaults) -> Self {
        self.defaults = Some(defaults); self
    }
    pub(in crate::cad) fn of(display: Option<&'a super::display::CadDisplay>, views: Option<&'a super::views::CadViews>, files: Option<&'a super::files::CadFiles>) -> Self {
        Self { defaults: None, display, views, files, components: None, composition: None, experiments: None, review: None, motion: None }
    }
    pub(in crate::cad) fn experiments(mut self, experiments: &'a super::experiments::ExperimentsState, review: &'a super::experiment_review::ReviewState, motion: &'a super::motion::MotionState) -> Self {
        self.experiments = Some(experiments);
        self.review = Some(review);
        self.motion = Some(motion);
        self
    }
    pub(in crate::cad) fn authoring(mut self, components: &'a super::components::ComponentsState, composition: &'a super::composition::CadCompositionState) -> Self {
        self.components = Some(components);
        self.composition = Some(composition);
        self
    }
}

/// `cad_state`: the document as this window shows it. Nothing is invented:
/// absent values are null.
/// `selection`: the shared selection's CAD items (`cad_state.selection`,
/// a list of `[node, kind, index]`).
pub(in crate::cad) fn state_json(doc: &CadDocument, selection: &[SelectionItem], meshes: Option<&CadMeshes>, plane: Option<&CadActivePlane>, parts: Parts) -> Value {
    let connection = match &doc.connection {
        Connection::Connecting { what, since } => json!({"state": "connecting", "what": what, "seconds": since.elapsed().as_secs()}),
        Connection::Connected => json!({"state": "connected"}),
        Connection::Lost { error, since } => json!({"state": "lost", "error": error, "seconds": since.elapsed().as_secs()}),
    };
    let nodes: Vec<Value> = match &doc.doc {
        Some(state) => state
            .nodes
            .iter()
            .zip(doc.rows(selection))
            .map(|(n, row)| json!({"id": n.id, "kind": n.kind, "name": n.name, "parent": n.parent, "depth": row.depth, "visible": n.visible, "effective_visible": n.effective_visible, "locked": n.locked, "disabled": n.disabled}))
            .collect(),
        None => Vec::new(),
    };
    let result = |r: &Result<Value, String>| match r {
        Ok(v) => json!({"ok": true, "value": v}),
        Err(e) => json!({"ok": false, "error": e}),
    };
    let meshes = meshes.map(|m| {
        json!({"shown": m.counts.shown, "pending": m.counts.pending, "no_mesh": m.counts.no_mesh,
            "failed": m.counts.failed.iter().map(|(id, e)| json!({"id": id, "error": e})).collect::<Vec<_>>()})
    });
    let mut state = json!({
        "target": doc.target.json(),
        "document": doc.document_name(),
        "service": { "kind": "in-process", "line": doc.service_line() },
        "connection": connection,
        "health": doc.health.as_ref().map(value),
        "unsaved": doc.unsaved(),
        "document_key": doc.doc_key.as_ref().map(|(id, revision)| json!({"document_id": id, "revision": revision})),
        "stale": doc.stale,
        "nodes": nodes,
        "selection": selection,
        "select_mode": doc.select_mode,
        "hover": doc.hover,
        "candidates": doc.candidates.as_ref().map(|c| json!({"items": c.items, "extend": c.extend, "toggle": c.toggle, "revision": c.revision})),
        "selection_error": doc.selection_error,
        "tool": doc.tool,
        "tool_state": super::transform::state_json(doc),
        "inspected": doc.detail.as_ref().map(|(id, revision, r)| json!({"id": id, "revision": revision, "detail": result(&r.as_ref().map(value).map_err(Clone::clone))})),
        "physical": doc.physical.as_ref().map(|(revision, r)| json!({"revision": revision, "result": result(r)})).unwrap_or(Value::Null),
        "physical_pending": doc.physical_job.is_some(),
        "history": doc.doc.as_ref().map(|d| value(&d.history)),
        "commands": doc.commands.as_ref().map(|c| result(&c.as_ref().map(value).map_err(Clone::clone))),
        "autosave": doc.autosave.as_ref().map(|a| result(&a.as_ref().map(value).map_err(Clone::clone))),
        "edit": doc.edit.as_ref().map(|e| json!({"label": e.label, "seconds": e.started.elapsed().as_secs()})),
        "meshes": meshes,
        "status": doc.status.as_ref().map(|s| match s { Ok(t) => json!({"ok": true, "text": t}), Err(e) => json!({"ok": false, "text": e}) }),
        "revision": doc.revision,
        "generation": doc.generation,
    });
    // Outside the macro: one more key there would pass json!'s recursion limit.
    state["local_open"] = doc.local_load.as_ref().map(|load| json!({"request": load.sequence, "path": load.target, "source_generation": load.source_generation, "source_revision": load.source_revision, "stage": load.job.progress().message, "cancel": "cad_file op cancel or Escape"})).unwrap_or(Value::Null);
    state["local_mass"] = doc.local.as_ref().map(|local| value(&local.masses)).unwrap_or(Value::Null);
    state["inspected"] = selection.first().and_then(|item| doc.local.as_ref().and_then(|local| local.masses.bodies.get(&item.0).map(|mass| json!({"id": item.0, "mass": value(mass)})))).unwrap_or(state["inspected"].clone());
    state["uncertain_edit"] = json!(doc.uncertain_edit);
    state["uncertain_edit_history"] = json!(doc.uncertain_history);
    state["preview_read_only"] = json!(doc.preview_read_only);
    state["ops"] = super::ops::state_json(doc);
    state["plane"] = plane.map_or(Value::Null, |p| super::sketch::plane::state_json(doc, p));
    // cad-views-export: display state and section, saved views, file jobs.
    state["display"] = parts.display.map_or(Value::Null, super::display::state_json);
    state["views"] = super::views::state_json(parts.views, doc);
    state["files"] = parts.files.map_or(Value::Null, super::files::state_json);
    // cad-physical-inspect: the robot description and panel, materials, the inspector's physical rows, results.
    state["robot"] = super::robot::state_json(doc);
    state["materials"] = super::materials::state_json(doc, selection);
    state["inspector_physical"] = super::inspector::physical_state_json(doc);
    state["results"] = super::results::state_json(doc);
    state["print"] = super::print::state_json(doc, parts.defaults);
    // cad-organize.
    state["tree"] = super::tree::state_json(doc);
    state["threads"] = super::threads::state_json(doc);
    state["references"] = super::references::state_json(doc);
    state["components"] = parts.components.map_or(Value::Null, |s| super::components::state_json(doc, s));
    state["composition"] = parts.composition.map_or(Value::Null, |s| super::composition::state_json(doc, s));
    state["experiments"] = parts.experiments.map_or(Value::Null, |s| super::experiments::state_json(doc, s));
    state["experiment_review"] = parts.review.map_or(Value::Null, |s| super::experiment_review::state_json(doc, s));
    state["motion"] = parts.motion.map_or(Value::Null, |s| super::motion::state_json(doc, s));
    state
}

/// Present: `/v1/state` (with `viewer_mode`) and `/v1/cad_state`, at most every 100 ms.
#[allow(clippy::too_many_arguments)]
pub(in crate::cad) fn publish(
    settings: Res<crate::app::settings::SettingsOwner>,
    rest: Option<ResMut<crate::rest::Rest>>,
    doc: Option<Res<CadDocument>>,
    meshes: Option<Res<CadMeshes>>,
    plane: Option<Res<CadActivePlane>>,
    display: Option<Res<super::display::CadDisplay>>,
    views: Option<Res<super::views::CadViews>>,
    files: Option<Res<super::files::CadFiles>>,
    selection: CadSelection,
    (components, composition): (Res<super::components::ComponentsState>, Res<super::composition::CadCompositionState>),
    (experiments, review, motion): (Res<super::experiments::ExperimentsState>, Res<super::experiment_review::ReviewState>, Res<super::motion::MotionState>),
) {
    let (Some(mut rest), Some(doc)) = (rest, doc) else { return };
    if rest.0.snapshot_due() {
        let state = state_json(&doc, &selection.items(), meshes.as_deref(), plane.as_deref(), Parts::of(display.as_deref(), views.as_deref(), files.as_deref()).authoring(&components, &composition).experiments(&experiments, &review, &motion).defaults(&settings.cad));
        let mut shown = state.clone();
        shown["viewer_mode"] = json!(ViewerMode::Cad.name());
        rest.0.publish("cad_state", state);
        rest.0.publish("state", shown);
    }
}

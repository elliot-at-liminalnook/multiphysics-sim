//! Show on model, Fit in view, the linked parts and their temporary
//! isolation (ui/comments.py:370-486), display only.
//!
//! - **Show on model** ([`show`], `focus_thread`): any isolation ends, the
//!   pin's part is selected (through the shared selection, as `cad_select`
//!   does) and the thread's saved RoboCAD camera (`annotations.camera_view`:
//!   the keys it has, over the current camera; `mode` "turntable" when
//!   absent) is restored on the shared camera through `views::convert`
//!   (`CameraAction::Set`), then its `inspection_view` (a whole view state,
//!   camera and display) when it has one. RoboCAD's `POST
//!   /threads/{id}/show` is GUI-only (409 headless) and moves RoboCAD's own
//!   camera, so it is not used.
//! - **Fit in view** ([`fit`], `fit_thread`): the thread's linked parts
//!   that still exist are framed at the current angle (`CadMeshes::frame`,
//!   as Focus Selection) and selected; the pins are shown.
//! - **Show only linked parts** ([`view_parts`], `view_parts`): the first
//!   time, the camera, the selection and the whole display state are captured; a single
//!   part with a saved view restores it (`restore_view`), otherwise the
//!   section is turned off, the camera becomes orthographic and the parts
//!   are framed; the parts and everything under them are isolated
//!   ([`Isolation`]) and selected. **Return to assembly** ([`end`],
//!   `end_inspection`, also Escape) restores what was captured (the
//!   selection without parts deleted since).
//! - **The isolation never writes RoboCAD's visibility** (no `PATCH
//!   visible`, no `Ops.isolate`): `cad/mesh.rs` hides the other bodies at
//!   display time only (and leaves them out of picks, snaps, box select and
//!   framing), [`shown`] tells the pins and the reference planes.
//! - **Part links** ([`part_link`], `open_part_link`): a press on
//!   `[label](part:ID)` shows the part alone (`view_parts`: it and the
//!   parts under it) and selects exactly what REST `cad_select {ids: [ID]}`
//!   selects (`selection::handle`): the part itself.
use super::read;
use crate::app::actions::Call;
use crate::cad::actions::{CadAction, Cx};
use crate::cad::display::CadDisplay;
use crate::cad::document::CadDocument;
use crate::cad::views::CadViews;
use crate::camera::{CameraAction, CameraState};
use serde_json::{Map, Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::{AnchorStatus, CadThread, SelectionItem, ViewState};
use std::collections::{BTreeSet, HashSet};

/// RoboCAD's annotation camera keys (`annotations.camera_view`).
pub(crate) const CAMERA_KEYS: [&str; 8] = ["target", "distance", "yaw", "pitch", "fov", "orthographic", "mode", "rot"];
const NO_VIEW: &str = "CAD mode's 3D view is not available in this window";

/// The linked parts shown alone, and what Return to assembly restores.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Isolation {
    /// The parts and everything under them.
    pub ids: BTreeSet<String>,
    /// The parts as asked for (named in the status line).
    pub parts: Vec<String>,
    /// The thread whose parts are shown (its pin stays drawn).
    pub thread: Option<String>,
    /// The camera before (None without a camera).
    pub camera: Option<CameraState>,
    pub selection: Vec<SelectionItem>,
    /// The display state before (None without one): a part's saved view may
    /// change the display mode, grid, pins and section; all come back.
    pub display: Option<CadDisplay>,
}

/// Whether node `id` is shown: false only while an isolation leaves it out.
pub(crate) fn shown(doc: &CadDocument, id: &str) -> bool {
    doc.threads.isolation.as_ref().is_none_or(|i| i.ids.contains(id))
}

/// Node ids and everything under them, in the tree's walk order (parents first).
pub(crate) fn expand(doc: &CadDocument, ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = ids.iter().filter(|id| doc.has_node(id)).cloned().collect();
    let Some(state) = &doc.doc else { return out };
    for n in &state.nodes {
        if n.parent.as_ref().is_some_and(|p| out.contains(p)) && !out.contains(&n.id) {
            out.push(n.id.clone());
        }
    }
    out
}

/// The thread's linked parts that still exist (`_fit_nodes`).
pub(crate) fn fit_nodes(doc: &CadDocument, t: &CadThread) -> Vec<String> {
    let in_tree = |id: &str| doc.doc.as_ref().is_some_and(|d| d.nodes.iter().any(|n| n.id == id));
    t.linked_parts.iter().map(|p| p.part.node_id.clone()).filter(|id| in_tree(id)).collect()
}

/// The camera now as a camera state (what Return to assembly restores).
fn camera_now(views: Option<&CadViews>) -> Option<CameraState> {
    let c = views?.camera.filter(|c| c.radius > 0.0)?;
    Some(CameraState { focus: c.focus.to_array(), radius: c.radius, yaw: c.yaw, pitch: c.pitch, orthographic: c.orthographic, fov_deg: Some(c.fov.to_degrees()), trackball: c.trackball.then(|| c.rotation.to_array()), seconds: None })
}

/// The camera now as RoboCAD's annotation camera (`saved_view`,
/// comments.py:91-95), through `views::convert::capture`; None without a camera.
pub(crate) fn camera_dict(views: Option<&CadViews>, display: Option<&CadDisplay>) -> Option<Map<String, Value>> {
    let camera = views?.camera.filter(|c| c.radius > 0.0)?;
    let fallback = CadDisplay::default();
    let state = crate::cad::views::convert::capture(&camera, display.unwrap_or(&fallback));
    let Ok(Value::Object(all)) = serde_json::to_value(&state) else { return None };
    Some(all.into_iter().filter(|(k, _)| CAMERA_KEYS.contains(&k.as_str())).collect())
}

/// The camera state of a thread's saved camera: its keys over `current`
/// (RoboCAD sets only the keys present), `mode` "turntable" when absent.
pub(crate) fn camera_of_dict(view: &Map<String, Value>, current: Option<Map<String, Value>>) -> Result<CameraState, String> {
    let mut merged = current.unwrap_or_default();
    for (k, v) in view {
        if CAMERA_KEYS.contains(&k.as_str()) && !v.is_null() {
            merged.insert(k.clone(), v.clone());
        }
    }
    if !view.contains_key("mode") {
        merged.insert("mode".into(), json!("turntable"));
    }
    let state: ViewState = serde_json::from_value(Value::Object(merged)).map_err(|e| format!("the annotation's saved camera could not be read: {e}"))?;
    crate::cad::views::convert::camera_of(&state).map(|(camera, _)| camera)
}

/// A whole view state (`inspection_view`, a part's `view`) onto the camera and display.
fn restore_view(cx: &mut Cx, view: &Value) -> Result<(), String> {
    let state: ViewState = serde_json::from_value(view.clone()).map_err(|e| format!("the saved view could not be read: {e}"))?;
    let (camera, _) = crate::cad::views::convert::camera_of(&state)?;
    if let Some(display) = cx.display.as_deref_mut() {
        crate::cad::views::convert::apply_display(&state, display)?;
    }
    cx.camera.push(CameraAction::Set { state: camera });
    Ok(())
}

/// Select `ids` as `cad_select {ids}` does (the one selection path).
fn select(cx: &mut Cx, call: &mut Call, ids: Vec<String>) -> Result<(), String> {
    let action = CadAction::CadSelect { ids, items: Vec::new(), extend: false, toggle: false, picked_at: None };
    match crate::cad::selection::handle(&action, call, cx) {
        Outcome::Done(Err(e)) => Err(e),
        _ => Ok(()),
    }
}

/// The thread as read, or why not.
fn thread(doc: &CadDocument, id: &str) -> Result<CadThread, String> {
    read::thread(doc, id).cloned().ok_or_else(|| format!("no comment thread {id} in RoboCAD's comments as last read"))
}

/// Show on model (see the module doc).
pub(super) fn show(cx: &mut Cx, call: &mut Call, id: &str) -> Result<Value, String> {
    let t = thread(cx.doc, id)?;
    if t.anchor_status == AnchorStatus::Evidence || t.anchor.node_id.is_none() {
        return Err("This annotation is experiment evidence with no pin on the model: RoboCAD opens it in its experiments panel, which CAD mode does not have yet".into());
    }
    end(cx, call)?;
    let node = t.anchor.node_id.clone().filter(|n| cx.doc.doc.as_ref().is_some_and(|d| d.nodes.iter().any(|x| x.id == *n)));
    if let Some(node) = node.clone() {
        select(cx, call, vec![node])?;
    }
    if let Some(view) = t.view.as_object().filter(|v| !v.is_empty()) {
        let current = camera_dict(cx.views.as_deref(), cx.display.as_deref());
        let state = camera_of_dict(view, current)?;
        cx.camera.push(CameraAction::Set { state });
    }
    if let Some(inspection) = t.inspection_view.as_ref().filter(|v| !v.is_null()) {
        restore_view(cx, inspection)?;
    }
    cx.doc.show(Ok(format!("Showing the annotation on {}", t.node_name)));
    Ok(json!({"thread": id, "selected": node, "camera": cx.camera.last(), "note": "display only: RoboCAD's own camera is unchanged"}))
}

/// Fit in view (see the module doc).
pub(super) fn fit(cx: &mut Cx, call: &mut Call, id: &str) -> Result<Value, String> {
    let t = thread(cx.doc, id)?;
    let ids = fit_nodes(cx.doc, &t);
    if ids.is_empty() {
        return Err("This annotation has no linked parts to frame".into());
    }
    let set: HashSet<String> = expand(cx.doc, &ids).into_iter().collect();
    let meshes = cx.meshes.as_deref_mut().ok_or(NO_VIEW)?;
    let bounds = meshes.bounds_of(&set).ok_or("This annotation has no geometry to frame")?;
    meshes.frame(bounds);
    select(cx, call, ids.clone())?;
    if let Some(display) = cx.display.as_deref_mut()
        && !display.comment_pins
    {
        display.comment_pins = true;
    }
    let names: Vec<String> = ids.iter().map(|i| cx.doc.node_name(i)).collect();
    cx.doc.show(Ok(format!("Fit annotation in view: {}", names.join(", "))));
    Ok(json!({"framed": names, "note": "display only: RoboCAD's view and the geometry are unchanged"}))
}

/// Show only linked parts (see the module doc): `ids` (default the
/// thread's linked parts that still exist) and everything under them,
/// which are selected too when `highlight` (RoboCAD's `highlight_parts`).
pub(super) fn view_parts(cx: &mut Cx, call: &mut Call, thread_id: Option<&str>, ids: Option<&[String]>, highlight: bool) -> Result<Value, String> {
    let t = thread_id.and_then(|id| read::thread(cx.doc, id)).cloned();
    let ids: Vec<String> = match (ids, &t) {
        (Some(ids), _) => ids.iter().filter(|id| cx.doc.doc.as_ref().is_some_and(|d| d.nodes.iter().any(|n| n.id == **id))).cloned().collect(),
        (None, Some(t)) => fit_nodes(cx.doc, t),
        (None, None) => Vec::new(),
    };
    if ids.is_empty() {
        return Err("No available linked parts to show".into());
    }
    if cx.doc.threads.isolation.is_none() {
        let isolation = Isolation {
            ids: BTreeSet::new(),
            parts: Vec::new(),
            thread: None,
            camera: camera_now(cx.views.as_deref()),
            selection: cx.shared.items(),
            display: cx.display.as_deref().cloned(),
        };
        cx.doc.threads.isolation = Some(isolation);
    }
    // One part with its own saved view: that view (`restore_view`).
    let saved = (ids.len() == 1).then(|| t.as_ref().and_then(|t| t.linked_parts.iter().find(|p| p.part.node_id == ids[0])).and_then(|p| p.part.view.clone())).flatten();
    let expanded = expand(cx.doc, &ids);
    match saved {
        Some(view) => restore_view(cx, &view)?,
        None => {
            if let Some(display) = cx.display.as_deref_mut()
                && display.section.enabled
            {
                display.section.enabled = false;
            }
            cx.camera.push(CameraAction::Projection { orthographic: Some(true) });
            if let Some(meshes) = cx.meshes.as_deref_mut() {
                let set: HashSet<String> = expanded.iter().cloned().collect();
                if let Some(bounds) = meshes.bounds_of(&set) {
                    meshes.frame(bounds);
                }
            }
        }
    }
    if let Some(i) = cx.doc.threads.isolation.as_mut() {
        i.ids = expanded.iter().cloned().collect();
        i.parts = ids.clone();
        i.thread = thread_id.map(str::to_string);
    }
    if highlight {
        select(cx, call, expanded)?;
    }
    let names: Vec<String> = ids.iter().map(|i| cx.doc.node_name(i)).collect();
    cx.doc.show(Ok(format!("Part view: {}", names.join(", "))));
    Ok(json!({"shown_alone": ids, "note": "display only: the other parts are hidden in this window; RoboCAD's visibility is unchanged. Return to assembly (or Escape) restores the view"}))
}

/// Return to assembly (see the module doc); nothing when no parts are shown alone.
pub(super) fn end(cx: &mut Cx, _call: &mut Call) -> Result<Value, String> {
    let Some(i) = cx.doc.threads.isolation.take() else { return Ok(json!({"returned": false})) };
    if let Some(state) = i.camera {
        cx.camera.push(CameraAction::Set { state });
    }
    // The display as it was (the exact section's request and answer stay the current ones).
    if let (Some(before), Some(display)) = (i.display, cx.display.as_deref_mut()) {
        let restored = CadDisplay { exact: display.exact.clone(), ..before };
        if *display != restored {
            *display = restored;
        }
    }
    let tree = cx.doc.doc.as_ref();
    let kept: Vec<SelectionItem> = i.selection.into_iter().filter(|s| tree.is_some_and(|d| d.nodes.iter().any(|n| n.id == s.0))).collect();
    if cx.shared.items() != kept {
        cx.shared.set(kept)?;
        crate::cad::selection::publish(cx.doc, cx.shared.view());
    }
    cx.doc.show(Ok("Returned to the assembly view".into()));
    Ok(json!({"returned": true}))
}

/// A linked part's row pressed (`highlight_parts([id])`): it and
/// everything under it selected; the row is the current one.
pub(super) fn highlight(cx: &mut Cx, call: &mut Call, node: &str) -> Result<Value, String> {
    if !cx.doc.doc.as_ref().is_some_and(|d| d.nodes.iter().any(|n| n.id == node)) {
        return Err(format!("{node}: this part was deleted"));
    }
    if cx.doc.threads.part.as_deref() != Some(node) {
        cx.doc.threads.part = Some(node.to_string());
        cx.doc.touch();
    }
    let ids = expand(cx.doc, &[node.to_string()]);
    select(cx, call, ids.clone())?;
    Ok(json!({"selected": ids}))
}

/// A part link pressed (see the module doc).
pub(super) fn part_link(cx: &mut Cx, call: &mut Call, thread_id: Option<&str>, node: &str) -> Result<Value, String> {
    if !cx.doc.doc.as_ref().is_some_and(|d| d.nodes.iter().any(|n| n.id == node)) {
        return Err(format!("{node}: this part was deleted"));
    }
    // Shown alone first (its Return restores the selection from before),
    // then selected exactly as `cad_select {ids: [node]}` selects it: the
    // part asked for, not the parts under it the isolation also shows.
    let one = [node.to_string()];
    let shown = view_parts(cx, call, thread_id, Some(&one[..]), false)?;
    select(cx, call, one.to_vec())?;
    Ok(shown)
}

/// `cad_state.threads.isolation`.
pub(super) fn state_json(doc: &CadDocument) -> Value {
    match &doc.threads.isolation {
        None => Value::Null,
        Some(i) => json!({"parts": i.parts, "shown": i.ids, "thread": i.thread, "restores_camera": i.camera.is_some(), "restores_selection": i.selection.len()}),
    }
}

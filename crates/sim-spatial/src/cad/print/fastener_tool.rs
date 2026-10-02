//! The fastener tool's face clicks (`FastenerTool.press`, ui/tools.py:1120-1155).
//!
//! While `tool.fastener` (`Flow::PrintPick`) is the active operation,
//! [`click`] turns a left press over the 3D view (not over a panel, not
//! with Alt, which orbits, not the press that closes a command surface and
//! not while one is open) into `CadPrint {op: pick, item, picked_at}`: the
//! face under the pointer only through `CadMeshes::face_at` at the shown
//! revision (nothing while the mesh is being redrawn), noting the hit
//! point and RoboCAD's vertex, midpoint, centre or endpoint snap there
//! (`EditsState::click`). Nothing else changes: the selection is not
//! touched (RoboCAD's tool picks with `request_pick`).
//!
//! [`pick`] applies a pick, from a click or a scripted run alike: the active fastener
//! tool, a face item of the shown tree, `picked_at` the shown revision
//! (each refused by name otherwise). The point is the noted click's snap,
//! else its hit (when the note is for this face and revision); else the
//! form's typed point; else the face's own point at the shown revision
//! (`FaceInfo::point`, else its centroid). Then one
//! `CadRun tool.fastener` on the face with the form's values, or the
//! remembered `last_fastener` ones when the form is closed (RoboCAD's
//! `ops.fastener_hole(nid, face, point, self.spec)`). The tool stays
//! active (Escape ends it); a refusal shows in its form too.
use super::edits::FastenerClick;
use super::{PrintArgs, PrintOp};
use crate::app::actions::{Act, Call};
use crate::app::{ViewerMode};
use crate::cad::actions::{CadAction, Cx};
use crate::cad::document::CadDocument;
use crate::cad::mesh::{CadBody, CadMeshes};
use crate::cad::ops::{self, Env, Flow};
use crate::cad::sketch::{CadActivePlane, CadSketches};
use crate::cad::snap::{self, SnapKind};
use crate::cad::topology::CadTopology;
use crate::cad::transform::{cursor_in_view, ray_hit};
use crate::cad::view::CadView;
use bevy::picking::hover::HoverMap;
use bevy::picking::mesh_picking::ray_cast::MeshRayCast;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use serde_json::{Map, Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::SelectionItem;

/// The active fastener tool's catalogue id (`Flow::PrintPick`).
pub(super) fn active_tool(doc: &CadDocument) -> Option<&'static str> {
    let id = doc.ops.active?;
    (ops::entry(id)?.flow == Flow::PrintPick).then_some(id)
}

/// The noted click for `item` at `picked_at`, taken (a note for another
/// face or revision is dropped: it was for a pick that did not come).
pub(super) fn take_click(doc: &mut CadDocument, item: &SelectionItem, picked_at: u64) -> Option<FastenerClick> {
    doc.print.edits.click.take().filter(|c| c.item == *item && c.picked_at == picked_at)
}

/// The point a pick sends (see the module doc): the click's snap or hit,
/// else the typed point, else the face's own point (`face`, read only then).
pub(super) fn point_for(click: Option<&FastenerClick>, typed: &str, face: impl FnOnce() -> Result<[f64; 3], String>) -> Result<Value, String> {
    if let Some(c) = click {
        // `s.point if s.kind in ("vertex", "midpoint", "center", "endpoint") else result["world"]`.
        return Ok(json!(c.snap.unwrap_or(c.point)));
    }
    if !typed.trim().is_empty() {
        return Ok(Value::String(typed.to_string()));
    }
    face().map(|p| json!(p))
}

/// Face `item`'s own point at the shown revision (`CadTopology`): its
/// `point`, else its centroid; or why not.
fn face_point(cx: &Cx, item: &SelectionItem) -> Result<[f64; 3], String> {
    let name = cx.doc.node_name(&item.0);
    let Some(topology) = cx.topology.as_deref() else { return Err("the faces are not available without CAD mode's 3D view".into()) };
    let Some(t) = topology.get(&item.0).filter(|t| t.revision == cx.doc.shown_revision()) else {
        return Err(match topology.error(&item.0) {
            Some(e) => format!("the faces of {name} could not be read: {e}"),
            None => format!("the faces of {name} are still being read from RoboCAD; click again in a moment"),
        });
    };
    let face = t.faces.iter().find(|f| f.index == item.2).ok_or_else(|| format!("{name} has no face {} at the shown revision ({} faces)", item.2, t.faces.len()))?;
    face.point.or(face.centroid).ok_or_else(|| format!("face {} of {name} has no point to place a hole at: click it in the 3D view or give the point", item.2))
}

/// Why a pick is refused before anything is read: no fastener tool, not a
/// face of the shown tree, or no or another revision. The active tool's id.
pub(super) fn check_pick(doc: &CadDocument, args: &PrintArgs) -> Result<(&'static str, SelectionItem, u64), String> {
    let Some(id) = active_tool(doc) else { return Err("no fastener tool is active: start Fastener hole… first".into()) };
    let Some(item) = args.item.clone() else { return Err("pick takes item: the face [node, \"face\", index] to place the hole on".into()) };
    if item.1 != "face" {
        return Err(format!("the fastener tool places a hole on a face, not a {} item", item.1));
    }
    if !doc.has_node(&item.0) {
        return Err(format!("no node {} in the shown tree", item.0));
    }
    let shown = doc.shown_revision();
    match args.picked_at {
        None => Err("pass picked_at: the RoboCAD revision the face index was read at".into()),
        Some(r) if r != shown => Err(format!("the face was picked at revision {r}; RoboCAD's faces may be renumbered since (now {shown}): click again")),
        Some(r) => Ok((id, item, r)),
    }
}

/// `CadPrint {op: pick}` (see the module doc).
pub(super) fn pick(args: &PrintArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    let (id, item, picked_at) = match check_pick(cx.doc, args) {
        Ok(p) => p,
        Err(e) => return Outcome::Done(Err(e)),
    };
    let Some(entry) = ops::entry(id) else { return Outcome::Done(Err(format!("{id} is not in the catalogue"))) };
    // The form's values as its OK sends them (`ops::form::submit`); without
    // the form, the remembered dialog values (RoboCAD's `last_fastener`) as
    // a newly opened form would show them, not the catalogue defaults.
    let texts: Vec<String> = match &cx.doc.ops.form {
        Some(f) if f.op == id && f.texts.len() == entry.params.len() => f.texts.clone(),
        _ => {
            let mut texts: Vec<String> = entry.params.iter().map(|p| p.default.to_string()).collect();
            super::edits::seed(entry, cx.doc, &Env { defaults: Some(&cx.settings.cad), ..Env::default() }, &mut texts);
            texts
        }
    };
    let typed = entry.params.iter().position(|p| p.name == "point").and_then(|i| texts.get(i)).cloned().unwrap_or_default();
    let click = take_click(cx.doc, &item, picked_at);
    let point = match point_for(click.as_ref(), &typed, || face_point(&*cx, &item)) {
        Ok(p) => p,
        Err(e) => return Outcome::Done(Err(e)),
    };
    let mut params: Map<String, Value> = entry.params.iter().zip(&texts).map(|(p, t)| (p.name.to_string(), Value::String(t.clone()))).collect();
    params.insert("point".into(), point);
    cx.doc.print.edits.last_pick = Some((item.clone(), picked_at));
    cx.doc.touch();
    let outcome = ops::run_entry_on(id, &params, std::slice::from_ref(&item), Some(picked_at), call, cx);
    // A refusal shows in the tool's form too, as its OK's does.
    if let Outcome::Done(Err(e)) = &outcome
        && let Some(f) = cx.doc.ops.form.as_mut().filter(|f| f.op == id)
    {
        f.error = Some(e.clone());
        cx.doc.touch();
    }
    outcome
}

/// Whether a command surface was open at the end of the last frame's
/// Input, i.e. when this frame's press was made (that press closes it).
#[derive(Default)]
pub(super) struct ClickState {
    surface_open: bool,
}

/// Input: the fastener tool's presses (see the module doc).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn click(
    doc: Option<ResMut<CadDocument>>,
    (view, meshes, topology, sketches, plane): (Option<Res<CadView>>, Option<Res<CadMeshes>>, Option<Res<CadTopology>>, Option<Res<CadSketches>>, Option<Res<CadActivePlane>>),
    windows: Query<&Window, With<PrimaryWindow>>,
    (buttons, keys): (Option<Res<ButtonInput<MouseButton>>>, Option<Res<ButtonInput<KeyCode>>>),
    hover: Option<Res<HoverMap>>,
    nodes: Query<(), With<Node>>,
    mut cast: MeshRayCast,
    bodies: Query<&CadBody>,
    mut out: MessageWriter<Act<CadAction>>,
    mut state: Local<ClickState>,
) {
    let Some(mut doc) = doc else { return };
    let surface_was_open = std::mem::replace(&mut state.surface_open, doc.ops.surface.is_some());
    if active_tool(&doc).is_none() {
        return;
    }
    let (Some(view), Some(meshes)) = (view, meshes) else { return };
    let pressed = buttons.as_ref().is_some_and(|b| b.just_pressed(MouseButton::Left));
    if !view.valid || !pressed || surface_was_open || doc.ops.surface.is_some() {
        return;
    }
    // Alt+left is RoboCAD's orbit, not a pick.
    if keys.as_ref().is_some_and(|k| k.any_pressed([KeyCode::AltLeft, KeyCode::AltRight])) {
        return;
    }
    let Some(cursor) = cursor_in_view(windows.single().ok(), &view, hover.as_deref(), &nodes) else { return };
    let shown = doc.shown_revision();
    let Some(hit) = ray_hit(&doc, &mut cast, &view, cursor, &bodies) else { return };
    // `result["hit"][0] == "face"`; nothing while the mesh is being redrawn
    // for a newer revision (its faces are the old revision's).
    let Some(face) = hit.triangle.and_then(|t| meshes.face_at(&hit.node, t, shown)) else { return };
    let item = SelectionItem(hit.node.clone(), "face".into(), face);
    let point = [f64::from(hit.point.x), f64::from(hit.point.y), f64::from(hit.point.z)];
    // `self.ctx.snap(pos)`, used when it is a vertex, midpoint, centre or endpoint.
    let candidates = snap::drawn_candidates(&doc, topology.as_deref(), Some(&*meshes), sketches.as_deref());
    let on = snap::snap_plane(plane.as_deref());
    let snapped = snap::snap_on(&view, cursor, &candidates, false, on.as_ref())
        .filter(|s| matches!(s.kind, SnapKind::Vertex | SnapKind::Midpoint | SnapKind::Center | SnapKind::Endpoint))
        .map(|s| s.exact);
    doc.print.edits.click = Some(FastenerClick { item: item.clone(), picked_at: shown, point, snap: snapped });
    out.write(Act::ui(CadAction::CadPrint(PrintArgs { op: PrintOp::Pick, item: Some(item), picked_at: Some(shown), ..PrintArgs::default() })));
}

/// CadPlugin: the fastener tool's 3D clicks.
pub(super) fn build(app: &mut App) {
    app.add_systems(Update, click.in_set(crate::app::InputSet::Window).run_if(in_state(ViewerMode::Cad)));
}

//! The active plane's systems (cad-sketch; RoboCAD's `viewport.active_plane`,
//! `set_active_plane`, `toggle_plane_snapping` and `PlaneTool`).
//!
//! - [`sync`] (SimSync, core, after the cache): the active plane belongs
//!   to one document generation (reset when it changes); a plane tool's new
//!   plane node becomes active once its edit succeeds (`ops.plane_created`,
//!   status "Active plane set", RoboCAD's `set_active_plane(pid)`). The
//!   edit only requests a `/doc` refetch, so the new node is not in the
//!   shown tree yet: it is kept until the shown tree has shown it, or the
//!   shown revision moved past the one it was adopted at without it. A
//!   plane node's frame is the cache's at the shown revision only (None,
//!   "(reading)", while it is refetched: operations never decide with an
//!   old revision's frame; `plane_draw` draws the last read meanwhile). A
//!   plane node that was shown and is gone from the shown tree is dropped
//!   with a status naming it; a read error is shown once.
//! - **Selecting a plane node makes it active** (native addition, recorded:
//!   RoboCAD has no such gesture; its plane nodes become active only when a
//!   plane tool creates them). When the selection changes to exactly one
//!   node of kind "plane", that node becomes the active plane.
//! - [`view_act`]: "Active plane: XY|XZ|YZ" and "Toggle 2D snapping"
//!   (app.py:1022-1031, the same status lines).
//! - [`begin`]: `PlaneTool.activate` (tools.py:1078-1081): picks cleared,
//!   selection mode face (from face, midplane) or vertex (three points,
//!   camera).
//! - **Picks** ([`picks`], SimSync; tools.py:1083-1111): a left press over
//!   the 3D view (not while a command surface is open, or was when the press
//!   came, nor while a text field has the keyboard): from face and midplane
//!   take the face under the cursor (`ray_hit`, then `CadMeshes::face_at` at
//!   the shown revision; a body being redrawn refuses by name), three
//!   points and camera take the snap (`snap::snap_on` over the drawn bodies'
//!   and sketches' candidates, on the active plane while 2D snapping is
//!   on; RoboCAD's `ctx.snap(pos)`, no Alt). When the tool has its picks it
//!   writes one `CadRun` (faces as `items` with the revision they were
//!   picked at; points as the "x, y, z" parameters a, b, c; the camera's
//!   direction is the view's at the run, `Arg::ViewDir`), clears the picks
//!   and stays active, as RoboCAD's tool does. A run that `commit_refusal`
//!   would refuse is not sent: the earlier picks are kept and the reason
//!   shown. A point pick while 2D snapping is on and the active plane node
//!   is being read is refused (`snap::press_snap_plane`). A face pick from an older
//!   revision is dropped before the next is added (its index may name
//!   another face now). The picks are drawn as markers ([`draw`], Present).
//! - [`state_json`]: `cad_state.plane`.
use super::{ActivePlane, BasePlane, CadActivePlane, CadSketches, PlaneMode, PlanePick, ViewAct};
use crate::app::actions::Act;
use crate::app::{ViewerMode, ViewerSet};
use crate::cad::actions::CadAction;
use crate::cad::document::{CadDocument, CadInputFocus, SelectMode};
use crate::cad::mesh::{CadBody, CadMeshes};
use crate::cad::ops::{Flow, entry};
use crate::cad::snap::{self, Candidate};
use crate::cad::topology::CadTopology;
use crate::cad::transform::{HOT, ToolGizmos, cursor_in_view, marker, num, ray_hit};
use crate::cad::view::CadView;
use bevy::picking::hover::HoverMap;
use bevy::picking::mesh_picking::ray_cast::MeshRayCast;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use serde_json::{Map, Value, json};
use sim_runtime::cad_client::SelectionItem;
use std::ops::DerefMut;

/// RoboCAD's status line when the active plane is set (app.py:1026).
const SET: &str = "Active plane set";

/// The window's plane systems: the plane tools' picks and their markers.
pub(in crate::cad) fn build(app: &mut App) {
    app.add_systems(
        Update,
        // After this frame's camera snapshot, the drawn bodies and the sketch
        // cache and active plane (`cache::sync` then `sync`): the picks read all of them.
        picks.after(crate::cad::view::update).after(crate::cad::mesh::sync).after(sync).in_set(ViewerSet::SimSync).run_if(in_state(ViewerMode::Cad)),
    )
    .add_systems(Update, draw.in_set(ViewerSet::Present).run_if(in_state(ViewerMode::Cad)));
}

/// What [`sync`] remembers across frames.
#[derive(Default)]
pub(in crate::cad) struct Seen {
    /// The selection last looked at (a change to one plane node activates it).
    selection: Option<Vec<SelectionItem>>,
    /// The plane node and revision whose read error was last shown.
    reported: Option<(String, u64)>,
    /// A plane tool's new node and the shown revision it was adopted at,
    /// until the shown tree has shown it.
    unseen: Option<(String, u64)>,
}

/// The node's kind in the shown tree.
fn kind<'a>(doc: &'a CadDocument, id: &str) -> Option<&'a str> {
    doc.doc.as_ref().and_then(|d| d.nodes.iter().find(|n| n.id == id)).map(|n| n.kind.as_str())
}

/// SimSync (core, after `cache::sync`): reset on a new generation, adopt a
/// plane tool's new node, fill and drop node frames, follow a selected plane node.
pub(in crate::cad) fn sync(doc: Option<ResMut<CadDocument>>, plane: Option<ResMut<CadActivePlane>>, sketches: Option<Res<super::CadSketches>>, mut seen: Local<Seen>) {
    let (Some(doc), Some(plane)) = (doc, plane) else { return };
    follow(doc, plane, sketches.as_deref(), &mut seen);
}

/// [`sync`]'s work, over anything that derefs to the resources (`ResMut`
/// in the system, so a frame that changes nothing marks nothing changed;
/// plain references in tests).
pub(in crate::cad) fn follow(mut doc: impl DerefMut<Target = CadDocument>, mut plane: impl DerefMut<Target = CadActivePlane>, sketches: Option<&CadSketches>, seen: &mut Seen) {
    if plane.generation != doc.generation {
        *plane = CadActivePlane { generation: doc.generation, ..default() };
        *seen = Seen::default();
    }
    // A plane tool's new plane (its edit succeeded): RoboCAD's `set_active_plane(pid)`.
    if doc.ops.plane_created.is_some()
        && let Some(id) = doc.ops.plane_created.take()
    {
        let frame = sketches.and_then(|s| s.plane(&id));
        seen.unseen = Some((id.clone(), doc.shown_revision()));
        plane.plane = Some(ActivePlane::Node { id, frame });
        doc.show(Ok(SET.to_string()));
    }
    // A selection that changed to exactly one plane node makes it active (native addition).
    if seen.selection.as_ref() != Some(&doc.selection) {
        seen.selection = Some(doc.selection.clone());
        let nodes = doc.selected_nodes();
        if let [only] = nodes.as_slice()
            && kind(&doc, only) == Some("plane")
            && !matches!(&plane.plane, Some(ActivePlane::Node { id, .. }) if id == only)
        {
            let frame = sketches.and_then(|s| s.plane(only));
            plane.plane = Some(ActivePlane::Node { id: only.clone(), frame });
            doc.show(Ok(SET.to_string()));
        }
    }
    let Some(ActivePlane::Node { id, frame }) = &plane.plane else {
        seen.unseen = None;
        return;
    };
    let id = id.clone();
    let shown = doc.shown_revision();
    if doc.doc.is_some() && doc.has_node(&id) {
        // The shown tree has shown it: from now on its absence means it is gone.
        if seen.unseen.as_ref().is_some_and(|(u, _)| *u == id) {
            seen.unseen = None;
        }
    } else if doc.doc.is_some() {
        // A plane tool's new node waits for the refetch that shows it.
        let waiting = seen.unseen.as_ref().is_some_and(|(u, at)| *u == id && shown <= *at);
        if !waiting {
            // Shown and now gone (or a newer tree still lacks it): no longer the active plane.
            seen.unseen = None;
            plane.plane = None;
            doc.show(Ok(format!("The active plane (node {id}) is no longer in the document: no plane is active (XY is used)")));
            return;
        }
    }
    // Its frame at the shown revision only (decisions never use an old revision's frame).
    let next = sketches.and_then(|s| s.plane(&id));
    if next != *frame {
        plane.plane = Some(ActivePlane::Node { id: id.clone(), frame: next });
    }
    // A frame that cannot be read is said once per revision (its label stays "(reading)").
    if next.is_none()
        && let Some(error) = sketches.and_then(|s| s.error(&id))
    {
        let at = (id.clone(), shown);
        if seen.reported.as_ref() != Some(&at) {
            let message = format!("The active plane {} could not be read from RoboCAD: {error}", doc.node_name(&id));
            seen.reported = Some(at);
            doc.show(Err(message));
        }
    }
}

/// `Flow::View`: set the active plane or toggle 2D snapping; the answer.
pub(in crate::cad) fn view_act(doc: &mut CadDocument, plane: &mut CadActivePlane, act: ViewAct) -> Value {
    if plane.generation != doc.generation {
        *plane = CadActivePlane { generation: doc.generation, ..default() };
    }
    match act {
        // RoboCAD's `set_active_plane(None, Plane.xy())` (app.py:1022-1027).
        ViewAct::Plane(b) => {
            plane.plane = Some(ActivePlane::Base(b));
            doc.show(Ok(SET.to_string()));
        }
        // RoboCAD's `toggle_plane_snapping` (app.py:1029-1031).
        ViewAct::Snap2d => {
            plane.snap_2d = !plane.snap_2d;
            doc.show(Ok(format!("2D snapping {}", if plane.snap_2d { "on" } else { "off" })));
        }
    }
    json!({"plane": plane.label(doc), "snap_2d": plane.snap_2d})
}

/// `Flow::PlanePick` starts: RoboCAD's `PlaneTool.activate` (tools.py:1078-1081).
pub(in crate::cad) fn begin(doc: &mut CadDocument, mode: PlaneMode) -> Result<(), String> {
    doc.ops.plane_picks.clear();
    let select = match mode {
        PlaneMode::Face | PlaneMode::Mid => SelectMode::Face,
        PlaneMode::Three | PlaneMode::Camera => SelectMode::Vertex,
    };
    if doc.select_mode != select {
        doc.select_mode = select;
        crate::cad::selection::publish(doc);
    }
    Ok(())
}

/// `cad_state.plane`: the label, the argument an operation sends for it
/// (null with no active plane), its frame (or the error while a plane
/// node is being read), 2D snapping and the generation.
pub(in crate::cad) fn state_json(doc: &CadDocument, plane: &CadActivePlane) -> Value {
    let arg = if plane.plane.is_some() { plane.arg_or(BasePlane::Xy) } else { Value::Null };
    let frame = match plane.frame() {
        Ok(Some(f)) => f.json(),
        Ok(None) => Value::Null,
        Err(e) => json!({"error": e}),
    };
    json!({"plane": plane.label(doc), "arg": arg, "frame": frame, "snap_2d": plane.snap_2d, "generation": plane.generation})
}

/// The plane tool that is active, with its mode.
fn plane_tool(doc: &CadDocument) -> Option<(&'static str, PlaneMode)> {
    let id = doc.ops.active?;
    match entry(id)?.flow {
        Flow::PlanePick(mode) => Some((id, mode)),
        _ => None,
    }
}

/// How many picks a mode needs (tools.py:1092-1111).
fn needed(mode: PlaneMode) -> usize {
    match mode {
        PlaneMode::Face => 1,
        PlaneMode::Mid | PlaneMode::Camera => 2,
        PlaneMode::Three => 3,
    }
}

/// A point as a POINT parameter's text ("x, y, z", mm).
fn point_text(p: [f64; 3]) -> Value {
    Value::String(p.map(num).join(", "))
}

/// The `CadRun` a complete set of picks sends, or None while picks are missing.
pub(in crate::cad) fn run_for(id: &str, mode: PlaneMode, picks: &[PlanePick]) -> Option<CadAction> {
    if picks.len() < needed(mode) {
        return None;
    }
    let faces: Vec<(SelectionItem, u64)> = picks.iter().filter_map(|p| match p {
        PlanePick::Face { node, face, revision } => Some((SelectionItem(node.clone(), "face".into(), *face), *revision)),
        PlanePick::Point(_) => None,
    }).collect();
    let points: Vec<[f64; 3]> = picks.iter().filter_map(|p| match p {
        PlanePick::Point(q) => Some(*q),
        PlanePick::Face { .. } => None,
    }).collect();
    let (params, items, revision) = match mode {
        PlaneMode::Face | PlaneMode::Mid => {
            let revision = faces.first().map(|(_, r)| *r);
            (Map::new(), Some(faces.into_iter().map(|(i, _)| i).collect::<Vec<_>>()), revision)
        }
        PlaneMode::Three | PlaneMode::Camera => {
            let params: Map<String, Value> = ["a", "b", "c"].iter().zip(&points).map(|(k, p)| ((*k).to_string(), point_text(*p))).collect();
            (params, None, None)
        }
    };
    Some(CadAction::CadRun { id: id.to_string(), params, items, revision })
}

/// What [`picks`] remembers across frames.
#[derive(Default)]
struct Picker {
    /// Snap candidates of the drawn bodies and sketches, by their epochs.
    cache: Option<((u64, u64, u64), Vec<Candidate>)>,
    /// A command surface was open at the end of the last frame's SimSync.
    surface_open: bool,
}

/// SimSync: a plane tool's picks (see the module doc).
#[allow(clippy::too_many_arguments)]
fn picks(
    doc: Option<ResMut<CadDocument>>,
    (view, topology, meshes, plane, sketches, focus): (Option<Res<CadView>>, Option<Res<CadTopology>>, Option<Res<CadMeshes>>, Option<Res<CadActivePlane>>, Option<Res<CadSketches>>, Option<Res<CadInputFocus>>),
    windows: Query<&Window, With<PrimaryWindow>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    hover: Option<Res<HoverMap>>,
    nodes: Query<(), With<Node>>,
    mut cast: MeshRayCast,
    bodies: Query<&CadBody>,
    mut out: MessageWriter<Act<CadAction>>,
    mut state: Local<Picker>,
) {
    let (Some(mut doc), Some(view), Some(meshes)) = (doc, view, meshes) else { return };
    let state = &mut *state;
    // The surface as this frame's Input saw it: an outside press closed it in Actions, before this system.
    let surface_was_open = std::mem::replace(&mut state.surface_open, doc.ops.surface.is_some());
    let Some((id, mode)) = plane_tool(&doc) else { return };
    let focused = focus.is_some_and(|f| f.0);
    let pressed = buttons.as_ref().is_some_and(|b| b.just_pressed(MouseButton::Left)) && !surface_was_open && doc.ops.surface.is_none() && !focused;
    if !pressed || !view.valid {
        return;
    }
    let Some(cursor) = cursor_in_view(windows.single().ok(), &view, hover.as_deref(), &nodes) else { return };
    let shown = doc.shown_revision();
    let pick = match mode {
        PlaneMode::Face | PlaneMode::Mid => {
            // RoboCAD's `request_pick`: only a face hit counts (tools.py:1086-1088).
            let Some(hit) = ray_hit(&doc, &mut cast, &view, cursor, &bodies) else { return };
            if meshes.drawn_revision(&hit.node).is_some_and(|r| r != shown) {
                let name = doc.node_name(&hit.node);
                doc.show(Err(format!("{name} is being redrawn for revision {shown}; click again in a moment")));
                return;
            }
            let Some(face) = hit.triangle.and_then(|t| meshes.face_at(&hit.node, t, shown)) else { return };
            PlanePick::Face { node: hit.node, face, revision: shown }
        }
        PlaneMode::Three | PlaneMode::Camera => {
            let key = snap::candidates_key(topology.as_deref(), Some(&*meshes), sketches.as_deref());
            if state.cache.as_ref().is_none_or(|(k, _)| *k != key) {
                state.cache = Some((key, snap::drawn_candidates(&doc, topology.as_deref(), Some(&*meshes), sketches.as_deref())));
            }
            let candidates: &[Candidate] = match state.cache.as_ref() {
                Some((_, c)) => c,
                None => &[],
            };
            // RoboCAD's `ctx.snap(pos)`: no Alt, the active plane only with 2D
            // snapping on (refused while that plane node is being read).
            let on = match snap::press_snap_plane(plane.as_deref()) {
                Ok(on) => on,
                Err(why) => {
                    doc.show(Err(why));
                    return;
                }
            };
            let Some(s) = snap::snap_on(&view, cursor, candidates, false, on.as_ref()) else { return };
            // The f64 point: a vertex or sketch endpoint exactly as RoboCAD gave it.
            PlanePick::Point(s.exact)
        }
    };
    // A face picked at an older revision may name another face now.
    doc.ops.plane_picks.retain(|p| !matches!(p, PlanePick::Face { revision, .. } if *revision != shown));
    let mut next = doc.ops.plane_picks.clone();
    next.push(pick);
    let n = next.len();
    match run_for(id, mode, &next) {
        Some(run) => {
            // A run that would be refused (edit in flight, not connected, stale,
            // faces from another revision) keeps the earlier picks: the last is picked again.
            let faces = matches!(mode, PlaneMode::Face | PlaneMode::Mid);
            if let Some(why) = doc.commit_refusal(faces.then_some(shown)) {
                doc.show(Err(format!("{why} (picks so far kept: {} of {}; pick the last again)", n - 1, needed(mode))));
                return;
            }
            // The tool stays active with no picks (RoboCAD's `self.picks = []`).
            doc.ops.plane_picks.clear();
            out.write(Act::ui(run));
        }
        None => {
            doc.ops.plane_picks = next;
            let hint = entry(id).map_or("", |e| e.hint);
            doc.show(Ok(format!("{hint} ({n} of {})", needed(mode))));
        }
    }
}

/// Present: the plane tool's picks as markers (display only): a point at
/// itself, a face at its centroid in the shown topology.
fn draw(doc: Option<Res<CadDocument>>, view: Option<Res<CadView>>, topology: Option<Res<CadTopology>>, mut gizmos: Gizmos<ToolGizmos>) {
    let (Some(doc), Some(view)) = (doc, view) else { return };
    if doc.ops.plane_picks.is_empty() || !view.valid || plane_tool(&doc).is_none() {
        return;
    }
    let at = |p: [f64; 3]| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    for pick in &doc.ops.plane_picks {
        let point = match pick {
            PlanePick::Point(p) => Some(at(*p)),
            PlanePick::Face { node, face, .. } => topology.as_deref().and_then(|t| t.get(node)).and_then(|t| t.faces.iter().find(|f| f.index == *face)).and_then(|f| f.centroid).map(at),
        };
        if let Some(p) = point {
            marker(&mut gizmos, &view, p, 9.0, HOT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A complete set of picks is one `CadRun`: faces as items at their
    /// revision, points as "x, y, z" parameters; fewer picks send nothing.
    #[test]
    fn complete_picks_make_one_run() {
        let face = |node: &str, face: i64| PlanePick::Face { node: node.into(), face, revision: 7 };
        assert_eq!(run_for("tool.plane_mid", PlaneMode::Mid, &[face("b1", 2)]), None);
        let Some(CadAction::CadRun { id, params, items, revision }) = run_for("tool.plane_mid", PlaneMode::Mid, &[face("b1", 2), face("b2", 5)]) else { panic!("no run") };
        assert_eq!((id.as_str(), params.is_empty(), revision), ("tool.plane_mid", true, Some(7)));
        assert_eq!(items, Some(vec![SelectionItem("b1".into(), "face".into(), 2), SelectionItem("b2".into(), "face".into(), 5)]));
        let picks = [PlanePick::Point([0.0, 0.0, 0.0]), PlanePick::Point([10.0, 0.0, 0.0]), PlanePick::Point([0.0, 10.5, 0.0])];
        assert_eq!(run_for("tool.plane_three", PlaneMode::Three, &picks[..2]), None);
        let Some(CadAction::CadRun { params, items, revision, .. }) = run_for("tool.plane_three", PlaneMode::Three, &picks) else { panic!("no run") };
        assert_eq!(Value::Object(params), json!({"a": "0, 0, 0", "b": "10, 0, 0", "c": "0, 10.5, 0"}));
        assert_eq!((items, revision), (None, None));
        let Some(CadAction::CadRun { params, .. }) = run_for("tool.plane_camera", PlaneMode::Camera, &picks[..2]) else { panic!("no run") };
        assert_eq!(Value::Object(params), json!({"a": "0, 0, 0", "b": "10, 0, 0"}));
    }
}

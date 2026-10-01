//! The catalogue's interactive flows (cad-modify): placing primitives
//! (RoboCAD's `PrimitiveTool`: drag the base, then the height; Tab for
//! exact sizes), pick-then-form tools (RoboCAD's `EdgeTool`/`ShellTool`:
//! clicks toggle edges or faces), "Set pivot at cursor snap", and the view
//! direction for "Project curve onto body". Previews are display only.
//!
//! - **Placing** ([`pointer`], SimSync; RoboCAD ui/tools.py:386-541) while
//!   `CadDocument::ops.active` is a `Flow::Place` op and the pointer is
//!   over the 3D view: a left press snaps on the active plane
//!   (`snap::snap_on` with RoboCAD's `want_plane`: the drawn bodies' and
//!   sketches' candidates projected onto the plane, then the 10 mm grid in
//!   the plane, else the plane hit; Alt suppresses) and begins the base
//!   (stage 1) with RoboCAD's revision at the press;
//!   dragging moves its second corner; the release finishes a sphere at
//!   once (RoboCAD's `release`), keeps a zero-size base waiting for a drag,
//!   else starts stage 2, where the pointer drags the height along the
//!   plane's normal on the plane through the second corner that faces the
//!   camera (RoboCAD's `hover`; Ctrl snaps to the 10 mm grid, half to even
//!   as Python's `round`); the next press finishes. Finishing writes one
//!   `CadRun {id, params, revision}` with the sizes RoboCAD's `_finish`
//!   computes ([`finish_params`]); the op's handler builds the Ops call,
//!   refuses it by name when the document changed since the press, and
//!   sends it as one edit. Tab during a drag writes the base's first point
//!   (projected onto the plane, except a sphere's centre) into the
//!   form's anchor field, found by its name ("corner", "center" or
//!   "base"), as a model point "x, y, z", so OK places the typed sizes
//!   there (RoboCAD's `commit` anchors at `p0`). Escape is the surfaces' key (`CadFormCancel` ends
//!   the placement). While a command surface is open (or was when the
//!   press came: the press that closes it is applied before this system
//!   runs) a press only closes it, as `pick` does.
//! - **The plane** is RoboCAD's `ctx.active_plane()`: the active plane
//!   (`CadActivePlane`), else XY. A placement keeps the plane it began on;
//!   if the active plane changes during it, the placement ends with nothing
//!   sent. A plane node whose frame is still being read refuses the press
//!   by name. Base points are (u, v) in the plane (`to_local`), the height
//!   runs along its normal, the preview is drawn on it, and the finished
//!   anchors are model points ([`finish_params`]); `args::place` turns them
//!   into the Ops call for that plane.
//! - **Preview** ([`draw`], Present): the base rectangle or ring and the
//!   top outline as overlay lines in RoboCAD's temporary-shape colour
//!   (0.4, 0.9, 1.0); the readout ("20 mm × 20 mm × 10 mm", "Ø 10 mm ×
//!   10 mm") in the tool bar. Nothing is sent and no geometry changes.
//! - **Pick-then-form** clicks are `pick`'s (they toggle the item through
//!   the one pick path, `CadMeshes::face_at` and the topology at the shown
//!   revision).
//! - **Cursor snap** (`ops.cursor_snap`, display state only): the snap
//!   under the pointer while it is over the 3D view, as RoboCAD's
//!   `set_pivot` reads `viewport.snap` at the pointer (`snap::snap_on`): a
//!   vertex, midpoint or centre of the drawn bodies or a sketch endpoint or
//!   centre within 12 px (projected onto the active plane while 2D
//!   snapping is on), else the 10 mm grid (on that plane, else z = 0), else
//!   the plane point (no surface hit, and so no ray cast). Recomputed at
//!   most every 33 ms (RoboCAD's hover timer) and only when the pointer,
//!   the view, the candidates or the 2D snapping plane changed; the
//!   candidates are cached by the topology's, meshes' and sketch cache's
//!   epochs, as the measure tool's. It carries the shown revision it was
//!   snapped at and is cleared when that revision moves on, when the
//!   pointer leaves the window, or when the search under the pointer finds
//!   nothing.
use super::{Flow, Primitive, entry};
use crate::app::actions::Act;
use crate::app::{ViewerMode, ViewerSet};
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::cad::mesh::CadMeshes;
use crate::cad::sketch::{CadActivePlane, CadSketches};
use crate::cad::snap::{self, Candidate, GRID_STEP};
use crate::cad::topology::CadTopology;
use crate::cad::transform::{ToolGizmos, cursor_in_view, fl, num, round6, view_back};
use crate::cad::view::{CadView, ray_plane};
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, RequestRedraw};
use serde_json::{Map, Value};
use sim_runtime::cad_client::PlaneFrame;
use std::time::{Duration, Instant};

/// A primitive being placed: the base's first and current points on the
/// plane (mm, RoboCAD's frame), the stage, the height being dragged and
/// the plane it is placed on.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    /// 1: dragging the base; 2: dragging the height.
    pub stage: u8,
    pub p0: [f64; 3],
    pub p1: [f64; 3],
    pub height: f64,
    /// The active plane (or XY) when the placement began.
    pub frame: PlaneFrame,
}

/// RoboCAD's temporary-shape colour for a primitive (tools.py:477-500).
const PREVIEW: Color = Color::srgb(0.4, 0.9, 1.0);
/// RoboCAD's `PrimitiveTool.h` before any height drag (mm).
const START_HEIGHT: f64 = 10.0;
/// Segments of a ring preview (tools.py:488).
const RING: usize = 48;
/// The cursor snap is recomputed at most this often (RoboCAD's hover timer).
const SNAP_PERIOD: Duration = Duration::from_millis(33);

/// The interactions' systems.
pub(in crate::cad) fn build(app: &mut App) {
    app.add_systems(
        Update,
        // After the camera snapshot (this frame's view) and the mesh sync (the drawn bodies the snap reads).
        pointer.after(crate::cad::view::update).after(crate::cad::mesh::sync).in_set(ViewerSet::SimSync).run_if(in_state(ViewerMode::Cad)),
    )
    .add_systems(Update, draw.in_set(ViewerSet::Present).run_if(in_state(ViewerMode::Cad)));
}

/// The pointer's state across frames.
#[derive(Default)]
struct Pointer {
    /// Snap candidates of the drawn bodies and sketches, keyed by the topology's, meshes' and sketches' epochs.
    cache: Option<((u64, u64, u64), Vec<Candidate>)>,
    /// The cursor snap's plane (2D snapping): a change searches again.
    snap_plane: Option<PlaneFrame>,
    /// RoboCAD's `PrimitiveTool.h`: the last dragged height, kept across placements.
    height: Option<f64>,
    /// RoboCAD's revision at the press that began the placement.
    began: u64,
    /// Where and when the cursor snap was last computed.
    snap_at: Option<(Vec2, Instant)>,
    /// The view or the candidates changed since: the cursor snap is computed again.
    snap_stale: bool,
    /// The readout is this module's (cleared when the placement ends).
    readout: bool,
    /// A command surface was open at the end of the last frame's SimSync,
    /// i.e. when this frame's Input saw the press.
    surface_open: bool,
}

fn vec3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

fn arr(p: Vec3) -> [f64; 3] {
    [f64::from(p.x), f64::from(p.y), f64::from(p.z)]
}

/// The op whose placement is active, and its primitive.
fn placing(doc: &CadDocument) -> Option<(&'static str, Primitive)> {
    let id = doc.ops.active?;
    match entry(id)?.flow {
        Flow::Place(kind) => Some((id, kind)),
        _ => None,
    }
}

/// The base rectangle in plane coordinates as RoboCAD's `_preview` draws
/// it: (x0, y0, w, d), w and d signed for the corner box.
fn base_rect(kind: Primitive, a: [f64; 2], b: [f64; 2]) -> (f64, f64, f64, f64) {
    if kind == Primitive::BoxCentre {
        let (w, d) = (2.0 * (b[0] - a[0]).abs(), 2.0 * (b[1] - a[1]).abs());
        (a[0] - w / 2.0, a[1] - d / 2.0, w, d)
    } else {
        (a[0], a[1], b[0] - a[0], b[1] - a[1])
    }
}

/// The plane coordinates (u, v) of a point (`plane.to_local`).
fn uv(frame: &PlaneFrame, p: [f64; 3]) -> [f64; 2] {
    let [u, v, _] = frame.to_local(p);
    [u, v]
}

/// The height shown and finished with: RoboCAD's `self.h` in stage 2, 0 before.
fn shown_height(place: &Place) -> f64 {
    if place.stage == 2 { place.height } else { 0.0 }
}

/// RoboCAD's `_finish` as `CadRun` parameters (the catalogue's names; a
/// point as the anchor field's model "x, y, z" text, sizes in mm, rounded
/// to 1e-6 as the tools send). Box (corner): the corner at the lower u and
/// v and the absolute sizes; box (centre): the first point and twice each
/// half-size; both with the signed height (`_make_box` extrudes |h| along
/// its sign, 1 mm when it is 0). Cylinder: the base on the plane, the
/// diameter (radius at least 1e-3) and the signed height (the axis is
/// flipped for a negative one). Sphere: the first point and its diameter.
/// `args::place` reads the anchors back through the same plane.
pub(crate) fn finish_params(kind: Primitive, place: &Place) -> Map<String, Value> {
    let f = &place.frame;
    let (a, b) = (uv(f, place.p0), uv(f, place.p1));
    let h = shown_height(place);
    let point = |p: [f64; 3]| Value::String(p.map(num).join(", "));
    let size = |v: f64| Value::from(round6(v));
    let radius = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-3);
    let mut m = Map::new();
    match kind {
        Primitive::BoxCorner => {
            let (w, d) = ((b[0] - a[0]).abs(), (b[1] - a[1]).abs());
            m.insert("corner".into(), point(f.to_world(a[0].min(b[0]), a[1].min(b[1]), 0.0)));
            m.insert("width".into(), size(w));
            m.insert("depth".into(), size(d));
            m.insert("height".into(), size(h));
        }
        Primitive::BoxCentre => {
            let (_, _, w, d) = base_rect(kind, a, b);
            m.insert("center".into(), point(f.to_world(a[0], a[1], 0.0)));
            m.insert("width".into(), size(w));
            m.insert("depth".into(), size(d));
            m.insert("height".into(), size(h));
        }
        Primitive::Cylinder => {
            m.insert("base".into(), point(f.to_world(a[0], a[1], 0.0)));
            m.insert("diameter".into(), size(2.0 * radius));
            m.insert("height".into(), size(h));
        }
        Primitive::Sphere => {
            m.insert("center".into(), point(place.p0));
            m.insert("diameter".into(), size(2.0 * radius));
        }
    }
    m
}

/// RoboCAD's readout while placing (`_preview`).
fn readout(kind: Primitive, place: &Place) -> String {
    let (a, b) = (uv(&place.frame, place.p0), uv(&place.frame, place.p1));
    match kind {
        Primitive::BoxCorner | Primitive::BoxCentre => {
            let (_, _, w, d) = base_rect(kind, a, b);
            format!("{} × {} × {}", fl(w.abs()), fl(d.abs()), fl(shown_height(place)))
        }
        Primitive::Cylinder | Primitive::Sphere => {
            let r = (b[0] - a[0]).hypot(b[1] - a[1]);
            let height = if kind == Primitive::Cylinder && place.stage == 2 { format!(" × {}", fl(place.height)) } else { String::new() };
            format!("Ø {}{height}", fl(2.0 * r))
        }
    }
}

/// The preview's line segments (mm, RoboCAD's frame), as `_preview`'s
/// temporary shapes on the plane: the base and top rectangles with the
/// four verticals, or the base ring (and a cylinder's top ring in stage 2).
fn outline(kind: Primitive, place: &Place) -> Vec<(Vec3, Vec3)> {
    let f = &place.frame;
    let (a, b) = (uv(f, place.p0), uv(f, place.p1));
    let at = |u: f64, v: f64, w: f64| vec3(f.to_world(u, v, w));
    let mut out = Vec::new();
    match kind {
        Primitive::BoxCorner | Primitive::BoxCentre => {
            let (x0, y0, w, d) = base_rect(kind, a, b);
            let h = shown_height(place);
            let corners = [(x0, y0), (x0 + w, y0), (x0 + w, y0 + d), (x0, y0 + d)];
            for i in 0..4 {
                let (p, q) = (corners[i], corners[(i + 1) % 4]);
                out.push((at(p.0, p.1, 0.0), at(q.0, q.1, 0.0)));
                out.push((at(p.0, p.1, h), at(q.0, q.1, h)));
                out.push((at(p.0, p.1, 0.0), at(p.0, p.1, h)));
            }
        }
        Primitive::Cylinder | Primitive::Sphere => {
            let r = (b[0] - a[0]).hypot(b[1] - a[1]);
            let ring = |z: f64| -> Vec<Vec3> { (0..=RING).map(|k| std::f64::consts::TAU * k as f64 / RING as f64).map(|t| at(a[0] + r * t.cos(), a[1] + r * t.sin(), z)).collect() };
            let mut rings = vec![ring(0.0)];
            if kind == Primitive::Cylinder && place.stage == 2 {
                rings.push(ring(place.height));
            }
            for points in rings {
                out.extend(points.windows(2).map(|w| (w[0], w[1])));
            }
        }
    }
    out
}

/// RoboCAD's `hover` in stage 2: the height along the plane's normal `n`
/// where the cursor ray meets the plane through `p1` that contains the
/// normal and faces the camera; Ctrl snaps to the grid; never exactly 0.
fn height_at(view: &CadView, cursor: Vec2, p1: Vec3, n: Vec3, back: Vec3, snap_grid: bool) -> Option<f64> {
    let n = n.try_normalize().unwrap_or(Vec3::Z);
    let side = n.cross(back.cross(n));
    let side = if side.length() < 1e-6 { back } else { side };
    let (o, d) = view.ray(cursor)?;
    let hp = ray_plane(o, d, p1, side)?;
    let mut h = f64::from((hp - p1).dot(n));
    if snap_grid {
        let step = f64::from(GRID_STEP);
        h = (h / step).round_ties_even() * step;
    }
    Some(if h.abs() > 1e-6 { h } else { 0.001 })
}

/// The cursor snap at `cursor`: RoboCAD's `viewport.snap` (see the module
/// doc), on the active plane only while 2D snapping is on (`plane`).
fn cursor_snap(view: &CadView, cursor: Vec2, candidates: &[Candidate], plane: Option<&PlaneFrame>) -> Option<[f64; 3]> {
    snap::snap_on(view, cursor, candidates, false, plane).map(|s| arr(s.point))
}

/// The catalogue's name of a placement's anchor parameter.
const ANCHORS: [&str; 3] = ["corner", "center", "base"];

/// The anchor Tab writes: RoboCAD's `commit` anchors at `p0` projected
/// onto the plane; a sphere's centre is the point itself.
fn anchor_text(kind: Primitive, p0: [f64; 3], frame: &PlaneFrame) -> String {
    let p = if kind == Primitive::Sphere { p0 } else { frame.project(p0) };
    p.map(num).join(", ")
}

/// SimSync: the cursor snap, and the placement's presses, drags and height
/// (see the module doc).
#[allow(clippy::too_many_arguments)]
fn pointer(
    doc: Option<ResMut<CadDocument>>,
    view: Option<Res<CadView>>,
    topology: Option<Res<CadTopology>>,
    meshes: Option<Res<CadMeshes>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    hover: Option<Res<HoverMap>>,
    nodes: Query<(), With<Node>>,
    (plane, sketches): (Option<Res<CadActivePlane>>, Option<Res<CadSketches>>),
    mut out: MessageWriter<Act<CadAction>>,
    mut redraw: MessageWriter<RequestRedraw>,
    mut state: Local<Pointer>,
) {
    let (Some(mut doc), Some(view)) = (doc, view) else { return };
    let state = &mut *state;
    let plane = plane.as_deref();
    // The surface as this frame's Input saw it: an outside press closed it in Actions, before this system.
    let surface_was_open = std::mem::replace(&mut state.surface_open, doc.ops.surface.is_some());
    // The candidates change only with the topology, the drawn bodies or the sketches.
    let key = snap::candidates_key(topology.as_deref(), meshes.as_deref(), sketches.as_deref());
    if state.cache.as_ref().is_none_or(|(k, _)| *k != key) {
        state.cache = Some((key, snap::drawn_candidates(&doc, topology.as_deref(), meshes.as_deref(), sketches.as_deref())));
        state.snap_stale = true;
    }
    // The cursor snap's plane: the active plane while 2D snapping is on.
    let on_plane = snap::snap_plane(plane);
    if view.is_changed() || state.snap_plane != on_plane {
        state.snap_plane = on_plane;
        state.snap_stale = true;
    }
    let candidates: &[Candidate] = match state.cache.as_ref() {
        Some((_, c)) => c,
        None => &[],
    };
    let cursor = if view.valid { cursor_in_view(windows.single().ok(), &view, hover.as_deref(), &nodes) } else { None };

    // The cursor snap is never sent stale: one snapped at an older shown
    // revision, or kept after the pointer left the window (or the view is
    // not drawn), is cleared and searched again once the pointer is back
    // over the view. Over a panel inside the window (a menu, the palette)
    // it is kept: a menu entry or key invoking "Set pivot at cursor snap"
    // reads the point the pointer last had over the view.
    let shown = doc.shown_revision();
    let off_window = windows.single().ok().is_none_or(|w| w.cursor_position().is_none());
    if doc.ops.cursor_snap.is_some_and(|(at, _)| at != shown || off_window || !view.valid) {
        doc.ops.cursor_snap = None;
        state.snap_stale = true;
    }
    // "Set pivot at cursor snap": coalesced to one search per 33 ms.
    if let Some(c) = cursor
        && (state.snap_stale || state.snap_at.is_none_or(|(at, _)| at != c))
    {
        if state.snap_at.is_some_and(|(_, t)| t.elapsed() < SNAP_PERIOD) {
            redraw.write(RequestRedraw);
        } else {
            state.snap_at = Some((c, Instant::now()));
            state.snap_stale = false;
            // Nothing snapped here: the last point is not this cursor's.
            let point = cursor_snap(&view, c, candidates, on_plane.as_ref()).map(|p| (shown, p));
            if doc.ops.cursor_snap != point {
                doc.ops.cursor_snap = point;
            }
        }
    }

    // Placing a primitive.
    let Some((id, kind)) = placing(&doc) else {
        // The op ended elsewhere (cancelled, another op): its placement and readout go.
        if doc.ops.place.is_some() {
            doc.ops.place = None;
        }
        if std::mem::take(&mut state.readout) && doc.tool_state.readout.is_some() {
            doc.tool_state.readout = None;
        }
        return;
    };
    // RoboCAD's `ctx.active_plane()`: the active plane, else XY (a plane node still being read: refused).
    let frame = plane.map_or(Ok(PlaneFrame::XY), CadActivePlane::frame_or_xy);
    let held = |codes: &[KeyCode]| keys.as_ref().is_some_and(|k| k.any_pressed(codes.iter().copied()));
    let alt = held(&[KeyCode::AltLeft, KeyCode::AltRight]);
    let ctrl = held(&[KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]);
    let tab = keys.as_ref().is_some_and(|k| k.just_pressed(KeyCode::Tab));
    // A press while a command surface is open only closes it (as `pick`).
    let pressed = buttons.as_ref().is_some_and(|b| b.just_pressed(MouseButton::Left)) && !surface_was_open && doc.ops.surface.is_none();
    let down = buttons.as_ref().is_some_and(|b| b.pressed(MouseButton::Left));
    // RoboCAD's `ctx.snap(pos, suppress, plane=active_plane)`: on the placement's plane.
    let snapped = |c: Vec2, f: &PlaneFrame| snap::snap_on(&view, c, candidates, alt, Some(f)).map(|s| arr(s.point));

    let before = doc.ops.place.clone();
    let mut place = before.clone();
    // The active plane changed during the placement: it ends with nothing sent.
    if place.as_ref().is_some_and(|p| frame.as_ref().is_ok_and(|f| !f.same(&p.frame, 1e-9)) || frame.is_err()) {
        place = None;
        doc.show(Ok("The active plane changed: the placement ended (nothing was sent)".to_string()));
    }
    let mut finish = false;
    match place.as_mut() {
        None => {
            // RoboCAD's `press` in stage 0: the base begins at the snap.
            if pressed && cursor.is_some() {
                match &frame {
                    Ok(f) => {
                        if let Some(p) = cursor.and_then(|c| snapped(c, f)) {
                            place = Some(Place { stage: 1, p0: p, p1: p, height: state.height.unwrap_or(START_HEIGHT), frame: *f });
                            state.began = doc.shown_revision();
                        }
                    }
                    Err(e) => doc.show(Err(e.clone())),
                }
            }
        }
        Some(p) if p.stage == 1 => {
            if down {
                // RoboCAD's `drag`: the second corner follows the snap (kept while over a panel).
                let f = p.frame;
                if let Some(q) = cursor.and_then(|c| snapped(c, &f)) {
                    p.p1 = q;
                }
            } else if kind == Primitive::Sphere {
                // RoboCAD's `release`: a sphere is finished at once.
                finish = true;
            } else if vec3(p.p0).distance(vec3(p.p1)) >= 1e-6 {
                p.stage = 2;
            }
            // A zero-size base stays in stage 1 (RoboCAD returns), waiting for a drag.
        }
        Some(p) => {
            if pressed && cursor.is_some() {
                // RoboCAD's `press` in stage 2.
                finish = true;
            } else if let Some(h) = cursor.and_then(|c| height_at(&view, c, vec3(p.p1), vec3(p.frame.normal), view_back(&view), ctrl)) {
                p.height = h;
                state.height = Some(h);
            }
        }
    }
    // Tab during a drag: the form's anchor (by name) is the base's first point (RoboCAD's `commit` anchors at p0).
    let anchor = place.as_ref().filter(|_| tab).map(|p| anchor_text(kind, p.p0, &p.frame));
    let slot = entry(id).and_then(|e| e.params.iter().position(|p| ANCHORS.contains(&p.name)));
    if let (Some(text), Some(i)) = (anchor, slot)
        && doc.ops.form.as_ref().is_some_and(|f| f.op == id && f.texts.get(i).is_some_and(|t| *t != text))
        && let Some(form) = doc.ops.form.as_mut()
    {
        form.texts[i] = text;
    }
    if finish && let Some(p) = &place {
        out.write(Act::ui(CadAction::CadRun { id: id.to_string(), params: finish_params(kind, p), items: None, revision: Some(state.began) }));
        place = None;
    }
    let text = place.as_ref().map(|p| readout(kind, p));
    if place != before {
        doc.ops.place = place;
    }
    match text {
        Some(r) => {
            if doc.tool_state.readout.as_deref() != Some(r.as_str()) {
                doc.tool_state.readout = Some(r);
            }
            state.readout = true;
        }
        None => {
            if std::mem::take(&mut state.readout) && doc.tool_state.readout.is_some() {
                doc.tool_state.readout = None;
            }
        }
    }
}

/// Present: the placement's preview lines (display only).
fn draw(doc: Option<Res<CadDocument>>, view: Option<Res<CadView>>, mut gizmos: Gizmos<ToolGizmos>) {
    let (Some(doc), Some(view)) = (doc, view) else { return };
    let (Some(place), Some((_, kind))) = (&doc.ops.place, placing(&doc)) else { return };
    if !view.valid {
        return;
    }
    let w = |p: Vec3| view.world_from_model.transform_point3(p);
    for (a, b) in outline(kind, place) {
        gizmos.line(w(a), w(b), PREVIEW);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(stage: u8, p0: [f64; 3], p1: [f64; 3], height: f64) -> Place {
        Place { stage, p0, p1, height, frame: PlaneFrame::XY }
    }

    /// `_finish`'s sizes: the corner box from its lower corner with
    /// absolute sizes, the centre box twice each half-size, the cylinder's
    /// base on the plane with its signed height, the sphere at the first point.
    #[test]
    fn finishing_sends_the_sizes_robocad_finishes_with() {
        let p = place(2, [10.0, 20.0, 0.0], [-10.0, 25.0, 0.0], 15.0);
        let m = finish_params(Primitive::BoxCorner, &p);
        assert_eq!(Value::Object(m), serde_json::json!({"corner": "-10, 20, 0", "width": 20.0, "depth": 5.0, "height": 15.0}));
        let m = finish_params(Primitive::BoxCentre, &p);
        assert_eq!(Value::Object(m), serde_json::json!({"center": "10, 20, 0", "width": 40.0, "depth": 10.0, "height": 15.0}));
        let c = place(2, [0.0, 0.0, 5.0], [3.0, 4.0, 5.0], -8.0);
        assert_eq!(Value::Object(finish_params(Primitive::Cylinder, &c)), serde_json::json!({"base": "0, 0, 0", "diameter": 10.0, "height": -8.0}));
        // A sphere finishes from stage 1 (RoboCAD's `release`); a click gives the 1e-3 mm minimum radius.
        let s = place(1, [1.0, 2.0, 3.0], [1.0, 2.0, 3.0], 10.0);
        assert_eq!(Value::Object(finish_params(Primitive::Sphere, &s)), serde_json::json!({"center": "1, 2, 3", "diameter": 0.002}));
        // Stage 1 shows (and would finish) a flat base.
        assert_eq!(readout(Primitive::BoxCorner, &place(1, [0.0; 3], [20.0, 20.0, 0.0], 10.0)), "20 mm × 20 mm × 0 mm");
        assert_eq!(readout(Primitive::Cylinder, &place(2, [0.0; 3], [5.0, 0.0, 0.0], 10.0)), "Ø 10 mm × 10 mm");
        assert_eq!(readout(Primitive::Sphere, &place(1, [0.0; 3], [5.0, 0.0, 0.0], 10.0)), "Ø 10 mm");
    }

    /// The preview: a box's base, top and verticals (12 segments); a ring of 48.
    #[test]
    fn the_preview_outlines_the_base_and_the_top() {
        let b = outline(Primitive::BoxCorner, &place(2, [0.0; 3], [20.0, 10.0, 0.0], 5.0));
        assert_eq!(b.len(), 12);
        assert!(b.contains(&(Vec3::new(20.0, 0.0, 5.0), Vec3::new(20.0, 10.0, 5.0))));
        let c = outline(Primitive::Cylinder, &place(2, [0.0; 3], [5.0, 0.0, 0.0], 5.0));
        assert_eq!(c.len(), 2 * RING);
        assert_eq!(outline(Primitive::Cylinder, &place(1, [0.0; 3], [5.0, 0.0, 0.0], 5.0)).len(), RING);
    }

    /// RoboCAD's `hover`: the height where the cursor ray meets the plane
    /// facing the camera through p1; Ctrl rounds to 10 mm, half to even.
    #[test]
    fn the_height_follows_the_cursor_along_the_normal() {
        // Looking along +Y (model), Z up on screen: a front view.
        let view = front_view();
        let p1 = Vec3::new(0.0, 0.0, 0.0);
        let back = view_back(&view);
        let over = view.project(Vec3::new(0.0, 0.0, 24.0)).unwrap();
        let h = height_at(&view, over, p1, Vec3::Z, back, false).unwrap();
        assert!((h - 24.0).abs() < 0.05, "{h}");
        assert_eq!(height_at(&view, over, p1, Vec3::Z, back, true), Some(20.0));
        let at_zero = view.project(Vec3::ZERO).unwrap();
        assert_eq!(height_at(&view, at_zero, p1, Vec3::Z, back, true), Some(0.001));
        // On the YZ plane the height runs along +X: a point 15 mm along X, seen from the front, is 15 up the normal.
        let side = view.project(Vec3::new(15.0, 0.0, 0.0)).unwrap();
        let h = height_at(&view, side, p1, Vec3::X, back, false).unwrap();
        assert!((h - 15.0).abs() < 0.05, "{h}");
    }

    /// On the XZ plane (u = x, v = z, normal −Y) the base, the anchors and
    /// the preview lie on the plane and the height runs along −Y.
    #[test]
    fn placing_on_another_plane_uses_its_coordinates() {
        let p = Place { stage: 2, p0: [10.0, 0.0, 20.0], p1: [-10.0, 0.0, 25.0], height: 15.0, frame: PlaneFrame::XZ };
        assert_eq!(Value::Object(finish_params(Primitive::BoxCorner, &p)), serde_json::json!({"corner": "-10, 0, 20", "width": 20.0, "depth": 5.0, "height": 15.0}));
        assert_eq!(Value::Object(finish_params(Primitive::Cylinder, &p))["base"], "10, 0, 20");
        let lines = outline(Primitive::BoxCorner, &p);
        assert!(lines.contains(&(Vec3::new(10.0, 0.0, 20.0), Vec3::new(10.0, -15.0, 20.0))), "a vertical runs along the normal: {lines:?}");
        assert_eq!(anchor_text(Primitive::BoxCorner, [1.0, 7.0, 2.0], &PlaneFrame::XZ), "1, 0, 2");
        assert_eq!(anchor_text(Primitive::Sphere, [1.0, 7.0, 2.0], &PlaneFrame::XZ), "1, 7, 2");
    }

    fn front_view() -> CadView {
        use bevy::camera::CameraProjection;
        use bevy::math::Affine3A;
        let projection = PerspectiveProjection { aspect_ratio: 2.0, near: 0.001, ..default() };
        let clip_from_view = projection.get_clip_from_view();
        // Model (x, y, z) mm is Bevy (x, z, −y) m: a camera 200 mm in front (model y = −200)
        // looks along Bevy −Z, which is model +Y, with model Z up on screen.
        let world_from_view = Affine3A::from_translation(Vec3::new(0.0, 0.0, 0.2));
        let root = crate::cad::mesh::root_transform();
        let world_from_model = Affine3A::from_scale_rotation_translation(root.scale, root.rotation, root.translation);
        CadView {
            valid: true,
            world_from_model,
            model_from_world: world_from_model.inverse(),
            view_from_world: world_from_view.inverse(),
            world_from_view,
            clip_from_view,
            view_from_clip: clip_from_view.inverse(),
            min: Vec2::new(10.0, 20.0),
            size: Vec2::new(200.0, 100.0),
        }
    }
}

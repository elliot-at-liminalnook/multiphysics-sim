//! The move, rotate and uniform-scale gizmo: RoboCAD's handles
//! (ui/viewport.py:1060-1132: three 90 px axes or rings and a centre
//! handle) and drag maths (`TransformTool`, ui/tools.py:234-385), on the
//! view snapshot (`CadView`). Why not Bevy's transform gizmo: see the
//! parent module's decisions.
//!
//! A left press on a handle (centre within 10 px, else the nearest axis or
//! ring within 14 px) starts a drag at the pivot. Each frame the cursor ray
//! meets the drag plane (an axis: the plane through the pivot containing
//! the axis and facing the camera; a ring: the plane normal to its axis;
//! the centre handle: the screen plane) and the change is computed as
//! RoboCAD does (Ctrl: 10 mm, 15°, ×0.1); the selected bodies show it
//! (`Preview`, display only). The release writes one `CadTransform` with
//! the revision the drag began at.
use super::{AXES, AXIS_COLOURS, CENTRE_COLOUR, Delta, HOT, Phase, Preview, ToolGizmos, cursor_in_view, fa, fl, is_transform, marker, mm, pivot, preview_bodies, r6, view_back};
use crate::app::actions::Act;
use crate::cad::actions::CadAction;
use crate::cad::document::{CadDocument, CadTool};
use crate::cad::measure::segment_distance;
use crate::cad::mesh::CadMeshes;
use crate::cad::snap::GRID_STEP;
use crate::cad::view::{CadView, ray_plane};
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// The handles' length on screen (RoboCAD: `world_per_pixel * 90`).
pub const HANDLE_PIXELS: f32 = 90.0;
/// The centre handle's pick radius (px).
pub const CENTRE_PIXELS: f32 = 10.0;
/// An axis or ring's pick distance (px).
pub const AXIS_PIXELS: f32 = 14.0;
/// Points per ring (drawn and hit-tested).
pub const RING_SAMPLES: usize = 48;
/// The handle index of the centre (screen-space) handle.
pub const CENTRE: usize = 3;

/// A gizmo drag in progress.
#[derive(Clone, Debug, PartialEq)]
pub struct Drag {
    pub tool: CadTool,
    /// 0, 1, 2 the axes, [`CENTRE`] the centre handle.
    pub handle: usize,
    /// The pivot (mm).
    pub origin: Vec3,
    /// The axis dragged along or about (unit); the view axis for the centre handle.
    pub axis: Vec3,
    /// Where the drag began on its plane (mm).
    pub start: Vec3,
    /// RoboCAD's revision the drag began at.
    pub began: u64,
    /// The nodes transformed and the drawn nodes that move with them.
    pub ids: Vec<String>,
    pub bodies: Vec<String>,
    /// The change so far.
    pub delta: Option<Delta>,
}

/// RoboCAD's ring basis (viewport.py:1074-1077): helper Z unless the axis is near Z.
pub fn ring_basis(axis: Vec3) -> (Vec3, Vec3) {
    let a = axis.try_normalize().unwrap_or(Vec3::Z);
    let helper = if a.z.abs() < 0.9 { Vec3::Z } else { Vec3::X };
    let u = helper.cross(a).try_normalize().unwrap_or(Vec3::X);
    (u, a.cross(u))
}

/// Ring sample `k` of [`RING_SAMPLES`] around `axis` at `origin`, radius `size`.
pub fn ring_point(origin: Vec3, axis: Vec3, size: f32, k: usize) -> Vec3 {
    let (u, v) = ring_basis(axis);
    let t = std::f32::consts::TAU * k as f32 / RING_SAMPLES as f32;
    origin + u * (size * t.cos()) + v * (size * t.sin())
}

/// The handles' length (mm) at the pivot.
pub fn handle_size(view: &CadView, origin: Vec3) -> Option<f32> {
    Some(view.mm_per_pixel(origin)? * HANDLE_PIXELS)
}

/// RoboCAD's `gizmo_hit_test`: the handle under `cursor`.
pub fn hit_test(view: &CadView, tool: CadTool, origin: Vec3, cursor: Vec2) -> Option<usize> {
    let size = handle_size(view, origin)?;
    let po = view.project(origin);
    if po.is_some_and(|p| p.distance(cursor) < CENTRE_PIXELS) {
        return Some(CENTRE);
    }
    let mut best = None;
    let mut best_d = AXIS_PIXELS;
    for (i, a) in AXES.iter().enumerate() {
        if tool == CadTool::Rotate {
            for k in 0..RING_SAMPLES {
                let Some(p) = view.project(ring_point(origin, *a, size, k)) else { continue };
                let d = p.distance(cursor);
                if d < best_d {
                    best = Some(i);
                    best_d = d;
                }
            }
        } else if let (Some(po), Some(tip)) = (po, view.project(origin + *a * size)) {
            let d = segment_distance(cursor, po, tip);
            if d < best_d {
                best = Some(i);
                best_d = d;
            }
        }
    }
    best
}

/// The normal of the plane a drag runs on (`TransformTool._drag_point`):
/// the screen plane for the centre handle, the ring's plane for a
/// rotation, else the plane containing the axis that faces the camera.
pub fn drag_normal(tool: CadTool, handle: usize, axis: Vec3, back: Vec3) -> Vec3 {
    if handle == CENTRE {
        return back;
    }
    if tool == CadTool::Rotate {
        return axis;
    }
    let n = axis.cross(back.cross(axis));
    if n.length() < 1e-6 { back } else { n }
}

/// Where the cursor ray meets the drag's plane (mm).
pub fn drag_point(view: &CadView, cursor: Vec2, tool: CadTool, handle: usize, origin: Vec3, axis: Vec3, back: Vec3) -> Option<Vec3> {
    let (o, d) = view.ray(cursor)?;
    ray_plane(o, d, origin, drag_normal(tool, handle, axis, back))
}

/// RoboCAD's move: the drag along `axis` (Ctrl: whole grid steps), or the
/// free screen-plane drag when `axis` is None (never snapped, as RoboCAD).
pub fn move_delta(start: Vec3, p: Vec3, axis: Option<Vec3>, snap: bool) -> Vec3 {
    let delta = p - start;
    let Some(a) = axis.and_then(Vec3::try_normalize) else { return delta };
    let mut d = delta.dot(a);
    if snap {
        d = (d / GRID_STEP).round_ties_even() * GRID_STEP;
    }
    a * d
}

/// RoboCAD's rotation angle (degrees, right-handed about `axis`; Ctrl: 15°
/// steps); None while either arm is too short to give a direction.
pub fn rotate_angle(origin: Vec3, start: Vec3, p: Vec3, axis: Vec3, snap: bool) -> Option<f32> {
    let a = axis.try_normalize()?;
    let flat = |v: Vec3| v - a * v.dot(a);
    let (v0, v1) = (flat(start - origin), flat(p - origin));
    if v0.length() <= 1e-9 || v1.length() <= 1e-9 {
        return None;
    }
    let mut angle = v0.cross(v1).dot(a).atan2(v0.dot(v1)).to_degrees();
    if snap {
        angle = (angle / 15.0).round_ties_even() * 15.0;
    }
    Some(angle)
}

/// RoboCAD's uniform scale: the ratio of the cursor's and the press's
/// distances from the pivot (Ctrl: 0.1 steps), at least 0.01.
pub fn scale_factor(origin: Vec3, start: Vec3, p: Vec3, snap: bool) -> f32 {
    let d0 = start.distance(origin);
    let d0 = if d0 == 0.0 { 1.0 } else { d0 };
    let mut f = p.distance(origin) / d0;
    if snap {
        f = (f * 10.0).round_ties_even() / 10.0;
    }
    f.max(0.01)
}

/// The change at cursor point `p` and its readout (RoboCAD's `ctx.readout`).
pub fn drag_delta(drag: &Drag, p: Vec3, snap: bool) -> Option<(Delta, String)> {
    match drag.tool {
        CadTool::Move => {
            let free = drag.handle == CENTRE;
            let d = move_delta(drag.start, p, (!free).then_some(drag.axis), snap);
            let readout = format!("Δ = ({}, {}, {})  |{:.3} mm|", fl(f64::from(d.x)), fl(f64::from(d.y)), fl(f64::from(d.z)), d.length());
            Some((Delta::Translate(d), readout))
        }
        CadTool::Rotate => {
            let angle = rotate_angle(drag.origin, drag.start, p, drag.axis, snap)?;
            Some((Delta::Rotate { axis: drag.axis, angle_deg: angle, center: drag.origin }, format!("angle = {}", fa(f64::from(angle)))))
        }
        CadTool::Scale => {
            let f = scale_factor(drag.origin, drag.start, p, snap);
            Some((Delta::Scale { factor: f, center: drag.origin }, format!("scale = ×{f:.3}")))
        }
        _ => None,
    }
}

/// The commit a release writes: one `CadTransform` of the drag's nodes,
/// with the revision it began at (RoboCAD's `TransformTool._apply`).
pub fn drag_action(drag: &Drag, delta: &Delta) -> CadAction {
    let (mut translation, mut axis, mut angle_deg, mut center, mut scale) = (None, None, None, None, None);
    match *delta {
        Delta::Translate(t) => translation = Some(mm(t)),
        Delta::Rotate { axis: a, angle_deg: angle, center: c } => {
            axis = Some(mm(a));
            angle_deg = Some(r6(angle));
            center = Some(mm(c));
        }
        Delta::Scale { factor, center: c } => {
            scale = Some(r6(factor));
            center = Some(mm(c));
        }
    }
    CadAction::CadTransform { ids: Some(drag.ids.clone()), translation, axis, angle_deg, center, scale, revision: Some(drag.began) }
}

/// SimSync: the gizmo's hover, drags and releases (move, rotate, scale tools).
#[allow(clippy::too_many_arguments)]
pub(super) fn drag(
    doc: Option<ResMut<CadDocument>>,
    view: Option<Res<CadView>>,
    meshes: Option<Res<CadMeshes>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    hover: Option<Res<HoverMap>>,
    nodes: Query<(), With<Node>>,
    mut out: MessageWriter<Act<CadAction>>,
    selection: crate::cad::selection::CadSelection,
) {
    let (Some(mut doc), Some(view)) = (doc, view) else { return };
    if !is_transform(doc.tool) {
        return;
    }
    let snap = keys.as_ref().is_some_and(|k| k.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]));
    let held = buttons.as_ref().is_some_and(|b| b.pressed(MouseButton::Left));
    let pressed = buttons.as_ref().is_some_and(|b| b.just_pressed(MouseButton::Left));
    let window = windows.single().ok();
    let back = view_back(&view);

    if let Some(mut drag) = doc.tool_state.drag.clone() {
        // The document changed under the drag: what it previews is gone.
        if doc.health.as_ref().map(|h| h.revision) != Some(drag.began) || doc.shown_revision() != drag.began {
            doc.tool_state.drag = None;
            doc.tool_state.preview = None;
            doc.show(Err("The document changed during the drag (another edit, or RoboCAD's own window); nothing was sent: drag again".into()));
            return;
        }
        if held {
            let cursor = window.and_then(Window::cursor_position);
            let Some(p) = cursor.and_then(|c| drag_point(&view, c, drag.tool, drag.handle, drag.origin, drag.axis, back)) else { return };
            let Some((delta, readout)) = drag_delta(&drag, p, snap) else { return };
            if drag.delta != Some(delta) {
                drag.delta = Some(delta);
                let generation = doc.generation;
                doc.tool_state.preview = Some(Preview { generation, bodies: drag.bodies.clone(), delta, began: drag.began, phase: Phase::Live });
                doc.tool_state.readout = Some(readout);
                doc.tool_state.drag = Some(drag);
            }
            return;
        }
        // Released (also when the button went up outside the window).
        doc.tool_state.drag = None;
        match drag.delta.filter(Delta::significant) {
            Some(delta) => {
                let action = drag_action(&drag, &delta);
                if let Some(p) = doc.tool_state.preview.as_mut() {
                    p.phase = Phase::Released { action: action.clone(), at: std::time::Instant::now() };
                }
                out.write(Act::ui(action));
            }
            None => doc.tool_state.preview = None,
        }
        return;
    }

    // Not dragging: the pivot follows the selection; the handle under the pointer lights.
    let waiting = doc.tool_state.preview.is_some();
    let selection = selection.items();
    let p = pivot(&doc, &selection, meshes.as_deref());
    if doc.tool_state.pivot != p {
        doc.tool_state.pivot = p;
    }
    let cursor = cursor_in_view(window, &view, hover.as_deref(), &nodes);
    let hovered = match (p, cursor) {
        (Some((origin, _)), Some(c)) if !waiting => hit_test(&view, doc.tool, origin, c),
        _ => None,
    };
    if doc.tool_state.hover != hovered {
        doc.tool_state.hover = hovered;
    }
    let (true, Some(handle), Some(c), Some((origin, _))) = (pressed, hovered, cursor, p) else { return };
    let began = doc.shown_revision();
    if doc.health.as_ref().map(|h| h.revision) != Some(began) {
        doc.show(Err("RoboCAD's document is being refetched; drag again in a moment".into()));
        return;
    }
    let axis = if handle == CENTRE { back } else { AXES[handle] };
    let Some(start) = drag_point(&view, c, doc.tool, handle, origin, axis, back) else { return };
    // RoboCAD's Ops.transform skips locked nodes: previewing them would show a move that never lands.
    let locked = |id: &String| doc.doc.as_ref().and_then(|d| d.nodes.iter().find(|n| &n.id == id)).is_some_and(|n| n.locked);
    let ids: Vec<String> = crate::cad::selection::CadItems::nodes(selection.as_slice()).into_iter().filter(|id| !locked(id)).collect();
    if ids.is_empty() {
        doc.show(Err("Every selected node is locked; RoboCAD would not move it (unlock it first)".into()));
        return;
    }
    let bodies = preview_bodies(&doc, &ids);
    let tool = doc.tool;
    doc.tool_state.axis = Some(handle);
    doc.tool_state.free_axis = (handle == CENTRE).then_some(back);
    doc.tool_state.drag = Some(Drag { tool, handle, origin, axis, start, began, ids, bodies, delta: None });
}

/// Present: the gizmo at the pivot (hidden while a commit waits to land),
/// the lit handle, and the move's line from the pivot (RoboCAD's temp shape).
pub(super) fn draw(doc: Option<Res<CadDocument>>, view: Option<Res<CadView>>, mut gizmos: Gizmos<ToolGizmos>) {
    let (Some(doc), Some(view)) = (doc, view) else { return };
    if !is_transform(doc.tool) || !view.valid {
        return;
    }
    let s = &doc.tool_state;
    if s.preview.as_ref().is_some_and(|p| p.phase != Phase::Live) {
        return;
    }
    let Some(origin) = s.drag.as_ref().map(|d| d.origin).or(s.pivot.map(|(p, _)| p)) else { return };
    let Some(size) = handle_size(&view, origin) else { return };
    let w = |p: Vec3| view.world_from_model.transform_point3(p);
    let lit = s.drag.as_ref().map(|d| d.handle).or(s.hover);
    for (i, axis) in AXES.iter().enumerate() {
        let colour = if lit == Some(i) { HOT } else { AXIS_COLOURS[i] };
        if doc.tool == CadTool::Rotate {
            gizmos.linestrip((0..=RING_SAMPLES).map(|k| w(ring_point(origin, *axis, size, k % RING_SAMPLES))), colour);
        } else {
            let tip = origin + *axis * size;
            gizmos.line(w(origin), w(tip), colour);
            marker(&mut gizmos, &view, tip, if doc.tool == CadTool::Scale { 10.0 } else { 7.0 }, colour);
        }
    }
    marker(&mut gizmos, &view, origin, 9.0, if lit == Some(CENTRE) { HOT } else { CENTRE_COLOUR });
    if let Some(Delta::Translate(t)) = s.drag.as_ref().and_then(|d| d.delta) {
        gizmos.line(w(origin), w(origin + t), HOT);
    }
}

//! Push/pull (D) and offset face (Shift+D): RoboCAD's `PushPullTool`
//! (ui/tools.py:547-632). A left press on a face in the 3D view targets it
//! (Bevy's `MeshRayCast` on the drawn bodies, the hit triangle mapped to
//! RoboCAD's face by `CadMeshes::face_at` at the shown revision; nothing
//! while the mesh lags it) and selects it (`CadSelect`, as
//! RoboCAD clears the selection and adds the face). Dragging moves along
//! the face normal on the plane through the press point that contains the
//! normal and faces the camera (Ctrl: whole 10 mm steps). The preview is
//! display only: a line from the face centroid along the normal and the
//! face's outline shifted by the distance. The release (|d| > 1e-6) writes
//! `CadOffsetFaces {faces: [f]}` when Shift is held, the tool is offset
//! face, or the face is not planar, else `CadPushPull`; both carry the
//! revision the press began at.
use super::{HOT, ToolGizmos, cursor_in_view, fl, ray_hit, view_back};
use crate::app::actions::Act;
use crate::cad::actions::CadAction;
use crate::cad::document::{CadDocument, CadTool};
use crate::cad::mesh::{CadBody, CadMeshes};
use crate::cad::snap::GRID_STEP;
use crate::cad::topology::{CadTopology, NodeTopology};
use crate::cad::view::{CadView, ray_plane};
use bevy::picking::hover::HoverMap;
use bevy::picking::mesh_picking::ray_cast::MeshRayCast;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use crate::cad::types::{FaceInfo, SelectionItem};

/// The face push/pull or offset acts on: its index at `revision`, and the
/// face as RoboCAD described it then (to find it again after an edit
/// renumbers the faces, as RoboCAD's `find_face` does after each edit).
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub node: String,
    pub face: i64,
    /// The shown revision `face` indexes.
    pub revision: u64,
    /// The face at `revision` (None until its topology is loaded).
    pub info: Option<FaceInfo>,
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn unit(a: [f64; 3]) -> [f64; 3] {
    let n = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if n > 0.0 { [a[0] / n, a[1] / n, a[2] / n] } else { a }
}

/// RoboCAD's `match_face` (kernel/base.py:457-472): the candidate nearest
/// `reference` by centroid distance plus 0.05 × the relative area
/// difference, plus 1e6 for another surface kind and 0.5 × (1 − n·n') when
/// both have normals; the first of equal scores. A face without a centroid
/// is not a candidate (RoboCAD's faces always have one).
pub fn match_face<'a>(candidates: &'a [FaceInfo], reference: &FaceInfo) -> Option<&'a FaceInfo> {
    let rc = reference.centroid?;
    let ra = reference.area.unwrap_or(0.0);
    candidates
        .iter()
        .filter_map(|c| {
            let mut score = dist(c.centroid?, rc) + 0.05 * (c.area.unwrap_or(0.0) - ra).abs() / ra.max(1e-6);
            if c.kind != reference.kind {
                score += 1e6;
            }
            if let (Some(rn), Some(cn)) = (reference.normal, c.normal) {
                let (a, b) = (unit(cn), unit(rn));
                score += 0.5 * (1.0 - (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]));
            }
            Some((score, c))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, c)| c)
}

/// The target at the shown revision `shown`: itself when current, else
/// matched in `topology` (the node's faces at `shown`); refused by name
/// when it cannot be found again.
pub fn resolve(target: &Target, shown: u64, topology: Option<&NodeTopology>, name: &str) -> Result<Target, String> {
    if target.revision == shown {
        return Ok(target.clone());
    }
    let stale = format!("face {} of {name} was targeted at revision {}, and the document is now at revision {shown}", target.face, target.revision);
    let Some(reference) = &target.info else { return Err(format!("{stale}: press the face again (or select it) to target it")) };
    let Some(t) = topology.filter(|t| t.revision == shown) else { return Err(format!("{stale}; its faces are not loaded yet: try again in a moment")) };
    let Some(m) = match_face(&t.faces, reference) else { return Err(format!("{stale}, and it has no faces now")) };
    Ok(Target { node: target.node.clone(), face: m.index, revision: shown, info: Some(m.clone()) })
}

/// A push/pull drag in progress (mm, RoboCAD's frame).
#[derive(Clone, Debug, PartialEq)]
pub struct PushDrag {
    pub node: String,
    pub face: i64,
    /// RoboCAD's surface kind of the face (plane, cylinder, …).
    pub kind: String,
    pub start: Vec3,
    pub normal: Vec3,
    pub centroid: Vec3,
    /// RoboCAD's revision the press began at.
    pub began: u64,
    pub distance: f32,
    /// The face's outline from the drawn tessellation.
    pub outline: Vec<(Vec3, Vec3)>,
}

/// The distance along `normal` at the cursor (`PushPullTool.drag`): the
/// plane through `start` containing the normal and facing the camera.
pub fn push_distance(view: &CadView, cursor: Vec2, start: Vec3, normal: Vec3, back: Vec3, snap: bool) -> Option<f32> {
    let n = normal.try_normalize()?;
    let side = n.cross(back.cross(n));
    let side = if side.length() < 1e-6 { back } else { side };
    let (o, d) = view.ray(cursor)?;
    let p = ray_plane(o, d, start, side)?;
    let mut distance = (p - start).dot(n);
    if snap {
        distance = (distance / GRID_STEP).round_ties_even() * GRID_STEP;
    }
    Some(distance)
}

/// The commit a release writes (`PushPullTool.release` / `_apply`).
pub fn release_action(drag: &PushDrag, offset_tool: bool, shift: bool) -> Option<CadAction> {
    if drag.distance.abs() <= 1e-6 {
        return None;
    }
    let distance = super::r6(drag.distance);
    let revision = Some(drag.began);
    Some(if shift || offset_tool || drag.kind != "plane" {
        CadAction::CadOffsetFaces { node: drag.node.clone(), faces: vec![drag.face], distance, revision }
    } else {
        CadAction::CadPushPull { node: drag.node.clone(), face: drag.face, distance, revision }
    })
}

fn vec3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

/// SimSync: push/pull and offset presses, drags and releases.
#[allow(clippy::too_many_arguments)]
pub(super) fn tool(
    doc: Option<ResMut<CadDocument>>,
    view: Option<Res<CadView>>,
    topology: Option<Res<CadTopology>>,
    meshes: Option<Res<CadMeshes>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    hover: Option<Res<HoverMap>>,
    nodes: Query<(), With<Node>>,
    mut cast: MeshRayCast,
    bodies: Query<&CadBody>,
    mut out: MessageWriter<Act<CadAction>>,
) {
    let (Some(mut doc), Some(view), Some(topology), Some(meshes)) = (doc, view, topology, meshes) else { return };
    if !matches!(doc.tool, CadTool::PushPull | CadTool::OffsetFace) {
        return;
    }
    let held_key = |pair: [KeyCode; 2]| keys.as_ref().is_some_and(|k| k.any_pressed(pair));
    let snap = held_key([KeyCode::ControlLeft, KeyCode::ControlRight]) || held_key([KeyCode::SuperLeft, KeyCode::SuperRight]);
    let shift = held_key([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let held = buttons.as_ref().is_some_and(|b| b.pressed(MouseButton::Left));
    let pressed = buttons.as_ref().is_some_and(|b| b.just_pressed(MouseButton::Left));
    let window = windows.single().ok();
    let label = if doc.tool == CadTool::OffsetFace { "offset" } else { "push/pull" };

    if let Some(mut drag) = doc.tool_state.push_drag.clone() {
        if doc.health.as_ref().map(|h| h.revision) != Some(drag.began) || doc.shown_revision() != drag.began {
            doc.tool_state.push_drag = None;
            doc.show(Err("The document changed during the drag (another edit, or RoboCAD's own window); nothing was sent: drag again".into()));
            return;
        }
        if held {
            let back = view_back(&view);
            let Some(d) = window.and_then(Window::cursor_position).and_then(|c| push_distance(&view, c, drag.start, drag.normal, back, snap)) else { return };
            if d != drag.distance {
                drag.distance = d;
                doc.tool_state.readout = Some(format!("{label} {}", fl(f64::from(d))));
                doc.tool_state.push_drag = Some(drag);
            }
            return;
        }
        doc.tool_state.push_drag = None;
        if let Some(action) = release_action(&drag, doc.tool == CadTool::OffsetFace, shift) {
            out.write(Act::ui(action));
        }
        return;
    }

    // The target follows the document: filled in once its faces load, found again after an edit.
    if let Some(t) = doc.tool_state.push.clone() {
        let shown = doc.shown_revision();
        let faces = topology.get(&t.node).map(|a| &**a);
        let next = if t.revision == shown && t.info.is_none() {
            faces.and_then(|f| f.faces.iter().find(|x| x.index == t.face)).map(|info| Target { info: Some(info.clone()), ..t.clone() })
        } else if t.revision != shown {
            resolve(&t, shown, faces, "").ok()
        } else {
            None
        };
        if let Some(next) = next {
            doc.tool_state.push = Some(next);
        }
    }
    if !pressed {
        return;
    }
    let Some(cursor) = cursor_in_view(window, &view, hover.as_deref(), &nodes) else { return };
    let Some(hit) = ray_hit(&doc, &mut cast, &view, cursor, &bodies) else { return };
    let name = doc.node_name(&hit.node);
    let shown = doc.shown_revision();
    // The face index is the drawn tessellation's: while that lags the shown
    // revision it indexes older faces, so nothing is selected or targeted
    // (`face_at` answers only at the shown revision).
    if meshes.drawn_revision(&hit.node) != Some(shown) {
        // The earlier target is not what was pressed: Tab must not act on it.
        doc.tool_state.push = None;
        doc.show(Err(format!("{name} is being redrawn for revision {shown}; press again in a moment")));
        return;
    }
    let Some(face) = hit.triangle.and_then(|t| meshes.face_at(&hit.node, t, shown)) else { return };
    // RoboCAD: the selection becomes the pressed face.
    out.write(Act::ui(CadAction::CadSelect { ids: Vec::new(), items: vec![SelectionItem(hit.node.clone(), "face".into(), face)], extend: false, toggle: false, picked_at: Some(shown) }));
    let info = topology.get(&hit.node).and_then(|t| t.faces.iter().find(|f| f.index == face).cloned());
    doc.tool_state.push = Some(Target { node: hit.node.clone(), face, revision: shown, info: info.clone() });
    let Some(info) = info else {
        doc.show(Err(format!("The faces of {name} are still loading; press again in a moment")));
        return;
    };
    let (Some(normal), Some(centroid)) = (info.normal, info.centroid) else {
        doc.show(Err(format!("RoboCAD gave no normal for face {face} of {name}; type the distance (Tab) instead")));
        return;
    };
    let began = doc.shown_revision();
    if doc.health.as_ref().map(|h| h.revision) != Some(began) {
        doc.show(Err("RoboCAD's document is being refetched; press again in a moment".into()));
        return;
    }
    let outline = meshes.mesh_data(&hit.node).map(|m| crate::cad::overlay::face_outline(m, face)).unwrap_or_default();
    doc.tool_state.readout = Some(format!("{label} {}", fl(0.0)));
    doc.tool_state.push_drag = Some(PushDrag { node: hit.node, face, kind: info.kind, start: hit.point, normal: vec3(normal), centroid: vec3(centroid), began, distance: 0.0, outline });
}

/// Present: the drag's line from the face centroid along the normal and
/// the face outline moved by the distance (display only).
pub(super) fn draw(doc: Option<Res<CadDocument>>, view: Option<Res<CadView>>, mut gizmos: Gizmos<ToolGizmos>) {
    let (Some(doc), Some(view)) = (doc, view) else { return };
    let Some(drag) = &doc.tool_state.push_drag else { return };
    if !view.valid {
        return;
    }
    let w = |p: Vec3| view.world_from_model.transform_point3(p);
    let shift = drag.normal.try_normalize().unwrap_or(Vec3::ZERO) * drag.distance;
    gizmos.line(w(drag.centroid), w(drag.centroid + shift), HOT);
    for (a, b) in &drag.outline {
        gizmos.line(w(*a + shift), w(*b + shift), HOT);
    }
}

//! The robot click tools' 3D clicks (`MotorTool.press`, `JointTool.press`,
//! ui/tools.py:1260-1355): while `robot.add_motor` or `robot.add_joint` is
//! the active operation, a left press over the 3D view (not over a panel,
//! not with Alt, which orbits, and not the press that closes a command
//! surface) picks what is under the pointer with the one pick path: the
//! first unlocked drawn body along the cursor ray (`transform::ray_hit`,
//! `MeshRayCast`), and its face from `CadMeshes::face_at` at the shown
//! revision when the tool wants a face (the motor tool; the joint tool's
//! third click). The joint tool's first click with Ctrl (Command) held is
//! the world. It writes `CadRobot {op: pick, item, world, picked_at}`
//! (`tools::handle` applies it) and notes the hit point and the motor
//! tool's snap (`ToolsState::click`) for that pick. Nothing else is
//! changed: the selection is not (RoboCAD's tools pick with
//! `request_pick`, not the selection).
use super::super::{RobotArgs, RobotOp};
use super::{Click, active_tool};
use crate::app::actions::Act;
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::cad::mesh::{CadBody, CadMeshes};
use crate::cad::ops::RobotTool;
use crate::cad::sketch::{CadActivePlane, CadSketches};
use crate::cad::snap::{self, SnapKind};
use crate::cad::topology::CadTopology;
use crate::cad::transform::{cursor_in_view, ray_hit};
use crate::cad::view::CadView;
use bevy::picking::hover::HoverMap;
use bevy::picking::mesh_picking::ray_cast::MeshRayCast;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use sim_runtime::cad_client::SelectionItem;

/// Whether a command surface was open at the end of the last frame's
/// Input, i.e. when this frame's press was made (that press closes it).
#[derive(Default)]
pub(super) struct ClickState {
    surface_open: bool,
}

/// Whether the active tool's next click wants a face (the motor tool, the
/// joint tool's axis face) rather than a body.
pub(super) fn wants_face(tool: RobotTool, joint_stage: u8) -> bool {
    tool == RobotTool::Motor || joint_stage >= 2
}

/// Input: the click tools' presses (see the module doc).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn click(
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
    let Some((_, tool)) = active_tool(&doc) else { return };
    let (Some(view), Some(meshes)) = (view, meshes) else { return };
    let pressed = buttons.as_ref().is_some_and(|b| b.just_pressed(MouseButton::Left));
    if !view.valid || !pressed || surface_was_open || doc.ops.surface.is_some() {
        return;
    }
    let held = |codes: &[KeyCode]| keys.as_ref().is_some_and(|k| k.any_pressed(codes.iter().copied()));
    // Alt+left is RoboCAD's orbit, not a pick.
    if held(&[KeyCode::AltLeft, KeyCode::AltRight]) {
        return;
    }
    let Some(cursor) = cursor_in_view(windows.single().ok(), &view, hover.as_deref(), &nodes) else { return };
    let ctrl = held(&[KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]);
    let shown = doc.shown_revision();
    let stage = doc.robot.tools.joint.stage;
    let pick = |item: Option<SelectionItem>, world: bool| Act::ui(CadAction::CadRobot(RobotArgs { op: RobotOp::Pick, open: None, item, world, picked_at: Some(shown) }));
    // `JointTool.press` stage 0: Ctrl means the world, whatever is under the pointer.
    if tool == RobotTool::Joint && stage == 0 && ctrl {
        out.write(pick(None, true));
        return;
    }
    let Some(hit) = ray_hit(&doc, &mut cast, &view, cursor, &bodies) else { return };
    let item = if wants_face(tool, stage) {
        // Nothing while the mesh is being redrawn for a newer revision (its faces are the old revision's).
        hit.triangle.and_then(|t| meshes.face_at(&hit.node, t, shown)).map(|f| SelectionItem(hit.node.clone(), "face".into(), f))
    } else {
        Some(SelectionItem(hit.node.clone(), "body".into(), 0))
    };
    let Some(item) = item else { return };
    let point = [f64::from(hit.point.x), f64::from(hit.point.y), f64::from(hit.point.z)];
    // `MotorTool.press`: `self.ctx.snap(pos)`, used when it is a vertex, midpoint, centre or endpoint.
    let snapped = (tool == RobotTool::Motor)
        .then(|| {
            let candidates = snap::drawn_candidates(&doc, topology.as_deref(), Some(&*meshes), sketches.as_deref());
            let on = snap::snap_plane(plane.as_deref());
            snap::snap_on(&view, cursor, &candidates, false, on.as_ref())
        })
        .flatten()
        .filter(|s| matches!(s.kind, SnapKind::Vertex | SnapKind::Midpoint | SnapKind::Center | SnapKind::Endpoint))
        .map(|s| s.exact);
    doc.robot.tools.click = Some(Click { item: item.clone(), picked_at: shown, point, snap: snapped });
    out.write(pick(Some(item), false));
}

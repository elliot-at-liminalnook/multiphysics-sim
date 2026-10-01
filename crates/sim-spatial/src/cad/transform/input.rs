//! The tools' keys and cursors: G, R, S, D, Shift+D, M and Escape as the
//! tool controls' actions, and RoboCAD's tool cursor over the 3D view.
use super::cursor_in_view;
use crate::app::actions::Act;
use crate::cad::actions::CadAction;
use crate::cad::document::{CadDocument, CadInputFocus, CadTool};
use crate::cad::ops::{Flow, entry};
use crate::cad::view::CadView;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::window::{CursorIcon, PrimaryWindow, SystemCursorIcon};

/// Whether a catalogue interaction that picks or places in the 3D view is
/// active (`CadDocument::ops.active` is a pick-then-form, place, plane,
/// sketch or extrude op: RoboCAD's tools other than Select).
fn op_picks_in_view(doc: &CadDocument) -> bool {
    doc.ops.active.and_then(entry).is_some_and(|e| matches!(e.flow, Flow::Place(_) | Flow::PickThenForm(_) | Flow::PlanePick(_) | Flow::Sketch(_) | Flow::Extrude { .. }))
}

/// RoboCAD's tool cursor (app.py:520-522: arrow for Select, size-all for
/// the transform tools, a cross for the others), over the 3D view only (as
/// RoboCAD sets it on its viewport); the default elsewhere. A catalogue
/// op that places or picks in the view (RoboCAD's `PrimitiveTool`,
/// `EdgeTool`, `ShellTool`) is a tool other than Select there: a cross.
pub(in crate::cad) fn tool_cursor(
    mut commands: Commands,
    doc: Option<Res<CadDocument>>,
    view: Option<Res<CadView>>,
    windows: Query<(Entity, &Window, Option<&CursorIcon>), With<PrimaryWindow>>,
    hover: Option<Res<HoverMap>>,
    nodes: Query<(), With<Node>>,
) {
    let Ok((entity, window, current)) = windows.single() else { return };
    let over_view = view.as_deref().is_some_and(|v| cursor_in_view(Some(window), v, hover.as_deref(), &nodes).is_some());
    let op = doc.as_deref().is_some_and(op_picks_in_view);
    let icon = match doc.map(|d| d.tool) {
        _ if op && over_view => SystemCursorIcon::Crosshair,
        Some(CadTool::Move | CadTool::Rotate | CadTool::Scale) if over_view => SystemCursorIcon::Move,
        Some(CadTool::PushPull | CadTool::OffsetFace | CadTool::Measure) if over_view => SystemCursorIcon::Crosshair,
        _ => SystemCursorIcon::Default,
    };
    let want = CursorIcon::System(icon);
    // Inserted only on change (winit applies it when the component changes).
    if current.map_or(icon != SystemCursorIcon::Default, |c| *c != want) {
        commands.entity(entity).try_insert(want);
    }
}

/// OnExit(Cad): the window's default cursor again.
pub(in crate::cad) fn restore_cursor(mut commands: Commands, windows: Query<(Entity, Option<&CursorIcon>), With<PrimaryWindow>>) {
    for (entity, current) in &windows {
        if current.is_some_and(|c| *c != CursorIcon::default()) {
            commands.entity(entity).try_insert(CursorIcon::default());
        }
    }
}

/// Input: G, R, S, D, Shift+D, M and Escape as the tool controls'
/// actions (`panel::controls`: cad:tool:<tool>, cad:cancel), ignored while
/// a text field has the keyboard (`CadInputFocus`; the numeric bar's own
/// Tab, Enter and Escape are `numeric::entry`'s), and while a catalogue
/// form, command surface or interaction is open (`CadDocument::ops`): the
/// surfaces' key system turns Escape into closing the surface or
/// cancelling the form then (`CadSurface` closed, `CadFormCancel`), and a
/// tool key would change the tool under the op's form.
pub(in crate::cad) fn keys(keys: Option<Res<ButtonInput<KeyCode>>>, focus: Option<Res<CadInputFocus>>, doc: Option<ResMut<CadDocument>>, mut out: MessageWriter<Act<CadAction>>, selection: crate::cad::selection::CadSelection) {
    let Some(keys) = keys else { return };
    if focus.is_some_and(|f| f.0) {
        return;
    }
    let Some(mut doc) = doc else { return };
    if doc.ops.form.is_some() || doc.ops.surface.is_some() || doc.ops.active.is_some() {
        return;
    }
    let command = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let id = if keys.just_pressed(KeyCode::Escape) {
        "cad:cancel"
    } else if command || alt {
        return;
    } else if !shift && keys.just_pressed(KeyCode::KeyG) {
        "cad:tool:move"
    } else if !shift && keys.just_pressed(KeyCode::KeyR) {
        "cad:tool:rotate"
    } else if !shift && keys.just_pressed(KeyCode::KeyS) {
        "cad:tool:scale"
    } else if keys.just_pressed(KeyCode::KeyD) {
        if shift { "cad:tool:offset_face" } else { "cad:tool:push_pull" }
    } else if !shift && keys.just_pressed(KeyCode::KeyM) {
        "cad:tool:measure"
    } else {
        return;
    };
    let Some(found) = crate::cad::panel::controls(&doc, &selection.items()).into_iter().find(|c| c.id == id) else { return };
    match found.ready {
        Ok(()) => {
            out.write(Act::ui(found.action));
        }
        Err(why) => doc.show(Err(format!("{}: {why}", found.label))),
    }
}

//! The 3D view's right-click menu (RoboCAD's `_context_menu`,
//! app.py:1103-1107): Annotate, Comments panel, Push/Pull face, Fillet,
//! Chamfer, Hollow / shell, Union, Subtract, Mirror, Array…, Measure,
//! Isolate, Hide, Delete (`registry::CONTEXT`), then a "Sketch" section
//! with the 13 sketch tools in RoboCAD's registry order (Line, Rectangle,
//! Rectangle (centre), Circle, Circle (two points), Circle (three points),
//! Arc (three points), Polygon, Slot, Spline, Ellipse, Spiral, Text:
//! `registry::SKETCH_CONTEXT`, under a "Sketch" heading), plus the
//! outliner's "Make unique (bake instance)" (widgets.py:431) when an
//! instance is selected.
//!
//! Two deliberate differences. The Sketch section is a native addition:
//! RoboCAD's `_context_menu` has no sketch tools; the cad-sketch epic's
//! surfaces requirement puts every sketch tool in the 3D view's own menu.
//! RoboCAD's 14 entries stay first, unchanged and in order. "Make unique"
//! is the outliner's context menu's in RoboCAD; it stays here as the
//! cad-modify row for that outliner entry, since the native outliner has no
//! context menu yet.
//! Each entry is enabled by the selection (`registry::ready`: a catalogue
//! operation's `Needs`, an action's button, a later epic's refusal).
//!
//! A right press and release over the 3D view (not over a panel) without
//! a drag (the orbit is a right drag) opens it at the pointer with
//! `CadSurface { context { at } }`; a click on an entry runs it and closes
//! the menu (`surfaces::input`).
use super::Surface;
use crate::app::actions::Act;
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::cad::view::CadView;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// How far the pointer may move between the right press and release for a click (px).
const CLICK_SLOP: f32 = 4.0;

/// A selected node is an instance (RoboCAD's make-unique filter).
pub(super) fn instance_selected(doc: &CadDocument) -> bool {
    let Some(state) = &doc.doc else { return false };
    doc.selected_nodes().iter().any(|id| state.nodes.iter().any(|n| n.id == *id && n.kind == "instance"))
}

/// Input: a right click in the 3D view opens the menu at the pointer.
#[allow(clippy::too_many_arguments)]
pub(super) fn input(
    doc: Option<Res<CadDocument>>,
    view: Option<Res<CadView>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    hover: Option<Res<HoverMap>>,
    nodes: Query<(), With<Node>>,
    mut press: Local<Option<Vec2>>,
    mut out: MessageWriter<Act<CadAction>>,
) {
    let (Some(doc), Some(view), Some(buttons)) = (doc, view, buttons) else { return };
    let cursor = windows.single().ok().and_then(Window::cursor_position);
    if buttons.just_pressed(MouseButton::Right) {
        // A press while a surface is open only closes it (`surfaces::input`).
        *press = cursor.filter(|p| doc.ops.surface.is_none() && view.contains(*p) && !crate::cad::scene::over_ui(hover.as_deref(), &nodes));
    }
    if buttons.just_released(MouseButton::Right)
        && let Some(start) = press.take()
        && let Some(at) = cursor
        && start.distance(at) <= CLICK_SLOP
        && doc.ops.surface.is_none()
    {
        out.write(Act::ui(CadAction::CadSurface { surface: Surface::Context { at: Some([at.x, at.y]) } }));
    }
}

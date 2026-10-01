//! RoboCAD's pie menus (`RadialMenu`, ui/widgets.py:821-882), drawn with
//! the kit's pie: Space (`view.radial`) opens the view radial at the
//! pointer (Front, Top, Right, Iso, Ortho, Grid, Mode, Fit: `view.front`,
//! `view.top`, `view.right`, `view.iso`, `view.ortho`, `view.grid`,
//! `view.mode_next`, `view.fit`; all run since cad-views-export: the views
//! and Ortho are camera intents, Grid and Mode the display state, Fit
//! `CadFit`, each through the registry), Q (`select.mode_radial`) the
//! selection-mode radial (Body, Face, Edge, Vertex, Point: `select.<mode>`).
//!
//! As RoboCAD's: the entry under the pointer's angle is highlighted (the
//! kit's `index_at`; none in the 18 px dead centre); a mouse press inside
//! the pie, or a release anywhere, runs the entry under the pointer and
//! closes the pie, or only closes it in the dead centre; a press outside
//! the pie closes it (a Qt popup closes on a press outside); Escape closes
//! it (`surfaces::input`). A button already held when the pie opened (the
//! click on a palette row that opened it) is ignored until released.
//! Running an entry writes `CadInvoke { id }` (`view.fit` is `CadFit`,
//! `view.front` a `camera_view`, `view.grid` a `cad_display` toggle,
//! `select.*` `CadSelectMode` through the registry), then
//! `CadSurface { closed }`; a disabled entry shows why on the status line.
use super::{Entry, Open, Surface, SurfaceRoot, entries, registry};
use crate::app::ModeScope;
use crate::app::actions::Act;
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::cad::panel::own_controls;
use crate::cad::selection::CadSelection;
use crate::ui_kit::Kit;
use crate::ui_kit::pie::{SIDE, index_at};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// A pie entry (display only: the pointer's angle picks, `index_at`).
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct RadialSlot;

/// The pie being read: which opening it is, and the buttons held when it opened.
#[derive(Default)]
pub(super) struct RadialHeld {
    seen: Option<(&'static str, [f32; 2])>,
    ignore: Vec<MouseButton>,
}

/// The pie for the open radial (`surfaces::draw`).
pub(super) fn spawn(commands: &mut Commands, k: &Kit, open: &Open, list: &[Entry]) {
    let label = match open.surface {
        Surface::ViewRadial { .. } => "View radial menu",
        _ => "Selection-mode radial menu",
    };
    let slots: Vec<(String, RadialSlot, bool)> = list.iter().map(|e| (e.label.clone(), RadialSlot, e.ready.is_ok())).collect();
    let root = k.pie(commands, Vec2::from(open.at), label, slots, open.highlight);
    commands.entity(root).insert((SurfaceRoot, DespawnOnExit(ModeScope::Cad)));
}

/// Input: hover, and a press or release running the entry under the pointer (see the module doc).
pub(super) fn input(doc: Option<ResMut<CadDocument>>, buttons: Option<Res<ButtonInput<MouseButton>>>, windows: Query<&Window, With<PrimaryWindow>>, mut held: Local<RadialHeld>, mut out: MessageWriter<Act<CadAction>>, selection: CadSelection) {
    let (Some(mut doc), Some(buttons)) = (doc, buttons) else { return };
    let Some(open) = doc.ops.surface.clone().filter(|o| matches!(o.surface, Surface::ViewRadial { .. } | Surface::SelectRadial { .. })) else {
        if held.seen.is_some() {
            held.seen = None;
            held.ignore.clear();
        }
        return;
    };
    let tag = (open.surface.kind(), open.at);
    if held.seen != Some(tag) {
        held.seen = Some(tag);
        held.ignore = buttons.get_pressed().copied().collect();
    }
    let selection = selection.items();
    let own = own_controls(&doc, &selection);
    let list = entries(&open.surface, &doc, &selection, &own);
    let centre = Vec2::from(open.at);
    let cursor = windows.single().ok().and_then(Window::cursor_position);
    let hover = cursor.and_then(|c| index_at(centre, c, list.len()));
    if hover != open.highlight
        && let Some(o) = doc.ops.surface.as_mut()
    {
        o.highlight = hover;
    }
    let pressed = buttons.get_just_pressed().next().is_some();
    let released = buttons.get_just_released().any(|b| !held.ignore.contains(b));
    held.ignore.retain(|b| buttons.pressed(*b));
    if !(pressed || released) {
        return;
    }
    let close = CadAction::CadSurface { surface: Surface::Closed };
    let inside = cursor.is_some_and(|c| (c - centre).abs().max_element() <= SIDE / 2.0);
    if pressed && !inside {
        out.write(Act::ui(close));
        return;
    }
    if let Some(entry) = hover.and_then(|i| list.get(i)) {
        match &entry.ready {
            Ok(()) => {
                out.write(Act::ui(entry.action.clone()));
            }
            Err(why) => doc.show(Err(registry::command(&entry.id).map_or_else(|| why.clone(), |cmd| registry::status_line(cmd, why)))),
        }
    }
    out.write(Act::ui(close));
}

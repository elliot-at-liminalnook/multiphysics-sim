//! CAD mode's keys, RoboCAD's own bindings (`cad/robocad/ui/keymap.json`):
//! edit.undo Ctrl+Z, edit.redo Ctrl+Shift+Z, edit.delete Delete/Backspace
//! (the selected node), view.fit Home, file.save Ctrl+S. Qt maps Ctrl to
//! Command on macOS, so Control or Super is accepted. Each key writes the
//! same `CadAction` its button, `system_ui` control and REST command do.
//!
//! A key agrees with its button: it acts only when the button's control
//! (`panel::controls`: cad:undo, cad:redo, cad:save, cad:delete, cad:fit) is
//! enabled. When it is not, the key writes nothing and the status line
//! shows the button's disabled reason ("Undo: nothing to undo in RoboCAD's
//! history"), so a key press is never silently ignored. Delete/Backspace
//! with nothing selected stays silent: Backspace is too common a key to
//! report on.
//!
//! Clashes, checked by grepping the viewer for keys active in every mode
//! (`app::CorePlugin`, the switcher, `ui_kit`): none of them reads a key;
//! the spatial view's keys (`lib.rs`) run only under `SpatialScreen`, and
//! robot and place keys only in their modes. Inside CAD mode, a text field
//! of the panel (`CadInputFocus`) takes every key while it has focus, so
//! Backspace edits the text and never deletes a node; `keys` runs after
//! `panel::name_entry` (mod.rs) so it always sees this frame's focus.
use super::actions::CadAction;
use super::document::{CadDocument, CadInputFocus};
use crate::app::actions::Act;
use bevy::prelude::*;

/// Input: RoboCAD's shortcuts as CAD actions.
pub(super) fn keys(keys: Option<Res<ButtonInput<KeyCode>>>, focus: Option<Res<CadInputFocus>>, doc: Option<ResMut<CadDocument>>, mut out: MessageWriter<Act<CadAction>>) {
    let Some(keys) = keys else { return };
    if focus.is_some_and(|f| f.0) {
        return;
    }
    let command = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    // The control a key stands for.
    let control = if command && !alt && keys.just_pressed(KeyCode::KeyZ) {
        if shift { "cad:redo" } else { "cad:undo" }
    } else if command && !alt && !shift && keys.just_pressed(KeyCode::KeyS) {
        "cad:save"
    } else if !command && !alt && keys.any_just_pressed([KeyCode::Delete, KeyCode::Backspace]) {
        "cad:delete"
    } else if !command && !alt && keys.just_pressed(KeyCode::Home) {
        "cad:fit"
    } else {
        return;
    };
    let Some(mut doc) = doc else { return };
    if control == "cad:delete" && doc.selected().is_none() {
        return;
    }
    // The button's own control: its action (the selected node's delete)
    // and whether it is enabled now. Built only on a key press.
    let Some(found) = super::panel::controls(&doc).into_iter().find(|c| c.id == control) else { return };
    match found.ready {
        Ok(()) => {
            out.write(Act::ui(found.action));
        }
        Err(why) => doc.show(Err(format!("{}: {why}", found.label))),
    }
}

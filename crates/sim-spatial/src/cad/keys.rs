//! CAD mode's keys, RoboCAD's own bindings (`cad/robocad/ui/keymap.json`):
//! edit.undo Ctrl+Z, edit.redo Ctrl+Shift+Z, edit.delete Delete/Backspace
//! (the selected node), view.fit Home, file.save Ctrl+S, edit.select_all
//! Ctrl+A, edit.invert Ctrl+Shift+I, edit.select_same_material
//! Ctrl+Shift+M, and the selection modes select.body B, select.face
//! Shift+B, select.edge E, select.vertex V, select.point P. Qt maps Ctrl to
//! Command on macOS, so Control or Super is accepted. Each key writes the
//! same `CadAction` its button, `system_ui` control and REST command do.
//!
//! A key agrees with its button: it acts only when the button's control
//! (`panel::controls`: cad:undo, cad:redo, cad:save, cad:delete, cad:fit,
//! cad:select_all, cad:invert_selection, cad:select_same_material,
//! cad:mode:<mode>) is enabled. When it is not, the key writes nothing and
//! the status line shows the button's disabled reason ("Undo: nothing to
//! undo in RoboCAD's history"), so a key press is never silently ignored.
//! Delete/Backspace with nothing selected stays silent: Backspace is too
//! common a key to report on.
//!
//! Clashes, checked by grepping the viewer for `KeyCode` (2026-10-01): no
//! key is read in every mode (`app::CorePlugin`, the switcher and `ui_kit`
//! read none); B, E, V, P, A, I and M are read elsewhere only in their own
//! modes (E explodes in Inspect, `inspect.rs`; P, A and E fly in Place,
//! `place_view.rs`; P steps the phenomena gallery, `phenomena/keys.rs`; A
//! jogs in Robot, `robot/actions.rs` and `robot/hardware/actions.rs`), and
//! those systems run only in their modes. Inside CAD mode, part D's tool
//! keys (G, R, S, D, Shift+D, M, Tab, Enter, Escape) are read by its own
//! system: M (measure) and S (scale) must ignore Command/Control, which
//! Ctrl+Shift+M (same material) and Ctrl+S (save) hold. B, E, V and P act
//! only without Command/Control and Alt (Ctrl+E is RoboCAD's export,
//! Ctrl+Shift+B its build plate, neither ported). A text field of the panel
//! (`CadInputFocus`: the name field, the numeric bar) takes every key while
//! it has focus, so Backspace edits the text and never deletes a node;
//! `keys` runs after `panel::name_entry` (mod.rs) so it always sees this
//! frame's focus.
use super::actions::CadAction;
use super::document::{CadDocument, CadInputFocus, SelectMode};
use crate::app::actions::Act;
use bevy::prelude::*;

/// The control a key press stands for this frame, if any.
fn control(keys: &ButtonInput<KeyCode>) -> Option<String> {
    let command = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let plain = !command && !alt;
    let id = if command && !alt && keys.just_pressed(KeyCode::KeyZ) {
        if shift { "cad:redo" } else { "cad:undo" }
    } else if command && !alt && !shift && keys.just_pressed(KeyCode::KeyS) {
        "cad:save"
    } else if command && !alt && !shift && keys.just_pressed(KeyCode::KeyA) {
        "cad:select_all"
    } else if command && !alt && shift && keys.just_pressed(KeyCode::KeyI) {
        "cad:invert_selection"
    } else if command && !alt && shift && keys.just_pressed(KeyCode::KeyM) {
        "cad:select_same_material"
    } else if plain && keys.any_just_pressed([KeyCode::Delete, KeyCode::Backspace]) {
        "cad:delete"
    } else if plain && keys.just_pressed(KeyCode::Home) {
        "cad:fit"
    } else if plain && keys.just_pressed(KeyCode::KeyB) {
        return Some(mode(if shift { SelectMode::Face } else { SelectMode::Body }));
    } else if plain && !shift && keys.just_pressed(KeyCode::KeyE) {
        return Some(mode(SelectMode::Edge));
    } else if plain && !shift && keys.just_pressed(KeyCode::KeyV) {
        return Some(mode(SelectMode::Vertex));
    } else if plain && !shift && keys.just_pressed(KeyCode::KeyP) {
        return Some(mode(SelectMode::Point));
    } else {
        return None;
    };
    Some(id.to_string())
}

fn mode(mode: SelectMode) -> String {
    format!("cad:mode:{}", mode.name())
}

/// Input: RoboCAD's shortcuts as CAD actions.
pub(super) fn keys(keys: Option<Res<ButtonInput<KeyCode>>>, focus: Option<Res<CadInputFocus>>, doc: Option<ResMut<CadDocument>>, mut out: MessageWriter<Act<CadAction>>) {
    let Some(keys) = keys else { return };
    if focus.is_some_and(|f| f.0) {
        return;
    }
    let Some(control) = control(&keys) else { return };
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

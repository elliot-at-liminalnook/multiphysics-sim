//! Phenomena mode's keys, sim-app's bindings (phenomena_app.rs:133–180):
//! `]` / N / Tab next; `[` / P / Shift+Tab previous; digits 1…9, 0 open
//! exhibits 1…10 (only those that exist); ←/→ nudge the knob one step
//! (Shift: five); R reset; Space pause or run; ↑/↓ double or halve the
//! speed. Each writes the same `PhenomenaAction` its button, `system_ui`
//! control and REST command do; refusals (the exhibits are still being
//! built) show in the header's status line.
//!
//! One deliberate difference: nothing is read while Control, Command
//! (Super) or Alt is held, so Cmd+Tab (switching applications on macOS) or
//! a system shortcut never also switches the exhibit. sim-app read the
//! keys whatever the modifiers.
//!
//! Clashes, checked by reading: keys active in every mode are none.
//! `app::CorePlugin`, `app::switcher` and `ui_kit` read no key (grep
//! `KeyCode` finds none in `app/` or `ui_kit/`); the spatial view's keys
//! (`lib.rs`, `inspect::input`) run only under `SpatialScreen`, the
//! builder's and lessons' under `ModeScope::Builder`, robot's (and its
//! hardware panel's) and place's only in their modes, CAD's only in CAD
//! mode. Bevy's own: `DefaultPlugins` with this crate's features adds
//! `InputFocusPlugin` and `InputDispatchPlugin` (bevy_internal
//! default_plugins.rs) but not `TabNavigationPlugin`, so Tab moves no focus;
//! `UiWidgetsPlugins` adds the slider's keyboard handler
//! (`slider_on_key_input`, ←/→/Home/End), which only acts on a focused
//! slider, and nothing in bevy_ui_widgets 0.19.1's slider.rs sets
//! `InputFocus` on a press (focus stays on the window, set by
//! `set_initial_focus`), so the knob slider never takes the arrows.
use super::ExhibitRef;
use super::actions::PhenomenaAction;
use super::gallery::Gallery;
use crate::app::actions::Act;
use bevy::prelude::*;

/// Digits 1…9, 0: exhibits 1…10.
const DIGITS: [KeyCode; 10] = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9, KeyCode::Digit0];

/// Input: sim-app's keys as phenomena actions, in sim-app's order.
pub(super) fn keys(keys: Option<Res<ButtonInput<KeyCode>>>, gallery: Option<Res<Gallery>>, mut out: MessageWriter<Act<PhenomenaAction>>) {
    let Some(keys) = keys else { return };
    if keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::AltLeft, KeyCode::AltRight]) {
        return;
    }
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    // Before the exhibits are built the count is unknown: a digit is written
    // and refused with the reason, rather than ignored.
    let count = gallery.as_deref().and_then(Gallery::ready).map(|f| f.catalogue.len());
    let mut write = |action: PhenomenaAction| {
        out.write(Act::ui(action));
    };
    if keys.just_pressed(KeyCode::BracketRight) || keys.just_pressed(KeyCode::KeyN) || (keys.just_pressed(KeyCode::Tab) && !shift) {
        write(PhenomenaAction::PhenomenaNext);
    }
    if keys.just_pressed(KeyCode::BracketLeft) || keys.just_pressed(KeyCode::KeyP) || (keys.just_pressed(KeyCode::Tab) && shift) {
        write(PhenomenaAction::PhenomenaPrevious);
    }
    for (i, key) in DIGITS.iter().enumerate() {
        if keys.just_pressed(*key) && count.is_none_or(|c| i < c) {
            write(PhenomenaAction::PhenomenaSelect { exhibit: ExhibitRef::Number(i + 1) });
        }
    }
    let mut nudge = 0.0;
    if keys.just_pressed(KeyCode::ArrowRight) {
        nudge += 1.0;
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        nudge -= 1.0;
    }
    if nudge != 0.0 {
        write(PhenomenaAction::PhenomenaKnob { value: None, steps: Some(if shift { nudge * 5.0 } else { nudge }) });
    }
    if keys.just_pressed(KeyCode::KeyR) {
        write(PhenomenaAction::PhenomenaReset);
    }
    if keys.just_pressed(KeyCode::Space) {
        write(PhenomenaAction::PhenomenaPause { paused: None });
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        write(PhenomenaAction::PhenomenaSpeed { speed: None, steps: Some(1) });
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        write(PhenomenaAction::PhenomenaSpeed { speed: None, steps: Some(-1) });
    }
}

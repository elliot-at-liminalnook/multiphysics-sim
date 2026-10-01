//! CAD mode's keys, RoboCAD's own bindings read from its command table
//! (`surfaces::registry`: keymap.json's keys for every command it names),
//! not written per command. A key press is matched exactly (modifiers
//! included) against every bound command's key sequences ([`parse`]:
//! "Ctrl+F", "Ctrl+Shift+U", "Ctrl+Alt+U", "Shift+J", "Space", "Delete",
//! "Home", "/", digits, and the two-step "Shift+A, B"); Qt maps Ctrl to
//! Command on macOS, so Control or Super is accepted for Ctrl.
//!
//! A matched command acts only when it is ready (`registry::ready`: a
//! catalogue operation's edit gate and selection needs, an action's button
//! state, a later epic's or a not-ported command's refusal). Ready, the key writes
//! `CadInvoke { id }`, the value its menu entry, toolbar button, palette row,
//! `system_ui` control (`cad:op:<id>`) and REST `cad_invoke` write; the
//! palette and the radials open with `CadSurface` at the pointer instead
//! (`cad_invoke` of them opens at the 3D view's centre). Not ready, the key
//! writes nothing and the status line shows why ("Fillet: …", "View front
//! belongs to the cad-views-export epic; …"), so a key press is never
//! silently ignored; Delete/Backspace with nothing selected stays silent:
//! Backspace is too common a key to report on.
//!
//! Not matched here (another CAD system reads them): `tool.select` Escape,
//! `tool.move` G, `tool.rotate` R, `tool.scale` S, `tool.push_pull` D,
//! `tool.offset_face` Shift+D and `tool.measure` M (`transform::input::keys`),
//! `numeric.entry` Tab (`numeric::entry`, and the open form's first field,
//! `surfaces::form`). Keys are ignored while a text field has the keyboard
//! (`CadInputFocus`), while a command surface is open (RoboCAD's popups take
//! the keyboard; the surfaces read their own keys) and while a modal
//! parameter form is open (RoboCAD's dialogs are modal; a pick-then-form or
//! place operation's form beside the view leaves the keys live, as
//! RoboCAD's tools do).
//!
//! **Two-step keys.** "Shift+A, B" (box), "Shift+A, C" (cylinder) and
//! "Shift+A, S" (sphere): Shift+A starts a [`Chord`], which the next
//! non-modifier key completes (or, if no command has that second step,
//! drops with a status line naming the pair); it lapses after 1.5 s. While
//! it is pending, [`gate`] (registered by `surfaces::build` after the name
//! field and the surfaces' own fields, before `numeric::entry` and so
//! before transform's keys) sets `CadInputFocus` for the frame, so the
//! second key is the chord's: S completes the sphere and does not also
//! pick the Scale tool, C and B do not also run Sketch circle or Select
//! bodies. [`keys`] knows the focus is the chord's own (`Chord::gated`). A
//! text field already holding the keyboard drops the chord.
//!
//! **Clashes** (grep of `KeyCode::` over crates/sim-spatial/src, 2026-10-01;
//! `app/`, `ui_kit/`, the switcher and REST read no keys, so no key is
//! read in every mode; Inspect, Place, Phenomena, Robot, Lessons and Build
//! read theirs only in their own modes):
//!
//! | Key | Commands | Resolution |
//! |---|---|---|
//! | Ctrl+Shift+M | `edit.select_same_material` (keymap), `robot.add_motor` (inline) | RoboCAD binds only the keymap's: Same Material runs; the palette shows RoboCAD's own conflict warning |
//! | Ctrl+Space | `command_palette` | macOS takes Command+Space (Spotlight); Control+Space or Shift+F opens it |
//! | Ctrl+H | `tool.fastener` (cad-print) | macOS's app menu takes Command+H (hide); Control+H reaches the refusal |
//! | Ctrl+M | `tool.mirror` | a macOS app menu binding Command+M (minimise) would take it; winit's default menu has none; Control+M always works |
//! | Shift+A, B / C / S | `tool.box` / `tool.cylinder` / `tool.sphere` | the second key is the chord's (see above), not B (select bodies), C (sketch circle) or S (scale) |
//! | S, G, R, D, Shift+D, M, Escape | tools (transform) | read by transform's keys only; Shift+S (sketch slot), Shift+R (revolve), Shift+J etc. differ by Shift, which transform's S/G/R/M refuse |
//! | Ctrl+S, Ctrl+Shift+S, Ctrl+Shift+D | save, save as, export drawing | transform's S and D act only without Ctrl |
//! | Ctrl+A, Ctrl+Shift+A, Shift+A | select all, array, chord start | exact modifiers keep them apart |
//! | Ctrl+Z, Z | undo, next display mode (cad-views-export) | exact modifiers |
//! | B, Shift+B, Ctrl+Shift+B | select bodies, select faces, build plate (cad-views-export) | exact modifiers |
//! | F, Shift+F, Ctrl+F, Ctrl+Shift+F | focus (cad-views-export), palette, fillet, chamfer | exact modifiers |
//! | H, Alt+H, Ctrl+H, Ctrl+Shift+H | hide, show all (cad-views-export), fastener, shell | exact modifiers |
//! | P, Shift+P, Ctrl+P | select points, sketch polygon (cad-sketch), plane from face (cad-sketch) | exact modifiers |
//! | Delete, Backspace | `edit.delete` | ignored while a text field (name, numeric bar, palette, form) has the keyboard |
//! | Space | `view.radial` | typed as a space while a text field has the keyboard |
//! | Tab | `numeric.entry` | an open form with a text field takes it (`surfaces::form::input`: its first field, then the next); during a placement drag `ops::interact` also reads it to copy the base point into the form's anchor field; else the numeric bar's (`numeric::entry`) |
//! | digits, J, Q, X, T, L, C, N, /, Home | views (cad-views-export), join, selection radial, extrude, sketch text/line/circle (cad-sketch), annotate (cad-organize), isolate, fit | no other reader in CAD mode |
//!
//! Commands of later epics keep their keys so a press says which epic owns
//! them (status line), as their menu entries do.
use super::actions::CadAction;
use super::document::{CadDocument, CadInputFocus};
use super::surfaces::registry::{self, COMMANDS, Command, Resolved};
use crate::app::actions::Act;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use std::time::{Duration, Instant};

/// One key with its modifiers (Ctrl is Control or Command).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Combo {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: KeyCode,
}

/// A RoboCAD key sequence: one combination or two in turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Binding {
    One(Combo),
    Chord(Combo, Combo),
}

/// A key sequence as RoboCAD writes it (Qt's `QKeySequence` text):
/// "Ctrl+Shift+U", "Space", "Shift+A, B".
pub(crate) fn parse(text: &str) -> Result<Binding, String> {
    let steps: Vec<&str> = text.split(',').map(str::trim).collect();
    match steps.as_slice() {
        [one] => Ok(Binding::One(combo(one).map_err(|e| format!("{text}: {e}"))?)),
        [a, b] => Ok(Binding::Chord(combo(a).map_err(|e| format!("{text}: {e}"))?, combo(b).map_err(|e| format!("{text}: {e}"))?)),
        _ => Err(format!("{text}: RoboCAD's keys have one or two steps")),
    }
}

fn combo(text: &str) -> Result<Combo, String> {
    let parts: Vec<&str> = text.split('+').map(str::trim).collect();
    let Some((name, modifiers)) = parts.split_last() else { return Err("empty key".into()) };
    let mut c = Combo { ctrl: false, shift: false, alt: false, key: key(name).ok_or_else(|| format!("unknown key {name:?}"))? };
    for m in modifiers {
        match *m {
            "Ctrl" => c.ctrl = true,
            "Shift" => c.shift = true,
            "Alt" => c.alt = true,
            other => return Err(format!("unknown modifier {other:?}")),
        }
    }
    Ok(c)
}

/// The key a RoboCAD key name stands for.
fn key(name: &str) -> Option<KeyCode> {
    let mut chars = name.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        return match ch.to_ascii_uppercase() {
            'A' => Some(KeyCode::KeyA),
            'B' => Some(KeyCode::KeyB),
            'C' => Some(KeyCode::KeyC),
            'D' => Some(KeyCode::KeyD),
            'E' => Some(KeyCode::KeyE),
            'F' => Some(KeyCode::KeyF),
            'G' => Some(KeyCode::KeyG),
            'H' => Some(KeyCode::KeyH),
            'I' => Some(KeyCode::KeyI),
            'J' => Some(KeyCode::KeyJ),
            'K' => Some(KeyCode::KeyK),
            'L' => Some(KeyCode::KeyL),
            'M' => Some(KeyCode::KeyM),
            'N' => Some(KeyCode::KeyN),
            'O' => Some(KeyCode::KeyO),
            'P' => Some(KeyCode::KeyP),
            'Q' => Some(KeyCode::KeyQ),
            'R' => Some(KeyCode::KeyR),
            'S' => Some(KeyCode::KeyS),
            'T' => Some(KeyCode::KeyT),
            'U' => Some(KeyCode::KeyU),
            'V' => Some(KeyCode::KeyV),
            'W' => Some(KeyCode::KeyW),
            'X' => Some(KeyCode::KeyX),
            'Y' => Some(KeyCode::KeyY),
            'Z' => Some(KeyCode::KeyZ),
            '0' => Some(KeyCode::Digit0),
            '1' => Some(KeyCode::Digit1),
            '2' => Some(KeyCode::Digit2),
            '3' => Some(KeyCode::Digit3),
            '4' => Some(KeyCode::Digit4),
            '5' => Some(KeyCode::Digit5),
            '6' => Some(KeyCode::Digit6),
            '7' => Some(KeyCode::Digit7),
            '8' => Some(KeyCode::Digit8),
            '9' => Some(KeyCode::Digit9),
            '/' => Some(KeyCode::Slash),
            _ => None,
        };
    }
    match name {
        "Space" => Some(KeyCode::Space),
        "Delete" => Some(KeyCode::Delete),
        "Backspace" => Some(KeyCode::Backspace),
        "Home" => Some(KeyCode::Home),
        "End" => Some(KeyCode::End),
        "Escape" | "Esc" => Some(KeyCode::Escape),
        "Tab" => Some(KeyCode::Tab),
        "Return" | "Enter" => Some(KeyCode::Enter),
        _ => None,
    }
}

/// The keypad's digits are the digits (Qt matches both).
fn normalise(key: KeyCode) -> KeyCode {
    match key {
        KeyCode::Numpad0 => KeyCode::Digit0,
        KeyCode::Numpad1 => KeyCode::Digit1,
        KeyCode::Numpad2 => KeyCode::Digit2,
        KeyCode::Numpad3 => KeyCode::Digit3,
        KeyCode::Numpad4 => KeyCode::Digit4,
        KeyCode::Numpad5 => KeyCode::Digit5,
        KeyCode::Numpad6 => KeyCode::Digit6,
        KeyCode::Numpad7 => KeyCode::Digit7,
        KeyCode::Numpad8 => KeyCode::Digit8,
        KeyCode::Numpad9 => KeyCode::Digit9,
        KeyCode::NumpadDivide => KeyCode::Slash,
        KeyCode::NumpadEnter => KeyCode::Enter,
        other => other,
    }
}

fn is_modifier(key: KeyCode) -> bool {
    matches!(key, KeyCode::ShiftLeft | KeyCode::ShiftRight | KeyCode::ControlLeft | KeyCode::ControlRight | KeyCode::SuperLeft | KeyCode::SuperRight | KeyCode::AltLeft | KeyCode::AltRight | KeyCode::CapsLock | KeyCode::Fn | KeyCode::FnLock)
}

/// `key` with the modifiers held now.
fn combo_now(keys: &ButtonInput<KeyCode>, key: KeyCode) -> Combo {
    Combo {
        ctrl: keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]),
        shift: keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        alt: keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]),
        key: normalise(key),
    }
}

/// A combination as RoboCAD writes it ("Shift+A").
fn text(c: Combo) -> String {
    let mut out = String::new();
    for (on, name) in [(c.ctrl, "Ctrl+"), (c.shift, "Shift+"), (c.alt, "Alt+")] {
        if on {
            out.push_str(name);
        }
    }
    let key = format!("{:?}", c.key);
    out.push_str(key.strip_prefix("Key").or_else(|| key.strip_prefix("Digit")).unwrap_or(&key));
    out
}

/// Commands whose keys another CAD system reads (see the module doc).
const ELSEWHERE: [&str; 8] = ["tool.select", "tool.move", "tool.rotate", "tool.scale", "tool.push_pull", "tool.offset_face", "tool.measure", "numeric.entry"];

/// Every key binding matched here: (command, binding), in registry order.
fn bindings() -> impl Iterator<Item = (&'static Command, Binding)> {
    COMMANDS.iter().filter(|c| c.bound && !ELSEWHERE.contains(&c.id)).flat_map(|c| c.keys.iter().filter_map(move |k| parse(k).ok().map(|b| (c, b))))
}

/// How long the first step of a two-step key waits for the second.
const CHORD_TIMEOUT: Duration = Duration::from_millis(1500);

/// A pending two-step key (see the module doc).
#[derive(Resource, Default, Debug)]
pub(super) struct Chord {
    first: Option<(Combo, Instant)>,
    /// `gate` set `CadInputFocus` this frame for the pending chord.
    gated: bool,
}

/// Input (before `numeric::entry`, so before transform's keys): while a
/// two-step key is pending, the keyboard is the chord's this frame.
pub(super) fn gate(chord: Option<ResMut<Chord>>, focus: Option<ResMut<CadInputFocus>>) {
    let (Some(mut chord), Some(mut focus)) = (chord, focus) else { return };
    let first = chord.first;
    match first {
        Some((_, at)) if at.elapsed() <= CHORD_TIMEOUT => {
            // `panel::name_entry` resets the focus each frame, so a set flag
            // here is a text field's (the name, a form field, the palette).
            if focus.0 {
                chord.first = None;
                chord.gated = false;
            } else {
                focus.0 = true;
                chord.gated = true;
            }
        }
        Some(_) => {
            chord.first = None;
            chord.gated = false;
        }
        None => {
            if chord.gated {
                chord.gated = false;
            }
        }
    }
}

/// A modal parameter form is open (a `Flow::Form` op's: no interaction is
/// active beside it).
fn modal_form(doc: &CadDocument) -> bool {
    doc.ops.form.is_some() && doc.ops.active.is_none()
}

/// Input: RoboCAD's shortcuts (see the module doc).
#[allow(clippy::too_many_arguments)]
pub(super) fn keys(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    focus: Option<Res<CadInputFocus>>,
    chord: Option<ResMut<Chord>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    doc: Option<ResMut<CadDocument>>,
    mut out: MessageWriter<Act<CadAction>>,
) {
    let Some(keys) = keys else { return };
    let Some(mut chord) = chord else { return };
    let gated = chord.gated;
    if focus.is_some_and(|f| f.0) && !gated {
        return;
    }
    let Some(mut doc) = doc else { return };
    let pending = chord.first.filter(|(_, at)| at.elapsed() <= CHORD_TIMEOUT).map(|(c, _)| c);
    let Some(key) = keys.get_just_pressed().copied().find(|k| !is_modifier(*k)) else { return };
    chord.first = None;
    chord.gated = false;
    if doc.ops.surface.is_some() || modal_form(&doc) {
        return;
    }
    let pressed = combo_now(&keys, key);
    let cmd = match pending {
        Some(first) => match bindings().find(|(_, b)| *b == Binding::Chord(first, pressed)) {
            Some((cmd, _)) => cmd,
            None => {
                doc.show(Err(format!("{}, {} is not one of RoboCAD's keys", text(first), text(pressed))));
                return;
            }
        },
        None => {
            if bindings().any(|(_, b)| matches!(b, Binding::Chord(f, _) if f == pressed)) {
                chord.first = Some((pressed, Instant::now()));
                return;
            }
            match bindings().find(|(_, b)| *b == Binding::One(pressed)) {
                Some((cmd, _)) => cmd,
                None => return,
            }
        }
    };
    let cursor = windows.single().ok().and_then(Window::cursor_position);
    // The panel's own controls: an action command is ready when its button is.
    let own = super::panel::own_controls(&doc);
    match registry::ready(cmd, &doc, &own) {
        Ok(()) => {
            let action = match registry::resolve(cmd) {
                Resolved::Surface(opens) => CadAction::CadSurface { surface: opens.surface(cursor.map(|p| [p.x, p.y])) },
                _ => CadAction::CadInvoke { id: cmd.id.to_string() },
            };
            out.write(Act::ui(action));
        }
        // Delete/Backspace with nothing selected stays silent.
        Err(_) if cmd.id == "edit.delete" && doc.selection.is_empty() => {}
        Err(why) => doc.show(Err(registry::status_line(cmd, &why))),
    }
}

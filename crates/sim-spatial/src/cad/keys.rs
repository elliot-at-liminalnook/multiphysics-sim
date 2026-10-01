//! CAD mode's keys, RoboCAD's own bindings read from its command table
//! (`surfaces::registry`: keymap.json's keys for every command it names),
//! not written per command. A key press is matched exactly (modifiers
//! included) against every bound command's key sequences ([`parse`]:
//! "Ctrl+F", "Ctrl+Shift+U", "Ctrl+Alt+U", "Shift+J", "Space", "Delete",
//! "Home", "/", digits, and the two-step "Shift+A, B"); Qt maps Ctrl to
//! Command on macOS, so Control or Super is accepted for Ctrl. The
//! cad-sketch keys (X extrude, Shift+R revolve, Ctrl+P plane from face, L,
//! Shift+L, C, A, Shift+P, Shift+S, Shift+C, T sketch tools) are matched
//! the same way, from the same table: A is the native binding of
//! keymap.json's dead `sketch.arc` to `sketch.arc_3pt` (`registry`).
//!
//! A matched command acts only when it is ready (`registry::ready`: a
//! catalogue operation's edit gate and selection needs, an action's button
//! state, a later epic's or a not-ported command's refusal). Ready, the key writes
//! `CadInvoke { id }`, the value its menu entry, toolbar button, palette row,
//! `system_ui` control (`cad:op:<id>`) and REST `cad_invoke` write; the
//! palette and the radials open with `CadSurface` at the pointer instead
//! (`cad_invoke` of them opens at the 3D view's centre). Not ready, the key
//! writes nothing and the status line shows why ("Fillet: …", "Annotate
//! belongs to the cad-organize epic; …"), so a key press is never
//! silently ignored; Delete/Backspace with nothing selected stays silent:
//! Backspace is too common a key to report on.
//!
//! Not matched here (another CAD system reads them): `tool.select` Escape,
//! `tool.move` G, `tool.rotate` R, `tool.scale` S, `tool.push_pull` D,
//! `tool.offset_face` Shift+D and `tool.measure` M (`transform::input::keys`),
//! `numeric.entry` Tab (`numeric::entry`, and the open form's first field,
//! `surfaces::form`). Keys are ignored while a text field has the keyboard
//! (the kit's `typing`, `ui_kit::text`: the name field, the inspector's
//! editors, the numeric bar, the form, the palette, the Saved Views panel's
//! fields and every other kit field; Command/Control chords the field leaves
//! unused reach `ButtonInput` but are not CAD keys while it types, as
//! before), while a command surface is open (RoboCAD's popups take
//! the keyboard; the surfaces read their own keys) and while a modal
//! parameter form is open (RoboCAD's dialogs are modal; a pick-then-form or
//! place operation's form beside the view leaves the keys live, as
//! RoboCAD's tools do).
//!
//! **Two-step keys.** "Shift+A, B" (box), "Shift+A, C" (cylinder) and
//! "Shift+A, S" (sphere): Shift+A starts a [`Chord`], which the next
//! non-modifier key completes (or, if no command has that second step,
//! drops with a status line naming the pair); it lapses after 1.5 s. While
//! it is pending, [`gate`] (registered by `surfaces::build`, last in its
//! chain, before the numeric bar and every other CAD key reader) marks the
//! frame the chord's (`Chord::gated`), and every other CAD
//! key, pick and tool reader runs under [`free`] (no text field typing and
//! no chord gating the frame), so the second key is the chord's: S
//! completes the sphere and does not also pick the Scale tool, C and B do
//! not also run Sketch circle or Select bodies. A text field holding the
//! keyboard drops the chord.
//!
//! **Clashes** (grep of `KeyCode::` over crates/sim-spatial/src, 2026-10-01;
//! `app/`, `ui_kit/`, the switcher and REST read no keys, so no key is
//! read in every mode; Inspect, Place, Phenomena, Robot, Lessons and Build
//! read theirs only in their own modes):
//!
//! | Key | Commands | Resolution |
//! |---|---|---|
//! | Ctrl+Shift+M | `edit.select_same_material` (keymap), `robot.add_motor` (inline) | RoboCAD binds only the keymap's: Same Material runs; the palette shows RoboCAD's own conflict warning; `robot.add_motor` stays unbound (its menu entry, palette row and the Robot panel's button start it) |
//! | Ctrl+Shift+J | `robot.add_joint` (inline only in RoboCAD, never bound there) | bound here deliberately (cad-physical-inspect): no other command or system reads it in CAD mode (J join, Shift+J unjoin differ by modifiers; Robot mode's J joint frames is another mode's); RoboCAD's USER_GUIDE.md:375 documents the key |
//! | Ctrl+Space | `command_palette` | macOS takes Command+Space (Spotlight); Control+Space or Shift+F opens it |
//! | Ctrl+H | `tool.fastener` (cad-print: starts the fastener tool, its form beside the view, then face clicks) | macOS's app menu takes Command+H (hide); Control+H starts the tool |
//! | Ctrl+W | `print.wall_check` (cad-print: the "Flag walls thinner than (mm):" form) | winit's default macOS menu has no Close item, so Command+W reaches the check; no other CAD reader of W (the shared camera's fly keys are not used in CAD mode) |
//! | V, Ctrl+V, Ctrl+Shift+V | select vertices, paste with placement, validate for printing (cad-print) | exact modifiers |
//! | Ctrl+M | `tool.mirror` | a macOS app menu binding Command+M (minimise) would take it; winit's default menu has none; Control+M always works |
//! | Shift+A, B / C / S | `tool.box` / `tool.cylinder` / `tool.sphere` | the second key is the chord's (see above), not B (select bodies), C (sketch circle) or S (scale) |
//! | S, G, R, D, Shift+D, M, Escape | tools (transform) | read by transform's keys only (`transform::input::keys`, and not while an operation's interaction, form or a surface is open: a sketch tool's keys stay the registry's); Shift+S (sketch slot), Shift+R (revolve), Shift+J etc. differ by Shift, which transform's S/G/R/M refuse (`!shift`) |
//! | Ctrl+S, Ctrl+Shift+S, Ctrl+Shift+D | save, save as, export drawing | transform's S and D act only without Ctrl |
//! | S, Shift+S, Ctrl+S, Ctrl+Shift+S | scale (transform), sketch slot, save, save as (the files part's row) | transform's S refuses Shift and Ctrl; the rest are exact modifiers here |
//! | R, Shift+R | rotate (transform), revolve | transform's R refuses Shift |
//! | A, Shift+A, Ctrl+A, Ctrl+Shift+A | sketch arc (three points; the native binding of keymap.json's dead `sketch.arc`), chord start (box, cylinder, sphere), select all, array | exact modifiers keep them apart: A alone is no chord's first step, so it runs the arc at once |
//! | L, Shift+L | sketch line, sketch rectangle | exact modifiers |
//! | C, Shift+C, Ctrl+C, Ctrl+Shift+C, Shift+A then C | sketch circle, sketch spline, copy, clearance offset (cad-print: needs selected faces, then its form), cylinder | exact modifiers; C after Shift+A is the chord's (see above), not the circle's |
//! | X, Ctrl+Shift+X | extrude, section analysis (`cad_section` toggle) | exact modifiers |
//! | T | sketch text | no other reader in CAD mode; while the Text tool's form has its text field focused (the kit's `typing`), T is typed |
//! | Ctrl+P | plane from face | exact modifiers (P select points, Shift+P sketch polygon; see the P row) |
//! | Enter | the spline's finish (`sketch::Finish::EnterOrDouble`, read by the sketch interaction), the open form's submit (`surfaces::form::input`, `CadFormSubmit`, only when no field has the keyboard), the numeric bar's commit (`numeric::entry`, while it is focused) | bound to no command here (`simulation.experiment`'s Ctrl+Return is listed, never bound); a focused field's Enter is that field's; with the Spline tool active and no field focused, Enter is the spline's finish, so the form's Enter submit must stand aside while a `Flow::Sketch` interaction is active (that guard is `surfaces::form`'s) |
//! | Ctrl+Z, Ctrl+Shift+Z, Z | undo, redo, next display mode (`cad_display` next) | exact modifiers |
//! | B, Shift+B, Ctrl+Shift+B | select bodies, select faces, build plate preview (`cad_display` toggle) | exact modifiers |
//! | F, Shift+F, Ctrl+F, Ctrl+Shift+F | focus selection (frames the selected nodes; Fit All with none), palette, fillet, chamfer | exact modifiers |
//! | H, Alt+H, Ctrl+H, Ctrl+Shift+H | hide, show all (catalogue: `Ops.set_visible`, `Ops.show_all`), fastener (cad-print), shell | exact modifiers; macOS's Option+H types "˙", but keys match the physical key (`KeyCode::KeyH`) with Alt held, so Alt+H reaches Show All |
//! | Ctrl+G | grid (`cad_display` toggle) | no other reader in CAD mode |
//! | 1, 3, 7, 0, Ctrl+1, Ctrl+3, Ctrl+7 | view front, right, top, iso; back, left, bottom (`camera_view`, RoboCAD's yaw/pitch table) | exact modifiers; the keypad's digits are the digits (`normalise`); the shared camera's numpad keys are off in CAD (`OrbitRules::keys` false, `scene`), so a digit is read once; macOS's Mission Control may take Control+digit ("Switch to Desktop n") when enabled, Command+digit still works |
//! | 5 | orthographic toggle (`camera_projection`) | as the digits above |
//! | / | isolate (catalogue: `Ops.isolate`) | the keypad's divide is `/` (`normalise`) |
//! | P, Shift+P, Ctrl+P | select points, sketch polygon, plane from face | exact modifiers |
//! | Delete, Backspace | `edit.delete` | ignored while a text field (name, numeric bar, palette, form) has the keyboard |
//! | Space | `view.radial` | typed as a space while a text field has the keyboard |
//! | Tab | `numeric.entry` | an open form with a text field takes it (`surfaces::form::input`: its first field, then the next); during a placement drag `ops::interact` also reads it to copy the base point into the form's anchor field; else the numeric bar's (`numeric::entry`) |
//! | J, Q, X, T, L, C, A, N, Home | join, selection radial, extrude, sketch text/line/circle/arc, annotate (cad-organize), fit | no other reader in CAD mode |
//! | Left press in the 3D view with a robot click tool active | the motor and joint tools (`robot::tools`'s click system) | Ctrl (Command) on the joint tool's first click is the world (`JointTool`); Alt+left is the orbit, not a pick |
//! | Left press in the 3D view with the fastener tool active (cad-print) | one fastener hole on the face under the pointer (`print::fastener_tool`) | the selection's click stands aside (`pick`'s `robot_tool` includes `Flow::PrintPick`); Alt+left is the orbit |
//!
//! Commands of later epics keep their keys so a press says which epic owns
//! them (status line), as their menu entries do.
use super::actions::CadAction;
use super::document::CadDocument;
use super::selection::CadSelection;
use super::surfaces::registry::{self, COMMANDS, Command, Resolved};
use crate::app::actions::Act;
use crate::ui_kit::text::Typing;
use bevy::ecs::system::SystemParam;
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
    /// `gate` gave this frame's keys to the pending chord: only [`keys`]
    /// reads them (the other readers run under [`free`]). Set and cleared
    /// by `gate` alone, so it holds for the whole frame.
    gated: bool,
}
impl Chord {
    /// This frame's keys are the pending chord's (`gate`).
    pub(super) fn gated(&self) -> bool {
        self.gated
    }
}

/// Input, before every other CAD key reader: while a two-step key is
/// pending, the keyboard is the chord's this frame; a text field holding
/// the keyboard drops the chord.
pub(super) fn gate(chord: Option<ResMut<Chord>>, typing: Typing) {
    let Some(mut chord) = chord else { return };
    let first = chord.first;
    match first {
        Some((_, at)) if at.elapsed() <= CHORD_TIMEOUT && !typing.get() => {
            if !chord.gated {
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

/// Whether CAD's keyboard is taken this frame: a kit text field has it
/// (`ui_kit::text::Typing`), or a pending two-step key owns the frame
/// (`Chord::gated`). CAD's key, pick and tool readers other than [`keys`]
/// stand aside while it is (`get`, or the [`free`] run condition).
#[derive(SystemParam)]
pub(in crate::cad) struct Held<'w, 's> {
    typing: Typing<'w, 's>,
    chord: Option<Res<'w, Chord>>,
}
impl Held<'_, '_> {
    /// A text field types or a two-step key owns the frame.
    pub(in crate::cad) fn get(&self) -> bool {
        self.typing() || self.chord.as_ref().is_some_and(|c| c.gated)
    }
    /// A kit text field has the keyboard.
    pub(in crate::cad) fn typing(&self) -> bool {
        self.typing.get()
    }
    /// The kit text field that has the keyboard.
    pub(in crate::cad) fn field(&self) -> Option<crate::ui_kit::text::FieldId> {
        self.typing.field()
    }
}

/// The run condition of CAD's key readers other than [`keys`]: the
/// keyboard is not [`Held`].
pub(in crate::cad) fn free(held: Held) -> bool {
    !held.get()
}

/// A modal parameter form is open: a `Flow::Form` op's dialog (a pick or
/// place tool's form sits beside its interaction and leaves the keys on).
fn modal_form(doc: &CadDocument) -> bool {
    doc.ops.form.as_ref().and_then(|f| super::ops::entry(f.op)).is_some_and(|e| e.flow == super::ops::Flow::Form)
}

/// Input: RoboCAD's shortcuts (see the module doc).
#[allow(clippy::too_many_arguments)]
pub(super) fn keys(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    typing: Typing,
    chord: Option<ResMut<Chord>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    doc: Option<ResMut<CadDocument>>,
    mut out: MessageWriter<Act<CadAction>>,
    selection: CadSelection,
) {
    let Some(keys) = keys else { return };
    let Some(mut chord) = chord else { return };
    // Not while a text field has the keyboard, Command/Control chords
    // included (the kit leaves the chords it does not use in `ButtonInput`;
    // CAD's keys stay off while typing, as before the kit).
    if typing.get() {
        return;
    }
    let Some(mut doc) = doc else { return };
    let pending = chord.first.filter(|(_, at)| at.elapsed() <= CHORD_TIMEOUT).map(|(c, _)| c);
    let Some(key) = keys.get_just_pressed().copied().find(|k| !is_modifier(*k)) else { return };
    // `gated` stays as `gate` set it for the rest of the frame (see `Chord`).
    chord.first = None;
    if doc.ops.surface.is_some() || modal_form(&doc) {
        return;
    }
    let pressed = combo_now(&keys, key);
    let cmd = match pending {
        Some(first) => match bindings().find(|(_, b)| *b == Binding::Chord(first, pressed)) {
            Some((cmd, _)) => cmd,
            // Escape abandons the chord (cleared above), silently.
            None if key == KeyCode::Escape => return,
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
    let selection = selection.items();
    let own = super::panel::own_controls(&doc, &selection);
    match registry::ready(cmd, &doc, &selection, &own) {
        Ok(()) => {
            let action = match registry::resolve(cmd) {
                Resolved::Surface(opens) => CadAction::CadSurface { surface: opens.surface(cursor.map(|p| [p.x, p.y])) },
                _ => CadAction::CadInvoke { id: cmd.id.to_string() },
            };
            out.write(Act::ui(action));
        }
        // Delete/Backspace with nothing selected stays silent.
        Err(_) if cmd.id == "edit.delete" && selection.is_empty() => {}
        Err(why) => doc.show(Err(registry::status_line(cmd, &why))),
    }
}

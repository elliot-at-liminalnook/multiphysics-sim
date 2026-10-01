//! The one input system for text ([`keys`]) and the held-key release it
//! applies when a field takes the keyboard ([`release_held`], moved from
//! the document picker unchanged).
use super::draft::DraftKey;
use super::{EnterKey, FieldEvent, FieldId, FieldMsg, KitInput, TabKey, TextField};
use crate::app::ViewerMode;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardFocusLost, KeyboardInput};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;

/// Releases every held input so the systems underneath see `just_released`
/// once (a robot walking on a held W, a hardware jog on a held Q/A stops),
/// and no new press: one made this frame is dropped, unless it re-presses
/// a key also released this frame (then its release stays). A release that
/// arrived this frame is kept: Bevy's input system already moved it out of
/// `pressed`, so dropping it would lose the only `just_released` it gets.
///
/// Released, never reset: a reset would forget a held key without a
/// release, so a robot walking on a held W, or a hardware jog on a held
/// Q/A, would never see the release and keep moving.
pub(crate) fn release_held<T: Clone + Eq + std::hash::Hash + Send + Sync + 'static>(input: &mut ButtonInput<T>) {
    let pressed_now: Vec<T> = input.get_just_pressed().cloned().collect();
    for key in pressed_now {
        if input.just_released(key.clone()) {
            input.clear_just_pressed(key);
        } else {
            input.reset(key);
        }
    }
    input.release_all();
}

/// Command/Control and Shift keys that make a typed key a chord or a
/// shifted Enter/Tab.
const COMMAND: [KeyCode; 4] = [KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::ControlLeft, KeyCode::ControlRight];
const SHIFT: [KeyCode; 2] = [KeyCode::ShiftLeft, KeyCode::ShiftRight];

/// The modifier keys held, followed from the keyboard messages in order
/// (not from `ButtonInput`, which [`release_held`] empties: a Command held
/// when a field took the keyboard still makes Cmd+V a chord).
#[derive(Default, Debug)]
pub(crate) struct Modifiers {
    held: Vec<KeyCode>,
}
impl Modifiers {
    fn follow(&mut self, e: &KeyboardInput) {
        if !Self::is_modifier(e.key_code) {
            return;
        }
        match e.state {
            ButtonState::Pressed if !self.held.contains(&e.key_code) => self.held.push(e.key_code),
            ButtonState::Pressed => {}
            ButtonState::Released => self.held.retain(|c| *c != e.key_code),
        }
    }
    fn is_modifier(code: KeyCode) -> bool {
        COMMAND.contains(&code) || SHIFT.contains(&code)
    }
    fn command(&self) -> bool {
        self.held.iter().any(|c| COMMAND.contains(c))
    }
    fn shift(&self) -> bool {
        self.held.iter().any(|c| SHIFT.contains(c))
    }
}

/// What one key did in the focused field.
enum Outcome {
    Edited,
    Submit,
    Cancel,
    Tab { back: bool },
    Ignored,
}

/// One key in `field` (`command`, `shift`: held at the key's message).
fn apply(field: &mut TextField, key: &Key, command: bool, shift: bool) -> Outcome {
    match key {
        Key::Enter => {
            let submit = match field.enter {
                EnterKey::Submit => true,
                EnterKey::ShiftNewline => !shift,
                EnterKey::CommandSubmits => command,
            };
            if submit {
                return Outcome::Submit;
            }
            field.draft.type_text("\n");
            Outcome::Edited
        }
        Key::Escape => Outcome::Cancel,
        Key::Tab => match field.tab {
            TabKey::Emit => Outcome::Tab { back: shift },
            TabKey::Indent => {
                field.draft.type_text("  ");
                Outcome::Edited
            }
        },
        key => match field.draft.key_filtered(key, command, field.filter) {
            DraftKey::Edited => Outcome::Edited,
            _ => Outcome::Ignored,
        },
    }
}

/// PreUpdate, after Bevy's input and UI focus systems: the one reader of
/// `KeyboardInput` for text.
///
/// 1. A field that lost the keyboard to a non-field entity (a Bevy
///    widget's press) is told ([`FieldEvent::Blur`]).
/// 2. A left press not on a kit input ([`KitInput`]) takes the keyboard
///    from a non-sticky field, and a mode switch from any field (`Blur`).
/// 3. A field that gained the keyboard since the last run: every held key
///    is released ([`release_held`]), so a robot walking on a held W or a
///    hardware jog on a held Q/A stops (its release reaches the systems
///    underneath once, a same-frame release kept).
/// 4. The focused field's keys: typing edits its draft (`Changed`), Enter
///    submits (`Submit`, as [`EnterKey`] reads it), Escape cancels
///    (`Cancel`; the keyboard is taken away), Tab is `Tab` or an indent.
///    The frame's later keys after a Submit, Cancel or Tab are dropped (as
///    every field's own loop did). Every key the field used, and every key
///    pressed without Command/Control, is consumed (`clear_just_pressed`),
///    so no mode key sees it; Command/Control chords the field ignores pass
///    through (CAD's Cmd+Z while typing).
#[allow(clippy::too_many_arguments)]
pub(crate) fn keys(
    mut events: MessageReader<KeyboardInput>,
    mut lost: MessageReader<KeyboardFocusLost>,
    focus: Option<ResMut<InputFocus>>,
    mut fields: Query<(&FieldId, &mut TextField)>,
    presses: Query<&Interaction, (Changed<Interaction>, With<KitInput>)>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    mode: Option<Res<State<ViewerMode>>>,
    (codes, logical): (Option<ResMut<ButtonInput<KeyCode>>>, Option<ResMut<ButtonInput<Key>>>),
    (mut modifiers, mut last): (Local<Modifiers>, Local<Option<Entity>>),
    mut out: MessageWriter<FieldMsg>,
) {
    if lost.read().count() > 0 {
        // The window lost the keyboard: a held modifier's release will not arrive.
        modifiers.held.clear();
    }
    let Some(mut focus) = focus else {
        for e in events.read() {
            modifiers.follow(e);
        }
        return;
    };
    let mut current = focus.get().filter(|e| fields.contains(*e));
    // 1. Bevy focused something that is not a field.
    if let Some(previous) = *last
        && current.is_none()
        && focus.get().is_some()
        && let Ok((&field, _)) = fields.get(previous)
    {
        out.write(FieldMsg { field, event: FieldEvent::Blur });
    }
    // 2. A press elsewhere, or a mode switch.
    if let Some(entity) = current {
        let pressed_elsewhere = mouse.as_ref().is_some_and(|m| m.just_pressed(MouseButton::Left)) && !presses.iter().any(|i| *i == Interaction::Pressed) && fields.get(entity).is_ok_and(|(_, f)| !f.sticky);
        let switched = mode.as_ref().is_some_and(|m| m.is_changed());
        if pressed_elsewhere || switched {
            focus.clear();
            current = None;
            if let Ok((&field, _)) = fields.get(entity) {
                out.write(FieldMsg { field, event: FieldEvent::Blur });
            }
        }
    }
    let (mut codes, mut logical) = (codes, logical);
    // 3. A field took the keyboard: release what is held.
    if current.is_some() && current != *last {
        if let Some(codes) = codes.as_mut() {
            release_held(&mut **codes);
        }
        if let Some(logical) = logical.as_mut() {
            release_held(&mut **logical);
        }
    }
    // 4. The focused field's keys.
    let mut used: Vec<(KeyCode, Key)> = Vec::new();
    let mut changed = false;
    let mut ended = false;
    for e in events.read() {
        let modifier = Modifiers::is_modifier(e.key_code);
        modifiers.follow(e);
        let Some(entity) = current else { continue };
        if e.state != ButtonState::Pressed || modifier {
            continue;
        }
        let command = modifiers.command();
        if ended {
            if !command {
                used.push((e.key_code, e.logical_key.clone()));
            }
            continue;
        }
        let Ok((&id, mut field)) = fields.get_mut(entity) else { continue };
        let outcome = apply(&mut field, &e.logical_key, command, modifiers.shift());
        if !matches!(outcome, Outcome::Ignored) || !command {
            used.push((e.key_code, e.logical_key.clone()));
        }
        // A Submit, Cancel or Tab is told after the edits before it.
        if !matches!(outcome, Outcome::Edited | Outcome::Ignored) && std::mem::take(&mut changed) {
            out.write(FieldMsg { field: id, event: FieldEvent::Changed(field.draft.clone()) });
        }
        match outcome {
            Outcome::Edited => changed = true,
            Outcome::Ignored => {}
            Outcome::Submit => {
                out.write(FieldMsg { field: id, event: FieldEvent::Submit(field.draft.text.clone()) });
                ended = true;
            }
            Outcome::Cancel => {
                out.write(FieldMsg { field: id, event: FieldEvent::Cancel });
                focus.clear();
                ended = true;
            }
            Outcome::Tab { back } => {
                out.write(FieldMsg { field: id, event: FieldEvent::Tab { back } });
                ended = true;
            }
        }
    }
    if changed
        && let Some(entity) = current
        && let Ok((&id, field)) = fields.get(entity)
    {
        out.write(FieldMsg { field: id, event: FieldEvent::Changed(field.draft.clone()) });
    }
    for (code, key) in used {
        if let Some(codes) = codes.as_mut() {
            codes.clear_just_pressed(code);
        }
        if let Some(logical) = logical.as_mut() {
            logical.clear_just_pressed(key);
        }
    }
    *last = focus.get().filter(|e| fields.contains(*e));
}

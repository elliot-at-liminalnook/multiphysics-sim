//! The open picker is modal ([`keys`]): its path field's events, Enter /
//! Escape while the field does not have the keyboard, the wheel, and
//! the held-key release; and [`sync`], which gives the kit field the
//! picker's draft when the picker changed it (a listing pick, "..", a
//! `system_ui` text, the examples prefill, a close) and gives it the
//! keyboard when the picker opens.
use super::{PATH, Picker, PickerList};
use crate::app::actions::Act;
use crate::app::switch::WindowAction;
use crate::ui_kit::text::{FieldEvent, FieldMsg, TextFocus, release_held};
use bevy::ecs::message::{MessageCursor, Messages};
use bevy::ecs::system::ParamSet;
use bevy::input::keyboard::Key;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;

/// PreUpdate, before the kit's input system: opening the picker gives
/// its path field the keyboard (a field of the mode underneath that had it
/// gets `Blur`, so nothing types under the modal picker); closed, the
/// field gives it up (a close from a click, `system_ui` or a mode change).
/// The picker's draft reaches the field before any key is typed into it:
/// every typed edit came back as `Changed` (read by [`keys`] in the same
/// frame), so a draft that differs here was changed by the picker since.
pub(crate) fn sync(picker: Option<Res<Picker>>, mut text: TextFocus, mut was_open: Local<bool>) {
    let Some(picker) = picker else { return };
    let open = picker.open.is_some();
    if open && !*was_open {
        text.focus_draft(PATH, picker.draft.clone());
    } else if !open {
        text.blur(PATH);
    }
    *was_open = open;
    if text.draft(PATH).is_some_and(|d| *d != picker.draft) {
        text.set(PATH, picker.draft.clone());
    }
}

/// PreUpdate, after the kit's input system (`ui_kit::text::input::keys`,
/// which has already typed into the path field and consumed the keys it
/// used): while the picker is open,
///
/// - the path field's events: an edit follows the listing, Enter (Submit)
///   opens the typed path, Escape (Cancel) closes the picker; Tab traverses
///   through the shared navigation contract without discarding the draft;
/// - while the field does not have the keyboard: Escape closes; ordinary
///   Enter/Space and Tab are owned by the kit, never a feature key loop;
/// - the wheel scrolls its sections;
///
/// then the wheel messages are cleared and every held key released, so no
/// mode shortcut underneath fires.
///
/// Safety: the key states are released, never reset ([`release_held`]): a
/// reset would forget a held key without a release, so a robot walking on
/// a held W, or a hardware jog on a held Q/A, would never see the release
/// and keep moving. The picker does not open over robot mode's Leg
/// calibration panel (`switch::start` refuses), because it would cover the
/// panel's STOP button and take its Z/Escape keys.
///
/// Closed, it only keeps its wheel cursor at the newest message.
#[allow(clippy::too_many_arguments)]
pub(crate) fn keys(
    picker: Option<ResMut<Picker>>,
    // The field's messages are read first, then `TextFocus` acts (a `ParamSet`: one at a time).
    mut field: ParamSet<(MessageReader<FieldMsg>, TextFocus)>,
    wheel: Option<ResMut<Messages<MouseWheel>>>,
    codes: Option<ResMut<ButtonInput<KeyCode>>>,
    logical: Option<ResMut<ButtonInput<Key>>>,
    mut cursor: Local<Option<MessageCursor<MouseWheel>>>,
    mut lists: Query<&mut ScrollPosition, With<PickerList>>,
    mut out: MessageWriter<Act<WindowAction>>,
) {
    let events: Vec<FieldEvent> = field.p0().read().filter(|m| m.field == PATH).map(|m| m.event.clone()).collect();
    let open = picker.as_ref().is_some_and(|p| p.open.is_some());
    let (Some(mut picker), true) = (picker, open) else {
        // Closed: what arrives from now on is the picker's once it opens.
        if let Some(wheel) = &wheel {
            *cursor = Some(wheel.get_cursor_current());
        }
        return;
    };
    let delta: f32 = match &wheel {
        Some(messages) => cursor
            .get_or_insert_with(|| messages.get_cursor_current())
            .read(messages)
            .map(|e| match e.unit {
                MouseScrollUnit::Line => e.y * crate::ui_kit::WHEEL_LINE,
                MouseScrollUnit::Pixel => e.y,
            })
            .sum(),
        None => 0.0,
    };
    let (mut submit, mut close) = (false, false);
    let mut text = field.p1();
    for event in events {
        if submit || close {
            break;
        }
        match event {
            FieldEvent::Changed(draft) => picker.set_text_selected(draft),
            FieldEvent::Submit(_) => submit = true,
            // The kit has already taken the keyboard away.
            FieldEvent::Cancel => close = true,
            FieldEvent::Tab { .. } => {}
            FieldEvent::Blur => picker.revision += 1,
            FieldEvent::Arrow { .. } => {}
        }
    }
    // Escape remains modal cancellation. Ordinary activation and traversal
    // already ran through focused dispatch before this release.
    if !submit && !close && !text.focused(PATH) {
        let pressed = |key: Key| logical.as_ref().is_some_and(|l| l.just_pressed(key));
        if pressed(Key::Escape) {
            close = true;
        }
    }
    if close {
        picker.close();
        text.blur(PATH);
    } else if submit && let Some(request) = picker.typed() {
        out.write(Act::ui(WindowAction::Switch(request)));
    }
    if delta != 0.0 {
        for mut position in &mut lists {
            // `ui_kit::clamp_scroll_positions` clamps the far end after layout.
            position.y = (position.y - delta).max(0.0);
            picker.scroll = position.y;
        }
    }
    // Modal: nothing underneath sees the keys or the wheel, and every held
    // key is released (never reset: see the doc above).
    if let Some(mut codes) = codes {
        release_held(&mut *codes);
    }
    if let Some(mut logical) = logical {
        release_held(&mut *logical);
    }
    if let Some(mut wheel) = wheel {
        wheel.clear();
    }
}

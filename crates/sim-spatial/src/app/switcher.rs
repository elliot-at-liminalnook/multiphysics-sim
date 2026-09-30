//! The mode switcher: one small row of buttons in every mode, one of the
//! entry points of the mode switch (with `system_ui` `mode:*`, REST
//! `viewer_mode`, the builder's Lessons button and the lesson screen's
//! toggles), all writing the same `WindowAction::Switch`.
use super::actions::Act;
use super::switch::{Documents, ModeSwitch, Switcher, WindowAction};
use super::{Persistent, ViewerMode};
use crate::ui_kit::{DANGER, Kit, Look, SUBTLE, UiFonts, size};
use bevy::prelude::*;

#[derive(Component)]
pub(crate) struct ModeButton(ViewerMode);
#[derive(Component)]
pub(crate) struct SwitchMessage;

/// Startup: the switcher, above every mode's panels. The top of each mode
/// is full (toolbars and run controls), so it sits in the bottom-right
/// corner; its last message (refusals name the reason) is shown above it.
/// The modes are kit segments, the current one chosen.
pub(crate) fn spawn_switcher(mut commands: Commands, fonts: Res<UiFonts>, mode: Res<State<ViewerMode>>) {
    let k = Kit::new(&fonts);
    let current = *mode.get();
    commands
        .spawn((
            Persistent,
            // Floats in the bottom-right corner over every mode (layout only).
            Node { position_type: PositionType::Absolute, right: Val::Px(8.0), bottom: Val::Px(6.0), max_width: Val::Px(560.0), flex_direction: FlexDirection::Column, align_items: AlignItems::FlexEnd, row_gap: Val::Px(3.0), ..default() },
            GlobalZIndex(40),
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn((k.text("", size::CAPTION, SUBTLE, 0), SwitchMessage, Pickable::IGNORE));
            // A translucent backdrop: the switcher floats over the 3D views.
            root.spawn((k.segments(), BackgroundColor(crate::view::BACKDROP.with_alpha(0.85)))).with_children(|row| {
                for mode in ViewerMode::ALL {
                    row.spawn(k.button(mode.label(), ModeButton(mode), Look::Segment(mode == current), true));
                }
            });
        });
}

/// Input: a click on the switcher asks for a switch (no document: the mode
/// reopens its own, or is refused naming what it needs).
pub(crate) fn switcher_clicks(buttons: Query<(&Interaction, &ModeButton), Changed<Interaction>>, mut switch: MessageWriter<Act<WindowAction>>) {
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed {
            switch.write(Act::ui(WindowAction::Switch(ModeSwitch { mode: button.0, document: None })));
        }
    }
}

/// Present: the active mode highlighted, the last outcome shown.
pub(crate) fn update_switcher(mode: Res<State<ViewerMode>>, switch: Res<Switcher>, mut buttons: Query<(&ModeButton, &mut Look)>, mut message: Query<(&mut Text, &mut TextColor), With<SwitchMessage>>) {
    for (button, mut look) in &mut buttons {
        look.set_if_neq(Look::Segment(button.0 == *mode.get()));
    }
    if !switch.is_changed() {
        return;
    }
    let (line, colour) = match &switch.message {
        Some(Ok(text)) => (text.clone(), SUBTLE),
        Some(Err(text)) => (text.clone(), DANGER),
        None => (String::new(), SUBTLE),
    };
    for (mut text, mut text_colour) in &mut message {
        if text.0 != line {
            text.0 = line.clone();
        }
        if text_colour.0 != colour {
            text_colour.0 = colour;
        }
    }
}

/// Present: `/v1/viewer_mode` follows the mode and every outcome.
pub(crate) fn publish(mode: Res<State<ViewerMode>>, switch: Res<Switcher>, documents: Res<Documents>, rest: Option<Res<crate::rest::Rest>>, mut last: Local<Option<(ViewerMode, u64)>>) {
    let Some(rest) = rest else { return };
    let key = (*mode.get(), switch.revision);
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    rest.0.publish("viewer_mode", switch.json(*mode.get(), &documents));
}

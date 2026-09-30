//! The mode switcher: one small row of buttons in every mode, one of the
//! three entry points of the mode switch (with `system_ui` `mode:*` and REST
//! `viewer_mode`).
use super::switch::{Documents, ModeSwitch, Switcher};
use super::{Persistent, ViewerMode};
use bevy::prelude::*;

#[derive(Component)]
pub(crate) struct ModeButton(ViewerMode);
#[derive(Component)]
pub(crate) struct SwitchMessage;

const BUTTON: Color = Color::srgb(0.16, 0.20, 0.25);
const BUTTON_HOVER: Color = Color::srgb(0.20, 0.26, 0.32);
const BUTTON_ACTIVE: Color = Color::srgb(0.12, 0.32, 0.31);

/// The robot header's button (`robot::run_button`'s look) for one mode.
fn mode_button(fonts: &crate::builder::ui::UiFonts, mode: ViewerMode) -> impl Bundle {
    (
        Button,
        ModeButton(mode),
        Node { border_radius: BorderRadius::all(Val::Px(4.0)), padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)), ..default() },
        BackgroundColor(BUTTON),
        children![crate::robot::label(fonts, mode.label(), 12.0, crate::INK)],
    )
}

/// Startup: the switcher, above every mode's panels. The top of each mode
/// is full (toolbars and run controls), so it sits in the bottom-right
/// corner; its last message (refusals name the reason) is shown above it.
pub(crate) fn spawn_switcher(mut commands: Commands, fonts: Res<crate::builder::ui::UiFonts>) {
    commands.spawn((
        Persistent,
        Node { position_type: PositionType::Absolute, right: Val::Px(8.0), bottom: Val::Px(6.0), max_width: Val::Px(560.0), flex_direction: FlexDirection::Column, align_items: AlignItems::FlexEnd, row_gap: Val::Px(3.0), ..default() },
        GlobalZIndex(40),
        Pickable::IGNORE,
        children![
            (crate::robot::label(&fonts, "", 11.5, crate::MUTED), SwitchMessage, Pickable::IGNORE),
            (
                Node { border_radius: BorderRadius::all(Val::Px(5.0)), flex_direction: FlexDirection::Row, column_gap: Val::Px(3.0), padding: UiRect::all(Val::Px(3.0)), ..default() },
                BackgroundColor(Color::srgba(0.04, 0.06, 0.08, 0.85)),
                children![
                    mode_button(&fonts, ViewerMode::Inspect),
                    mode_button(&fonts, ViewerMode::Build),
                    mode_button(&fonts, ViewerMode::Lessons),
                    mode_button(&fonts, ViewerMode::Robot),
                    mode_button(&fonts, ViewerMode::Place),
                ]
            ),
        ],
    ));
}

/// Input: a click on the switcher submits a switch (no document: the mode
/// reopens its own, or is refused naming what it needs).
pub(crate) fn switcher_clicks(buttons: Query<(&Interaction, &ModeButton), Changed<Interaction>>, mut switch: ResMut<Switcher>) {
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed {
            switch.submit(ModeSwitch { mode: button.0, document: None });
        }
    }
}

/// Present: the active mode highlighted, the last outcome shown.
pub(crate) fn update_switcher(mode: Res<State<ViewerMode>>, switch: Res<Switcher>, mut buttons: Query<(&ModeButton, &Interaction, &mut BackgroundColor)>, mut message: Query<(&mut Text, &mut TextColor), With<SwitchMessage>>) {
    for (button, interaction, mut background) in &mut buttons {
        let colour = if button.0 == *mode.get() {
            BUTTON_ACTIVE
        } else if *interaction == Interaction::Hovered {
            BUTTON_HOVER
        } else {
            BUTTON
        };
        if background.0 != colour {
            background.0 = colour;
        }
    }
    if !switch.is_changed() {
        return;
    }
    let (line, colour) = match &switch.message {
        Some(Ok(text)) => (text.clone(), crate::MUTED),
        Some(Err(text)) => (text.clone(), crate::builder::ui::DANGER),
        None => (String::new(), crate::MUTED),
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

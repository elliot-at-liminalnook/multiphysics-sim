//! The mode switcher: one strip along the window's bottom in every mode,
//! one of the entry points of the mode switch (with `system_ui` `mode:*`,
//! REST `viewer_mode`, the builder's Lessons button and the lesson screen's
//! toggles), all writing the same `WindowAction::Switch`.
//!
//! Layout (window-first-usability): the switcher is the kit's
//! `Dock::Strip`, the `SWITCHER_STRIP` px every mode's docks end above, so
//! it never covers a mode's panels. The last outcome message is on the
//! left; it wraps within the strip and is clipped to two lines, with the
//! full text kept as the message's `AccessibleLabel`. The mode segments are
//! on the right.
use super::actions::Act;
use super::switch::{Documents, ModeSwitch, Switcher, WindowAction};
use crate::document::DocumentRegistry;
use super::{Persistent, ViewerMode};
use crate::ui_kit::{DANGER, Dock, Kit, Look, SUBTLE, UiFonts, size};
use bevy::prelude::*;
use bevy::ui::prelude::AccessibleLabel;

#[derive(Component)]
pub(crate) struct ModeButton(ViewerMode);
#[derive(Component)]
pub(crate) struct SwitchMessage;

/// Two caption lines (default line height 1.2 × `size::CAPTION` = 13.8 px
/// each): the message box clips anything longer.
const MESSAGE_HEIGHT: f32 = 28.0;

/// Startup: the switcher strip, above every mode's panels (z 40). The
/// message takes the free width on the left; the modes are kit segments on
/// the right, the current one chosen. The strip blocks picking (no
/// `Pickable` override), like any dock.
pub(crate) fn spawn_switcher(mut commands: Commands, fonts: Res<UiFonts>, mode: Res<State<ViewerMode>>) {
    let k = Kit::new(&fonts);
    let current = *mode.get();
    commands
        .spawn((
            Persistent,
            k.dock(Dock::Strip, Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, padding: UiRect::axes(Val::Px(10.0), Val::Px(0.0)), column_gap: Val::Px(12.0), ..default() }),
            GlobalZIndex(40),
        ))
        .with_children(|strip| {
            // The message box: the free width, at most two lines, clipped.
            strip
                .spawn((Node { flex_grow: 1.0, flex_shrink: 1.0, flex_basis: Val::Px(0.0), min_width: Val::Px(0.0), max_height: Val::Px(MESSAGE_HEIGHT), overflow: Overflow::clip(), ..default() }, Pickable::IGNORE))
                .with_children(|clip| {
                    clip.spawn((k.text("", size::CAPTION, SUBTLE, 0), Node { max_width: Val::Percent(100.0), ..default() }, SwitchMessage, AccessibleLabel::new(""), Pickable::IGNORE));
                });
            // The segments keep their content width: a holder that never
            // shrinks (the message box takes any shortfall), so the kit's
            // segments style stays the kit's.
            strip.spawn(Node { flex_shrink: 0.0, ..default() }).with_children(|holder| {
                holder.spawn(k.segments()).with_children(|row| {
                    for mode in ViewerMode::ALL {
                        row.spawn(k.button(mode.label(), ModeButton(mode), Look::Segment(mode == current), true));
                    }
                });
            });
        });
}

/// Input: a click on the switcher asks for a switch (no document: the mode
/// reopens its own, or is refused naming what it needs).
pub(crate) fn switcher_clicks(buttons: Query<&ModeButton, With<crate::ui_kit::activation::Activated>>, mut switch: MessageWriter<Act<WindowAction>>) {
    for button in &buttons {
        switch.write(Act::ui(WindowAction::Switch(ModeSwitch { mode: button.0, document: None, reveal: None })));
    }
}

/// Present: the active mode highlighted, the last outcome shown (and
/// given in full as the message's accessible label).
pub(crate) fn update_switcher(mut commands: Commands, mode: Res<State<ViewerMode>>, switch: Res<Switcher>, mut buttons: Query<(&ModeButton, &mut Look)>, mut message: Query<(Entity, &mut Text, &mut TextColor), With<SwitchMessage>>) {
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
    for (entity, mut text, mut text_colour) in &mut message {
        if text.0 != line {
            text.0 = line.clone();
            // `AccessibleLabel` is immutable: re-insert it to update it.
            commands.entity(entity).insert(AccessibleLabel::new(line.clone()));
        }
        if text_colour.0 != colour {
            text_colour.0 = colour;
        }
    }
}

/// Present: `/v1/viewer_mode` follows the mode, every outcome and every
/// change of the document registry.
pub(crate) fn publish(mode: Res<State<ViewerMode>>, switch: Res<Switcher>, documents: Res<Documents>, registry: Res<DocumentRegistry>, rest: Option<Res<crate::rest::Rest>>, mut last: Local<Option<(ViewerMode, u64, u64)>>) {
    let Some(rest) = rest else { return };
    let key = (*mode.get(), switch.revision, registry.changed);
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    rest.0.publish("viewer_mode", switch.json(*mode.get(), &documents, &registry));
}

#[cfg(test)]
mod activation_tests {
    use super::*;
    #[test]
    fn actual_switcher_segment_converts_shared_activation_to_existing_window_action() {
        let mut app=App::new();
        app.insert_resource(UiFonts { regular:default(),medium:default(),semibold:default(),
            italic:default(),mono:default(),icons:default() })
            .insert_resource(State::new(ViewerMode::Inspect))
            .add_message::<Act<WindowAction>>()
            .add_systems(Startup,spawn_switcher)
            .add_systems(Update,switcher_clicks);
        app.update();
        let world=app.world_mut();
        let entity=world.query::<(Entity,&ModeButton)>().iter(world)
            .find_map(|(e,b)|(b.0==ViewerMode::Build).then_some(e)).unwrap();
        assert!(world.get::<crate::ui_kit::activation::Ordinary>(entity).is_some());
        assert!(world.get::<bevy::ui_widgets::ActivateOnPress>(entity).is_some());
        world.entity_mut(entity).insert(crate::ui_kit::activation::Activated);
        app.update();
        let actions:Vec<_>=app.world_mut().resource_mut::<Messages<Act<WindowAction>>>().drain().collect();
        assert_eq!(actions.len(),1);
        let WindowAction::Switch(request)=&actions[0].action else{panic!("wrong action")};
        assert_eq!(request,&ModeSwitch{mode:ViewerMode::Build,document:None,reveal:None});
    }
}

//! Written isolated fixtures only; T47 does not execute them.
use super::*;
use std::collections::BTreeMap;

#[derive(Resource)]
struct FixtureSnapshot(CloseSnapshot);

fn fonts() -> UiFonts {
    UiFonts { regular: default(), italic: default(), mono: default(),
        medium: default(), semibold: default(), icons: BTreeMap::new() }
}

fn fixture_render(mut commands: Commands, fonts: Res<UiFonts>, state: Res<FixtureSnapshot>) {
    spawn_panel(&mut commands, &fonts, &state.0);
}

fn fixture(state: CloseSnapshot) -> App {
    let mut app = App::new();
    app.insert_resource(fonts()).insert_resource(FixtureSnapshot(state))
        .add_message::<Act<CloseAction>>()
        .add_systems(Startup, fixture_render)
        .add_systems(Update, clicks);
    app.world_mut().spawn(Window::default());
    app.update();
    app
}

fn actual_button(app: &mut App, action: CloseAction) -> Entity {
    let world = app.world_mut();
    world.query_filtered::<(Entity, &CloseAction), With<Button>>()
        .iter(world).find_map(|(entity, a)| (*a == action).then_some(entity)).unwrap()
}

fn windows(app: &mut App) -> usize {
    let mut query = app.world_mut().query::<&Window>();
    query.iter(app.world()).count()
}

#[test]
fn idle_controls_are_actual_kit_buttons_and_leave_window_alive() {
    let mut app = fixture(CloseSnapshot { request_enabled: true, ..default() });
    let entity = actual_button(&mut app, CloseAction::CloseRequest);
    assert!(app.world().get::<Enabled>(entity).unwrap().0);
    assert!(app.world().get::<crate::ui_kit::activation::Ordinary>(entity).is_some());
    assert!(app.world().get::<bevy::ui_widgets::ActivateOnPress>(entity).is_some());
    assert_eq!(controls_in(app.world())[0]["id"], "close:request");
    assert_eq!(windows(&mut app), 1);
    let mut roots = app.world_mut().query_filtered::<Entity, With<ClosePanel>>();
    let root = roots.iter(app.world()).next().unwrap();
    assert!(app.world().get::<Persistent>(root).is_some());
}

#[test]
fn pending_failed_publication_renders_retry_cancel_and_scoped_acknowledgment() {
    let mut app = fixture(CloseSnapshot {
        pending: true, status: "Publication failed".into(),
        retry_enabled: true, cancel_enabled: true, preference_exit_enabled: true,
        ..default()
    });
    for action in [CloseAction::CloseRetry, CloseAction::CloseCancel, CloseAction::CloseWithoutPreferences] {
        let entity = actual_button(&mut app, action);
        assert!(app.world().get::<Enabled>(entity).unwrap().0);
        assert!(app.world().get::<AccessibleLabel>(entity).is_some());
    }
    let texts: Vec<String> = app.world_mut().query::<&Text>().iter(app.world()).map(|text| text.0.clone()).collect();
    assert!(texts.iter().any(|text| text == "Publication failed"));
    assert!(texts.iter().any(|text| text.contains("New preference changes require another acknowledgment")));
}

#[test]
fn authored_blocker_disables_actual_preference_loss_button_and_click_producer() {
    let mut app = fixture(CloseSnapshot {
        pending: true, status: "Preserve authored work".into(),
        blockers: vec!["Finish or cancel the active study form".into()],
        cancel_enabled: true, preference_exit_enabled: false, ..default()
    });
    let loss = actual_button(&mut app, CloseAction::CloseWithoutPreferences);
    app.world_mut().entity_mut(loss).insert(crate::ui_kit::activation::Activated);
    app.update();
    assert_eq!(app.world().resource::<Messages<Act<CloseAction>>>().len(), 0);
    assert_eq!(windows(&mut app), 1);
    let cancel = actual_button(&mut app, CloseAction::CloseCancel);
    app.world_mut().entity_mut(cancel).insert(crate::ui_kit::activation::Activated);
    app.update();
    let actions: Vec<_> = app.world_mut().resource_mut::<Messages<Act<CloseAction>>>().drain().collect();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].action, CloseAction::CloseCancel);
}

#[test]
fn request_button_only_writes_the_shared_typed_action() {
    let mut app = fixture(CloseSnapshot { request_enabled: true, ..default() });
    let entity = actual_button(&mut app, CloseAction::CloseRequest);
    app.world_mut().entity_mut(entity).insert(crate::ui_kit::activation::Activated);
    app.update();
    let actions: Vec<_> = app.world_mut().resource_mut::<Messages<Act<CloseAction>>>().drain().collect();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].action, CloseAction::CloseRequest);
    assert_eq!(windows(&mut app), 1);
}

#[test]
fn unchanged_owner_projection_keeps_actual_controls_across_every_mode() {
    let mut app = App::new();
    app.insert_resource(fonts()).init_resource::<CloseOwner>()
        .add_systems(Update, render);
    app.update();
    let button = actual_button(&mut app, CloseAction::CloseRequest);
    // The global root has no DespawnOnExit; changing the mode cannot replace
    // it. Idle frame polling must retain the entity and its interaction state.
    for mode in crate::app::ViewerMode::ALL {
        app.insert_resource(State::new(mode));
        app.update();
        assert_eq!(actual_button(&mut app, CloseAction::CloseRequest), button);
    }
}

/// Actual OS request, renderer, pinned dispatch and authoritative cancellation.
#[test]
fn pending_close_keyboard_scope_over_retained_form_restores_focus() {
    use crate::ui_kit::activation::{self, ModalFocus};
    use crate::ui_kit::text::{TextEntryPlugin, TextFieldApp, TextField, FieldId, TextDraft};
    use bevy::input_focus::{InputFocus, FocusCause, InputFocusSystems, dispatch_focused_input};
    use bevy::input::keyboard::{KeyboardInput, Key};
    const FIELD: FieldId = FieldId("close.fixture.form");
    let mut app = App::new();
    app.insert_resource(fonts()).add_plugins(TextEntryPlugin)
        .add_text_field(FIELD, TextField::new("Retained CAD draft"))
        .init_resource::<crate::app::actions::Replies>()
        .init_resource::<crate::app::settings::SettingsOwner>()
        .add_message::<bevy::window::WindowCloseRequested>()
        .add_plugins(super::super::ClosePlugin)
        .add_systems(PreUpdate, dispatch_focused_input::<KeyboardInput>
            .in_set(InputFocusSystems::Dispatch).after(bevy::input::InputSystems));
    crate::app::configure_sets(&mut app);
    activation::install(&mut app);
    let window = app.world_mut().spawn((Window::default(), bevy::window::PrimaryWindow)).id();
    let font = fonts(); let k = Kit::new(&font);
    let underlying = app.world_mut().spawn((Node::default(), ModalFocus, AccessibleLabel("Retained CAD form".into()))).id();
    let anchor = app.world_mut().spawn((k.input("unsubmitted", "draft", CloseAction::CloseStatus, true), ChildOf(underlying))).id();
    app.update(); app.update();
    let field = app.world_mut().query::<(Entity, &FieldId)>().iter(app.world()).find_map(|(e,id)| (*id == FIELD).then_some(e)).unwrap();
    let identity = app.world().get::<activation::InputIdentity>(anchor).unwrap().0.clone();
    {
        let mut text = app.world_mut().get_mut::<TextField>(field).unwrap();
        text.draft = TextDraft::new("unsubmitted", false);
        text.focus_anchor = Some(anchor); text.navigation_identity = Some(identity);
    }
    app.world_mut().resource_mut::<InputFocus>().set(field, FocusCause::Navigated);
    app.update();
    let saved = app.world().get::<TextField>(field).unwrap().draft.clone();
    app.world_mut().write_message(bevy::window::WindowCloseRequested { window });
    app.update(); app.update();
    let cancel = actual_button(&mut app, CloseAction::CloseCancel);
    let close_root = app.world_mut().query_filtered::<Entity, With<ClosePanel>>().iter(app.world()).next().unwrap();
    assert_eq!(app.world().get::<activation::ModalPriority>(close_root).unwrap().0, 100);
    // Traverse through the pinned window observer, never manufacture FocusedInput.
    for _ in 0..4 {
        if app.world().resource::<InputFocus>().get() == Some(cancel) { break; }
        app.world_mut().resource_mut::<Messages<KeyboardInput>>().write(KeyboardInput {
            key_code: KeyCode::Tab, logical_key: Key::Tab, state: bevy::input::ButtonState::Pressed,
            text: None, repeat: false, window,
        });
        app.update();
    }
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(cancel));
    app.world_mut().resource_mut::<Messages<KeyboardInput>>().write(KeyboardInput {
        key_code: KeyCode::Enter, logical_key: Key::Enter, state: bevy::input::ButtonState::Pressed,
        text: None, repeat: false, window,
    });
    app.update();
    assert!(!app.world().resource::<CloseOwner>().pending());
    // Production render replaces the cancelled pending projection. The next
    // PreUpdate restores its suspended durable editor through the modal stack.
    app.update();
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));
    assert_eq!(app.world().get::<TextField>(field).unwrap().draft, saved);
}

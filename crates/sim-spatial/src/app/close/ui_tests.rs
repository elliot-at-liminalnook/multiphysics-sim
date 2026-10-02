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

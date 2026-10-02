//! Written source fixtures only; this batch does not execute them.
use super::*;
fn fixture(settings:SettingsOwner)->(App,Entity) {
    let mut app=App::new();
    actions::register::<CloseAction>(&mut app);
    app.init_resource::<Replies>().init_resource::<CloseOwner>()
        .init_resource::<StudyOwner>().init_resource::<StudyUi>()
        .insert_resource(settings)
        .add_systems(Update,apply).add_systems(Last,authorize);
    let window=app.world_mut().spawn(Window::default()).id();
    (app,window)
}
fn action(app:&mut App,action:CloseAction) { app.world_mut().write_message(Act::ui(action)); app.update(); }
fn finish(mut app:App) { app.world_mut().resource_mut::<SettingsOwner>().ready=false; }

#[test]
fn ordinary_window_request_and_all_mode_wire_commands_reach_the_same_owner() {
    let (mut app, window) = fixture(SettingsOwner::default());
    app.add_message::<WindowCloseRequested>()
        .add_systems(Update, window_input.before(apply));
    app.world_mut().write_message(WindowCloseRequested { window });
    app.update();
    assert!(app.world().resource::<CloseOwner>().pending());
    assert!(app.world().get::<Window>(window).is_some());
    for mode in super::super::ViewerMode::ALL {
        let command = sim_api::Command {
            command: "system_ui".into(),
            args: serde_json::json!({"action":{"operation":"activate","id":"close:cancel"}}),
        };
        assert_eq!(super::super::route::route(mode, true, &command).unwrap().name, "close");
        assert_eq!(CloseAction::parse(&command).unwrap(), CloseAction::CloseCancel);
        assert!(super::super::route::route(mode, false, &command).is_err());
    }
    action(&mut app, CloseAction::CloseCancel);
    assert!(app.world().get::<Window>(window).is_some());
    finish(app);
}
#[test]
fn acknowledged_clean_close_keeps_window_until_second_authorization() {
    let (mut app,window)=fixture(SettingsOwner::fixture_durable());
    action(&mut app,CloseAction::CloseRequest);
    assert!(app.world().get::<Window>(window).is_some());
    assert!(app.world().get::<ClosingWindow>(window).is_some());
    app.update();
    assert!(app.world().get::<Window>(window).is_none());
    finish(app);
}
#[test]
fn close_during_load_is_durable_and_cancel_keeps_window() {
    let (mut app,window)=fixture(SettingsOwner::default());
    action(&mut app,CloseAction::CloseRequest);
    for _ in 0..4 { app.update(); }
    assert!(app.world().resource::<CloseOwner>().pending());
    assert!(app.world().get::<ClosingWindow>(window).is_none());
    action(&mut app,CloseAction::CloseRequest);
    assert!(app.world().resource::<CloseOwner>().pending());
    action(&mut app,CloseAction::CloseCancel);
    assert!(!app.world().resource::<CloseOwner>().pending());
    assert!(app.world().get::<Window>(window).is_some());
    finish(app);
}
#[test]
fn preference_only_acknowledgement_is_invalidated_by_new_recent() {
    let (mut app,window)=fixture(SettingsOwner::default());
    action(&mut app,CloseAction::CloseRequest);
    action(&mut app,CloseAction::CloseWithoutPreferences);
    assert!(app.world().get::<ClosingWindow>(window).is_some());
    app.world_mut().resource_mut::<SettingsOwner>().record(super::super::ViewerMode::Cad,
        super::super::switch::Document::Url("http://127.0.0.1:8420".into()));
    app.update();
    assert!(app.world().get::<Window>(window).is_some());
    assert!(app.world().get::<ClosingWindow>(window).is_none());
    assert!(app.world().resource::<CloseOwner>().bypass.is_none());
    finish(app);
}
#[test]
fn active_text_refuses_request_and_preference_bypass_preserving_buffer() {
    let (mut app,window)=fixture(SettingsOwner::default());
    { let mut ui=app.world_mut().resource_mut::<StudyUi>();
      ui.focus=Some((None,crate::builder::calibration::study::forms::Field::Archive));
      ui.buffer="retained path draft".into(); }
    action(&mut app,CloseAction::CloseRequest);
    action(&mut app,CloseAction::CloseWithoutPreferences);
    assert!(!app.world().resource::<CloseOwner>().pending());
    assert!(app.world().get::<Window>(window).is_some());
    assert_eq!(app.world().resource::<StudyUi>().buffer,"retained path draft");
    finish(app);
}
#[test]
fn newer_revision_revokes_previously_armed_authorization_and_stale_ack() {
    let (mut app,window)=fixture(SettingsOwner::fixture_durable());
    action(&mut app,CloseAction::CloseRequest);
    { let mut settings=app.world_mut().resource_mut::<SettingsOwner>();
      let mut cad=settings.cad.clone();cad.clearance+=0.1;settings.set_cad(cad).unwrap();
      settings.fixture_acknowledge(0); }
    app.update();
    assert!(app.world().get::<Window>(window).is_some());
    assert!(app.world().get::<ClosingWindow>(window).is_none());
    finish(app);
}
#[test]
fn blocker_arriving_after_acknowledgment_revokes_destruction() {
    let (mut app,window)=fixture(SettingsOwner::fixture_durable());
    action(&mut app,CloseAction::CloseRequest);
    app.world_mut().resource_mut::<StudyUi>().buffer="late draft".into();
    app.world_mut().resource_mut::<StudyUi>().focus=Some((None,crate::builder::calibration::study::forms::Field::Archive));
    app.update();
    assert!(app.world().get::<Window>(window).is_some());
    assert!(app.world().get::<ClosingWindow>(window).is_none());
    finish(app);
}

#[test]
fn lifecycle_loading_status_drives_actual_enabled_controls_and_window_lifetime() {
    let (mut app,window)=fixture(SettingsOwner::default());
    app.insert_resource(crate::ui_kit::UiFonts {
        regular:default(),italic:default(),mono:default(),medium:default(),semibold:default(),
        icons:std::collections::BTreeMap::new(),
    }).add_systems(Update,(present,ui::render).chain().after(apply));
    action(&mut app,CloseAction::CloseRequest);
    let controls=ui::controls_in(app.world());
    for id in ["close:retry","close:cancel","close:without_preferences"] {
        assert!(controls.iter().any(|row|row["id"]==id && row["enabled"]==true));
    }
    assert!(app.world().get::<Window>(window).is_some());
    assert!(app.world().get::<ClosingWindow>(window).is_none());
    action(&mut app,CloseAction::CloseCancel);
    assert!(ui::controls_in(app.world()).iter().any(|row|row["id"]=="close:request" && row["enabled"]==true));
    finish(app);
}
#[test]
fn pending_evidence_refuses_close_and_preference_only_exit() {
    use crate::builder::calibration::study::jobs::{PendingJob,JobKind,JobOutput};
    let (mut app,window)=fixture(SettingsOwner::default());
    app.world_mut().resource_mut::<StudyOwner>().pending.push(PendingJob {
        id:1,kind:JobKind::Save,stamp:None,document:None,source:"fixture-review.json".into(),
        trial_ids:vec![],launch:serde_json::Value::Null,cancel_requested:false,
        job:crate::jobs::Job::finished(1,Ok(JobOutput::Published)),selection_epoch:0,gate:None,captured:None,
    });
    action(&mut app,CloseAction::CloseRequest);
    action(&mut app,CloseAction::CloseWithoutPreferences);
    assert!(app.world().get::<Window>(window).is_some());
    assert!(!app.world().resource::<CloseOwner>().pending());
    assert_eq!(app.world().resource::<StudyOwner>().pending.len(),1);
    assert!(app.world().resource::<CloseOwner>().snapshot().blockers.iter().any(|s|s.contains("pending")));
    finish(app);
}
#[test]
fn dirty_retained_study_refuses_even_preference_only_exit() {
    let root=crate::workspace::root().unwrap();
    let archive=sim_runtime::experiment_comparison::hx_archive::load(
        &root.join(crate::builder::calibration::DEFAULT_ARCHIVE),root).unwrap();
    let study=sim_runtime::experiment_study::Study::new(archive).unwrap();
    let (mut app,window)=fixture(SettingsOwner::default());
    app.world_mut().resource_mut::<StudyOwner>().retain(study,"fixture archive".into(),None,false,true);
    action(&mut app,CloseAction::CloseRequest);
    action(&mut app,CloseAction::CloseWithoutPreferences);
    assert!(app.world().get::<Window>(window).is_some());
    assert!(!app.world().resource::<CloseOwner>().pending());
    assert!(app.world().resource::<StudyOwner>().active().unwrap().dirty());
    finish(app);
}

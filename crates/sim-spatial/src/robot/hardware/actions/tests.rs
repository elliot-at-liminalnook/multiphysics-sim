//! Written isolated fixtures; no settings paths, jobs or hardware clients.
use super::*;
use crate::robot::hardware::{HardwareConfig, settings};

#[test]
fn close_stop_is_immediate_while_preferences_are_loading() {
    use crate::app::{close::{CloseAction, ClosePlugin}, actions::Act, settings::SettingsOwner};
    let mut hw = Hardware::new(HardwareConfig::default(), settings::Settings::default());
    hw.form.held_upper = true;
    hw.form.held_lower = true;
    let mut app = App::new();
    app.add_message::<bevy::window::WindowCloseRequested>()
        .init_resource::<crate::app::actions::Replies>()
        .insert_resource(SettingsOwner::default())
        .insert_resource(hw)
        .insert_resource(crate::ui_kit::UiFonts {
            regular:default(), italic:default(), mono:default(), medium:default(), semibold:default(),
            icons:std::collections::BTreeMap::new(),
        });
    crate::app::configure_sets(&mut app);
    app.add_plugins(ClosePlugin);
    let window = app.world_mut().spawn(Window::default()).id();
    app.world_mut().write_message(Act::ui(CloseAction::CloseRequest));
    app.update();
    let hw = app.world().resource::<Hardware>();
    assert!(!hw.form.held_upper && !hw.form.held_lower);
    assert!(app.world().resource::<crate::app::close::CloseOwner>().pending());
    assert!(app.world().get::<Window>(window).is_some());
    assert!(!app.world().resource::<SettingsOwner>().drain_ready());
    assert!(hw.link.is_none() && !hw.sync.engaged());
}

#[test]
fn late_publication_changes_only_inactive_remembered_choices() {
    let mut hw = Hardware::new(HardwareConfig::default(), settings::Settings::default());
    hw.form.target_percent = 31.0;
    hw.form.step = 7;
    let mut loaded = settings::Settings::default();
    loaded.calibration.drive_mode = Some(DriveMode::ServoSpeed);
    loaded.calibration.hold_others = Some(false);
    loaded.mirror.bindings.insert(3, settings::MirrorBinding {
        joint: "Foot servo output".into(), polarity: -1, align: Align::Mid,
    });
    loaded.sync.leg = Some("-X".into());
    loaded.sync.amplitude = Some(0.05);
    loaded.sync.bindings.push(settings::SyncBinding {
        coordinate: "joint.-X | Hip servo output".into(), motor_id: 4, polarity: -1,
    });
    seed_preferences(&mut hw, &loaded);
    assert!(hw.preferences_loaded);
    assert_eq!(hw.settings, loaded);
    assert_eq!(hw.form.inputs.drive_mode, DriveMode::ServoSpeed);
    assert!(!hw.form.inputs.hold_others);
    assert_eq!(hw.form.target_percent, 31.0);
    assert_eq!(hw.form.step, 7);
    assert!(!hw.form.tune_ok && !hw.form.campaign_ok && !hw.form.gait_ok);
    assert!(!hw.form.held_upper && !hw.form.held_lower);
    assert!(hw.link.is_none() && hw.connecting.is_none());
    assert!(hw.stops.is_empty());
    assert!(!hw.sync.engaged());
    assert!(!hw.sync.connected() && !hw.sync.connecting());
    assert_eq!(hw.sync.to_save(), loaded.sync);
    assert_eq!(hw.mirror.to_save(), loaded.mirror);
}

#[test]
fn publication_does_not_replace_transient_confirmations_or_jog_state() {
    let mut hw = Hardware::new(HardwareConfig::default(), settings::Settings::default());
    hw.form.tune_ok = true;
    hw.form.held_upper = true;
    seed_preferences(&mut hw, &settings::Settings::default());
    assert!(hw.form.tune_ok && hw.form.held_upper);
    stop_immediate(&mut hw);
    assert!(!hw.form.held_upper && !hw.form.held_lower);
}

#[test]
fn preference_dispatch_inventory_includes_equal_value_choices() {
    assert!(!preference_paths(&HardwareAction::DriveMode { mode: DriveMode::Pwm }).is_empty());
    assert!(!preference_paths(&HardwareAction::MirrorEnabled { on: true }).is_empty());
    assert!(!preference_paths(&HardwareAction::SyncScale { scale: 0.03 }).is_empty());
    assert!(preference_paths(&HardwareAction::TuneConfirm { on: true }).is_empty());
    assert!(preference_paths(&HardwareAction::Stop).is_empty());
}

#[test]
fn claims_are_narrow_and_mirror_pose_reference_is_distinct_from_joint() {
    assert_eq!(preference_paths(&HardwareAction::MirrorAlign { id: 3, align: Align::Home }), vec!["/mirror/bindings/3/align"]);
    assert_eq!(preference_paths(&HardwareAction::MirrorJoint { id: 3, joint: "Foot servo output".into() }), vec!["/mirror/bindings/3/joint"]);
    assert_eq!(preference_paths(&HardwareAction::SyncLeg { leg: "-X".into() }), vec!["/sync/leg", "/sync/bindings"]);
    assert_eq!(preference_paths(&HardwareAction::SyncStart), vec!["/sync"]);
}

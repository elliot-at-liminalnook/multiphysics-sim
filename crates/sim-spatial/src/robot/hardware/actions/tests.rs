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

// Remote gait authorization (HW-10/HW-11): written and source-inspected, not
// executed. Pure: `authorize_with` on constructed snapshots, no link thread.
mod remote_gait {
    use super::*;
    use crate::robot::hardware::link::{LinkSnapshot, STALE_AFTER};
    use sim_runtime::hardware_client::calibration::ExecutionIdentity;
    use std::time::{Duration, Instant};

    fn identity(kind: &str) -> ExecutionIdentity {
        ExecutionIdentity {
            schema_version: 1,
            kind: kind.into(),
            server_instance: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into(),
            bench_instance: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into(),
        }
    }

    /// A link of generation 7 pinned to a virtual bench, its status read `now`, its bus connected.
    fn fresh(now: Instant) -> LinkSnapshot {
        let mut s = LinkSnapshot { generation: 7, execution: Some(identity("virtual_calibration")), connection_valid: true, read_at: Some(now), ..Default::default() };
        s.state.connected = true;
        s
    }

    /// Every gait intent `starts_motion` lists.
    fn gait_intents() -> Vec<HardwareAction> {
        vec![
            HardwareAction::GaitSelect { index: 0 },
            HardwareAction::GaitMode { mode: GaitMode::Leg },
            HardwareAction::GaitMode { mode: GaitMode::Both },
            HardwareAction::GaitSpeed { percent: 50.0 },
            HardwareAction::GaitEffort { percent: 40.0 },
            HardwareAction::GaitConfirm { on: true },
            HardwareAction::GaitPlay,
        ]
    }

    #[test]
    fn gait_intents_pass_on_a_fresh_pinned_virtual_link_of_the_current_generation() {
        let now = Instant::now();
        let s = fresh(now);
        for action in gait_intents() {
            assert!(action.starts_motion(), "{action:?}");
            assert_eq!(action.authorize_with(Some((7, &s)), 7, now), Ok(()), "{action:?}");
        }
    }

    #[test]
    fn gait_intents_are_refused_on_every_other_link() {
        let now = Instant::now();
        let ok = fresh(now);
        let mut cases: Vec<(&str, LinkSnapshot, u64, u64)> = vec![
            ("physical identity", LinkSnapshot { execution: Some(identity("physical")), ..ok.clone() }, 7, 7),
            ("no identity", LinkSnapshot { execution: None, ..ok.clone() }, 7, 7),
            ("stale status", LinkSnapshot { read_at: Some(now - STALE_AFTER - Duration::from_millis(1)), ..ok.clone() }, 7, 7),
            ("never read", LinkSnapshot { read_at: None, ..ok.clone() }, 7, 7),
            ("revoked", LinkSnapshot { authorization_revoked: true, ..ok.clone() }, 7, 7),
            ("connection lost", LinkSnapshot { connection_valid: false, ..ok.clone() }, 7, 7),
            ("disconnected", LinkSnapshot { disconnected: Some("the server reports its bus disconnected".into()), ..ok.clone() }, 7, 7),
            ("snapshot of another generation", LinkSnapshot { generation: 6, ..ok.clone() }, 7, 7),
            // The panel reconnected since (a newer generation), or the link is an older one.
            ("replaced generation", ok.clone(), 7, 8),
            ("older link", ok.clone(), 6, 7),
        ];
        let mut bus_down = ok.clone();
        bus_down.state.connected = false;
        cases.push(("bus not connected", bus_down, 7, 7));
        for action in gait_intents() {
            for (why, s, link_generation, generation) in &cases {
                assert!(action.authorize_with(Some((*link_generation, s)), *generation, now).is_err(), "{action:?} on {why}");
            }
            assert_eq!(action.authorize_with(None, 7, now), Err(action.remote_refusal()), "{action:?} without a link");
            // The real entry point without a link refuses the same way.
            let hw = Hardware::new(HardwareConfig::default(), settings::Settings::default());
            assert!(action.authorize(&hw, now).is_err(), "{action:?}");
        }
    }

    #[test]
    fn stop_and_the_gait_stop_are_never_refused() {
        let now = Instant::now();
        let hw = Hardware::new(HardwareConfig::default(), settings::Settings::default());
        let mut lost = fresh(now);
        lost.execution = None;
        lost.authorization_revoked = true;
        for action in [HardwareAction::Stop, HardwareAction::GaitStop] {
            assert!(!action.starts_motion(), "{action:?}");
            assert_eq!(action.authorize(&hw, now), Ok(()), "{action:?} without a link");
            assert_eq!(action.authorize_with(Some((7, &lost)), 8, now), Ok(()), "{action:?} on a lost link");
        }
    }

    #[test]
    fn bindings_flip_and_raw_step_stay_refused_remotely_on_a_virtual_link() {
        let now = Instant::now();
        let s = fresh(now);
        for action in [
            HardwareAction::MirrorPolarity { id: 1, polarity: -1 },
            HardwareAction::MirrorJoint { id: 1, joint: "Foot servo output".into() },
            HardwareAction::MirrorLeg { leg: "+X".into() },
            HardwareAction::MirrorAlign { id: 1, align: Align::Mid },
            HardwareAction::Flip,
            HardwareAction::RawStep,
            HardwareAction::RawStepValue { delta: 5 },
            HardwareAction::SyncStart,
        ] {
            assert_eq!(action.authorize_with(Some((7, &s)), 7, now), Err(action.remote_refusal()), "{action:?}");
        }
    }
}

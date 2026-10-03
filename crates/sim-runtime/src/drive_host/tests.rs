//! The drive host's pure parts (no Python process is started here):
//! TwistState's request rules, the deadman on sim time, halt, the end of a
//! replay (moved from sim-spatial's robot run tests with TwistState), the
//! REST field rules and the interpretation against the wheeled robot's
//! profile. Written by reading; not yet executed.
use super::*;
use sim_domain_control::drive::kinematics::{BodyTwist, Deadman, Limits, OnLoss};

const PERIOD: f64 = 0.02;
fn limits() -> Limits {
    Limits { supported: [true, false, true], max_speed: [0.3, 0.0, 3.0], max_accel: [0.6, 0.6, 6.0] }
}
fn ramp() -> Deadman {
    Deadman { timeout_s: 0.5, on_loss: OnLoss::Ramp { decel: [1.2, 1.2, 12.0] } }
}

#[test]
fn twist_requests_raise_the_heartbeat_and_are_limited_on_sim_time() {
    let (l, d) = (limits(), ramp());
    let mut s = TwistState::default();
    // Before any request: zero request at t = 0, heartbeat 0.
    assert_eq!(s.advance(0.0, PERIOD, &l, &d).unwrap(), [0.0, 0.0, 0.0, 0.0]);
    s.request(BodyTwist::new(0.3, 0.0, 1.0), false, PERIOD, &l).unwrap();
    assert_eq!((s.heartbeat, s.last_request_s), (1, PERIOD));
    // One period moves each axis by at most max_accel × period.
    let a = s.advance(PERIOD, PERIOD, &l, &d).unwrap();
    assert!((a[0] - 0.6 * PERIOD).abs() < 1e-12 && (a[2] - 6.0 * PERIOD).abs() < 1e-12 && a[1] == 0.0 && a[3] == 1.0, "{a:?}");
    // Fresh requests keep the deadman alive; the twist reaches the request and never exceeds the profile.
    let mut t = 2.0 * PERIOD;
    for _ in 0..40 {
        s.request(BodyTwist::new(0.3, 0.0, 1.0), false, t, &l).unwrap();
        let a = s.advance(t, PERIOD, &l, &d).unwrap();
        assert!(a[0] <= 0.3 && a[2] <= 3.0 && !s.expired, "{a:?}");
        t += PERIOD;
    }
    assert_eq!((s.commanded.forward_m_s, s.commanded.yaw_rad_s), (0.3, 1.0));
    assert_eq!(s.heartbeat, 41);
    // Refused by name, with no heartbeat change: an unsupported axis and a speed beyond the profile.
    let e = s.request(BodyTwist::new(0.0, 0.1, 0.0), false, t, &l).unwrap_err();
    assert!(e.contains("lateral") && e.contains("not supported"), "{e}");
    let e = s.request(BodyTwist::new(0.31, 0.0, 0.0), false, t, &l).unwrap_err();
    assert!(e.contains("forward") && e.contains("0.3"), "{e}");
    assert!(s.request(BodyTwist::new(f64::NAN, 0.0, 0.0), false, t, &l).unwrap_err().contains("finite"));
    assert_eq!(s.heartbeat, 41);
}

#[test]
fn the_deadman_expires_on_sim_time_without_fresh_requests() {
    let l = limits();
    let mut s = TwistState { commanded: BodyTwist::new(0.3, 0.0, 0.0), request: BodyTwist::new(0.3, 0.0, 0.0), heartbeat: 7, ..TwistState::default() };
    // Ramp: live until age 0.5 s (exclusive), then the request is ignored and the twist ramps down at stop_decel.
    let mut t = 0.0;
    while t < 0.5 - 1e-9 {
        s.advance(t, PERIOD, &l, &ramp()).unwrap();
        assert!(!s.expired && s.commanded.forward_m_s == 0.3, "t = {t}");
        t += PERIOD;
    }
    let a = s.advance(0.5, PERIOD, &l, &ramp()).unwrap();
    assert!(s.expired && (a[0] - (0.3 - 1.2 * PERIOD)).abs() < 1e-12, "{a:?}");
    assert_eq!(a[3], 7.0, "the heartbeat does not change without a request");
    // Pausing passes no sim time: the status age is frozen at the last period's time.
    assert!((s.status(0.5).age_s - 0.5).abs() < 1e-12);
    // Immediate: zero at once.
    let mut s = TwistState { commanded: BodyTwist::new(0.3, 0.0, 0.0), request: BodyTwist::new(0.3, 0.0, 0.0), ..TwistState::default() };
    let a = s.advance(0.6, PERIOD, &l, &Deadman { timeout_s: 0.5, on_loss: OnLoss::Immediate }).unwrap();
    assert!(s.expired && a[..3] == [0.0, 0.0, 0.0]);
    // A fresh request revives it.
    s.request(BodyTwist::new(0.1, 0.0, 0.0), false, 0.62, &l).unwrap();
    s.advance(0.62, PERIOD, &l, &ramp()).unwrap();
    assert!(!s.expired && s.commanded.forward_m_s > 0.0);
}

#[test]
fn halt_zeroes_the_request_and_the_commanded_twist_at_once() {
    let (l, d) = (limits(), ramp());
    let mut s = TwistState { commanded: BodyTwist::new(0.3, 0.0, 2.0), request: BodyTwist::new(0.3, 0.0, 2.0), heartbeat: 3, ..TwistState::default() };
    s.request(BodyTwist::ZERO, true, 1.0, &l).unwrap();
    assert_eq!((s.request, s.commanded, s.heartbeat, s.halted), (BodyTwist::ZERO, BodyTwist::ZERO, 4, true));
    assert_eq!(s.advance(1.0, PERIOD, &l, &d).unwrap(), [0.0, 0.0, 0.0, 4.0]);
    // A stop (not a halt) only requests zero: the twist ramps down under max_accel.
    let mut s = TwistState { commanded: BodyTwist::new(0.3, 0.0, 0.0), request: BodyTwist::new(0.3, 0.0, 0.0), ..TwistState::default() };
    s.request(BodyTwist::ZERO, false, 0.0, &l).unwrap();
    let a = s.advance(0.0, PERIOD, &l, &d).unwrap();
    assert!((a[0] - (0.3 - 0.6 * PERIOD)).abs() < 1e-12 && !s.halted, "{a:?}");
}

#[test]
fn a_finished_drive_replay_leaves_no_request_driving() {
    let (l, d) = (limits(), ramp());
    let mut s = TwistState::default();
    // Recorded periods starting at 0 and PERIOD; a new heartbeat is stamped at its period's start, like a live request.
    s.replayed(&[0.3, 0.0, 1.0, 1.0], 0.0);
    assert_eq!((s.last_request_s, s.heartbeat, s.request, s.periods), (0.0, 1, BodyTwist::new(0.3, 0.0, 1.0), 1));
    s.replayed(&[0.3, 0.0, 1.0, 1.0], PERIOD);
    assert_eq!(s.last_request_s, 0.0, "the same heartbeat is no fresh request");
    // The replay ends at 2 × PERIOD: no request, the deadman counts as expired; heartbeat and periods are kept.
    s.replay_ended(2.0 * PERIOD, &d);
    assert!(s.request.is_zero() && s.expired && !s.halted && s.heartbeat == 1 && s.periods == 2, "{s:?}");
    assert!(s.status(2.0 * PERIOD).age_s >= d.timeout_s);
    // The next live period (Run after the replay) ramps down at stop_decel instead of driving the last recorded twist.
    let a = s.advance(2.0 * PERIOD, PERIOD, &l, &d).unwrap();
    assert!(s.expired && (a[0] - (0.3 - 1.2 * PERIOD)).abs() < 1e-12 && (a[2] - (1.0 - 12.0 * PERIOD)).abs() < 1e-12, "{a:?}");
    // A fresh request drives again.
    s.request(BodyTwist::new(0.3, 0.0, 0.0), false, 3.0 * PERIOD, &l).unwrap();
    s.advance(3.0 * PERIOD, PERIOD, &l, &d).unwrap();
    assert!(!s.expired && s.heartbeat == 2);
}

/// A state driving (0.3, 0, 1.0) with a fresh request live at t = 1 s: heartbeat 6, 50 periods.
fn driving_at_one_second(l: &Limits) -> TwistState {
    let mut s = TwistState { commanded: BodyTwist::new(0.3, 0.0, 1.0), heartbeat: 5, periods: 50, sent: [0.3, 0.0, 1.0, 5.0], ..TwistState::default() };
    s.request(BodyTwist::new(0.3, 0.0, 1.0), false, 1.0, l).unwrap();
    s
}

#[test]
fn pause_invalidates_a_live_request_and_resume_applies_the_on_loss_rule() {
    let (l, d) = (limits(), ramp());
    let s = driving_at_one_second(&l);
    // Without the pause the request is live at t = 1 (age 0) and keeps driving.
    let mut unpaused = s;
    unpaused.advance(1.0, PERIOD, &l, &d).unwrap();
    assert!(!unpaused.expired && unpaused.commanded.forward_m_s == 0.3, "{unpaused:?}");
    // Paused at t = 1 (PAUSE_RULE): zero request, deadman expired; commanded, heartbeat, periods and sent kept.
    let mut s = driving_at_one_second(&l);
    let before = s;
    s.pause(1.0, &d);
    assert!(s.request.is_zero() && s.expired && !s.halted, "{s:?}");
    assert_eq!((s.commanded, s.heartbeat, s.periods, s.sent), (before.commanded, 6, 50, before.sent));
    assert!(s.status(1.0).age_s >= d.timeout_s);
    // Pausing again at the same time changes nothing.
    let once = s;
    s.pause(1.0, &d);
    assert_eq!(s, once);
    // Resume (no sim time passed): the first period ramps down at stop_decel, it does not drive the old request.
    let a = s.advance(1.0, PERIOD, &l, &d).unwrap();
    assert!(s.expired && (a[0] - (0.3 - 1.2 * PERIOD)).abs() < 1e-12 && (a[2] - (1.0 - 12.0 * PERIOD)).abs() < 1e-12 && a[1] == 0.0, "{a:?}");
    assert_eq!((a[3], s.heartbeat, s.periods), (6.0, 6, 51), "a pause sends no heartbeat");
    // Immediate on-loss: zero at once on resume.
    let immediate = Deadman { timeout_s: 0.5, on_loss: OnLoss::Immediate };
    let mut s = driving_at_one_second(&l);
    s.pause(1.0, &immediate);
    assert_eq!(s.commanded, BodyTwist::new(0.3, 0.0, 1.0), "the pause itself does not zero the commanded twist");
    let a = s.advance(1.0, PERIOD, &l, &immediate).unwrap();
    assert!(s.expired && a == [0.0, 0.0, 0.0, 6.0], "{a:?}");
}

#[test]
fn a_fresh_request_after_pause_drives_again() {
    let (l, d) = (limits(), ramp());
    let mut s = driving_at_one_second(&l);
    s.pause(1.0, &d);
    s.advance(1.0, PERIOD, &l, &d).unwrap();
    assert!(s.expired);
    let ramped = s.commanded;
    // A fresh request one period later is live: approached under max_accel, not the stop rule.
    s.request(BodyTwist::new(0.1, 0.0, 0.0), false, 1.0 + PERIOD, &l).unwrap();
    assert_eq!(s.heartbeat, 7);
    let a = s.advance(1.0 + PERIOD, PERIOD, &l, &d).unwrap();
    assert!(!s.expired && s.request == BodyTwist::new(0.1, 0.0, 0.0), "{s:?}");
    assert!((a[0] - (ramped.forward_m_s - 0.6 * PERIOD)).abs() < 1e-12 && (a[2] - (ramped.yaw_rad_s - 6.0 * PERIOD)).abs() < 1e-12 && a[3] == 7.0, "{a:?}");
}

#[test]
fn pause_with_no_request_is_harmless() {
    let (l, d) = (limits(), ramp());
    let mut s = TwistState::default();
    s.pause(0.0, &d);
    assert!(s.request.is_zero() && s.commanded.is_zero() && s.expired && s.heartbeat == 0 && s.periods == 0, "{s:?}");
    // Resume: the stop rule from zero is zero.
    assert_eq!(s.advance(0.0, PERIOD, &l, &d).unwrap(), [0.0, 0.0, 0.0, 0.0]);
    assert_eq!(s.periods, 1);
    // The first request still drives.
    s.request(BodyTwist::new(0.3, 0.0, 0.0), false, PERIOD, &l).unwrap();
    let a = s.advance(PERIOD, PERIOD, &l, &d).unwrap();
    assert!(!s.expired && (a[0] - 0.6 * PERIOD).abs() < 1e-12 && a[3] == 1.0, "{a:?}");
}

#[test]
fn pause_keeps_a_halt_reported() {
    let (l, d) = (limits(), ramp());
    let mut s = TwistState { commanded: BodyTwist::new(0.3, 0.0, 0.0), request: BodyTwist::new(0.3, 0.0, 0.0), ..TwistState::default() };
    s.request(BodyTwist::ZERO, true, 1.0, &l).unwrap();
    s.pause(1.0, &d);
    assert!(s.halted && s.expired && s.request.is_zero() && s.commanded.is_zero(), "{s:?}");
    // A replay's end clears it (the recorded requests were not a halt).
    s.replay_ended(1.0, &d);
    assert!(!s.halted, "{s:?}");
}

#[test]
fn the_session_inputs_must_be_the_four_command_channels_in_order() {
    let channel = |name: &str| InputChannel { name: name.into(), kind: sim_core::QuantityKind::Dimensionless, lower: 0.0, upper: 1.0, initial: 0.0 };
    let good: Vec<InputChannel> = COMMAND_CHANNELS.iter().map(|n| channel(n)).collect();
    assert!(check_inputs(&good).is_ok());
    let mut swapped = good.clone();
    swapped.swap(0, 1);
    let e = check_inputs(&swapped).unwrap_err();
    assert!(e.contains("command.lateral, command.forward") && e.contains("COMMAND_CHANNELS"), "{e}");
}

#[test]
fn rest_fields_make_exactly_one_request_and_name_the_command() {
    let f = |forward, lateral, yaw, action: Option<&str>, stop| DriveRequest::from_fields(forward, lateral, yaw, action.map(str::to_string), stop, "system_drive");
    assert_eq!(f(Some(0.5), None, None, None, None), Ok(DriveRequest::Axes { forward: 0.5, lateral: 0.0, yaw: 0.0 }));
    assert_eq!(f(None, None, Some(-1.0), None, None), Ok(DriveRequest::Axes { forward: 0.0, lateral: 0.0, yaw: -1.0 }));
    assert_eq!(f(None, None, None, Some("halt"), None), Ok(DriveRequest::Action { name: "halt".into() }));
    assert_eq!(f(None, None, None, None, Some(true)), Ok(DriveRequest::Stop));
    let e = f(None, None, None, None, Some(false)).unwrap_err();
    assert!(e.starts_with("system_drive stop must be true"), "{e}");
    let e = f(Some(1.0), None, None, Some("stop"), None).unwrap_err();
    assert!(e.starts_with("system_drive needs exactly one of") && e.ends_with("given: axes (forward/lateral/yaw) and action"), "{e}");
    let e = f(None, None, None, None, None).unwrap_err();
    assert!(e.ends_with("given: none"), "{e}");
    let e = DriveRequest::from_fields(None, None, None, Some("stop".into()), Some(true), "robot_drive").unwrap_err();
    assert!(e.starts_with("robot_drive needs exactly one of") && e.ends_with("given: action and stop"), "{e}");
}

/// The example binding (no Python process: the binding, profile and script are only read and hashed).
fn wheeled() -> ControlledRobot {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join("examples/wheeled-robot/baseline/robot.simrobot.json");
    let model = sim_domain_robot::PhysicalModel::load(&path.to_string_lossy()).unwrap();
    crate::controller_binding::load(&crate::controller_binding::binding_path_for(&path), &model).unwrap()
}

#[test]
fn a_request_is_interpreted_against_the_profile() {
    let c = wheeled();
    // robot.drive.json: forward 0.26 m/s, yaw 4.3 rad/s, no lateral axis; actions stop and halt.
    let (t, halt) = DriveRequest::Axes { forward: 1.0, lateral: 0.0, yaw: -0.5 }.interpret(&c).unwrap();
    assert!(!halt && (t.forward_m_s - 0.26).abs() < 1e-12 && (t.yaw_rad_s + 2.15).abs() < 1e-12 && t.lateral_m_s == 0.0, "{t:?}");
    let e = DriveRequest::Axes { forward: 0.0, lateral: 0.5, yaw: 0.0 }.interpret(&c).unwrap_err();
    assert!(e.starts_with("drive request refused: ") && e.contains("lateral") && e.contains("robot.drive.json"), "{e}");
    assert!(DriveRequest::Axes { forward: 1.5, lateral: 0.0, yaw: 0.0 }.interpret(&c).is_err());
    assert_eq!(DriveRequest::Action { name: "halt".into() }.interpret(&c), Ok((BodyTwist::ZERO, true)));
    assert_eq!(DriveRequest::Action { name: "stop".into() }.interpret(&c), Ok((BodyTwist::ZERO, false)));
    assert_eq!(DriveRequest::Stop.interpret(&c), Ok((BodyTwist::ZERO, false)));
    let e = DriveRequest::Action { name: "boost".into() }.interpret(&c).unwrap_err();
    assert!(e.contains("no action `boost`") && e.contains("stop, halt"), "{e}");
}

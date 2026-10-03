//! The leg gait clock and the link's health (HW-10/HW-11): pure functions of
//! a snapshot and an instant, so every case builds `now` from the read
//! instant (`read + d`) instead of sleeping.
use super::*;
use sim_runtime::hardware::protocol::calibration::GaitState;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-12
}

#[test]
fn leg_gait_time_advances_linearly_inside_the_cap() {
    assert!(close(leg_gait_time(1.0, 0.5, true, Duration::from_millis(100)), 1.05));
    assert!(close(leg_gait_time(1.0, 1.0, true, Duration::ZERO), 1.0));
    assert!(close(leg_gait_time(2.0, 0.25, true, Duration::from_millis(40)), 2.01));
}

#[test]
fn leg_gait_time_never_runs_more_than_one_active_poll_past_the_read() {
    // 1 s since the read at half speed: capped at 150 ms × 0.5.
    assert!(close(leg_gait_time(1.0, 0.5, true, Duration::from_secs(1)), 1.075));
    for since_ms in [150, 151, 500, 10_000] {
        let t = leg_gait_time(3.0, 0.8, true, Duration::from_millis(since_ms));
        assert!(t - 3.0 <= POLL_ACTIVE.as_secs_f64() * 0.8 + 1e-12, "{since_ms} ms: {t}");
    }
}

#[test]
fn leg_gait_time_holds_when_not_advancing_or_the_scale_is_unusable() {
    assert_eq!(leg_gait_time(1.0, 0.5, false, Duration::from_millis(100)), 1.0);
    assert_eq!(leg_gait_time(1.0, 0.0, true, Duration::from_millis(100)), 1.0);
    assert_eq!(leg_gait_time(1.0, -0.5, true, Duration::from_millis(100)), 1.0);
    assert_eq!(leg_gait_time(1.0, f64::NAN, true, Duration::from_millis(100)), 1.0);
    assert_eq!(leg_gait_time(1.0, f64::INFINITY, true, Duration::from_millis(100)), 1.0);
}

fn run(mode: GaitMode) -> GaitRun {
    let leg = mode != GaitMode::Sim;
    GaitRun { mode, period_s: 2.0, t: 1.0, playing: true, scale: 0.5, leg, skipped: Vec::new(), started: true }
}
fn gait(t: f64, phase: &str) -> GaitState {
    GaitState { running: true, t: Some(t), speed_scale: Some(0.5), phase: Some(phase.into()), ..Default::default() }
}
/// A live snapshot of a leg gait in `mode`, read at `read`, with the server at `t`.
fn snapshot(mode: GaitMode, read: Instant, t: f64) -> LinkSnapshot {
    let mut s = LinkSnapshot { connection_valid: true, read_at: Some(read), gait: Some(run(mode)), ..Default::default() };
    s.state.gait = Some(gait(t, "playing"));
    s.gait.as_mut().unwrap().t = t;
    s
}

#[test]
fn health_is_waiting_live_stale_or_disconnected() {
    let read = Instant::now();
    let mut s = LinkSnapshot { connection_valid: true, ..Default::default() };
    assert_eq!(s.health(read), LinkHealth::Waiting);
    s.read_at = Some(read);
    assert_eq!(s.health(read + Duration::from_millis(100)), LinkHealth::Live);
    assert_eq!(s.health(read + STALE_AFTER), LinkHealth::Live, "stale only past STALE_AFTER");
    let age = STALE_AFTER + Duration::from_millis(600);
    assert_eq!(s.health(read + age), LinkHealth::Stale { age });
    assert_eq!(LinkHealth::Stale { age: Duration::from_millis(3900) }.leg_note().as_deref(), Some("Leg data stale — last read 3 s ago; not live"));
    // A connection found invalid after a read is lost, not live.
    s.connection_valid = false;
    assert!(matches!(s.health(read + Duration::from_millis(100)), LinkHealth::Disconnected { .. }));
    // The session's reason wins, also over stale.
    s.connection_valid = true;
    s.disconnected = Some("the bus was lost".into());
    assert_eq!(s.health(read + age), LinkHealth::Disconnected { why: "the bus was lost".into() });
    assert_eq!(s.health(read + age).name(), "disconnected");
    assert_eq!(LinkHealth::Live.leg_note(), None);
}

#[test]
fn leg_clock_interpolates_and_restarts_from_each_new_status() {
    let read = Instant::now();
    let mut s = snapshot(GaitMode::Both, read, 1.0);
    let c = s.leg_clock(read + Duration::from_millis(100)).unwrap();
    assert!(c.advancing && c.frozen.is_none());
    assert!(close(c.t, 1.05) && c.scale == 0.5);
    // A new status: the clock restarts from the server's new time and read instant.
    let read2 = read + Duration::from_millis(150);
    s.read_at = Some(read2);
    s.state.gait = Some(gait(1.06, "playing"));
    s.gait.as_mut().unwrap().t = 1.06;
    let c = s.leg_clock(read2).unwrap();
    assert!(close(c.t, 1.06));
    let c = s.leg_clock(read2 + Duration::from_millis(20)).unwrap();
    assert!(close(c.t, 1.07));
}

#[test]
fn leg_clock_freezes_while_stale_and_recovers_on_a_fresh_read() {
    let read = Instant::now();
    let mut s = snapshot(GaitMode::Leg, read, 1.0);
    let late = read + STALE_AFTER + Duration::from_secs(1);
    let c = s.leg_clock(late).unwrap();
    assert!(matches!(c.frozen, Some(LinkHealth::Stale { .. })));
    assert!(!c.advancing);
    assert_eq!(c.t, 1.0, "frozen at the last read time");
    // A fresh read: live and advancing again.
    s.read_at = Some(late);
    let c = s.leg_clock(late + Duration::from_millis(50)).unwrap();
    assert!(c.frozen.is_none() && c.advancing);
    assert!(close(c.t, 1.025));
}

#[test]
fn a_disconnected_link_freezes_the_clock_as_disconnected_even_when_stale() {
    let read = Instant::now();
    let mut s = snapshot(GaitMode::Both, read, 1.0);
    s.disconnected = Some("the virtual bench was lost".into());
    let c = s.leg_clock(read + STALE_AFTER + Duration::from_secs(5)).unwrap();
    assert_eq!(c.frozen, Some(LinkHealth::Disconnected { why: "the virtual bench was lost".into() }));
    assert_eq!(c.t, 1.0);
}

#[test]
fn a_paused_or_approaching_leg_gait_holds_its_time() {
    let read = Instant::now();
    let later = read + Duration::from_millis(100);
    for phase in ["paused", "approach"] {
        let mut s = snapshot(GaitMode::Both, read, 1.0);
        s.state.gait = Some(gait(1.0, phase));
        let c = s.leg_clock(later).unwrap();
        assert!(!c.advancing && c.frozen.is_none(), "{phase}");
        assert_eq!(c.t, 1.0, "{phase}");
    }
    // Paused here (the UI's Pause, before the server reports it).
    let mut s = snapshot(GaitMode::Both, read, 1.0);
    s.gait.as_mut().unwrap().playing = false;
    assert_eq!(s.leg_clock(later).unwrap().t, 1.0);
    // Not yet started on the server.
    let mut s = snapshot(GaitMode::Both, read, 1.0);
    s.gait.as_mut().unwrap().started = false;
    assert_eq!(s.leg_clock(later).unwrap().t, 1.0);
}

#[test]
fn the_clock_takes_the_runs_scale_until_the_server_reports_one() {
    let read = Instant::now();
    let mut s = snapshot(GaitMode::Both, read, 1.0);
    s.state.gait.as_mut().unwrap().speed_scale = None;
    s.gait.as_mut().unwrap().scale = 0.2;
    let c = s.leg_clock(read + Duration::from_millis(100)).unwrap();
    assert_eq!(c.scale, 0.2);
    assert!(close(c.t, 1.02));
}

#[test]
fn a_sim_only_run_or_no_run_has_no_leg_clock() {
    let read = Instant::now();
    assert!(snapshot(GaitMode::Sim, read, 1.0).leg_clock(read).is_none());
    let s = LinkSnapshot { connection_valid: true, read_at: Some(read), ..Default::default() };
    assert!(s.leg_clock(read).is_none());
}

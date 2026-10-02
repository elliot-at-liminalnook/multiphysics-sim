use super::*;
use sim_runtime::hardware_client::calibration::{CalibrationDoc, GaitLimit, GaitState, Sweep, SweepSample, Telemetry};
use std::time::Duration;

fn axis(lower: Option<i64>, upper: Option<i64>, reverse: bool) -> Axis {
    Axis { lower, upper, reverse, ..Default::default() }
}

/// A live link's snapshot (read now, its connection holding).
fn snapshot(id: u8, position: i64, a: Axis) -> LinkSnapshot {
    let mut s = LinkSnapshot { id: Some(id), read_at: Some(Instant::now()), connection_valid: true, ..Default::default() };
    s.state.samples.insert(id, Telemetry { position_raw: position, ..Default::default() });
    s.state.calibration = Some(CalibrationDoc { axes: [(id, a)].into_iter().collect(), ..Default::default() });
    s
}

#[test]
fn angle_prints_one_decimal_motor_degrees() {
    assert_eq!(angle(1024.0), "90.0°");
    assert_eq!(angle(-2048.0), "-180.0°");
    assert_eq!(angle(1.0), "0.1°");
    // JavaScript's toFixed rounds an exact tie away from zero.
    assert_eq!(fixed(2.5, 0), "3");
    assert_eq!(fixed(0.125, 2), "0.13");
    assert_eq!(fixed(-2.5, 0), "-3");
    assert_eq!(fixed(1.005, 2), format!("{:.2}", 1.005));
    // A product that only rounds to .5 is not a tie: the exact binary value
    // is below it, and toFixed gives the lower digit.
    assert_eq!(fixed(1.45, 1), "1.4");
    assert_eq!(fixed(12.45, 1), "12.4");
    // Negative zero prints unsigned; a negative value rounding to zero
    // keeps its sign, as toFixed does.
    assert_eq!(fixed(-0.0, 1), "0.0");
    assert_eq!(fixed(-0.04, 1), "-0.0");
    assert_eq!(fixed(0.0, 0), "0");
    assert_eq!(js_num(35.0), "35");
    assert_eq!(js_num(35.5), "35.5");
}

#[test]
fn fraction_between_poses_wraps_and_reverses() {
    let taught = axis(Some(1000), Some(3000), false);
    assert_eq!(fraction(2000.0, &taught), 0.5);
    assert_eq!(fraction(3500.0, &taught), 1.25);
    // Taught in reverse: lower above upper.
    let back = axis(Some(3000), Some(1000), false);
    assert_eq!(fraction(2500.0, &back), 0.25);
    // Untaught: the position within one turn, wrapping negatives.
    let free = axis(None, None, false);
    assert!((fraction(4095.0 + 4096.0, &free) - 1.0).abs() < 1e-12);
    assert!((fraction(-1.0, &free) - 1.0).abs() < 1e-12);
    assert_eq!(fraction(0.0, &free), 0.0);
    let reversed = axis(Some(10), None, true);
    assert_eq!(fraction(0.0, &reversed), 1.0);
}

#[test]
fn speed_curve_spans_crawl_to_the_server_maximum() {
    assert_eq!(speed(None, 0.0), 5.0);
    assert!((speed(None, 100.0) - 500.0).abs() < 1e-9);
    assert!((speed(Some(1000.0), 50.0) - 5.0 * 200f64.sqrt()).abs() < 1e-9);
    let s = snapshot(1, 0, Axis::default());
    let v = render(&s, &Form::default(), Instant::now());
    assert_eq!(v.speed_text, "0.44°/s motor");
}

#[test]
fn outside_pose_names_the_pose_or_the_session() {
    let s = snapshot(1, 3500, axis(Some(1000), Some(3000), false));
    assert_eq!(outside_pose(&s.state, s.id), Some("upper"));
    let s = snapshot(1, 500, axis(Some(1000), Some(3000), false));
    assert_eq!(outside_pose(&s.state, s.id), Some("lower"));
    let s = snapshot(1, 500, axis(Some(3000), Some(1000), false));
    assert_eq!(outside_pose(&s.state, s.id), Some("upper"));
    let s = snapshot(1, 2000, axis(Some(1000), Some(3000), false));
    assert_eq!(outside_pose(&s.state, s.id), None);
    let mut a = axis(Some(1000), Some(3000), false);
    a.coordinate_session = Some("old".into());
    let mut s = snapshot(1, 2000, a);
    s.state.coordinate_session = Some("new".into());
    assert_eq!(outside_pose(&s.state, s.id), Some("reference"));
    // No readback: nothing to judge.
    let mut s = snapshot(1, 2000, axis(Some(1000), Some(3000), false));
    s.state.samples.clear();
    assert_eq!(outside_pose(&s.state, s.id), None);
}

#[test]
fn status_line_precedence() {
    let now = Instant::now();
    let form = Form::default();
    let mut s = LinkSnapshot { read_at: Some(now), connection_valid: true, ..Default::default() };
    assert_eq!(render(&s, &form, now).status, "Choose the motor you want to calibrate.");
    s.busy = true;
    assert_eq!(render(&s, &form, now).status, "Connecting and checking this motor at zero drive…");
    let mut s = snapshot(1, 3500, axis(Some(1000), Some(3000), false));
    assert_eq!(status_line(&s), "Ready");
    s.state.message = Some("Hold".into());
    assert_eq!(status_line(&s), "Hold");
    s.ready = true;
    assert_eq!(status_line(&s), "Beyond the saved upper pose. Move back inward freely; driving further out is blocked.");
    s.state.calibration.as_mut().unwrap().axes.get_mut(&1).unwrap().disabled = true;
    s.state.calibration.as_mut().unwrap().axes.get_mut(&1).unwrap().role = "Knee".into();
    s.ready = false;
    assert_eq!(status_line(&s), "Knee is disabled. Enable it to move it.");
    // Stale: the native rule replaces the line and blocks motion.
    s.read_at = Some(now - std::time::Duration::from_secs(5));
    let v = render(&s, &form, now);
    assert_eq!(v.status, "Status stale — last read 5 s ago");
    assert!(!v.jog_enabled && v.chips.iter().all(|c| !c.enabled));
    s.read_at = None;
    assert_eq!(render(&s, &form, now).status, "Reading the calibration server's status…");
}

#[test]
fn stats_rows_format_missing_values_as_dashes() {
    let mut stats = BTreeMap::new();
    stats.insert("1".to_string(), GaitStatistics { role: "Knee".into(), tracking_rms_deg: Some(1.234), lag_s: Some(0.25), mean_effort: Some(0.5), ..Default::default() });
    let rows = stats_rows(&stats);
    assert_eq!(rows[0], ["Knee", "1.23°", "—°", "—", "250 ms", "50%", "—%", "—%", "—", "—", "—"].map(String::from));
    stats.get_mut("1").unwrap().simulated_tracking_rms_counts = Some(4096.0);
    assert_eq!(stats_rows(&stats)[0][3], "360.00°");
}

#[test]
fn gait_option_labels() {
    let poses = GaitEntry { kind: Some("pose_sequence".into()), study: "study".into(), trial: "trial".into(), ..Default::default() };
    assert_eq!(gait_option_label(&poses), "Poses · study · trial");
    let found = GaitEntry { study: "gait-search-study-without-gait-search-".into(), trial: "trial".into(), speed_m_s: Some(0.1234), measured_actuators: true, ..Default::default() };
    assert_eq!(gait_option_label(&found), "★ 0.123 m/s · study-without-gait-search- · trial");
    let plain = GaitEntry { study: "s".into(), trial: "t".into(), ..Default::default() };
    assert_eq!(gait_option_label(&plain), "0.000 m/s · s · t");
}

#[test]
fn needles_follow_the_latest_request() {
    let mut s = snapshot(2, 2000, axis(Some(1000), Some(3000), false));
    s.state.sweep = Some(Sweep { motor_id: Some(2), latest: Some(SweepSample { target_raw: Some(2500.0), ..Default::default() }), ..Default::default() });
    let v = render(&s, &Form::default(), Instant::now());
    assert_eq!(v.needles, Some((0.5, 0.75)));
    assert_eq!(v.target_value, Some(75.0));
    assert_eq!(v.position, "175.8° motor");
    assert_eq!(v.poses[0].0, "87.9° motor");
    assert_eq!(v.poses[2].0, "Not aligned");
}

#[test]
fn leg_gait_status_prints_missing_errors_and_limits_as_dashes() {
    let mut s = snapshot(1, 2000, Axis { role: "Knee".into(), ..Default::default() });
    s.gait = Some(super::super::link::GaitRun { mode: GaitMode::Leg, period_s: 2.0, t: 0.5, playing: true, scale: 0.5, leg: true, skipped: Vec::new(), started: true });
    let limit = GaitLimit { role: "knee".into(), governor_speed_counts_s: Some(812.5), governor_acceleration_counts_s2: None };
    s.state.gait = Some(GaitState {
        phase: Some("playing".into()),
        limits: [("1".to_string(), limit)].into_iter().collect(),
        errors: Some([("1".to_string(), Some(-3.5)), ("2".to_string(), None)].into_iter().collect()),
        ..Default::default()
    });
    let text = render(&s, &Form::default(), Instant::now()).gait.status;
    assert_eq!(text, "Leg only · gait time 0.50 s of 2.00 s period · 50% speed\nLimits: knee ≤ 813 counts/s, — counts/s²\nLeg: playing · error Knee -3, 2 — counts");
    // No limits: no "Limits:" line (the page prints a bare one).
    s.state.gait.as_mut().unwrap().limits.clear();
    assert!(!render(&s, &Form::default(), Instant::now()).gait.status.contains("Limits"));
}

#[test]
fn chips_are_the_page_three_whatever_the_calibration_names() {
    // A calibration with other ids and roles: the page's chips stay
    // Knee 1, Worm 2, Belt 3; only `.off` comes from the axes.
    let mut s = snapshot(2, 0, Axis { role: "Elbow".into(), disabled: true, ..Default::default() });
    s.state.calibration.as_mut().unwrap().axes.insert(7, Axis { role: "Wrist".into(), ..Default::default() });
    let v = render(&s, &Form::default(), Instant::now());
    let chips: Vec<_> = v.chips.iter().map(|c| (c.id, c.label.as_str(), c.pressed, c.off)).collect();
    assert_eq!(chips, [(1, "Knee 1", false, false), (2, "⊘ Worm 2", true, true), (3, "Belt 3", false, false)]);
    // No calibration: the same three.
    let v = render(&LinkSnapshot::default(), &Form::default(), Instant::now());
    assert_eq!(v.chips.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(), ["Knee 1", "Worm 2", "Belt 3"]);
}

// HW-10/HW-11 labels, the one leg clock and the disconnected marker: written
// and source-inspected, not executed.

/// A leg gait playing on the server: Leg only, at t 0.5 s of a 2 s period, half speed.
fn leg_gait(s: &mut LinkSnapshot, simulated: bool) {
    s.gait = Some(super::super::link::GaitRun { mode: GaitMode::Leg, period_s: 2.0, t: 0.5, playing: true, scale: 0.5, leg: true, skipped: Vec::new(), started: true });
    s.state.gait = Some(GaitState {
        running: true,
        phase: Some("playing".into()),
        errors: Some([("1".to_string(), Some(2.0))].into_iter().collect()),
        simulated,
        ..Default::default()
    });
}

#[test]
fn a_simulated_leg_gait_and_run_are_labelled_virtual() {
    use sim_runtime::hardware_client::calibration::GaitRun as RunRecord;
    let now = Instant::now();
    let mut s = snapshot(1, 2000, Axis { role: "Knee".into(), ..Default::default() });
    s.read_at = Some(now);
    leg_gait(&mut s, true);
    let text = render(&s, &Form::default(), now).gait.status;
    assert!(text.contains("\nVIRTUAL (simulated) · Leg: playing · error Knee 2 counts"), "{text}");
    // Not simulated by the server's word, but the link is pinned to a virtual bench.
    leg_gait(&mut s, false);
    s.execution = Some(sim_runtime::hardware_client::calibration::ExecutionIdentity {
        schema_version: 1,
        kind: "virtual_calibration".into(),
        server_instance: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into(),
        bench_instance: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into(),
    });
    assert!(render(&s, &Form::default(), now).gait.status.contains("\nVIRTUAL (simulated) · Leg: playing"));
    // A physical or unknown leg: no label.
    s.execution = None;
    let text = render(&s, &Form::default(), now).gait.status;
    assert!(text.contains("\nLeg: playing") && !text.contains("VIRTUAL"), "{text}");
    // Recent leg runs: a simulated row is labelled; a row without the field is not.
    s.gait = None;
    s.state.gait_runs = vec![
        RunRecord { gait: Some("study/trial/compiled.json".into()), effort: Some(0.5), speed_scale: Some(1.0), gait_time_s: Some(4.0), outcome: Some("completed".into()), simulated: true, ..Default::default() },
        serde_json::from_value(serde_json::json!({"gait": "study/older/compiled.json", "effort": 0.5, "speed_scale": 1.0, "gait_time_s": 4.0, "outcome": "completed"})).unwrap(),
    ];
    let runs = render(&s, &Form::default(), now).gait.runs;
    assert_eq!(runs[0].0, "VIRTUAL (simulated) · trial · effort 50% · speed 100% · 4.0 s · completed");
    assert_eq!(runs[1].0, "older · effort 50% · speed 100% · 4.0 s · completed");
}

#[test]
fn the_leg_gait_time_is_the_one_clock_and_freezes_with_why_when_not_live() {
    let read = Instant::now();
    let mut s = snapshot(1, 2000, Axis::default());
    s.read_at = Some(read);
    leg_gait(&mut s, false);
    s.state.gait.as_mut().unwrap().speed_scale = Some(1.0);
    let line = |s: &LinkSnapshot, now: Instant| render(s, &Form::default(), now).gait.status.lines().next().unwrap_or_default().to_string();
    // Live: the line shows the time as last read (steady between reads, at
    // the server's speed); the mirror samples it interpolated (link/tests.rs).
    assert_eq!(line(&s, read + Duration::from_millis(100)), "Leg only · gait time 0.50 s of 2.00 s period · 100% speed");
    assert!(s.leg_clock(read + Duration::from_secs(1)).is_some_and(|c| (c.t - 0.65).abs() < 1e-9));
    // Stale: frozen at the last read, with the reason under it.
    let now = read + Duration::from_secs(3);
    let text = render(&s, &Form::default(), now).gait.status;
    assert!(text.starts_with("Leg only · gait time 0.50 s (clock frozen) of 2.00 s period · 100% speed\nLeg data stale — last read 3 s ago; not live\n"), "{text}");
    // A fresh status restores the live text.
    s.read_at = Some(now);
    let text = render(&s, &Form::default(), now).gait.status;
    assert!(!text.contains("frozen") && !text.contains("not live"), "{text}");
    // Disconnected: frozen with that reason.
    s.disconnected = Some("the server reports its bus disconnected".into());
    let text = render(&s, &Form::default(), now).gait.status;
    assert!(text.contains("(clock frozen)") && text.contains("Leg disconnected — the server reports its bus disconnected; not live"), "{text}");
}

#[test]
fn a_disconnected_link_prefixes_the_status_line_without_blocking() {
    let now = Instant::now();
    let mut s = snapshot(1, 2000, axis(Some(1000), Some(3000), false));
    s.read_at = Some(now);
    s.ready = true;
    s.state.message = Some("Ready".into());
    s.disconnected = Some("the server reports its bus disconnected: Readback lost from Knee: timed out. Select a motor to reconnect.".into());
    let v = render(&s, &Form::default(), now);
    assert_eq!(v.status, "DISCONNECTED — the server reports its bus disconnected: Readback lost from Knee: timed out. Select a motor to reconnect. Ready");
    // Selecting a motor is how the server reconnects: nothing is blocked.
    assert!(v.blocked.is_none() && v.chips.iter().all(|c| c.enabled));
    // The server's own message already says it: not repeated.
    s.state.message = Some("Readback lost from Knee: timed out. Select a motor to reconnect.".into());
    assert_eq!(render(&s, &Form::default(), now).status, "DISCONNECTED — the server reports its bus disconnected: Readback lost from Knee: timed out. Select a motor to reconnect.");
    // Idempotent (the panel says it again after a block).
    let mut v = render(&s, &Form::default(), now);
    let once = v.status.clone();
    mark_disconnected(&mut v, &s, now);
    assert_eq!(v.status, once);
    // A status that reports the bus connected clears the marker (the session's `adopt`): no prefix.
    s.disconnected = None;
    assert!(!render(&s, &Form::default(), now).status.starts_with("DISCONNECTED"));
}

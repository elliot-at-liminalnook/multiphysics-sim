use super::*;
use sim_runtime::hardware_client::calibration::{CalibrationDoc, GaitLimit, GaitState, Sweep, SweepSample, Telemetry};

fn axis(lower: Option<i64>, upper: Option<i64>, reverse: bool) -> Axis {
    Axis { lower, upper, reverse, ..Default::default() }
}

fn snapshot(id: u8, position: i64, a: Axis) -> LinkSnapshot {
    let mut s = LinkSnapshot { id: Some(id), read_at: Some(Instant::now()), ..Default::default() };
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
    let mut s = LinkSnapshot { read_at: Some(now), ..Default::default() };
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

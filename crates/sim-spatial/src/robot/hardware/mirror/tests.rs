use super::*;
use sim_runtime::hardware_client::calibration::{CalibrationDoc, Telemetry};

fn roles() -> BTreeMap<u8, String> {
    BTreeMap::from([(1, "belt/hip".to_string()), (2, "worm".to_string()), (3, "knee".to_string())])
}
fn coordinates() -> Vec<Coordinate> {
    ["Hip servo output", "Worm servo output", "Foot servo output"]
        .iter()
        .map(|j| Coordinate { joint: format!("+X | {j}"), home: 0.1, lower: Some(-1.0), upper: Some(0.5) })
        .collect()
}
fn mirror() -> Mirror {
    let mut m = Mirror::new(&MirrorSettings::default());
    m.set_roles(roles());
    m.coordinates = Some(coordinates());
    // Live leg data unless a test says otherwise (`mirror_sync` sets it every frame).
    m.set_leg_health(&LinkHealth::Live);
    m
}

#[test]
fn roles_take_the_default_joints_and_alignments() {
    let m = mirror();
    let b = &m.settings.bindings;
    assert_eq!(b[&1].joint, "Hip servo output");
    assert_eq!(b[&2].joint, "Worm servo output");
    assert_eq!((b[&3].joint.as_str(), b[&3].align), ("Foot servo output", Align::Mid));
    assert_eq!(b[&1].polarity, 1);
    assert_eq!(m.alignment_angle(3), Some(-0.25));
    assert_eq!(m.alignment_angle(1), Some(0.1));
}

#[test]
fn update_follows_the_pages_lines_and_values() {
    let mut m = mirror();
    m.shown = true;
    m.worker = Some(crate::jobs::RunThread::spawn("hardware-mirror", MirrorShared::default(), worker));
    let mut state = Status { coordinate_session: Some("s1".into()), ..Default::default() };
    let mut doc = CalibrationDoc::default();
    doc.axes.insert(1, Axis { role: "belt/hip".into(), reference: Some(1000), reference_joint_rad: Some(0.2), ..Default::default() });
    doc.axes.insert(2, Axis { role: "worm".into(), reference: Some(5000), reference_session: Some("s0".into()), ..Default::default() });
    doc.axes.insert(3, Axis { role: "knee".into(), ..Default::default() });
    state.calibration = Some(doc);
    state.samples.insert(1, Telemetry { position_raw: 1512, ..Default::default() });
    m.update(&state, false);
    assert_eq!(m.text, "belt/hip: 45.0° from its alignment pose · worm: alignment is from an earlier session — re-align (shown at cad home) · knee: not aligned — shown at mid-travel");
    let v = m.pending.clone().unwrap();
    assert!((v[0] - (0.2 + std::f64::consts::FRAC_PI_4)).abs() < 1e-12);
    assert_eq!((v[1], v[2]), (0.1, -0.25));
    // Unchanged values are not solved again unless forced.
    let sent = m.pose_sent;
    m.update(&state, false);
    assert_eq!(m.pose_sent, sent);
    m.update(&state, true);
    assert_eq!(m.pose_sent, sent + 1);
}

#[test]
fn gait_bindings_skip_with_the_pages_reasons() {
    let m = mirror();
    let mut doc = CalibrationDoc::default();
    doc.axes.insert(1, Axis { lower: Some(0), upper: Some(100), reference: Some(50), ..Default::default() });
    doc.axes.insert(2, Axis { disabled: true, ..Default::default() });
    doc.axes.insert(3, Axis { lower: Some(0), upper: Some(100), ..Default::default() });
    let state = Status { calibration: Some(doc), ..Default::default() };
    let (bindings, skipped) = m.gait_bindings(&state);
    assert_eq!(bindings, vec![GaitBinding { id: 1, joint: "+X | Hip servo output".into(), polarity: 1.0, home_rad: 0.1 }]);
    assert_eq!(skipped, vec!["worm (disabled)".to_string(), "knee (not aligned to the sim)".to_string()]);
}

#[test]
fn status_and_record() {
    let mut m = mirror();
    assert_eq!(m.status_text(), "");
    m.error = Some("Bind each motor to a different CAD joint".into());
    assert_eq!(m.status_text(), "Mirror unavailable: Bind each motor to a different CAD joint");
    let r = m.record();
    assert_eq!(r["counts_per_revolution"], 4096);
    assert_eq!(r["lift_m"], 0.25);
    assert_eq!(r["bindings"]["3"]["align"], "mid");
    assert_eq!(r["bindings"]["1"]["joint"], "Hip servo output");
    assert_eq!(r["note"], RECORD_NOTE);
    // Before roles are known, saving keeps the file's bindings.
    let mut saved = MirrorSettings::default();
    saved.bindings.insert(9, MirrorBinding { joint: "Worm servo output".into(), polarity: -1, align: Align::Home });
    assert_eq!(Mirror::new(&saved).to_save(), saved);
}

#[test]
fn a_dead_worker_is_reported_and_replaced_on_the_next_begin() {
    let mut m = mirror();
    m.coordinates = None;
    m.loading = true;
    m.shown = true;
    m.worker = Some(crate::jobs::RunThread::spawn("hardware-mirror", MirrorShared::default(), |_, _| panic!("load failed")));
    let started = Instant::now();
    while !m.worker.as_ref().unwrap().finished() {
        assert!(started.elapsed().as_secs() < 5, "the panicking worker never finished");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(m.poll().is_none());
    assert!(m.worker.is_none() && !m.loading && !m.shown && !m.working());
    assert_eq!(m.status_text(), "Mirror unavailable: Mirror worker failed");
    assert!(m.take_ended(), "the display is cleared");
    assert!(!m.due(), "no respawn every frame: only a new begin retries");
    m.begin();
    assert!(m.due());
}

#[test]
fn stored_signs_are_clamped() {
    let mut saved = MirrorSettings::default();
    saved.bindings.insert(1, MirrorBinding { joint: "Hip servo output".into(), polarity: -5, align: Align::Home });
    let mut m = Mirror::new(&saved);
    m.set_roles(roles());
    assert_eq!(m.settings.bindings[&1].polarity, -1);
}

#[test]
fn mirror_angles_round_as_to_fixed() {
    // 128 counts = 11.25°: `toFixed(1)` gives "11.3" (ties away from zero); `format!` would give "11.2".
    assert_eq!(degrees_text(128.0 * std::f64::consts::TAU / COUNTS), "11.3");
    assert_eq!(degrees_text(-128.0 * std::f64::consts::TAU / COUNTS), "-11.3");
    let mut m = mirror();
    let mut doc = CalibrationDoc::default();
    doc.axes.insert(1, Axis { role: "belt/hip".into(), reference: Some(1000), ..Default::default() });
    let mut state = Status { calibration: Some(doc), ..Default::default() };
    state.samples.insert(1, Telemetry { position_raw: 1128, ..Default::default() });
    m.update(&state, false);
    assert!(m.text.starts_with("belt/hip: 11.3° from its alignment pose"), "{}", m.text);
}

#[test]
fn a_replaced_worker_is_released_off_the_ui_thread() {
    let mut m = mirror();
    // A worker blocked mid-solve for longer than JOIN_BOUND.
    m.worker = Some(crate::jobs::RunThread::spawn("hardware-mirror", MirrorShared::default(), |_rx, _| std::thread::sleep(std::time::Duration::from_millis(600))));
    let started = Instant::now();
    m.release_worker();
    assert!(m.worker.is_none());
    assert!(started.elapsed() < crate::jobs::JOIN_BOUND, "{:?}", started.elapsed());
}

#[test]
fn late_disabled_preferences_clear_an_already_shown_mirror() {
    let mut m=mirror();m.shown=true;
    let saved=MirrorSettings { enabled:false, ..Default::default() };
    m.load_preferences(&saved);
    assert!(!m.shown());assert!(m.take_ended());assert!(!m.settings.enabled);
    assert!(m.worker.is_none(),"publication does not start a worker");
}

/// A status with motor 1 aligned at 1000 counts and read at `raw`.
fn hip_state(raw: i64) -> Status {
    let mut doc = CalibrationDoc::default();
    doc.axes.insert(1, Axis { role: "belt/hip".into(), reference: Some(1000), ..Default::default() });
    let mut state = Status { calibration: Some(doc), ..Default::default() };
    state.samples.insert(1, Telemetry { position_raw: raw, ..Default::default() });
    state
}

#[test]
fn stale_leg_data_holds_the_pose_and_says_so_until_live_again() {
    let mut m = mirror();
    m.update(&hip_state(1128), false);
    let live_pending = m.pending.clone();
    let live_text = m.text.clone();
    assert!(live_text.starts_with("belt/hip: 11.3° from its alignment pose"), "{live_text}");
    // What `poll` does when the solve lands.
    (m.line, m.line_is_reading) = (m.text.clone(), m.text_is_reading);
    assert_eq!(m.status_text(), live_text);

    // Stale: a new reading (or a forced update from `prepare`) solves nothing.
    assert!(!m.set_leg_health(&LinkHealth::Stale { age: std::time::Duration::from_millis(3400) }));
    let sent = m.pose_sent;
    m.update(&hip_state(1512), false);
    m.update(&hip_state(1512), true);
    assert_eq!(m.pending, live_pending, "the last solved pose is held");
    assert_eq!((m.pose_sent, m.text.as_str()), (sent, live_text.as_str()));
    assert_eq!(m.status_text(), format!("Leg data stale — last read 3 s ago; not live · last reading: {live_text}"));
    assert_eq!(m.state_json()["leg_data"], "stale");
    assert_eq!(m.state_json()["leg_note"], "Leg data stale — last read 3 s ago; not live");

    // Disconnected says so too.
    m.set_leg_health(&LinkHealth::Disconnected { why: "the bus was lost".into() });
    assert!(m.status_text().starts_with("Leg disconnected — the bus was lost; not live · last reading: belt/hip: 11.3°"), "{}", m.status_text());

    // Live again: the caller is told to force an update, which solves the newest reading.
    assert!(m.set_leg_health(&LinkHealth::Live));
    assert!(!m.set_leg_health(&LinkHealth::Live), "only the transition reports a recovery");
    m.update(&hip_state(1512), true);
    assert!(m.text.starts_with("belt/hip: 45.0° from its alignment pose"), "{}", m.text);
    assert_ne!(m.pending, live_pending);
    (m.line, m.line_is_reading) = (m.text.clone(), m.text_is_reading);
    assert_eq!(m.status_text(), m.text);
    assert_eq!(m.state_json()["leg_data"], "live");
    assert!(m.state_json()["leg_note"].is_null());
}

#[test]
fn a_loading_line_is_not_called_a_reading() {
    let mut m = mirror();
    m.set_leg_health(&LinkHealth::Stale { age: std::time::Duration::from_secs(5) });
    m.line = "Preparing the suspended robot…".into();
    assert_eq!(m.status_text(), "Leg data stale — last read 5 s ago; not live · Preparing the suspended robot…");
}

/// A mirror with a gait loaded (number 7) on a worker that only reports the
/// samples it is sent: (t, scale, reset).
fn gait_mirror() -> (Mirror, std::sync::mpsc::Receiver<(f64, f64, bool)>, (u64, Arc<Value>, String)) {
    let mut m = mirror();
    let (tx, rx) = std::sync::mpsc::channel();
    m.worker = Some(crate::jobs::RunThread::spawn("hardware-mirror", MirrorShared::default(), move |commands, _| {
        for c in commands {
            if let MirrorCommand::Sample { t, scale, reset, .. } = c {
                let _ = tx.send((t, scale, reset));
            }
        }
    }));
    (m.gait_number, m.gait_ready) = (Some(7), true);
    (m, rx, (7, Arc::new(json!({})), "trial".to_string()))
}
fn leg_run(mode: GaitMode, t: f64) -> GaitRun {
    GaitRun { mode, period_s: 2.0, t, playing: true, scale: 0.3, leg: mode != GaitMode::Sim, skipped: Vec::new(), started: true }
}
fn clock(t: f64, frozen: Option<LinkHealth>) -> LegClock {
    LegClock { t, scale: 0.5, advancing: frozen.is_none(), frozen }
}
fn sent(rx: &std::sync::mpsc::Receiver<(f64, f64, bool)>) -> (f64, f64, bool) {
    rx.recv_timeout(std::time::Duration::from_secs(5)).expect("a sample was sent")
}

#[test]
fn both_samples_the_leg_clock_and_holds_while_it_is_frozen() {
    let (mut m, rx, compiled) = gait_mirror();
    let run = leg_run(GaitMode::Both, 1.0);
    let now = Instant::now();
    // Frozen (stale): no sample, the gait pose holds.
    let stale = clock(1.0, Some(LinkHealth::Stale { age: std::time::Duration::from_secs(3) }));
    m.follow_gait(Some(&run), Some(&compiled), Some(&stale), now);
    assert_eq!(m.sample_sent, 0);
    // A leg run without a clock is held the same way.
    m.follow_gait(Some(&run), Some(&compiled), None, now);
    assert_eq!(m.sample_sent, 0);
    // Live: sampled at the clock's time and scale, not the run's.
    m.follow_gait(Some(&run), Some(&compiled), Some(&clock(1.075, None)), now);
    assert_eq!(m.sample_sent, 1);
    assert_eq!(sent(&rx), (1.075, 0.5, true));
    assert!(m.sample_both);
    // The next status lands slightly behind the interpolation: the sample waits at 1.075.
    m.sample_done = m.sample_sent;
    m.follow_gait(Some(&run), Some(&compiled), Some(&clock(1.06, None)), now + std::time::Duration::from_millis(16));
    assert_eq!(sent(&rx), (1.075, 0.5, false));
    // A step back beyond one active poll is the server's own: taken.
    m.sample_done = m.sample_sent;
    m.follow_gait(Some(&run), Some(&compiled), Some(&clock(0.5, None)), now + std::time::Duration::from_millis(32));
    assert_eq!(sent(&rx).0, 0.5);
}

#[test]
fn sim_only_samples_the_runs_own_clock() {
    let (mut m, rx, compiled) = gait_mirror();
    let run = leg_run(GaitMode::Sim, 2.0);
    // A leg clock (there is none for Sim) would be ignored.
    m.follow_gait(Some(&run), Some(&compiled), Some(&clock(9.0, None)), Instant::now());
    assert_eq!(sent(&rx), (2.0, 0.3, true));
    assert!(!m.sample_both);
}

#[test]
fn leg_only_never_samples() {
    let (mut m, _rx, compiled) = gait_mirror();
    m.follow_gait(Some(&leg_run(GaitMode::Leg, 1.0)), Some(&compiled), Some(&clock(1.0, None)), Instant::now());
    assert_eq!(m.sample_sent, 0);
}

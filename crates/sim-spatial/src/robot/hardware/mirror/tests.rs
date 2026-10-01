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

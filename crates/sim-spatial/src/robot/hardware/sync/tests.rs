use super::*;
use crate::robot::hardware::sync_panel::{chart_note, charts, rms_and_saturation, row_title, stats_text};
use sim_runtime::hardware_client::calibration::Telemetry;
use sim_runtime::hardware_client::{Endpoint, new_client_id};

fn coords() -> Vec<String> {
    ["+X | Hip servo output", "+X | Worm servo output", "+X | Foot servo output", "-X | Hip servo output"].iter().map(|c| format!("joint.{c}")).collect()
}
fn sample(id: u8, raw: i64, home: f64, prev: Option<f64>, target: Option<f64>, pwm: f64, frame: f64) -> bench::LiveSample {
    bench::LiveSample { id, telemetry: Telemetry { position_raw: raw, voltage_v: 12.34, temperature_c: 35.0, ..Default::default() }, home_raw: home, previous_target_counts: prev, target_counts: target, pwm, pwm_limit: 100.0, frame, total_frames: Some(11.0), live_source: None }
}

#[test]
fn legs_and_mapping_rows() {
    assert_eq!(legs(&coords()), vec!["+X".to_string(), "-X".to_string()]);
    let rows = mapping(&coords(), &[4, 5, 6], "+X", None);
    assert_eq!(rows.iter().map(|r| (r.motor_id, r.polarity)).collect::<Vec<_>>(), vec![(4, 1), (5, 1), (6, 1)]);
    assert_eq!(row_title(&rows[1].coordinate), "Worm");
    let saved = vec![SyncBinding { coordinate: "x".into(), motor_id: 6, polarity: -1 }];
    let rows = mapping(&coords(), &[4, 5, 6], "+X", Some(saved.as_slice()));
    assert_eq!((rows[0].motor_id, rows[0].polarity), (6, -1));
    assert_eq!(rows[0].coordinate, "joint.+X | Hip servo output");
    assert_eq!(mapping(&coords(), &[4, 5, 6], "-X", None).len(), 1);
    assert_eq!(distinct(&mapping(&coords(), &[4, 5, 6], "+X", None)), Ok(()));
    assert_eq!(distinct(&mapping(&coords(), &[4, 5, 6], "+X", Some(saved.as_slice()))), Err(DISTINCT.to_string()));
}

#[test]
fn current_sample_needs_live_named_targets() {
    let names = vec!["joint.+X | Hip servo output".to_string(), "joint.+X | Worm servo output".to_string()];
    let input = |live, targets: &'static [f64], done| Some(LiveInput { live, coordinates: &[], targets, time_s: 1.5, done });
    assert_eq!(sample_from(None, 1), Err(NOT_LIVE.into()));
    assert_eq!(sample_from(input(true, &[0.1, 0.2], false), 1), Err(NOT_LIVE.into()), "no names");
    let ok = |live, done, targets: &[f64]| sample_from(Some(LiveInput { live, coordinates: &names, targets, time_s: 1.5, done }), 7);
    assert_eq!(ok(false, false, &[0.1, 0.2]), Err(NOT_LIVE.into()), "not live");
    assert_eq!(ok(true, true, &[0.1, 0.2]), Err(NOT_LIVE.into()), "episode done");
    assert_eq!(ok(true, false, &[0.1]), Err(NOT_LIVE.into()), "length mismatch");
    assert_eq!(ok(true, false, &[]), Err(NOT_LIVE.into()), "no targets");
    let s = ok(true, false, &[0.1, 0.2]).unwrap();
    assert_eq!((s.sequence, s.time_s), (7, 1.5));
    assert_eq!(s.targets, vec![(names[0].clone(), 0.1), (names[1].clone(), 0.2)]);
}

#[test]
fn charts_and_readings_follow_the_page() {
    let rows = mapping(&coords(), &[4, 5, 6], "+X", None);
    // 1024 counts = 90°; target 512 = 45° (the previously applied one wins).
    let samples = vec![sample(4, 1024, 0.0, Some(512.0), Some(0.0), 100.0, 0.0), sample(4, 0, 0.0, None, Some(0.0), -10.0, 10.0)];
    let values: Vec<&bench::LiveSample> = samples.iter().collect();
    let (rms, sat) = rms_and_saturation(&values);
    assert!((rms - (45f64.powi(2) / 2.0).sqrt()).abs() < 1e-12);
    assert_eq!(sat, 0.5);
    let c = charts(&samples, &rows);
    assert_eq!(c[0].title, "+X | Hip servo output · ID 4");
    assert_eq!(c[0].stats, "RMS error 31.82° · at drive limit 50%");
    assert_eq!(c[0].actual, vec![[0.0, 90.0], [1.0, 0.0]]);
    assert!((c[0].bound - 108.0).abs() < 1e-12);
    assert!(c[1].waiting && c[1].stats == "Waiting for measured motion");
    let lines = reading_lines(&samples, &rows);
    assert_eq!(lines, "ID 4: 0.00° · 12.3 V · 35 °C\nID 5: waiting\nID 6: waiting");
    assert_eq!(chart_note(2), "Angles relative to measured start. Errors use the previously applied target at each feedback read. 2 real samples retained.");
}

#[test]
fn state_text_and_banner() {
    assert_eq!(banner_text(true, false, "x"), "MOTOR SYNC · x");
    assert_eq!(banner_text(false, true, "x"), "CONNECTING MOTORS · x");
    assert_eq!(banner_text(false, false, "x"), "SIMULATION ONLY · x");
    let mut s = LiveSync::new(None, &SyncSettings::default());
    assert_eq!(s.banner(), (INITIAL_BANNER, false));
    assert_eq!(s.amplitude(), 0.03);
    assert!(s.connect().is_err(), "no bench configured");
    s.state_text(READY);
    assert_eq!((s.status(), s.banner().0), (READY, "SIMULATION ONLY · Ready. Choose a leg, then start sync and steer with WASD."));
    // Stop does nothing when no session is open.
    s.stop("Operator stop");
    assert_eq!(s.status(), READY);
    assert!(!s.start_enabled());
}

/// A client of a loopback port nothing listens on (port 1): requests fail at
/// once with "connection refused"; no server is started.
fn nowhere() -> Client {
    Client::new(Endpoint { ip: std::net::Ipv4Addr::LOCALHOST.into(), port: 1 }, "token".into(), new_client_id())
}
fn sample_seq(sequence: u64) -> bench::Sample {
    bench::Sample { sequence, time_s: sequence as f64, targets: Vec::new() }
}

#[test]
fn stop_posts_whenever_a_session_may_be_open() {
    let mut s = LiveSync::new(None, &SyncSettings::default());
    s.client = Some(nowhere());
    s.thread = Some(RunThread::idle("hardware-sync", SyncShared::default()));
    s.stop("Operator stop");
    assert_eq!(s.stops_posted, 0, "nothing open, opening, stopping or busy: nothing to stop");
    let setters: [fn(&mut LiveSync); 4] = [|s| s.active = true, |s| s.preparing = true, |s| s.stopping = true, |s| s.server_busy = true];
    for set in setters {
        let posted = s.stops_posted;
        set(&mut s);
        s.stop("Operator stop");
        assert_eq!(s.stops_posted, posted + 1, "STOP is posted, not gated on `active`");
        assert!(s.stopping && !s.active && !s.preparing);
        assert_eq!(s.status(), "Operator stop — verifying physical stop…");
        (s.stopping, s.server_busy) = (false, false);
    }
    // Stopped while `/live/open` was in flight, then the open succeeds: STOP again, never active.
    s.preparing = true;
    s.stop("Operator stop");
    let posted = s.stops_posted;
    s.stopping = false;
    s.thread.as_ref().unwrap().lock().open = Some(Ok(()));
    let _ = s.poll();
    assert!(!s.active && s.stopping, "the late open is not taken as a session");
    assert_eq!(s.stops_posted, posted + 1, "the late open is stopped at once");
    // Not stopping or busy any more: dropping it writes no STOP.
    (s.stopping, s.server_busy) = (false, false);
}

#[test]
fn newest_sample_wins_and_commands_apply_in_order() {
    let (tx, rx) = mpsc::channel();
    let mut opens = 0;
    let mut open = |_: &Body| -> Result<(), String> {
        opens += 1;
        Ok(())
    };
    // Samples queued while idle: the newest is kept; no open, nothing to send at once.
    let mut out = Outbox::default();
    tx.send(SyncCommand::Latest(sample_seq(2))).unwrap();
    tx.send(SyncCommand::Latest(sample_seq(3))).unwrap();
    assert!(!drain(&rx, Some(SyncCommand::Latest(sample_seq(1))), &mut out, &mut open));
    assert_eq!((out.latest.as_ref().map(|s| s.sequence), out.active), (Some(3), false));
    // Open, then newer samples: active, send at once, the newest wins.
    tx.send(SyncCommand::Latest(sample_seq(5))).unwrap();
    tx.send(SyncCommand::Latest(sample_seq(6))).unwrap();
    assert!(drain(&rx, Some(SyncCommand::Open { body: Body::empty(), last_sent: 4 }), &mut out, &mut open));
    assert_eq!((out.latest.as_ref().map(|s| s.sequence), out.active, out.last_sent), (Some(6), true, 4));
    // A stop queued behind an open is applied before anything is sent.
    tx.send(SyncCommand::Deactivate).unwrap();
    tx.send(SyncCommand::Latest(sample_seq(7))).unwrap();
    assert!(!drain(&rx, Some(SyncCommand::Open { body: Body::empty(), last_sent: 6 }), &mut out, &mut open));
    assert_eq!((out.latest.as_ref().map(|s| s.sequence), out.active), (Some(7), false));
    // A failed open stays inactive.
    let mut failing = |_: &Body| -> Result<(), String> { Err("refused".to_string()) };
    assert!(!drain(&rx, Some(SyncCommand::Open { body: Body::empty(), last_sent: 7 }), &mut out, &mut failing));
    assert!(!out.active);
    drop(open);
    assert_eq!(opens, 2);
}

#[test]
fn numbers_round_as_to_fixed() {
    // `toFixed` rounds exact binary ties away from zero; `format!` would print "12%", "12.2" and "5.62".
    assert_eq!(stats_text(0.125, 1.0 / 8.0), "RMS error 0.13° · at drive limit 13%");
    let rows = mapping(&coords(), &[4, 5, 6], "+X", None);
    // 64 counts = 5.625°; 12.25 V.
    let mut p = sample(4, 64, 0.0, None, None, 0.0, 0.0);
    p.telemetry.voltage_v = 12.25;
    p.live_source = Some(vec![serde_json::json!(0), serde_json::json!(0), serde_json::json!(0.0125)]);
    let lines = reading_lines(&[p], &rows);
    assert_eq!(lines.lines().next(), Some("ID 4: 5.63° · 12.3 V · 35 °C · input age 13 ms"));
}

#[test]
fn replays_recordings_and_gait_previews_are_not_live() {
    use crate::robot::run::ReplayPhase as R;
    assert!(live_run(false, true, R::Idle, false, false), "a preset's own run");
    assert!(!live_run(true, false, R::Idle, false, false), "recorded playback");
    assert!(!live_run(false, false, R::Idle, false, false), "--robot FILE (no named targets)");
    assert!(!live_run(false, true, R::Replaying, false, false), "replay in progress");
    assert!(!live_run(false, true, R::Cancelled, true, false), "cancelled partial replay (replay_block)");
    assert!(!live_run(false, true, R::Done, true, false), "a run a replay replaced, until Reset");
    assert!(live_run(false, true, R::Failed, false, false), "a replay refused before replacing the run");
    assert!(!live_run(false, true, R::Idle, false, true), "a gait preview holds the run");
}

/// A connected LiveSync with three distinct motors, posting to nowhere.
fn connected() -> LiveSync {
    let mut s = LiveSync::new(None, &SyncSettings::default());
    s.client = Some(nowhere());
    s.thread = Some(RunThread::idle("hardware-sync", SyncShared::default()));
    s.config = Some(bench::Config { ids: vec![4, 5, 6], coordinates: coords(), ..Default::default() });
    s.rows = mapping(&coords(), &[4, 5, 6], "+X", None);
    s
}
fn input(start: Result<bool, String>) -> StartInput {
    StartInput { initial: Ok(sample_seq(1)), source: String::new(), key: None, running: false, start }
}

#[test]
fn start_refused_by_the_run_opens_no_session() {
    let mut s = connected();
    let now = Instant::now();
    let mut acts = Vec::new();
    let why = "the run failed; Reset rebuilds the robot before it can run again";
    s.start_with(input(Err(why.into())), &mut |a| acts.push(a), now);
    assert!(!s.preparing && !s.active && s.start_sent.is_none(), "no /live/open");
    assert_eq!(s.status(), why);
    assert!(matches!(acts.as_slice(), [RobotAction::Run { action: RunAction::Pause }]), "the page's error path pauses; no Start");
    // Not live (a replay, recorded playback, gait preview): the page's text, before the run's check.
    let mut acts = Vec::new();
    s.start_with(StartInput { initial: Err(NOT_LIVE.into()), ..input(Ok(true)) }, &mut |a| acts.push(a), now);
    assert!(!s.preparing && s.status() == NOT_LIVE);
    // Accepted: Start is sent and the session opens.
    let mut acts = Vec::new();
    s.start_with(input(Ok(true)), &mut |a| acts.push(a), now);
    assert!(s.preparing && s.start_sent == Some(now));
    assert!(matches!(acts.as_slice(), [RobotAction::Run { action: RunAction::Start }]));
    // Already running: no Start.
    let mut t = connected();
    let mut acts = Vec::new();
    t.start_with(StartInput { running: true, ..input(Ok(false)) }, &mut |a| acts.push(a), now);
    assert!(t.preparing && acts.is_empty() && t.start_sent.is_none());
    (t.preparing, t.stopping) = (false, false);
}

#[test]
fn stop_rules_without_frames() {
    let now = Instant::now();
    let seen = |phase, live: Result<(), String>| RunSeen { key: None, phase: Some(phase), live };
    // Start accepted but the run never reports Running: stopped after the grace.
    let mut s = connected();
    s.start_with(input(Ok(true)), &mut |_| {}, now);
    s.watch(seen(Phase::Paused, Ok(())), now + START_GRACE / 2);
    assert!(s.preparing, "within the grace");
    let posted = s.stops_posted;
    s.watch(seen(Phase::Paused, Ok(())), now + START_GRACE * 2);
    assert!(!s.preparing && s.stops_posted == posted + 1);
    assert_eq!(s.status(), format!("{NOT_STARTED} — verifying physical stop…"));
    // Running, then no longer live without a new frame (a gait preview opened): stopped with the page's text.
    let mut s = connected();
    s.start_with(input(Ok(true)), &mut |_| {}, now);
    s.watch(seen(Phase::Running, Ok(())), now);
    assert!(s.preparing && s.start_sent.is_none() && s.was_running);
    s.watch(seen(Phase::Running, Err(NOT_LIVE.into())), now);
    assert_eq!(s.status(), format!("{NOT_LIVE} — verifying physical stop…"));
    // The run failed.
    let mut s = connected();
    s.start_with(input(Ok(true)), &mut |_| {}, now);
    s.watch(seen(Phase::Failed, Ok(())), now);
    assert_eq!(s.status(), format!("{RUN_FAILED} — verifying physical stop…"));
    s.stopping = false;
}

#[test]
fn session_ended_by_the_bench_deactivates_the_sync_thread() {
    let (tx, seen) = mpsc::channel();
    let mut s = LiveSync::new(None, &SyncSettings::default());
    s.client = Some(nowhere());
    s.thread = Some(RunThread::spawn("hardware-sync", SyncShared::default(), move |rx: mpsc::Receiver<SyncCommand>, _| {
        for c in rx {
            let _ = tx.send(matches!(c, SyncCommand::Deactivate));
        }
    }));
    s.active = true;
    // The bench answers that no session is active (completed, or stopped elsewhere).
    s.status_job = Some(Job::finished(0, Ok(bench::Status::default())));
    assert!(s.poll(), "the run is paused");
    assert!(!s.active && !s.stopping);
    assert_eq!(seen.recv_timeout(Duration::from_secs(5)), Ok(true), "Deactivate sent: no further sample is posted");
}

#[test]
fn leave_stop_only_for_our_session() {
    let mut s = connected();
    s.post_stop_on_leave();
    assert!(!s.left.load(std::sync::atomic::Ordering::SeqCst), "not ours: nothing written");
    s.preparing = true;
    // Port 1 refuses at once: the write fails fast and is logged, and the
    // once-only flag is cleared so a later exit path (the drop at
    // teardown) tries again.
    s.post_stop_on_leave();
    assert!(!s.left.load(std::sync::atomic::Ordering::SeqCst), "a failed write is retried by a later exit path");
    s.preparing = false;
}

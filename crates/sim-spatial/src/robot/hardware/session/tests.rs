//! The session against an in-process fake calibration server (plain HTTP on
//! 127.0.0.1:0, served from a `jobs::Pool::Dedicated` job): no window, no
//! hardware. Most tests drive [`Session`]'s handlers directly; the disconnect
//! test runs [`run`] on the test thread, and late preference publication drives
//! the real [`super::super::link::Link`] against the fake server. The heartbeats
//! come from the session's beat thread, as in the viewer; the fake answers
//! each connection on its own job, as the real server does, so a request it
//! holds does not hold the others.
use super::*;
use crate::jobs::{Job, Pool};
use serde_json::json;
use sim_runtime::hardware_client::Endpoint;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream};

type Handler = Arc<dyn Fn(&str, &Value) -> Result<Value, String> + Send + Sync>;
/// (path, body, when it was parsed) of every request, in arrival order.
type Log = Arc<Mutex<Vec<(String, Value, Instant)>>>;

/// The fake server; dropping it cancels its job, which ends its accept loop.
struct Fake {
    port: u16,
    log: Log,
    _job: Job<()>,
}
impl Fake {
    fn start(handler: Handler) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.set_nonblocking(true).expect("nonblocking");
        let port = listener.local_addr().expect("address").port();
        let log: Log = Arc::default();
        let served = log.clone();
        let job = Job::spawn(Pool::Dedicated, 0, "fake calibration server", move |ctx| {
            while !ctx.cancelled() {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let (handler, served) = (handler.clone(), served.clone());
                        let request = Job::spawn(Pool::Dedicated, 0, "fake calibration request", move |_| {
                            answer(stream, &handler, &served);
                            Ok(())
                        });
                        // Runs to its end when the handle is dropped (a
                        // cancelled dedicated job may never start).
                        drop(request.complete_on_drop());
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(1)),
                    Err(e) => return Err(e.to_string()),
                }
            }
            Ok(())
        });
        Fake { port, log, _job: job }
    }
    fn client(&self) -> Client {
        let endpoint = Endpoint::loopback(IpAddr::V4(Ipv4Addr::LOCALHOST), self.port).expect("loopback");
        Client::new(endpoint, "token".into(), "00000000-0000-4000-8000-000000000000".into()).with_timeout(Duration::from_secs(2))
    }
    /// (path, body) of every request so far; a GET's body is null.
    fn requests(&self) -> Vec<(String, Value)> {
        self.log.lock().unwrap().iter().map(|(p, b, _)| (p.clone(), b.clone())).collect()
    }
    /// The commands posted so far, with when each arrived.
    fn timed_commands(&self) -> Vec<(Value, Instant)> {
        self.log.lock().unwrap().iter().filter(|(p, _, _)| p == COMMAND).map(|(_, b, t)| (b.clone(), *t)).collect()
    }
    /// The commands posted so far.
    fn commands(&self) -> Vec<Value> {
        self.requests().into_iter().filter(|(p, _)| p == COMMAND).map(|(_, b)| b).collect()
    }
}

/// One request per connection, as the client sends them.
fn answer(mut stream: TcpStream, handler: &Handler, log: &Log) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut data = Vec::new();
    let mut chunk = [0u8; 4096];
    let end = loop {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => data.extend_from_slice(&chunk[..n]),
        }
        if let Some(p) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break p + 4;
        }
    };
    let head = String::from_utf8_lossy(&data[..end]).to_string();
    let length = head
        .lines()
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while data.len() < end + length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => data.extend_from_slice(&chunk[..n]),
        }
    }
    let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
    let body: Value = if length > 0 { serde_json::from_slice(&data[end..end + length]).unwrap_or(Value::Null) } else { Value::Null };
    log.lock().unwrap().push((path.clone(), body.clone(), Instant::now()));
    // As the calibration server answers: a binding refusal (its text starts
    // with `calibration::BINDING_REFUSED`) is 409, any other refusal 400.
    let (status, reply) = match handler(path.as_str(), &body) {
        Ok(v) => (200, v),
        Err(e) if e.starts_with(calibration::BINDING_REFUSED) => (calibration::BINDING_REFUSED_STATUS, json!({ "error": e })),
        Err(e) => (400, json!({ "error": e })),
    };
    let text = reply.to_string();
    let _ = write!(stream, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
    let _ = stream.flush();
}

/// A server that enables the chosen motor, starts run 7 and accepts heartbeats.
fn motor_server(enabled: u8) -> Handler {
    Arc::new(move |path: &str, body: &Value| -> Result<Value, String> {
        if path == STATUS {
            return Ok(json!({ "connected": true, "enabled_id": enabled }));
        }
        Ok(match body["action"].as_str().unwrap_or("") {
            "select" => json!({ "connected": true, "enabled_id": body["id"] }),
            "motion_start" => json!({ "enabled_id": body["id"], "sweep": { "running": true, "run_id": 7, "motor_id": body["id"] } }),
            "motion_update" => json!({ "ok": true }),
            "stop" => json!({ "enabled_id": null, "message": "Stopped" }),
            other => return Err(format!("unexpected {other}")),
        })
    })
}

/// The actions of `commands`, in order.
fn actions(commands: &[Value]) -> Vec<String> {
    commands.iter().map(|c| c["action"].as_str().unwrap_or("").to_string()).collect()
}
/// The sequences of the sequenced `commands`, in arrival order.
fn sequences(commands: &[Value]) -> Vec<u64> {
    commands.iter().filter_map(|c| c["sequence"].as_u64()).collect()
}

impl Session {
    /// Returns once the beat has handled everything sent to it so far,
    /// including a heartbeat it was sending.
    fn beat_flush(&self) {
        let (tx, rx) = mpsc::channel();
        if let Some(b) = &self.beat {
            b.send(beat::Beat::Flush(tx)).expect("beat running");
            rx.recv_timeout(Duration::from_secs(5)).expect("beat answers");
        }
    }
}

fn session(fake: &Fake, epoch: Arc<AtomicU64>) -> (Session, Arc<Mutex<LinkSnapshot>>) {
    let shared = Arc::new(Mutex::new(LinkSnapshot::default()));
    (Session::new(fake.client(), 1, Arc::new(AtomicU64::new(0)), epoch, shared.clone()), shared)
}

/// Written only: the real connected link receives publication before later
/// explicit selection/jog. Its client is an isolated loopback fake, never leg
/// hardware; no settings file or connection job is used.
#[test]
fn connected_link_uses_late_published_form_inputs_without_publication_motion() {
    use crate::robot::hardware::{Hardware, HardwareConfig, actions, link, settings};
    let fake = Fake::start(motor_server(1));
    let mut hw = Hardware::new(HardwareConfig::default(), settings::Settings::default());
    let connected = link::Link::spawn(fake.client(), 1);
    // Match poll_jobs: initialize the connected host from the startup form.
    connected.send(LinkCommand::Inputs(hw.form.inputs.clone()));
    hw.link = Some(connected);
    let mut loaded = settings::Settings::default();
    loaded.calibration.drive_mode = Some(actions::DriveMode::ServoSpeed);
    loaded.calibration.hold_others = Some(false);
    actions::seed_preferences(&mut hw, &loaded);
    assert_eq!(hw.form.inputs.drive_mode, actions::DriveMode::ServoSpeed);
    assert!(!hw.form.inputs.hold_others);
    assert!(!hw.form.tune_ok && !hw.form.campaign_ok && !hw.form.gait_ok);
    assert!(!hw.form.held_upper && !hw.form.held_lower);
    assert!(!hw.sync.engaged());
    // Inputs itself cannot issue a request: verify the actual Session handler
    // independently, without relying on asynchronous timing for that claim.
    let (mut host, _) = session(&fake, Arc::default());
    host.handle(LinkCommand::Inputs(hw.form.inputs.clone()));
    assert!(fake.commands().is_empty(), "publication sends no hardware command");
    assert_eq!(host.input().drive_mode, "servo_speed");
    assert!(!host.input().hold_others);
    let connected = hw.link.as_ref().unwrap();
    // These are explicit user commands, FIFO after the publication Inputs.
    connected.send(LinkCommand::Select { id: 1 });
    connected.send(LinkCommand::Press { direction: Direction::Upper });
    let deadline = Instant::now() + Duration::from_secs(2);
    while !fake.commands().iter().any(|c| c["action"] == "motion_start") {
        assert!(Instant::now() < deadline, "explicit jog did not reach fake");
        std::thread::sleep(Duration::from_millis(1));
    }
    let commands = fake.commands();
    assert_eq!(commands[0]["action"], "select");
    assert_eq!(commands[0]["hold_others"], false);
    let jog = commands.iter().find(|c| c["action"] == "motion_start").unwrap();
    assert_eq!(jog["drive_mode"], "servo_speed");
    assert_eq!(jog["hold_others"], false);
}

#[test]
fn select_press_and_heartbeat_carry_the_run_and_increasing_sequences() {
    let fake = Fake::start(motor_server(1));
    let (mut s, shared) = session(&fake, Arc::default());
    s.handle(LinkCommand::Select { id: 1 });
    assert!(s.snap.ready && !s.snap.busy, "{:?}", s.snap.state.message);
    s.handle(LinkCommand::Press { direction: Direction::Upper });
    assert_eq!((s.snap.run, s.snap.intent), (Some(7), Intent::Upper));
    // An intent change's `update()`, then the beat's own 100 ms heartbeats.
    s.update();
    std::thread::sleep(Duration::from_millis(350));
    s.beat_flush();
    let commands = fake.commands();
    let actions = actions(&commands);
    assert_eq!(actions[..3], ["select", "motion_start", "motion_update"], "{actions:?}");
    assert!(actions[2..].iter().all(|a| a == "motion_update"), "{actions:?}");
    // The press's and the update's, and at least two periodic ones (100 ms after each answer).
    assert!(actions.len() >= 6, "{actions:?}");
    let sequences = sequences(&commands);
    assert_eq!(sequences.len(), commands.len(), "every command carries a sequence");
    assert!(sequences.windows(2).all(|w| w[0] < w[1]), "{sequences:?}");
    for heartbeat in &commands[2..] {
        assert_eq!((heartbeat["run_id"].as_u64(), heartbeat["motion"].as_str(), heartbeat["id"].as_u64()), (Some(7), Some("upper"), Some(1)));
    }
    let published = shared.lock().unwrap().clone();
    assert_eq!((published.run, published.generation), (Some(7), 1));
    assert!(published.read_at.is_some());
}

#[test]
fn stopped_clears_the_session_and_a_pending_stop_sends_nothing() {
    let fake = Fake::start(motor_server(1));
    let epoch = Arc::new(AtomicU64::new(0));
    let (mut s, shared) = session(&fake, epoch.clone());
    s.handle(LinkCommand::Select { id: 1 });
    s.handle(LinkCommand::Press { direction: Direction::Lower });
    assert_eq!(s.snap.run, Some(7));
    // The UI's immediate STOP: epoch bumped, `Stopped` still queued. A
    // heartbeat the beat was already sending is let finish (it went before the STOP).
    let stopped = epoch.fetch_add(1, SeqCst) + 1;
    assert!(s.interrupted());
    s.beat_flush();
    let sent = fake.commands().len();
    s.update();
    s.handle(LinkCommand::Release);
    s.handle(LinkCommand::Capture { boundary: crate::robot::hardware::actions::Boundary::Upper, reference_joint_rad: None });
    assert_eq!(s.snap.state.capture_message.as_deref(), Some(STOP_PENDING));
    // Several heartbeat periods: the beat sends nothing either.
    std::thread::sleep(Duration::from_millis(350));
    s.beat_flush();
    assert_eq!(fake.commands().len(), sent, "no heartbeat while a STOP is pending");
    s.handle(LinkCommand::Stopped { epoch: stopped });
    assert!(!s.interrupted());
    assert_eq!((s.snap.run, s.snap.ready, s.snap.starting, s.snap.intent), (None, false, false, Intent::Hold));
    assert_eq!(fake.commands().len(), sent, "Stopped sends nothing (the UI posted STOP)");
    assert_eq!(shared.lock().unwrap().run, None);
    s.handle(LinkCommand::StopAnswered(Ok(json!({ "enabled_id": null, "message": "Stopped" }))));
    assert_eq!(s.snap.state.message.as_deref(), Some("Stopped"));
}

#[test]
fn a_closed_channel_sends_stop_for_the_selected_motor() {
    let fake = Fake::start(motor_server(3));
    let (tx, rx) = mpsc::channel();
    tx.send(LinkCommand::Select { id: 3 }).unwrap();
    drop(tx);
    let shared = Arc::new(Mutex::new(LinkSnapshot::default()));
    run(fake.client(), 1, Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)), Arc::default(), rx, shared.clone());
    let commands = fake.commands();
    let last = commands.last().expect("a request");
    assert_eq!((last["action"].as_str(), last["id"].as_u64()), (Some("stop"), Some(3)), "{commands:?}");
    let published = shared.lock().unwrap().clone();
    assert!(!published.ready && published.run.is_none());
}

#[test]
fn an_answer_from_before_a_stop_is_dropped_and_stopped_again() {
    let epoch = Arc::new(AtomicU64::new(0));
    let bump = epoch.clone();
    // STOP is pressed while the select is in flight (the UI's bump; its own
    // request goes on another connection, not to this fake).
    let handler: Handler = Arc::new(move |_: &str, body: &Value| -> Result<Value, String> {
        if body["action"].as_str() == Some("select") {
            bump.fetch_add(1, SeqCst);
        }
        Ok(json!({ "connected": true, "enabled_id": body["id"] }))
    });
    let fake = Fake::start(handler);
    let (mut s, _) = session(&fake, epoch.clone());
    s.handle(LinkCommand::Select { id: 2 });
    let stopped = epoch.load(SeqCst);
    assert_eq!(s.snap.id, Some(2));
    assert!(!s.snap.ready && !s.snap.busy);
    assert!(!s.snap.state.connected, "the stale answer is not adopted");
    assert_eq!(s.snap.state.message, None);
    // The select the server may have parsed after the STOP is stopped again at once.
    let commands = fake.commands();
    let sent: Vec<(Option<&str>, Option<u64>)> = commands.iter().map(|c| (c["action"].as_str(), c["id"].as_u64())).collect();
    assert_eq!(sent, [(Some("select"), Some(2)), (Some("stop"), Some(2))]);
    assert!(commands[0]["sequence"].as_u64() < commands[1]["sequence"].as_u64());
    // Until `Stopped` arrives nothing but stop is sent.
    assert_eq!(s.send(calibration::flip(2, s.seq())), Err(STOP_PENDING.to_string()));
    s.handle(LinkCommand::Stopped { epoch: stopped });
    assert!(!s.interrupted());
}

#[test]
fn a_command_between_two_stops_sends_nothing_until_the_second_is_applied() {
    let fake = Fake::start(motor_server(1));
    let epoch = Arc::new(AtomicU64::new(0));
    let (mut s, _) = session(&fake, epoch.clone());
    // Two STOPs posted while the link thread was busy, a chip click queued between them.
    let first = epoch.fetch_add(1, SeqCst) + 1;
    let second = epoch.fetch_add(1, SeqCst) + 1;
    s.handle(LinkCommand::Stopped { epoch: first });
    assert!(s.interrupted(), "the second STOP is still pending");
    s.handle(LinkCommand::Select { id: 1 });
    assert!(s.interrupted(), "the select's own bump does not apply the second STOP");
    s.handle(LinkCommand::Stopped { epoch: second });
    assert!(!s.interrupted(), "the select's own bump counts once both STOPs are applied");
    let commands = fake.commands();
    assert!(commands.iter().all(|c| c["action"].as_str() != Some("select")), "{commands:?}");
    assert!(!s.snap.ready && s.snap.run.is_none());
    // Afterwards a chip click selects again.
    s.handle(LinkCommand::Select { id: 1 });
    assert!(s.snap.ready, "{:?}", s.snap.state.message);
}

#[test]
fn inputs_sent_before_a_speed_reset_keep_the_reset_speed() {
    let fake = Fake::start(motor_server(1));
    let (mut s, _) = session(&fake, Arc::default());
    s.handle(LinkCommand::Inputs(Inputs { speed_percent: 60.0, ..Inputs::default() }));
    s.handle(LinkCommand::Select { id: 1 });
    // "Try saved range": the session starts, sweeps and resets the speed.
    s.handle(LinkCommand::Sweep);
    assert_eq!((s.snap.run, s.snap.intent), (Some(7), Intent::Sweep));
    assert_eq!((s.snap.speed_reset, s.inputs.speed_percent), (1, 0.0));
    // Sent by the UI before it applied that reset: the speed stays 0, the rest is taken.
    s.handle(LinkCommand::Inputs(Inputs { speed_percent: 60.0, pwm_percent: 50.0, ..Inputs::default() }));
    assert_eq!((s.inputs.speed_percent, s.inputs.pwm_percent), (0.0, 50.0));
    // After the UI applied it, its speed is taken again.
    s.handle(LinkCommand::Inputs(Inputs { speed_percent: 30.0, speed_reset: 1, ..Inputs::default() }));
    assert_eq!(s.inputs.speed_percent, 30.0);
}

#[test]
fn drive_active_counts_every_driving_sequence() {
    use crate::robot::hardware::link::drive_active;
    assert!(!drive_active(&LinkSnapshot::default()));
    let cases: [fn(&mut LinkSnapshot); 8] = [
        |s| s.ready = true,
        |s| s.starting = true,
        |s| s.run = Some(1),
        |s| s.busy = true,
        |s| s.sweep_all = true,
        |s| s.tuning = true,
        |s| s.campaigning = true,
        |s| s.gait = Some(crate::robot::hardware::link::GaitRun { mode: crate::robot::hardware::actions::GaitMode::Leg, period_s: 1.0, t: 0.0, playing: true, scale: 1.0, leg: true, skipped: Vec::new(), started: false }),
    ];
    for set in cases {
        let mut s = LinkSnapshot::default();
        set(&mut s);
        assert!(drive_active(&s), "{s:?}");
    }
}

#[test]
fn heartbeats_keep_the_lease_while_the_link_thread_waits_and_a_pending_stop_silences_them() {
    let epoch = Arc::new(AtomicU64::new(0));
    let bump = epoch.clone();
    let pressed: Arc<Mutex<Option<Instant>>> = Arc::default();
    let marked = pressed.clone();
    let motor = motor_server(1);
    // The gait list is held 1.6 s, longer than the server's 1.5 s motion
    // lease; 1 s in, the UI's STOP is pressed (its own request goes on
    // another connection, not to this fake).
    let handler: Handler = Arc::new(move |path: &str, body: &Value| -> Result<Value, String> {
        if path == "/calibration/gaits" {
            std::thread::sleep(Duration::from_millis(1000));
            bump.fetch_add(1, SeqCst);
            *marked.lock().unwrap() = Some(Instant::now());
            std::thread::sleep(Duration::from_millis(600));
            return Ok(json!({ "gaits": [] }));
        }
        motor(path, body)
    });
    let fake = Fake::start(handler);
    let (mut s, _) = session(&fake, epoch.clone());
    s.handle(LinkCommand::Select { id: 1 });
    s.handle(LinkCommand::Press { direction: Direction::Upper });
    assert_eq!(s.snap.run, Some(7), "{:?}", s.snap.state.message);
    let asked = Instant::now();
    s.handle(LinkCommand::LoadGaits);
    assert!(asked.elapsed() >= Duration::from_millis(1600), "the link thread waited on the gait list");
    assert!(s.snap.gaits_loaded, "{:?}", s.snap.gait_notice);
    let stop_at = pressed.lock().unwrap().expect("STOP pressed");
    s.beat_flush();
    let timed = fake.timed_commands();

    // Until the STOP, a heartbeat about every 100 ms, never a gap near the lease.
    let beats: Vec<Instant> = timed.iter().filter(|(c, t)| c["action"] == "motion_update" && *t >= asked && *t < stop_at).map(|(_, t)| *t).collect();
    assert!(beats.len() >= 5, "{} heartbeats in 1 s", beats.len());
    let mut last = asked;
    for t in beats.iter().chain([&stop_at]) {
        assert!(t.duration_since(last) < Duration::from_millis(500), "a {:?} gap between heartbeats", t.duration_since(last));
        last = *t;
    }
    // After it, at most the heartbeat already being sent when STOP was pressed.
    let after: Vec<&Value> = timed.iter().filter(|(_, t)| *t >= stop_at).map(|(c, _)| c).collect();
    assert!(after.len() <= 1 && after.iter().all(|c| c["action"] == "motion_update"), "{after:?}");

    // Every request carries a larger sequence than the one before it, and
    // every heartbeat the run, the motor and the intent.
    let commands: Vec<Value> = timed.into_iter().map(|(c, _)| c).collect();
    let sequences = sequences(&commands);
    assert_eq!(sequences.len(), commands.len(), "every command carries a sequence");
    assert!(sequences.windows(2).all(|w| w[0] < w[1]), "{sequences:?}");
    for heartbeat in commands.iter().filter(|c| c["action"] == "motion_update") {
        assert_eq!((heartbeat["run_id"].as_u64(), heartbeat["id"].as_u64(), heartbeat["motion"].as_str()), (Some(7), Some(1), Some("upper")));
    }

    // While the STOP is pending nothing is sent: not an intent change, not
    // a pose, not the beat's periodic heartbeat.
    let sent = commands.len();
    s.update();
    s.handle(LinkCommand::Release);
    s.handle(LinkCommand::Capture { boundary: crate::robot::hardware::actions::Boundary::Lower, reference_joint_rad: None });
    std::thread::sleep(Duration::from_millis(350));
    s.beat_flush();
    assert_eq!(fake.commands().len(), sent, "{:?}", &fake.commands()[sent..]);
    // Once applied, the session is over and the beat stays quiet.
    s.handle(LinkCommand::Stopped { epoch: epoch.load(SeqCst) });
    assert!(!s.interrupted());
    assert_eq!((s.snap.run, s.snap.ready), (None, false));
    std::thread::sleep(Duration::from_millis(250));
    s.beat_flush();
    assert_eq!(fake.commands().len(), sent);
}

#[test]
fn a_heartbeat_the_server_refuses_ends_the_session_on_the_link_thread() {
    let refuse = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let refusing = refuse.clone();
    let motor = motor_server(1);
    let handler: Handler = Arc::new(move |path: &str, body: &Value| -> Result<Value, String> {
        if body["action"] == "motion_update" && refusing.load(SeqCst) {
            return Err("Sweep browser lease expired; start again explicitly".into());
        }
        motor(path, body)
    });
    let fake = Fake::start(handler);
    let (mut s, _) = session(&fake, Arc::default());
    s.handle(LinkCommand::Select { id: 1 });
    s.handle(LinkCommand::Press { direction: Direction::Lower });
    assert_eq!(s.snap.run, Some(7));
    refuse.store(true, SeqCst);
    // Only the beat's failure is under test (a status poll would replace the message).
    s.next_poll = Instant::now() + Duration::from_secs(60);
    // The beat's next periodic heartbeat is refused; the link thread takes
    // the failure on its next pass and stops, as the page's `update()` does.
    std::thread::sleep(Duration::from_millis(250));
    s.beat_flush();
    s.run_due(Instant::now());
    assert_eq!((s.snap.run, s.snap.ready), (None, false));
    assert_eq!(s.snap.state.message.as_deref(), Some("Sweep browser lease expired; start again explicitly"));
    let actions = actions(&fake.commands());
    assert!(actions.contains(&"stop".to_string()), "{actions:?}");
}

/// Waits until `worker` has handled everything sent to it so far.
fn flush(worker: &beat::Handle) {
    let (tx, rx) = mpsc::channel();
    worker.send(beat::Beat::Flush(tx)).expect("beat running");
    rx.recv_timeout(Duration::from_secs(5)).expect("beat answers");
}

#[test]
fn the_gait_lease_renews_on_its_period_at_once_on_a_change_and_never_during_a_stop() {
    let handler: Handler = Arc::new(|_: &str, body: &Value| -> Result<Value, String> {
        match body["action"].as_str() {
            Some("gait_update") => Ok(json!({ "ok": true })),
            other => Err(format!("unexpected {other:?}")),
        }
    });
    let fake = Fake::start(handler);
    let (sequence, epoch) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
    let worker = beat::spawn(fake.client(), sequence.clone(), epoch.clone());
    let plan = |seen_epoch: u64, scale: f64, playing: bool| beat::Plan { seen_epoch, motion: None, gait: Some(beat::GaitBeat { scale, playing }) };
    worker.send(beat::Beat::Plan(plan(0, 1.0, true))).unwrap();
    // Every 300 ms (the first a period after the gait started).
    std::thread::sleep(Duration::from_millis(1000));
    let periodic = fake.commands().len();
    assert!((2..=4).contains(&periodic), "{periodic} lease updates in 1 s");
    // A pause goes at once, not a period later.
    let paused = Instant::now();
    worker.send(beat::Beat::Plan(plan(0, 0.5, false))).unwrap();
    std::thread::sleep(Duration::from_millis(150));
    flush(&worker);
    let timed = fake.timed_commands();
    let at_once = |(c, t): &(Value, Instant)| *t >= paused && t.duration_since(paused) < Duration::from_millis(100) && c["speed_scale"].as_f64() == Some(0.5) && c["playing"] == false;
    assert!(timed.iter().any(at_once), "{timed:?}");
    // A STOP pending (the epoch ahead of the plan's): nothing.
    epoch.fetch_add(1, SeqCst);
    std::thread::sleep(Duration::from_millis(100));
    flush(&worker);
    let held = fake.commands().len();
    std::thread::sleep(Duration::from_millis(700));
    flush(&worker);
    assert_eq!(fake.commands().len(), held, "no lease update while a STOP is pending");
    // Applied (the plan caught up): renewed again.
    worker.send(beat::Beat::Plan(plan(1, 0.5, false))).unwrap();
    std::thread::sleep(Duration::from_millis(400));
    flush(&worker);
    assert!(fake.commands().len() > held, "the lease is renewed once the STOP is applied");
    // The lease carries no sequence: the shared counter is untouched.
    assert_eq!(sequence.load(SeqCst), 0);
    // Dropping the handle ends it: nothing more is sent.
    drop(worker);
    std::thread::sleep(Duration::from_millis(100));
    let ended = fake.commands().len();
    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(fake.commands().len(), ended);
}


// LC1–LC2 fixtures: written and source-inspected; not executed this turn.
fn virtual_identity(server: &str) -> calibration::ExecutionIdentity {
    calibration::ExecutionIdentity { schema_version: 1, kind: "virtual_calibration".into(),
        server_instance: server.into(), bench_instance: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into() }
}
fn virtual_document(identity: &calibration::ExecutionIdentity) -> Value {
    json!({"connected":true,"execution":identity,"enabled_id":1,
        "calibration":{"axes":{"1":{"lower":100,"upper":1000,"reference":500,
            "tuning":{"pid":{"kp":2.0,"ki":0.1,"kd":0.01},"friction_duty":0.2,"record":"retained-tune.json"}}}},
        "campaign":{"running":false,"completed":2,"stage":"stopped","directory":"retained-campaign"}})
}
fn virtual_status(identity: &calibration::ExecutionIdentity) -> Status {
    serde_json::from_value(virtual_document(identity)).unwrap()
}
fn pinned_session(fake: &Fake, identity: calibration::ExecutionIdentity) -> Session {
    let shared = Arc::new(Mutex::new(LinkSnapshot::default()));
    let mut session = Session::new(fake.client().with_calibration_execution(identity.clone(), 1), 1,
        Arc::default(), Arc::default(), shared);
    session.adopt(virtual_status(&identity));
    session.snap.id = Some(1);
    session.snap.ready = true;
    session
}

#[test]
fn real_session_replacement_revokes_and_preserves_accepted_records() {
    let first = virtual_identity("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    let replacement = virtual_identity("cccccccc-cccc-4ccc-8ccc-cccccccccccc");
    let response = virtual_document(&first);
    let fake = Fake::start(Arc::new(move |_, _| Ok(response.clone())));
    let mut session = pinned_session(&fake, first.clone());
    session.adopt(virtual_status(&replacement));
    assert!(session.snap.authorization_revoked);
    assert!(!session.snap.connection_valid && !session.snap.ready);
    assert_eq!(session.axis().tuning.unwrap().record, "retained-tune.json");
    assert_eq!(session.snap.state.campaign.as_ref().unwrap().completed, 2);
    session.adopt(virtual_status(&first));
    assert!(session.snap.authorization_revoked, "a later matching status cannot renew a revoked generation");
    session.handle(LinkCommand::Checked { ticket: 1, epoch: session.epoch_now(), generation: 1, inputs: None, command:Box::new(LinkCommand::Select {id:1}) });
    assert!(session.snap.command_results[&1].is_err());
    assert!(fake.commands().iter().all(|c| c["action"] == "stop"));
}

#[test]
fn checked_real_session_refuses_stale_physical_unknown_and_interrupted_work() {
    let identity = virtual_identity("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    let fake = Fake::start(Arc::new(|_, _| panic!("refused command must not reach HTTP")));
    let mut session = pinned_session(&fake, identity);
    session.snap.read_at = Some(Instant::now() - crate::robot::hardware::link::STALE_AFTER - Duration::from_millis(1));
    session.handle(LinkCommand::Checked { ticket: 1, epoch: 0, generation: 1, inputs: None, command:Box::new(LinkCommand::Select {id:1})});
    assert!(session.snap.command_results[&1].is_err());
    session.snap.read_at = Some(Instant::now());
    session.snap.execution = None;
    session.handle(LinkCommand::Checked { ticket: 2, epoch: 0, generation: 1, inputs: None, command:Box::new(LinkCommand::Select {id:1})});
    assert!(session.snap.command_results[&2].is_err());
    session.snap.execution = Some(calibration::ExecutionIdentity {kind:"physical".into(), ..virtual_identity("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")});
    session.handle(LinkCommand::Checked { ticket: 3, epoch: 0, generation: 1, inputs: None, command:Box::new(LinkCommand::Select {id:1})});
    assert!(session.snap.command_results[&3].is_err());
    session.epoch.fetch_add(1, SeqCst);
    session.handle(LinkCommand::Checked { ticket: 4, epoch: 0, generation: 1, inputs: None, command:Box::new(LinkCommand::Capture {boundary:crate::robot::hardware::actions::Boundary::Lower,reference_joint_rad:None})});
    assert_eq!(session.snap.command_results[&4], Err(STOP_PENDING.into()));
    assert_eq!(session.axis().lower, Some(100));
    assert_eq!(session.snap.state.campaign.as_ref().unwrap().completed, 2);
    assert!(fake.commands().is_empty());
}

#[test]
fn actual_remote_handler_waits_for_real_link_consumer_refusal() {
    use crate::app::actions::{Call, Origin, Replies};
    use crate::robot::hardware::{Hardware, HardwareConfig, actions::HardwareAction, handlers::{dispatch, Answer}, settings, link::Link};
    let identity = virtual_identity("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    let status = json!({"connected":true,"execution":identity,"calibration":{"axes":{"1":{"role":"Knee"}}}});
    let fake = Fake::start(Arc::new(move |path, body| {
        if path == STATUS || body["action"] == "stop" { Ok(status.clone()) }
        else { Err("authoritative selection refused".into()) }
    }));
    let mut hw = Hardware::new(HardwareConfig::default(), settings::Settings::default());
    hw.generation = 1;
    hw.link = Some(Link::spawn(fake.client().with_calibration_execution(identity, 1), 1));
    let deadline = Instant::now() + Duration::from_secs(5);
    while hw.link.as_ref().unwrap().snapshot().read_at.is_none() {
        assert!(Instant::now() < deadline, "fresh status deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
    hw.snapshot = hw.link.as_ref().unwrap().snapshot();
    let mut replies = Replies::default();
    let origin = Origin::Rest(replies.open());
    let mut continuation = Value::Null;
    let action = HardwareAction::Select {id:1};
    let mut call = Call {origin,continuation:&mut continuation,cancelled:false,replies:&mut replies};
    assert!(matches!(dispatch(&mut hw,&action,&mut call,Instant::now(),None,&mut |_| {}), Answer::Pending));
    loop {
        assert!(Instant::now() < deadline, "authoritative acknowledgement deadline");
        match dispatch(&mut hw,&action,&mut call,Instant::now(),None,&mut |_| {}) {
            Answer::Pending => std::thread::sleep(Duration::from_millis(1)),
            Answer::Done(Err(error)) => { assert!(error.contains("authoritative selection refused")); break; }
            Answer::Done(Ok(_)) => panic!("queue submission must not acknowledge rejected execution"),
        }
    }
    assert_eq!(fake.commands().iter().filter(|c| c["action"] == "select").count(), 1);
    // An ordinary refusal (400) is surfaced but leaves the pinned binding intact.
    let after = hw.link.as_ref().unwrap().snapshot();
    assert!(!after.authorization_revoked && after.connection_valid, "a 400 refusal must not revoke");
}

// Written fixtures for binding loss, epochs and postconditions; not executed this turn.
const SERVER_A: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

#[test]
fn a_binding_refusal_revokes_but_an_ordinary_refusal_does_not() {
    let identity = virtual_identity(SERVER_A);
    let status = virtual_document(&identity);
    let refuse_binding = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let refusing = refuse_binding.clone();
    let fake = Fake::start(Arc::new(move |_: &str, body: &Value| -> Result<Value, String> { match body["action"].as_str() {
        Some("select") if refusing.load(SeqCst) => Err(format!("{}: connection generation replaced", calibration::BINDING_REFUSED)),
        Some("select") => Err("Unknown motor ID".into()),
        _ => Ok(status.clone()),
    } }));
    let mut session = pinned_session(&fake, identity);
    // 400: the error is the command's result; automation stays authorized.
    session.handle(LinkCommand::Checked { ticket: 1, epoch: session.epoch_now(), generation: 1, inputs: None, command: Box::new(LinkCommand::Select { id: 1 }) });
    assert_eq!(session.snap.command_results[&1], Err("Unknown motor ID".to_string()));
    assert!(!session.snap.authorization_revoked && session.snap.connection_valid);
    // 409: the binding is gone; revoked until an explicit reconnect.
    refuse_binding.store(true, SeqCst);
    session.handle(LinkCommand::Checked { ticket: 2, epoch: session.epoch_now(), generation: 1, inputs: None, command: Box::new(LinkCommand::Select { id: 1 }) });
    assert!(session.snap.command_results[&2].as_ref().is_err_and(|e| e.starts_with(calibration::BINDING_REFUSED)));
    assert!(session.snap.authorization_revoked && !session.snap.connection_valid);
    // Nothing more reaches the server for automation.
    session.handle(LinkCommand::Checked { ticket: 3, epoch: session.epoch_now(), generation: 1, inputs: None, command: Box::new(LinkCommand::Select { id: 1 }) });
    assert!(session.snap.command_results[&3].is_err());
    assert_eq!(fake.commands().iter().filter(|c| c["action"] == "select").count(), 2);
}

#[test]
fn own_epoch_bumps_do_not_refuse_a_queued_checked_command_but_a_ui_stop_does() {
    let identity = virtual_identity(SERVER_A);
    let status = virtual_document(&identity);
    let fake = Fake::start(Arc::new(move |_: &str, _: &Value| -> Result<Value, String> { Ok(status.clone()) }));
    let mut session = pinned_session(&fake, identity);
    let select = || Box::new(LinkCommand::Select { id: 1 });
    // Both queued before the link thread took either: the same captured epoch.
    let queued = session.epoch_now();
    session.handle(LinkCommand::Checked { ticket: 1, epoch: queued, generation: 1, inputs: None, command: select() });
    assert_eq!(session.snap.command_results[&1], Ok(()));
    assert!(session.epoch_now() > queued, "select bumped the shared epoch itself");
    session.handle(LinkCommand::Checked { ticket: 2, epoch: queued, generation: 1, inputs: None, command: select() });
    assert_eq!(session.snap.command_results[&2], Ok(()), "the session's own bump is not a STOP");
    // A UI STOP pressed after a command was queued refuses it, before and after its `Stopped`.
    let queued = session.epoch_now();
    let stopped = session.epoch.fetch_add(1, SeqCst) + 1;
    session.handle(LinkCommand::Checked { ticket: 3, epoch: queued, generation: 1, inputs: None, command: select() });
    assert_eq!(session.snap.command_results[&3], Err(STOP_PENDING.to_string()));
    session.handle(LinkCommand::Stopped { epoch: stopped });
    session.handle(LinkCommand::Checked { ticket: 4, epoch: queued, generation: 1, inputs: None, command: select() });
    assert_eq!(session.snap.command_results[&4], Err(STOP_PENDING.to_string()));
    // Queued after the STOP: runs.
    session.handle(LinkCommand::Checked { ticket: 5, epoch: session.epoch_now(), generation: 1, inputs: None, command: select() });
    assert_eq!(session.snap.command_results[&5], Ok(()));
    // Authorized for another generation: refused without a request.
    let sent = fake.commands().len();
    session.handle(LinkCommand::Checked { ticket: 6, epoch: session.epoch_now(), generation: 2, inputs: None, command: select() });
    assert!(session.snap.command_results[&6].is_err());
    assert_eq!(fake.commands().len(), sent);
}

#[test]
fn a_select_the_server_does_not_honour_is_an_error_not_an_acknowledgement() {
    let identity = virtual_identity(SERVER_A);
    let mut status = virtual_document(&identity);
    status["enabled_id"] = Value::Null;
    let fake = Fake::start(Arc::new(move |_: &str, _: &Value| -> Result<Value, String> { Ok(status.clone()) }));
    let mut session = pinned_session(&fake, identity);
    session.handle(LinkCommand::Checked { ticket: 1, epoch: session.epoch_now(), generation: 1, inputs: None, command: Box::new(LinkCommand::Select { id: 1 }) });
    let result = session.snap.command_results[&1].clone();
    assert!(result.as_ref().is_err_and(|e| e.contains("did not enable motor 1")), "{result:?}");
    assert!(!session.snap.ready && !session.snap.authorization_revoked);
}

#[test]
fn an_automatic_stop_from_a_link_that_never_drove_sends_nothing_without_a_motor() {
    let fake = Fake::start(motor_server(1));
    let (mut s, _) = session(&fake, Arc::default());
    assert!(s.snap.id.is_none() && !s.snap.drove);
    s.stop();
    assert!(fake.commands().is_empty(), "no id-less STOP from a link that never selected");
    s.handle(LinkCommand::Select { id: 1 });
    assert!(s.snap.drove);
}

#[test]
fn tune_start_failure_and_stop_keep_records_and_clear_terminal_stage_state() {
    let identity = virtual_identity("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    let status = virtual_document(&identity);
    let fake = Fake::start(Arc::new(move |_, body| if body["action"] == "select" {
        Err("select rejected before tuning".into())
    } else { Ok(status.clone()) }));
    let mut session = pinned_session(&fake, identity);
    session.handle(LinkCommand::Checked { ticket: 1, epoch: 0, generation: 1, inputs: None, command:Box::new(LinkCommand::Tune)});
    assert!(session.snap.command_results[&1].is_err());
    assert!(!session.snap.tuning && session.snap.tune_done > 0);
    // The dedicated STOP/loss receipt preserves the durable campaign metadata.
    let identity = session.snap.execution.clone().unwrap();
    session.adopt(virtual_status(&identity));
    session.handle(LinkCommand::Stopped {epoch:session.epoch_now()});
    assert_eq!(session.snap.state.campaign.as_ref().unwrap().completed, 2);
    assert_eq!(session.axis().tuning.unwrap().record, "retained-tune.json");
}

// Remote hold-others and drive mode go through the checked path; written and
// source-inspected, not executed this turn.

/// A bare form-input change takes the link's inputs only once authorized,
/// and keeps them only with an Ok result (`Session::handle`'s restore).
#[test]
fn checked_input_change_is_kept_only_when_it_holds() {
    use crate::robot::hardware::actions::DriveMode;
    let identity = virtual_identity(SERVER_A);
    let fake = Fake::start(Arc::new(|_, _| panic!("a form-input change must not reach HTTP")));
    let mut session = pinned_session(&fake, identity);
    session.snap.read_at = Some(Instant::now());
    let before = session.inputs.clone();
    let changed = Inputs { drive_mode: DriveMode::ServoPosition, hold_others: false, ..before.clone() };
    let checked = |ticket, epoch, generation| LinkCommand::Checked { ticket, epoch, generation, inputs: Some(changed.clone()), command: Box::new(LinkCommand::Inputs(changed.clone())) };
    // Authorized for another generation: refused, inputs untouched.
    session.handle(checked(1, session.epoch_now(), 2));
    assert!(session.snap.command_results[&1].is_err());
    assert_eq!(session.inputs, before);
    // A UI STOP pressed after it was queued: refused, inputs untouched.
    let queued = session.epoch_now();
    let stopped = session.epoch.fetch_add(1, SeqCst) + 1;
    session.handle(checked(2, queued, 1));
    assert_eq!(session.snap.command_results[&2], Err(STOP_PENDING.to_string()));
    assert_eq!(session.inputs, before);
    session.handle(LinkCommand::Stopped { epoch: stopped });
    // Queued after the STOP: Ok, and the link sends with the new values.
    session.snap.read_at = Some(Instant::now());
    session.handle(checked(3, session.epoch_now(), 1));
    assert_eq!(session.snap.command_results[&3], Ok(()));
    assert_eq!(session.inputs, changed);
    assert_eq!(session.input().drive_mode, "servo_position");
    assert!(!session.input().hold_others);
    // A revoked binding refuses the change back to the old values.
    session.snap.authorization_revoked = true;
    session.handle(LinkCommand::Checked { ticket: 4, epoch: session.epoch_now(), generation: 1, inputs: Some(before.clone()), command: Box::new(LinkCommand::Inputs(before.clone())) });
    assert!(session.snap.command_results[&4].is_err());
    assert_eq!(session.inputs, changed);
    assert!(fake.commands().is_empty());
}

/// A `Hardware` with a real link pinned to a virtual execution on `fake`,
/// once its first status is read (as `actual_remote_handler_waits_for_real_link_consumer_refusal`).
fn remote_hardware(fake: &Fake) -> crate::robot::hardware::Hardware {
    use crate::robot::hardware::{Hardware, HardwareConfig, settings, link::Link};
    let mut hw = Hardware::new(HardwareConfig::default(), settings::Settings::default());
    hw.generation = 1;
    hw.link = Some(Link::spawn(fake.client().with_calibration_execution(virtual_identity(SERVER_A), 1), 1));
    let deadline = Instant::now() + Duration::from_secs(5);
    while hw.link.as_ref().unwrap().snapshot().read_at.is_none() {
        assert!(Instant::now() < deadline, "fresh status deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
    hw.snapshot = hw.link.as_ref().unwrap().snapshot();
    hw
}

/// A fake that answers every request with the pinned virtual status.
fn virtual_status_server() -> Fake {
    let status = json!({"connected":true,"execution":virtual_identity(SERVER_A),"calibration":{"axes":{"1":{"role":"Knee"}}}});
    Fake::start(Arc::new(move |_, _| Ok(status.clone())))
}

fn dispatch_as(hw: &mut crate::robot::hardware::Hardware, action: &crate::robot::hardware::actions::HardwareAction,
    origin: crate::app::actions::Origin, continuation: &mut Value, replies: &mut crate::app::actions::Replies) -> crate::robot::hardware::handlers::Answer {
    let mut call = crate::app::actions::Call { origin, continuation, cancelled: false, replies };
    crate::robot::hardware::handlers::dispatch(hw, action, &mut call, Instant::now(), None, &mut |_| {})
}

/// Re-dispatches a pending remote call until its ticket resolves.
fn settle_remote(hw: &mut crate::robot::hardware::Hardware, action: &crate::robot::hardware::actions::HardwareAction,
    origin: crate::app::actions::Origin, continuation: &mut Value, replies: &mut crate::app::actions::Replies) -> Result<Option<Value>, String> {
    use crate::robot::hardware::handlers::Answer;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match dispatch_as(hw, action, origin, continuation, replies) {
            Answer::Pending => {
                assert!(Instant::now() < deadline, "authoritative acknowledgement deadline");
                std::thread::sleep(Duration::from_millis(1));
            }
            Answer::Done(result) => return result,
        }
    }
}

/// (a) Refused by the link (a UI STOP pending when it is taken): the form
/// and the remembered preference stay as they were.
#[test]
fn remote_hold_others_and_drive_mode_refused_by_the_link_leave_the_form() {
    use crate::app::actions::{Origin, Replies};
    use crate::robot::hardware::{actions::{DriveMode, HardwareAction}, handlers::Answer};
    let fake = virtual_status_server();
    let mut hw = remote_hardware(&fake);
    // STOP pressed, its `Stopped` not sent: the UI's authorization does not
    // look at the epoch, the link thread refuses whatever is queued under it.
    hw.link.as_ref().unwrap().epoch.fetch_add(1, SeqCst);
    let form = hw.form.inputs.clone();
    let mut replies = Replies::default();
    for action in [HardwareAction::HoldOthers { on: !form.hold_others }, HardwareAction::DriveMode { mode: DriveMode::ServoPosition }] {
        let origin = Origin::Rest(replies.open());
        let mut continuation = Value::Null;
        assert!(matches!(dispatch_as(&mut hw, &action, origin, &mut continuation, &mut replies), Answer::Pending));
        assert_eq!(hw.form.inputs, form, "queued, not yet the form's");
        let result = settle_remote(&mut hw, &action, origin, &mut continuation, &mut replies);
        assert_eq!(result, Err(STOP_PENDING.to_string()), "{action:?}");
        assert_eq!(hw.form.inputs, form, "{action:?}");
        assert_eq!((hw.settings.calibration.hold_others, hw.settings.calibration.drive_mode), (None, None));
    }
    assert!(fake.commands().is_empty(), "form inputs send no command");
}

/// (b) Ok: the form and the preference take the value at resolution.
#[test]
fn remote_hold_others_and_drive_mode_accepted_by_the_link_are_adopted() {
    use crate::app::actions::{Origin, Replies};
    use crate::robot::hardware::{actions::{DriveMode, HardwareAction}, handlers::Answer};
    let fake = virtual_status_server();
    let mut hw = remote_hardware(&fake);
    let mut replies = Replies::default();
    let hold = HardwareAction::HoldOthers { on: false };
    assert!(hw.form.inputs.hold_others, "the page's default");
    let origin = Origin::Rest(replies.open());
    let mut continuation = Value::Null;
    assert!(matches!(dispatch_as(&mut hw, &hold, origin, &mut continuation, &mut replies), Answer::Pending));
    assert!(hw.form.inputs.hold_others, "unchanged until the link accepts it");
    assert!(settle_remote(&mut hw, &hold, origin, &mut continuation, &mut replies).is_ok());
    assert!(!hw.form.inputs.hold_others);
    assert_eq!(hw.settings.calibration.hold_others, Some(false));
    let drive = HardwareAction::DriveMode { mode: DriveMode::ServoSpeed };
    let origin = Origin::Rest(replies.open());
    let mut continuation = Value::Null;
    assert!(matches!(dispatch_as(&mut hw, &drive, origin, &mut continuation, &mut replies), Answer::Pending));
    assert_eq!(hw.form.inputs.drive_mode, DriveMode::Pwm);
    assert!(settle_remote(&mut hw, &drive, origin, &mut continuation, &mut replies).is_ok());
    assert_eq!(hw.form.inputs.drive_mode, DriveMode::ServoSpeed);
    assert_eq!(hw.settings.calibration.drive_mode, Some(DriveMode::ServoSpeed));
    assert!(!hw.form.inputs.hold_others, "the earlier adoption is kept");
}

/// (c) An operator edit made while the remote change was pending wins.
#[test]
fn an_operator_drive_mode_edit_while_pending_is_not_overwritten() {
    use crate::app::actions::{Origin, Replies};
    use crate::robot::hardware::{actions::{DriveMode, HardwareAction}, handlers::Answer};
    let fake = virtual_status_server();
    let mut hw = remote_hardware(&fake);
    let mut replies = Replies::default();
    let remote = HardwareAction::DriveMode { mode: DriveMode::ServoPosition };
    let origin = Origin::Rest(replies.open());
    let mut continuation = Value::Null;
    assert!(matches!(dispatch_as(&mut hw, &remote, origin, &mut continuation, &mut replies), Answer::Pending));
    // The operator's click: the local path changes the form at once.
    let mut local = Value::Null;
    let operator = HardwareAction::DriveMode { mode: DriveMode::ServoSpeed };
    assert!(matches!(dispatch_as(&mut hw, &operator, Origin::Ui, &mut local, &mut replies), Answer::Done(Ok(_))));
    assert_eq!(hw.form.inputs.drive_mode, DriveMode::ServoSpeed);
    // The remote change still succeeds on the link, but the form keeps the newer edit.
    assert!(settle_remote(&mut hw, &remote, origin, &mut continuation, &mut replies).is_ok());
    assert_eq!(hw.form.inputs.drive_mode, DriveMode::ServoSpeed);
    assert_eq!(hw.settings.calibration.drive_mode, Some(DriveMode::ServoSpeed));
}

// Virtual scope, jog toggle and refused-press fixtures.

/// A virtual link lists the commands its bench does not simulate (flip, raw
/// step) disabled with the scope reason; a physical or unknown one keeps
/// their ordinary reasons. Gait playback on the leg (Leg only, Both, Play)
/// is in scope on a virtual bench (HW-10/HW-11).
#[test]
fn a_virtual_link_lists_flip_and_raw_step_out_of_scope_and_leg_gaits_in_scope() {
    use crate::robot::hardware::{Hardware, HardwareConfig, actions::GaitMode, panel::{OUT_OF_VIRTUAL_SCOPE, controls}, settings};
    let mut hw = Hardware::new(HardwareConfig::default(), settings::Settings::default());
    hw.form.gait_mode = GaitMode::Leg;
    let reason = |hw: &Hardware, id: &str| controls(hw).into_iter().find(|(i, ..)| i == id).unwrap_or_else(|| panic!("{id} is not listed")).3;
    let out_of_scope = ["hardware:flip", "hardware:raw_step"];
    let gait = ["hardware:gait_mode_sim", "hardware:gait_mode_leg", "hardware:gait_mode_both", "hardware:gait_play"];
    for id in out_of_scope.into_iter().chain(gait) {
        assert_ne!(reason(&hw, id), Err(OUT_OF_VIRTUAL_SCOPE.to_string()), "{id} without a virtual execution");
    }
    hw.snapshot.execution = Some(virtual_identity(SERVER_A));
    for id in out_of_scope {
        assert_eq!(reason(&hw, id), Err(OUT_OF_VIRTUAL_SCOPE.to_string()), "{id}");
    }
    for id in gait {
        assert_ne!(reason(&hw, id), Err(OUT_OF_VIRTUAL_SCOPE.to_string()), "{id} is in scope");
    }
    // The radios follow only "not while a gait plays".
    assert_eq!(reason(&hw, "hardware:gait_mode_leg"), Ok(()));
    assert_eq!(reason(&hw, "hardware:gait_mode_both"), Ok(()));
    assert_eq!(reason(&hw, "hardware:raw_step_plus_1"), Ok(()));
}

/// The server answers an out-of-scope command on a virtual bench with an
/// ordinary 400: its text is the result and the pinned binding stays
/// authorized (only 409, transport or decode revoke).
#[test]
fn an_out_of_scope_400_does_not_revoke_the_virtual_binding() {
    let identity = virtual_identity(SERVER_A);
    let status = virtual_document(&identity);
    let fake = Fake::start(Arc::new(move |_: &str, body: &Value| -> Result<Value, String> {
        if body["action"] == "flip" { Err("Out of virtual calibration scope: flip is refused on a virtual bench".into()) } else { Ok(status.clone()) }
    }));
    let mut session = pinned_session(&fake, identity);
    session.flip();
    assert!(actions(&fake.commands()).iter().any(|a| a == "flip"), "{:?}", fake.commands());
    assert!(!session.snap.authorization_revoked && session.snap.connection_valid, "a 400 refusal must not revoke");
    assert!(session.snap.state.message.as_deref().is_some_and(|m| m.contains("Out of virtual calibration scope: flip is refused on a virtual bench")), "{:?}", session.snap.state.message);
}

/// A fake virtual bench: motor 1 enabled; a jog start answers a session.
fn jog_server() -> Fake {
    let status = virtual_document(&virtual_identity(SERVER_A));
    Fake::start(Arc::new(move |_: &str, body: &Value| -> Result<Value, String> {
        let mut answer = status.clone();
        if body["action"] == "motion_start" {
            answer["sweep"] = json!({ "running": true, "run_id": 7, "motor_id": 1 });
        }
        Ok(answer)
    }))
}

/// A remote `hardware` link with motor 1 selected and ready (through the
/// checked path, as automation selects).
fn ready_remote_hardware(fake: &Fake) -> crate::robot::hardware::Hardware {
    use crate::app::actions::{Origin, Replies};
    use crate::robot::hardware::actions::HardwareAction;
    let mut hw = remote_hardware(fake);
    let mut replies = Replies::default();
    let select = HardwareAction::Select { id: 1 };
    let origin = Origin::Rest(replies.open());
    let mut continuation = Value::Null;
    assert!(settle_remote(&mut hw, &select, origin, &mut continuation, &mut replies).is_ok());
    assert!(hw.snapshot.ready && !hw.snapshot.busy, "{:?}", hw.snapshot.state.message);
    hw
}

/// The listed `hardware:jog_upper` control: (label, action, ready).
fn jog_upper(hw: &crate::robot::hardware::Hardware) -> (String, crate::robot::hardware::actions::HardwareAction, Result<(), String>) {
    let (_, label, action, ready) = crate::robot::hardware::panel::controls(hw).into_iter().find(|(id, ..)| id == "hardware:jog_upper").expect("jog_upper is listed");
    (label, action, ready)
}

/// Activating `hardware:jog_upper` twice is a press, then its release: the
/// listed action follows the form's held flag, as `hold_others` follows its value.
#[test]
fn activating_jog_upper_twice_presses_then_releases() {
    use crate::app::actions::{Origin, Replies};
    use crate::robot::hardware::actions::{Direction, HardwareAction};
    let fake = jog_server();
    let mut hw = ready_remote_hardware(&fake);
    let mut replies = Replies::default();
    let (label, press, ready) = jog_upper(&hw);
    assert_eq!(label, "Q  Upper ↑");
    assert_eq!(press, HardwareAction::JogPress { direction: Direction::Upper });
    assert_eq!(ready, Ok::<(), String>(()));
    let origin = Origin::Rest(replies.open());
    let mut continuation = Value::Null;
    assert!(settle_remote(&mut hw, &press, origin, &mut continuation, &mut replies).is_ok());
    assert!(hw.form.held_upper && hw.pending_presses.is_empty());
    assert!(actions(&fake.commands()).iter().any(|a| a == "motion_start"));
    let (label, release, ready) = jog_upper(&hw);
    assert_eq!(label, "Release ↑ (hold)");
    assert_eq!(release, HardwareAction::JogRelease { direction: Direction::Upper });
    assert_eq!(ready, Ok::<(), String>(()), "a held direction's release stays enabled");
    // Listed as automation sees it (`robot::actions` Controls: `{"hardware": action}`).
    assert_eq!(json!({ "hardware": release }), json!({ "hardware": { "jog_release": { "direction": "upper" } } }));
    assert_eq!(release.authorize(&hw, Instant::now()), Ok(()));
    let origin = Origin::Rest(replies.open());
    let mut continuation = Value::Null;
    assert!(settle_remote(&mut hw, &release, origin, &mut continuation, &mut replies).is_ok());
    assert!(!hw.form.held_upper);
    assert_eq!(hw.link.as_ref().unwrap().snapshot().intent, Intent::Hold);
    assert_eq!(jog_upper(&hw).1, HardwareAction::JogPress { direction: Direction::Upper });
}

/// A remote press the link refuses (a STOP pending when it is taken) holds
/// nothing: the held flag set when it was queued is cleared as it resolves,
/// and the control offers the press again. A one-way press (nobody waits on
/// it) is settled from the snapshot (`actions::poll_jobs`); a newer operator
/// press of the same direction keeps the flag.
#[test]
fn a_refused_remote_jog_press_clears_its_held_flag() {
    use crate::app::actions::{Origin, Replies};
    use crate::robot::hardware::{actions::{Direction, HardwareAction}, handlers::{Answer, settle_presses}};
    let fake = jog_server();
    let mut hw = ready_remote_hardware(&fake);
    // STOP pressed, its `Stopped` not sent: the link refuses what is queued under it.
    hw.link.as_ref().unwrap().epoch.fetch_add(1, SeqCst);
    let press = HardwareAction::JogPress { direction: Direction::Upper };
    let mut replies = Replies::default();
    let origin = Origin::Rest(replies.open());
    let mut continuation = Value::Null;
    assert!(matches!(dispatch_as(&mut hw, &press, origin, &mut continuation, &mut replies), Answer::Pending));
    assert!(hw.form.held_upper, "held while queued");
    assert_eq!(settle_remote(&mut hw, &press, origin, &mut continuation, &mut replies), Err(STOP_PENDING.to_string()));
    assert!(!hw.form.held_upper && hw.pending_presses.is_empty());
    assert_eq!(jog_upper(&hw).1, press);
    // One-way: answered at once, settled when the link records its verdict
    // (as `actions::poll_jobs` does each frame).
    let deadline = Instant::now() + Duration::from_secs(5);
    let one_way = |hw: &mut crate::robot::hardware::Hardware, replies: &mut Replies| -> u64 {
        let mut continuation = Value::Null;
        assert!(matches!(dispatch_as(hw, &press, Origin::SystemUi, &mut continuation, replies), Answer::Done(Ok(_))));
        assert!(hw.form.held_upper && hw.pending_presses.len() == 1, "held while queued");
        let ticket = hw.pending_presses[0].ticket;
        while !hw.link.as_ref().unwrap().snapshot().command_results.contains_key(&ticket) {
            assert!(Instant::now() < deadline, "one-way press verdict deadline");
            std::thread::sleep(Duration::from_millis(1));
        }
        ticket
    };
    let ticket = one_way(&mut hw, &mut replies);
    hw.snapshot = hw.link.as_ref().unwrap().snapshot();
    let results = hw.snapshot.command_results.clone();
    assert_eq!(results[&ticket], Err(STOP_PENDING.to_string()));
    settle_presses(&mut hw, &results);
    assert!(!hw.form.held_upper && hw.pending_presses.is_empty());
    // The operator presses the same direction before the frame settles a
    // refused one: the flag is the operator's now and stays. (Last: the
    // operator's press reaches the link unchecked and changes its state.)
    let ticket = one_way(&mut hw, &mut replies);
    let mut local = Value::Null;
    assert!(matches!(dispatch_as(&mut hw, &press, Origin::Ui, &mut local, &mut replies), Answer::Done(Ok(_))));
    let results = hw.link.as_ref().unwrap().snapshot().command_results;
    assert!(results[&ticket].is_err());
    settle_presses(&mut hw, &results);
    assert!(hw.form.held_upper && hw.pending_presses.is_empty(), "the operator's newer press keeps its flag");
}

// Held-direction refusal, one-way notice, lost bench and pin-less export
// labelling fixtures.

/// A remote press of a direction the operator already holds, refused by the
/// link, leaves the operator's hold in place (the flag goes back to what it
/// was before the press, not to false): the control still lists the
/// release, so the operator's release still reaches the link. A REST
/// refusal is answered to its caller, not put in the panel's notice.
#[test]
fn a_refused_remote_press_of_a_held_direction_keeps_the_operators_hold() {
    use crate::app::actions::{Origin, Replies};
    use crate::robot::hardware::{actions::{Direction, HardwareAction}, handlers::Answer};
    let fake = jog_server();
    let mut hw = ready_remote_hardware(&fake);
    let press = HardwareAction::JogPress { direction: Direction::Upper };
    let mut replies = Replies::default();
    // The operator holds Q.
    let mut local = Value::Null;
    assert!(matches!(dispatch_as(&mut hw, &press, Origin::Ui, &mut local, &mut replies), Answer::Done(Ok(_))));
    assert!(hw.form.held_upper && hw.jog_presses[0] == 1);
    // STOP pressed, its `Stopped` not sent: the link refuses the remote press.
    hw.link.as_ref().unwrap().epoch.fetch_add(1, SeqCst);
    let origin = Origin::Rest(replies.open());
    let mut continuation = Value::Null;
    assert!(matches!(dispatch_as(&mut hw, &press, origin, &mut continuation, &mut replies), Answer::Pending));
    assert_eq!(hw.pending_presses.len(), 1);
    assert!(hw.pending_presses[0].before && !hw.pending_presses[0].one_way);
    assert_eq!(settle_remote(&mut hw, &press, origin, &mut continuation, &mut replies), Err(STOP_PENDING.to_string()));
    assert!(hw.form.held_upper && hw.pending_presses.is_empty(), "the operator's hold survives the refused remote press");
    assert_eq!(jog_upper(&hw).1, HardwareAction::JogRelease { direction: Direction::Upper }, "the release is still offered");
    assert_eq!(hw.notice, None, "a REST refusal goes to its caller");
}

/// A one-way (`Origin::SystemUi`) press is answered at once; when the link
/// later refuses it, the refusal is shown in the panel's notice, and a
/// press that never reached the link is not counted.
#[test]
fn a_refused_one_way_press_is_shown_in_the_notice() {
    use crate::app::actions::{Origin, Replies};
    use crate::robot::hardware::{actions::{Direction, HardwareAction}, handlers::{Answer, settle_presses}};
    let fake = jog_server();
    let mut hw = ready_remote_hardware(&fake);
    hw.link.as_ref().unwrap().epoch.fetch_add(1, SeqCst);
    let press = HardwareAction::JogPress { direction: Direction::Upper };
    let mut replies = Replies::default();
    let mut continuation = Value::Null;
    assert!(matches!(dispatch_as(&mut hw, &press, Origin::SystemUi, &mut continuation, &mut replies), Answer::Done(Ok(_))));
    assert!(hw.form.held_upper && hw.pending_presses.len() == 1 && hw.pending_presses[0].one_way);
    assert_eq!(hw.jog_presses[0], 1);
    let ticket = hw.pending_presses[0].ticket;
    let deadline = Instant::now() + Duration::from_secs(5);
    while !hw.link.as_ref().unwrap().snapshot().command_results.contains_key(&ticket) {
        assert!(Instant::now() < deadline, "one-way press verdict deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
    let results = hw.link.as_ref().unwrap().snapshot().command_results;
    settle_presses(&mut hw, &results);
    assert!(!hw.form.held_upper && hw.pending_presses.is_empty());
    assert_eq!(hw.notice, Some(format!("jog upper press refused: {STOP_PENDING}")));
}

/// A status without an execution (the server lost its virtual bench)
/// revokes as an identity change does, and says the bench was lost.
#[test]
fn a_lost_virtual_bench_revokes_and_says_so() {
    let identity = virtual_identity(SERVER_A);
    let response = virtual_document(&identity);
    let fake = Fake::start(Arc::new(move |_, _| Ok(response.clone())));
    let mut session = pinned_session(&fake, identity.clone());
    let mut lost = virtual_document(&identity);
    lost.as_object_mut().unwrap().remove("execution");
    session.adopt(serde_json::from_value(lost).unwrap());
    assert!(session.snap.authorization_revoked && !session.snap.connection_valid && !session.snap.ready);
    let message = session.snap.state.message.clone().unwrap_or_default();
    assert!(message.contains("virtual calibration bench was lost") && message.contains("reconnect required"), "{message}");
    // Marked disconnected with the same reason (the panel's DISCONNECTED line, `link_state`).
    assert!(session.snap.disconnected.as_deref().is_some_and(|why| why.contains("virtual calibration bench was lost")), "{:?}", session.snap.disconnected);
    assert!(matches!(session.snap.health(Instant::now()), crate::robot::hardware::link::LinkHealth::Disconnected { .. }));
    assert_eq!(session.axis().tuning.unwrap().record, "retained-tune.json", "accepted records stay");
}

/// A download over a link with no virtual pin is still labelled when the
/// server labelled it (`"simulated": true`, or a `virtual_calibration`
/// execution): its keys are kept, `"simulated": true` is set and the file
/// name ends `-virtual`. An unlabelled one is written as before.
#[test]
fn a_pinless_download_the_server_labels_simulated_is_written_as_virtual() {
    use crate::robot::hardware::handlers::{Exported, write_export};
    let dir = std::env::temp_dir().join(format!("sim-spatial-hardware-pinless-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let out = dir.to_string_lossy().into_owned();
    let exports = dir.join("viewer-exports");
    let read = |path: &std::path::Path| serde_json::from_str::<Value>(&std::fs::read_to_string(path).unwrap()).unwrap();
    let identity = json!(virtual_identity(SERVER_A));
    let written = write_export(json!({"a": 1, "simulated": true}), Value::Null, Some(&out), 1, None).unwrap();
    assert_eq!(written, Exported { path: exports.join("leg-calibration-1-virtual.json"), simulated: true });
    assert_eq!(read(&written.path), json!({"a": 1, "simulated": true}));
    let written = write_export(json!({"a": 2, "execution": identity}), Value::Null, Some(&out), 2, None).unwrap();
    assert_eq!(written, Exported { path: exports.join("leg-calibration-2-virtual.json"), simulated: true });
    assert_eq!(read(&written.path), json!({"a": 2, "execution": identity, "simulated": true}));
    let written = write_export(json!({"a": 3, "simulated": false}), Value::Null, Some(&out), 3, None).unwrap();
    assert_eq!(written, Exported { path: exports.join("leg-calibration-3.json"), simulated: false });
    let _ = std::fs::remove_dir_all(&dir);
}

// HW-10/HW-11: the disconnected marker, the capture answered after the save
// and remote gait playback. Written and source-inspected, not executed.

fn status_of(v: Value) -> Status {
    serde_json::from_value(v).expect("status")
}

/// `adopt` marks the link disconnected when its bus goes from connected to
/// not (with the server's message), keeps the mark while it stays so, and
/// clears it when the bus reports connected. A server whose bus never
/// connected is not disconnected; a lost request's mark goes with the next status read.
#[test]
fn adopt_marks_a_bus_lost_after_it_connected_and_clears_it_on_reconnect() {
    use crate::robot::hardware::link::LinkHealth;
    let fake = Fake::start(motor_server(1));
    let (mut s, _) = session(&fake, Arc::default());
    // A physical server before its first inspect: never connected, not disconnected.
    s.adopt(status_of(json!({ "connected": false, "message": "Select a motor to connect" })));
    assert_eq!(s.snap.disconnected, None);
    assert_eq!(s.snap.health(Instant::now()), LinkHealth::Live);
    s.adopt(status_of(json!({ "connected": true, "enabled_id": 1 })));
    assert_eq!(s.snap.disconnected, None);
    // A STOP that lost readback: the server reports its bus disconnected.
    s.adopt(status_of(json!({ "connected": false, "message": "Readback lost from Knee: timed out. Select a motor to reconnect." })));
    let why = s.snap.disconnected.clone().expect("disconnected");
    assert_eq!(why, "the server reports its bus disconnected: Readback lost from Knee: timed out. Select a motor to reconnect.");
    assert!(matches!(s.snap.health(Instant::now()), LinkHealth::Disconnected { .. }));
    assert!(!s.snap.authorization_revoked && s.snap.connection_valid, "a physical bus loss revokes nothing");
    // Still down: still marked, with the first reason.
    s.adopt(status_of(json!({ "connected": false })));
    assert_eq!(s.snap.disconnected.as_deref(), Some(why.as_str()));
    // Reconnected (a motor selected): cleared.
    s.adopt(status_of(json!({ "connected": true, "enabled_id": 1 })));
    assert_eq!(s.snap.disconnected, None);
    assert_eq!(s.snap.health(Instant::now()), LinkHealth::Live);
    // A request that found the connection gone marks it; the next status read clears it.
    s.lose_binding();
    assert_eq!(s.snap.disconnected.as_deref(), Some("the connection or its execution binding was lost or refused"));
    s.adopt(status_of(json!({ "connected": true })));
    assert_eq!(s.snap.disconnected, None);
    // Also when that server's bus never connected (nothing was revoked).
    let (mut never, _) = session(&fake, Arc::default());
    never.lose_binding();
    never.adopt(status_of(json!({ "connected": false })));
    assert_eq!(never.snap.disconnected, None);
}

/// A server that answers `capture_hold` with `answer` (and as `motor_server(1)` otherwise).
fn capture_server(answer: Value) -> Handler {
    let motor = motor_server(1);
    Arc::new(move |path: &str, body: &Value| -> Result<Value, String> {
        if body["action"] == "capture_hold" { Ok(answer.clone()) } else { motor(path, body) }
    })
}

/// A session holding motor 1 in run 7 (selected, jogged, released).
fn holding(fake: &Fake) -> (Session, Arc<Mutex<LinkSnapshot>>) {
    let (mut s, shared) = session(fake, Arc::default());
    s.handle(LinkCommand::Select { id: 1 });
    s.handle(LinkCommand::Press { direction: Direction::Lower });
    s.handle(LinkCommand::Release);
    assert_eq!((s.snap.run, s.snap.intent), (Some(7), Intent::Hold), "{:?}", s.snap.state.message);
    (s, shared)
}

/// `capture_hold` is answered after the save with the full status, which is
/// adopted and published at once (the panel and the mirror show the saved
/// pose); an answer that is not a status (an older server's `{"ok":true}`)
/// is an error, not a save.
#[test]
fn a_held_capture_adopts_the_saved_status_and_an_unconfirmed_answer_is_an_error() {
    use crate::robot::hardware::actions::Boundary;
    let saved = json!({ "connected": true, "enabled_id": 1, "capture_message": "Saved lower pose", "calibration": { "axes": { "1": { "lower": 1234 } } } });
    let fake = Fake::start(capture_server(saved));
    let (mut s, shared) = holding(&fake);
    let revision = shared.lock().unwrap().revision;
    s.command_error = None;
    s.handle(LinkCommand::Capture { boundary: Boundary::Lower, reference_joint_rad: None });
    assert!(actions(&fake.commands()).iter().any(|a| a == "capture_hold"), "{:?}", fake.commands());
    assert_eq!(s.command_error, None);
    assert_eq!(s.axis().lower, Some(1234));
    assert_eq!(s.snap.state.capture_message.as_deref(), Some("Saved lower pose"));
    let published = shared.lock().unwrap().clone();
    assert!(published.revision > revision, "published with the capture");
    assert_eq!(published.state.calibration.as_ref().and_then(|c| c.axes.get(&1)).and_then(|a| a.lower), Some(1234));

    let fake = Fake::start(capture_server(json!({ "ok": true })));
    let (mut s, _) = holding(&fake);
    let before = s.axis();
    s.command_error = None;
    s.handle(LinkCommand::Capture { boundary: Boundary::Upper, reference_joint_rad: None });
    assert_eq!(s.command_error.as_deref(), Some(super::buttons::CAPTURE_UNCONFIRMED));
    assert_eq!(s.snap.state.capture_message.as_deref(), Some(super::buttons::CAPTURE_UNCONFIRMED));
    assert_eq!(s.axis(), before, "nothing adopted from an unconfirmed answer");
}

/// The compiled gait the remote gait tests play (the gait search's comparison trial).
const GAIT_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/full-robot/measured-actuator-integration/gait-search-comparison-2026-09-19/comparison/2301-CmaEs-000/compiled.json");
const GAIT_PATH: &str = "examples/full-robot/measured-actuator-integration/gait-search-comparison-2026-09-19/comparison/2301-CmaEs-000/compiled.json";

/// A virtual bench (SERVER_A) that lists and serves the fixture gait and
/// starts a simulated leg gait, or refuses `gait_start` with a 400.
fn gait_server(refuse_start: bool) -> Fake {
    let status = virtual_document(&virtual_identity(SERVER_A));
    let compiled: Value = serde_json::from_slice(&std::fs::read(GAIT_FIXTURE).expect("gait fixture")).expect("gait fixture JSON");
    Fake::start(Arc::new(move |path: &str, body: &Value| -> Result<Value, String> {
        if path == "/calibration/gaits" {
            return Ok(json!({ "gaits": [{ "path": GAIT_PATH, "study": "comparison", "trial": "2301-CmaEs-000" }] }));
        }
        if path.starts_with("/calibration/gait?") {
            return Ok(compiled.clone());
        }
        match body["action"].as_str() {
            Some("gait_start") if refuse_start => Err("Out of virtual calibration scope: gait_start".into()),
            Some("gait_start") => {
                let mut started = status.clone();
                started["gait"] = json!({ "running": true, "phase": "approach", "t": 0.0, "speed_scale": 1.0, "simulated": true });
                Ok(started)
            }
            _ => Ok(status.clone()),
        }
    }))
}

/// A remote Leg play on a virtual bench (a checked command) is answered Ok
/// only once `gait_start` was answered and the gait runs; a refused start
/// is the answer, with the play's own reason, and revokes nothing.
#[test]
fn a_checked_leg_gait_play_answers_ok_only_after_the_gait_started() {
    use crate::robot::hardware::actions::GaitMode;
    let play = || LinkCommand::GaitPlay {
        entry: calibration::GaitEntry { path: GAIT_PATH.into(), study: "comparison".into(), trial: "2301-CmaEs-000".into(), ..Default::default() },
        mode: GaitMode::Leg,
        bindings: vec![calibration::GaitBinding { id: 1, joint: "+X | Foot servo output".into(), polarity: 1.0, home_rad: 0.0 }],
        skipped: Vec::new(),
    };
    let fake = gait_server(false);
    let mut session = pinned_session(&fake, virtual_identity(SERVER_A));
    session.handle(LinkCommand::Checked { ticket: 1, epoch: session.epoch_now(), generation: 1, inputs: None, command: Box::new(play()) });
    assert_eq!(session.snap.command_results[&1], Ok(()));
    assert!(actions(&fake.commands()).iter().any(|a| a == "gait_start"), "{:?}", fake.commands());
    let run = session.snap.gait.clone().expect("the gait plays");
    assert!(run.leg && run.started && run.mode == GaitMode::Leg);
    assert!(session.snap.state.gait.as_ref().is_some_and(|g| g.running && g.simulated));
    // Play again: a pause, judged by what it did.
    session.handle(LinkCommand::Checked { ticket: 2, epoch: session.epoch_now(), generation: 1, inputs: None, command: Box::new(LinkCommand::GaitToggle) });
    assert_eq!(session.snap.command_results[&2], Ok(()));
    assert!(session.snap.gait.as_ref().is_some_and(|g| !g.playing));

    let fake = gait_server(true);
    let mut session = pinned_session(&fake, virtual_identity(SERVER_A));
    session.handle(LinkCommand::Checked { ticket: 1, epoch: session.epoch_now(), generation: 1, inputs: None, command: Box::new(play()) });
    let result = session.snap.command_results[&1].clone();
    assert!(result.as_ref().is_err_and(|e| e.contains("Out of virtual calibration scope: gait_start")), "{result:?}");
    assert!(session.snap.gait.is_none());
    assert!(!session.snap.authorization_revoked && session.snap.connection_valid, "a 400 refusal must not revoke");
}

/// Through the panel's remote dispatch: the gait form intents are answered at
/// once once authorized, and Play waits for the link's verdict; a Leg play
/// without the mirror's bindings is refused with the play's reason.
#[test]
fn remote_gait_intents_are_authorized_and_play_waits_for_the_link() {
    use crate::app::actions::{Origin, Replies};
    use crate::robot::hardware::{actions::{GaitMode, HardwareAction}, handlers::Answer};
    let fake = gait_server(false);
    let mut hw = remote_hardware(&fake);
    hw.link.as_ref().unwrap().send(LinkCommand::LoadGaits);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !hw.link.as_ref().unwrap().snapshot().gaits_loaded {
        assert!(Instant::now() < deadline, "gait list deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
    hw.snapshot = hw.link.as_ref().unwrap().snapshot();
    let mut replies = Replies::default();
    for action in [HardwareAction::GaitSelect { index: 0 }, HardwareAction::GaitMode { mode: GaitMode::Leg }, HardwareAction::GaitConfirm { on: true }] {
        let origin = Origin::Rest(replies.open());
        let mut continuation = Value::Null;
        assert!(matches!(dispatch_as(&mut hw, &action, origin, &mut continuation, &mut replies), Answer::Done(Ok(_))), "{action:?}");
    }
    assert!(hw.form.gait_ok && hw.form.gait_mode == GaitMode::Leg);
    // No robot model in the mirror: no bindings, so the leg play is refused with why.
    let origin = Origin::Rest(replies.open());
    let mut continuation = Value::Null;
    assert!(matches!(dispatch_as(&mut hw, &HardwareAction::GaitPlay, origin, &mut continuation, &mut replies), Answer::Pending));
    let refused = settle_remote(&mut hw, &HardwareAction::GaitPlay, origin, &mut continuation, &mut replies);
    assert!(refused.as_ref().is_err_and(|e| e.contains("No motor is aligned")), "{refused:?}");
    assert!(!actions(&fake.commands()).iter().any(|a| a == "gait_start"));
    // Sim only: answered once the gait plays.
    let origin = Origin::Rest(replies.open());
    let mut continuation = Value::Null;
    assert!(matches!(dispatch_as(&mut hw, &HardwareAction::GaitMode { mode: GaitMode::Sim }, origin, &mut continuation, &mut replies), Answer::Done(Ok(_))));
    let origin = Origin::Rest(replies.open());
    let mut continuation = Value::Null;
    assert!(settle_remote(&mut hw, &HardwareAction::GaitPlay, origin, &mut continuation, &mut replies).is_ok());
    assert!(hw.snapshot.gait.as_ref().is_some_and(|g| g.mode == GaitMode::Sim && g.playing));
    // The gait's Stop is never refused.
    let mut local = Value::Null;
    assert!(matches!(dispatch_as(&mut hw, &HardwareAction::GaitStop, Origin::SystemUi, &mut local, &mut replies), Answer::Done(Ok(_))));
}

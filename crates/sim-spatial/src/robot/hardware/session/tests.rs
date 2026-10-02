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
    let (status, reply) = match handler(path.as_str(), &body) {
        Ok(v) => (200, v),
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
    run(fake.client(), 1, Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)), rx, shared.clone());
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

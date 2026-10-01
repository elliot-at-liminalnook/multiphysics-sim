//! The session against an in-process fake calibration server (plain HTTP on
//! 127.0.0.1:0, served from a `jobs::Pool::Dedicated` job): no window, no
//! hardware. Each test drives [`Session`]'s handlers directly, except the
//! disconnect test, which runs [`run`] on the test thread.
use super::*;
use crate::jobs::{Job, Pool};
use serde_json::json;
use sim_runtime::hardware_client::Endpoint;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream};

type Handler = Arc<dyn Fn(&str, &Value) -> Result<Value, String> + Send + Sync>;
type Log = Arc<Mutex<Vec<(String, Value)>>>;

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
                    Ok((stream, _)) => answer(stream, &handler, &served),
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
        self.log.lock().unwrap().clone()
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
    log.lock().unwrap().push((path.clone(), body.clone()));
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

fn session(fake: &Fake, epoch: Arc<AtomicU64>) -> (Session, Arc<Mutex<LinkSnapshot>>) {
    let shared = Arc::new(Mutex::new(LinkSnapshot::default()));
    (Session::new(fake.client(), 1, Arc::new(AtomicU64::new(0)), epoch, shared.clone()), shared)
}

#[test]
fn select_press_and_heartbeat_carry_the_run_and_increasing_sequences() {
    let fake = Fake::start(motor_server(1));
    let (mut s, shared) = session(&fake, Arc::default());
    s.handle(LinkCommand::Select { id: 1 });
    assert!(s.snap.ready && !s.snap.busy, "{:?}", s.snap.state.message);
    s.handle(LinkCommand::Press { direction: Direction::Upper });
    assert_eq!((s.snap.run, s.snap.intent), (Some(7), Intent::Upper));
    // The 100 ms heartbeat.
    s.update();
    let commands = fake.commands();
    let actions: Vec<&str> = commands.iter().map(|c| c["action"].as_str().unwrap()).collect();
    assert_eq!(actions, ["select", "motion_start", "motion_update", "motion_update"]);
    let sequences: Vec<u64> = commands.iter().map(|c| c["sequence"].as_u64().unwrap()).collect();
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
    let sent = fake.commands().len();
    // The UI's immediate STOP: epoch bumped, `Stopped` still queued.
    let stopped = epoch.fetch_add(1, SeqCst) + 1;
    assert!(s.interrupted());
    s.update();
    s.handle(LinkCommand::Release);
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

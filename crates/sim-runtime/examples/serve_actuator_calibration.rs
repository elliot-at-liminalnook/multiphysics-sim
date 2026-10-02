//! Loopback calibration adapter: serialized physical I/O, shared encoder policy,
//! persisted operator measurements, and the existing CAD/WASM viewer.
use serde::Deserialize;
use serde_json::{Value, json};
use sim_runtime::hardware_client::calibration::{BINDING_REFUSED, BINDING_REFUSED_STATUS, ExecutionIdentity, virtual_command_allowed};
use sim_runtime::acquisition::{
    calibration::{AxisCalibration, Calibration},
    calibration_serial::CalibrationBus,
    calibration_sweep::{DriveMode, MotionCommand, SweepInput, SweepTuning},
};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
type R<T> = Result<T, String>;
const STOP_INTERRUPTED: &str = "STOP interrupted this command; start again explicitly";
const STOP_LATCHED_MESSAGE: &str = "STOP latched; all configured axes torque off. Records retained.";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    serial: String,
    viewer: PathBuf,
    output: PathBuf,
    fixture: String,
    roles: std::collections::BTreeMap<u8, String>,
    sweep_tuning: SweepTuning,
    /// Characterization campaign plan (PLAN.md); axes come from taught poses.
    #[serde(default)]
    campaign_plan: Option<PathBuf>,
}
fn default_drive() -> u16 {
    25
}
fn nullable_motor_id<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
    Ok(Option::<u8>::deserialize(d)?.unwrap_or(0))
}
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Request {
    action: String,
    #[serde(default, deserialize_with = "nullable_motor_id")]
    id: u8,
    #[serde(default)]
    delta: i16,
    #[serde(default = "default_drive")]
    drive_pwm: u16,
    #[serde(default)]
    boundary: String,
    #[serde(default)]
    reverse: bool,
    #[serde(default)]
    supported: bool,
    #[serde(default)]
    sequence: u64,
    #[serde(default)]
    speed_counts_s: f64,
    #[serde(default)]
    clearance_counts: u16,
    #[serde(default)]
    run_id: u64,
    #[serde(default)]
    motion: String,
    #[serde(default)]
    target_raw: f64,
    #[serde(default)]
    disabled: bool,
    /// Energize the other enabled motors to hold position during a session.
    #[serde(default)]
    hold_others: bool,
    /// Servo drive for a session: pwm (host loop), servo_position or servo_speed.
    #[serde(default)]
    drive_mode: DriveMode,
    /// Campaign: reuse completed stages of the last interrupted campaign.
    #[serde(default)]
    resume: bool,
    /// Gait playback: repository path of a compiled gait (compiled.json).
    #[serde(default)]
    gait: String,
    /// Gait playback: display bindings of motors to CAD joints (the leg
    /// mirror's), with the CAD home angle from the same Rust mirror.
    #[serde(default)]
    bindings: Vec<GaitBinding>,
    /// Gait playback speed, a fraction of the gait's own timing.
    #[serde(default)]
    speed_scale: f64,
    #[serde(default)]
    playing: bool,
    /// Gait playback on the leg: fraction of each motor's measured capability.
    #[serde(default)]
    effort: f64,
    /// Saving `reference`: the CAD joint angle (rad) the leg mirror shows as the
    /// alignment pose. Omitted means the joint's CAD home.
    #[serde(default)]
    reference_joint_rad: Option<f64>,
    /// Lesson lab step: the joint role, open-loop duty and duration.
    #[serde(default)]
    role: String,
    #[serde(default)]
    duty: f64,
    #[serde(default)]
    seconds: f64,
}
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct GaitBinding {
    id: u8,
    joint: String,
    polarity: f64,
    home_rad: f64,
}
/// Browser-held playback controls for a hardware gait session (lease).
struct GaitLease {
    owner: String,
    speed_scale: f64,
    playing: bool,
    last_seen: Instant,
}
/// How long the HTTP handler waits for the worker's reply.
const HTTP_WAIT: Duration = Duration::from_secs(8);
struct Job {
    /// When the handler queued it; older than [`HTTP_WAIT`] at pickup means the
    /// client already gave up, so it is refused unrun.
    queued: Instant,
    execution: Option<(ExecutionIdentity, u64)>,
    stop_epoch: u64,
    request: Request,
    client: String,
    reply: mpsc::Sender<R<Value>>,
}
struct BrowserSweep {
    owner: String,
    id: u8,
    run_id: u64,
    sequence: u64,
    input: SweepInput,
    last_seen: Instant,
    motion: MotionCommand,
    teaching: bool,
    /// Pose to save once settled, with the alignment joint angle for `reference`.
    capture: Option<(String, Option<f64>)>,
}
impl BrowserSweep {
    fn update(&mut self, client: &str, r: &Request, tuning: &SweepTuning) -> R<()> {
        if client != self.owner
            || r.id != self.id
            || r.run_id != self.run_id
            || r.sequence <= self.sequence
        {
            return Err("Sweep update rejected: wrong owner, run, axis or stale sequence".into());
        }
        if self.last_seen.elapsed() > Duration::from_millis(1500) {
            return Err("Sweep browser lease expired; start again explicitly".into());
        }
        let input = SweepInput {
            speed_counts_s: r.speed_counts_s,
            pwm_limit: r.drive_pwm,
        };
        input.validate(tuning)?;
        if self.teaching {
            self.motion = motion_request(r)?;
        }
        self.input = input;
        self.sequence = r.sequence;
        self.last_seen = Instant::now();
        Ok(())
    }
}
/// Poses taught in another multi-turn tracking session are not meaningful now;
/// motion continues as if they were untaught instead of being refused.
fn usable(a: &AxisCalibration, session: Option<&str>) -> AxisCalibration {
    let mut a = a.clone();
    if a.coordinate_session.is_some() && a.coordinate_session.as_deref() != session {
        a.lower = None;
        a.upper = None;
    }
    a
}
/// Whether an axis has poses or an alignment tied to the multi-turn session.
fn multi_turn(a: &AxisCalibration) -> bool {
    a.coordinate_session.is_some() || a.reference_session.is_some()
}
/// Whether the axis's taught poses or alignment are still usable in the live
/// multi-turn session `session` (a re-stamp is what invalidates them).
fn bound_to_session(a: &AxisCalibration, session: Option<&str>) -> bool {
    session.is_some() && (a.coordinate_session.as_deref() == session || a.reference_session.as_deref() == session)
}
/// Only lost or untrustworthy readback can hide encoder turns; motion faults
/// with good readback keep them. Lost: no reply, a receive/serial fault, a
/// corrupt or foreign frame (servo_bus `reply`, bridge stream decoding), an
/// ambiguous half-turn jump, or the serial device itself gone ([`device_lost`]).
fn readback_lost(e: &str) -> bool {
    link_readback_lost(e) || device_lost(e)
}
fn link_readback_lost(e: &str) -> bool {
    let e = e.to_ascii_lowercase();
    ["timeout", "readback", "receive", "serial", "bad checksum", "framing", "foreign reply", "bridge stream",
        "payload width", "ambiguous"].iter().any(|k| e.contains(k))
}
/// OS errors of a vanished serial adapter: ENXIO after a USB unplug ("Device
/// not configured" on macOS, "No such device or address" on Linux) and EIO.
fn device_lost(e: &str) -> bool {
    let e = e.to_ascii_lowercase();
    ["device not configured", "no such device or address", "input/output error"].iter().any(|k| e.contains(k))
}
/// `stop()`'s bounded-retry wrapper always says "readback"; the attempts'
/// own errors after it say whether readback was actually lost.
fn strip_stop_retry(e: &str) -> &str {
    e.strip_prefix("Stop readback unverified after bounded retries: ").unwrap_or(e)
}
/// Transport-class failure of a bus call: readback loss, or a socket-only OS
/// error ("Broken pipe (os error 32)", "Connection reset by peer", EOF).
/// Deliberately not every "os error": filesystem failures (disk full in the
/// bus log) are not a lost link. Apply it only to errors a bus call returned
/// (a stop, readback or probe), never to whole-command results that may hold
/// save/receipt failures. `stop()`'s bounded-retry wrapper text is stripped
/// first, since it always says "readback" even when a motor failed to settle.
/// A corrupt frame counts: a virtual bench that produces one is dropped like a
/// silent one. [`device_lost`] does not: a virtual bench is a socket, never a
/// serial adapter, and EIO can come from the bus log's disk.
fn transport_lost(e: &str) -> bool {
    let e = strip_stop_retry(e);
    let lower = e.to_ascii_lowercase();
    link_readback_lost(e) || ["broken pipe", "connection reset", "connection aborted", "connection refused", "not connected",
        "resource temporarily unavailable", "unexpected end of file", "failed to fill whole buffer"].iter().any(|k| lower.contains(k))
}
/// Drops a lost bus: no further I/O on it (closing a virtual socket makes the
/// bench torque off on descriptor loss). Turn counts went with it, so the
/// coordinate session restarts; a virtual execution is revoked, so every later
/// command fails `check_execution` with the 409 binding refusal.
fn lose_bus(app: &App, bus: &mut Option<CalibrationBus>) {
    *bus = None;
    let mut s = app.state.lock().unwrap();
    s["connected"] = json!(false);
    s["enabled_id"] = Value::Null;
    s["busy"] = json!(false);
    s["coordinate_session"] = json!(stamp().to_string());
    if app.execution.is_virtual_calibration() {
        s["execution"] = Value::Null;
    }
}
/// The `/calibration/export` document. A virtual execution's export names it
/// and says simulated, the same keys (and identity JSON) the viewer's
/// `write_export` adds, so the file is labelled even if no client pinned it.
/// A physical export is the calibration unchanged.
fn export_document(app: &App) -> Value {
    let calibration = app.state.lock().unwrap()["calibration"].clone();
    if !app.execution.is_virtual_calibration() {
        return calibration;
    }
    let mut doc = match calibration {
        Value::Object(m) => m,
        _ => serde_json::Map::new(),
    };
    doc.insert("execution".into(), serde_json::to_value(&app.execution).unwrap());
    doc.insert("simulated".into(), json!(true));
    Value::Object(doc)
}
/// Refusal of a command outside the virtual calibration scope: a business
/// refusal (HTTP 400) that does not revoke the execution binding.
fn out_of_scope(action: &str) -> String {
    format!("Out of virtual calibration scope: {action} is refused on a virtual bench")
}
fn motion_request(r: &Request) -> R<MotionCommand> {
    match r.motion.as_str() {
        "upper" => Ok(MotionCommand::Jog(1)),
        "lower" => Ok(MotionCommand::Jog(-1)),
        "hold" => Ok(MotionCommand::Hold),
        "sweep" => Ok(MotionCommand::Sweep),
        "learn" => Ok(MotionCommand::Learn),
        "target" if r.target_raw.is_finite() => Ok(MotionCommand::Target(r.target_raw)),
        _ => Err("Choose upper, lower, hold or target".into()),
    }
}
struct App {
    execution: ExecutionIdentity,
    virtual_socket: Option<PathBuf>,
    generations: Mutex<std::collections::BTreeMap<String, u64>>,
    safety: Mutex<u64>,
    state: Mutex<Value>,
    jobs: mpsc::SyncSender<Job>,
    stop: AtomicBool,
    cancel: AtomicBool,
    cancel_sequence: AtomicU64,
    origin: String,
    token: String,
    viewer: PathBuf,
    sweep: Mutex<Option<BrowserSweep>>,
    gait: Mutex<Option<GaitLease>>,
    sweep_tuning: SweepTuning,
}
impl App {
    fn check_execution(&self, action: &str, client: &str, pin: Option<&(ExecutionIdentity, u64)>) -> R<()> {
        if action == "stop" { return Ok(()); }
        if self.execution.is_virtual_calibration() && self.state.lock().unwrap()["execution"].is_null() {
            return Err(format!("{BINDING_REFUSED}: Virtual bench disconnected; restart and reconnect explicitly"));
        }
        match pin {
            Some((identity, generation)) => {
                if !self.execution.is_virtual_calibration() || identity != &self.execution
                    || *generation == 0
                    || self.generations.lock().unwrap().get(client) != Some(generation) {
                    return Err(format!("{BINDING_REFUSED}: identity/generation mismatch"));
                }
                // The binding holds; refusing this command leaves it intact
                // (an ordinary 400, never the 409 that revokes the client's pin).
                if !virtual_command_allowed(action) {
                    return Err(out_of_scope(action));
                }
            }
            None if self.execution.is_virtual_calibration() => return Err(format!("{BINDING_REFUSED}: Virtual execution identity required")),
            None => (), // Physical operator/browser compatibility; native remote policy refuses automation.
        }
        Ok(())
    }
    /// Clears stop/cancel for a command captured at `epoch`. A STOP (or a
    /// select/clear/generation latch) since then refuses it; a stop flag left
    /// latched by a finished session (gait, sweep, fault) needs a fresh select.
    fn arm(&self, epoch: u64, explicit_rearm: bool) -> R<()> {
        let safety = self.safety.lock().unwrap();
        if *safety != epoch {
            return Err(STOP_INTERRUPTED.into());
        }
        if !explicit_rearm && self.stop.load(Ordering::SeqCst) {
            return Err("Stop is latched; select a motor again".into());
        }
        self.cancel.store(false, Ordering::SeqCst);
        self.stop.store(false, Ordering::SeqCst);
        Ok(())
    }
    fn latch_stop(&self) -> u64 {
        let mut safety = self.safety.lock().unwrap();
        *safety = safety.wrapping_add(1);
        self.stop.store(true, Ordering::SeqCst);
        self.cancel.store(true, Ordering::SeqCst);
        *safety
    }
    fn open_bus(&self, cfg: &Config) -> R<CalibrationBus> {
        match &self.virtual_socket {
            Some(socket) => CalibrationBus::open_virtual(socket, Some(&self.execution.bench_instance), &cfg.output.join("serial.jsonl")).map(|v| v.0),
            None if self.execution.is_virtual_calibration() => Err(format!("{BINDING_REFUSED}: virtual execution has no capability socket; serial is never opened")),
            None => CalibrationBus::open(&cfg.serial, &cfg.output.join("serial.jsonl")),
        }
    }
}
fn stamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}
fn save(path: &PathBuf, c: &Calibration) -> R<()> {
    c.validate()?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(c).unwrap()).map_err(|e| e.to_string())?;
    fs::rename(tmp, path).map_err(|e| e.to_string())
}
/// What a failed STOP torque-off on axis `id` means (see [`observe_stop`]).
#[derive(Debug, PartialEq, Eq)]
struct StopOutcome {
    /// Turns may have been missed while unread: forget them.
    reset_turns: bool,
    /// Torque-off unverified: an error. Otherwise only a note.
    failure: bool,
    /// The bench is marked disconnected.
    disconnect: bool,
    /// Transport-class loss: a virtual bus is dropped (physical keeps it).
    link_lost: bool,
}
fn stop_outcome(id: u8, disabled: bool, error: &str) -> StopOutcome {
    let attempt = strip_stop_retry(error);
    let lost = readback_lost(attempt);
    // The one expected failure: an absent disabled motor never answers its
    // readback. The FPGA (ID 254) must still answer, and a device, socket or
    // corrupt-frame error on a disabled axis is as unverified as on any other.
    // Zero reply bytes: a motor that sent part of a reply is present, and its
    // corrupt answer is an unverified torque-off.
    let absent = disabled && attempt.starts_with(&format!("ID {id}: serial reply timeout (0 reply bytes received)"));
    StopOutcome { reset_turns: lost, failure: !absent, disconnect: lost && !absent, link_lost: transport_lost(error) && !absent }
}
/// Acts on a STOP epoch newer than `handled`: per-axis torque-off with
/// stationary readback for every configured axis on an open bus (regardless of
/// selection), then clears ownership. Returns the epoch now handled.
///
/// Operator-disabled axes still get the torque-off, but in one attempt
/// (`stop_single_attempt`) and with no readback expected: only that motor's
/// reply timeout is a note; any other error (it answered but did not settle,
/// a device error, a lost bus) is an unverified torque-off ([`stop_outcome`]).
/// A lost readback forgets that axis's turn count; if its poses or alignment
/// are still bound to the live multi-turn session, the session is re-stamped
/// so they stop being usable (see [`usable`]). That includes a disabled axis:
/// the shared re-stamp then invalidates every axis's multi-turn poses, which is
/// protective since that axis's turns really were reset. A lost readback other
/// than an absent disabled motor marks the bench disconnected.
#[allow(clippy::too_many_arguments)]
fn observe_stop(app: &App, cfg: &Config, cal: &Calibration, bus: &mut Option<CalibrationBus>, handled: u64, selected: &mut u8, verified: &mut bool, owner: &mut String) -> u64 {
    let epoch = *app.safety.lock().unwrap();
    if epoch == handled {
        return handled;
    }
    let virtual_mode = app.execution.is_virtual_calibration();
    let session = app.state.lock().unwrap()["coordinate_session"].as_str().map(str::to_string);
    let mut failures = Vec::new();
    let mut notes = Vec::new();
    let mut link_lost = false;
    let mut readback_gone = false;
    let mut restamp = false;
    if let Some(b) = bus.as_mut() {
        for id in cfg.roles.keys() {
            let axis = cal.axes.get(id);
            let disabled = axis.is_some_and(|a| a.disabled);
            let result = if disabled { b.stop_single_attempt(*id) } else { b.stop(*id) };
            match result {
                Ok(t) => app.state.lock().unwrap()["samples"][id.to_string()] = json!(t),
                Err(error) => {
                    let outcome = stop_outcome(*id, disabled, &error);
                    // A motor that read back but never settled kept its turns.
                    if outcome.reset_turns {
                        b.reset_turn_tracking_for(*id);
                        restamp |= axis.is_some_and(|a| bound_to_session(a, session.as_deref()));
                    }
                    readback_gone |= outcome.disconnect;
                    if outcome.failure {
                        failures.push(format!("{}: {error}", cfg.roles[id]));
                    } else {
                        notes.push(format!("disabled {} (ID {id}): no readback expected ({error})", cfg.roles[id]));
                    }
                    // A hung or dead virtual link would block each remaining
                    // axis for its full retry budget (past the HTTP wait):
                    // stop here. Physical hardware attempts every axis and
                    // keeps its bus.
                    if outcome.link_lost && virtual_mode {
                        link_lost = true;
                        break;
                    }
                }
            }
        }
    }
    if link_lost {
        lose_bus(app, bus);
    }
    *selected = 0;
    *verified = false;
    owner.clear();
    let mut s = app.state.lock().unwrap();
    if restamp {
        s["coordinate_session"] = json!(stamp().to_string());
    }
    if readback_gone {
        // A virtual execution is revoked only with its bus (lose_bus).
        s["connected"] = json!(false);
    }
    s["enabled_id"] = Value::Null;
    s["busy"] = json!(false);
    let notes = if notes.is_empty() { String::new() } else { format!(" Note: {}.", notes.join("; ")) };
    if failures.is_empty() {
        s["message"] = json!(format!("{STOP_LATCHED_MESSAGE}{notes}"));
    } else {
        s["error"] = json!(failures.join("; "));
        s["message"] = json!(format!("STOP latched; torque-off readback unverified ({}). Cut motor power. Records retained.{notes}", failures.join("; ")));
    }
    epoch
}
fn worker(app: Arc<App>, rx: mpsc::Receiver<Job>, cfg: Config, initial_bus: Option<CalibrationBus>) {
    let path = cfg.output.join("calibration.json");
    let mut cal = if path.exists() {
        match fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|v| serde_json::from_slice::<Calibration>(&v).map_err(|e| e.to_string()))
            .and_then(|c| c.validate().map(|_| c))
        {
            Ok(c) => c,
            Err(e) => {
                app.state.lock().unwrap()["error"] = json!(e);
                return;
            }
        }
    } else {
        let mut c = Calibration::default();
        c.fixture = cfg.fixture.clone();
        c.axes = cfg
            .roles
            .iter()
            .map(|(id, role)| {
                (
                    *id,
                    AxisCalibration {
                        role: role.clone(),
                        ..Default::default()
                    },
                )
            })
            .collect();
        c
    };
    if app.execution.is_virtual_calibration() {
        cal.provenance = format!("SIMULATED virtual calibration; server {}; bench {}. Not measured physical hardware. No CAD/registry promotion.", app.execution.server_instance, app.execution.bench_instance);
    }
    cal.units="Continuous motor encoder counts in the powered tracking session; 4096 counts/revolution; not joint degrees".into();
    let mut bus: Option<CalibrationBus> = initial_bus;
    let mut idle_polling = false;
    let mut owner = String::new();
    let mut selected = 0;
    let mut last_seq = 0;
    let mut verified = false;
    // Motors whose zero-drive watchdogs were proven for the current selection.
    let mut proven = std::collections::BTreeSet::<u8>::new();
    {
        let mut s = app.state.lock().unwrap();
        s["calibration"] = json!(cal);
        s["message"] = json!("Connect and inspect to read motors. No motion on page load.");
    }
    // STOP epochs already acted on. Every newer epoch torques off all axes,
    // including ones held by hold_others or left after inspect/clear.
    let mut handled_stop_epoch = *app.safety.lock().unwrap();
    loop {
        handled_stop_epoch = observe_stop(&app, &cfg, &cal, &mut bus, handled_stop_epoch, &mut selected, &mut verified, &mut owner);
        let job = match rx.recv_timeout(Duration::from_millis(150)) {
            Ok(job) => {
                // A STOP that landed while waiting is acted on before this job.
                handled_stop_epoch = observe_stop(&app, &cfg, &cal, &mut bus, handled_stop_epoch, &mut selected, &mut verified, &mut owner);
                // Refusals here never ran anything, so they skip the error
                // path's stop/disown of a live session. Binding first (409; a
                // generation bump already latched STOP) and virtual scope (400), then expiry, then a
                // job captured before the latest STOP/latch.
                let refusal = app.check_execution(&job.request.action, &job.client, job.execution.as_ref()).err()
                    .or_else(|| (job.queued.elapsed() + Duration::from_secs(1) >= HTTP_WAIT).then(|| "Command expired before execution; nothing ran".to_string()))
                    .or_else(|| (job.stop_epoch != handled_stop_epoch).then(|| STOP_INTERRUPTED.to_string()));
                if let Some(refusal) = refusal {
                    let _ = job.reply.send(Err(refusal));
                    continue;
                }
                job
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let mut idle_lost = false;
                if idle_polling {
                    if let Some(b) = bus.as_mut() {
                        // A disabled motor may be absent: no readback is expected
                        // from it (as in `observe_stop`), so it is not polled.
                        for id in cfg.roles.keys().filter(|id| !cal.axes.get(*id).is_some_and(|a| a.disabled)) {
                            match b.feedback(*id) {
                                Ok(t) => {
                                    app.state.lock().unwrap()["samples"][id.to_string()] = json!(t)
                                }
                                Err(e) => {
                                    idle_polling = false;
                                    verified = false;
                                    owner.clear();
                                    let _ = b.stop(*id);
                                    // Only this axis's turns are uncertain; the others kept reading.
                                    b.reset_turn_tracking_for(*id);
                                    let mut s = app.state.lock().unwrap();
                                    s["enabled_id"] = Value::Null;
                                    s["connected"] = json!(false);
                                    // A virtual execution is revoked only with its bus (lose_bus below).
                                    if multi_turn(&cal.axes[id]) {
                                        s["coordinate_session"] = json!(stamp().to_string());
                                    }
                                    s["message"] = json!(format!(
                                        "Readback lost from {}: {e}. Select a motor to reconnect.",
                                        cfg.roles[id]
                                    ));
                                    idle_lost = app.execution.is_virtual_calibration() && transport_lost(&e);
                                    break;
                                }
                            }
                        }
                    }
                }
                if idle_lost {
                    // The virtual execution is revoked; its link is not reused.
                    lose_bus(&app, &mut bus);
                    app.state.lock().unwrap()["message"] = json!("Virtual bench readback lost; bus closed. Restart and reconnect explicitly.");
                }
                continue;
            }
        };
        let virtual_mode = app.execution.is_virtual_calibration();
        // A session failed inside an Ok reply; a bus probe afterwards decides
        // whether the virtual link is gone (its text may be a save failure).
        let mut probe_link = false;
        let result = (|| -> R<Value> {
            let r = &job.request;
            if r.action == "inspect" {
                if bus.is_none() {
                    bus = Some(app.open_bus(&cfg)?)
                }
                let b = bus.as_mut().unwrap();
                idle_polling = true;
                verified = false;
                owner.clear();
                selected = 0;
                // The STOP readback proof comes from an enabled motor: a
                // disabled one may be absent (ID 3 as before if all are disabled).
                let probe = cfg.roles.keys().copied().find(|id| !cal.axes.get(id).is_some_and(|a| a.disabled)).unwrap_or(3);
                b.reconnect_stopped(probe)?;
                let mut samples = serde_json::Map::new();
                // Disabled motors may be absent; inspect reads the enabled ones.
                for id in cfg.roles.keys().filter(|id| !cal.axes.get(*id).is_some_and(|a| a.disabled)) {
                    samples.insert(id.to_string(), json!(b.feedback(*id)?));
                }
                let mut s = app.state.lock().unwrap();
                s["samples"] = json!(samples);
                s["connected"] = json!(true);
                s["enabled_id"] = Value::Null;
                s["message"] =
                    json!("Readback received. Select an axis and enable supervised teaching.");
                return Ok(s.clone());
            }
            if !cfg.roles.contains_key(&r.id) {
                return Err("Unknown motor ID".into());
            }
            if r.action == "set_disabled" {
                if app.state.lock().unwrap()["enabled_id"] == json!(r.id) {
                    return Err("Stop this motor before disabling it".into());
                }
                let mut next = cal.clone();
                next.axes.get_mut(&r.id).unwrap().disabled = r.disabled;
                save(&cfg.output.join(format!("calibration-{}.json", stamp())), &next)?;
                save(&path, &next)?;
                cal = next;
                let mut s = app.state.lock().unwrap();
                s["calibration"] = json!(cal);
                s["message"] = json!(if r.disabled {
                    "Motor disabled. It cannot be selected or driven until enabled."
                } else {
                    "Motor enabled. Select it to reconnect."
                });
                return Ok(s.clone());
            }
            if cal.axes[&r.id].disabled && !matches!(r.action.as_str(), "stop" | "clear" | "flip" | "direction") {
                return Err(format!("{} is disabled; enable it before moving it", cfg.roles[&r.id]));
            }
            if r.action == "select" {
                if bus.is_none() {
                    bus = Some(app.open_bus(&cfg)?);
                }
                let b = bus.as_mut().unwrap();
                idle_polling = true;
                verified = false;
                owner.clear();
                selected = r.id;
                b.reconnect_stopped(r.id)?;
                proven.clear();
                let t = b.prove_watchdogs(r.id)?;
                proven.insert(r.id);
                let mut unproven = Vec::new();
                if r.hold_others {
                    for other in cfg.roles.keys().copied().filter(|k| *k != r.id && !cal.axes[k].disabled) {
                        match b.prove_watchdogs(other) {
                            Ok(_) => {
                                proven.insert(other);
                            }
                            Err(e) => unproven.push(format!("{}: {e}", cfg.roles[&other])),
                        }
                    }
                }
                owner = job.client.clone();
                idle_polling = true;
                verified = true;
                last_seq = r.sequence;
                app.cancel_sequence.store(r.sequence, Ordering::SeqCst);
                app.arm(job.stop_epoch, true)?;
                let mut s = app.state.lock().unwrap();
                s["connected"] = json!(true);
                s["samples"][r.id.to_string()] = json!(t);
                s["enabled_id"] = json!(r.id);
                s["capture_message"] = Value::Null;
                s["sweep"] = Value::Null;
                s["message"] = json!(if unproven.is_empty() {
                    "Ready. Hold Q toward upper or A toward lower; release to hold position.".to_string()
                } else {
                    format!("Ready. Not held (watchdog check failed): {}", unproven.join("; "))
                });
                return Ok(s.clone());
            }
            let b = bus.as_mut().ok_or("Select a motor first")?;
            if r.action == "enable" {
                if !r.supported {
                    return Err("Confirm the fixture is supported with torque disabled".into());
                }
                // Re-proves the axis this tab just selected; never a rearm
                // after STOP (STOP clears the selection and owner, and leaves
                // the stop flag set until a fresh select).
                if selected != r.id || owner != job.client {
                    return Err("Select this motor in this tab first".into());
                }
                verified = false;
                last_seq = 0;
                app.cancel_sequence.store(0, Ordering::SeqCst);
                app.arm(job.stop_epoch, false)?;
                let t = b.prove_watchdogs(r.id)?;
                app.arm(job.stop_epoch, false)?;
                verified = true;
                let mut s = app.state.lock().unwrap();
                s["samples"][r.id.to_string()] = json!(t);
                s["enabled_id"] = json!(selected);
                s["message"] = json!(
                    "Ready for deliberate jogs. Zero-drive watchdogs verified; mechanical stopping distance remains unqualified."
                );
                return Ok(s.clone());
            }
            if r.action == "clear" {
                // The worker serializes this after any sweep has stopped; immutable
                // snapshots preserve the exact bounds being replaced.
                let t = b.stop(r.id)?;
                let mut next = cal.clone();
                next.axes.get_mut(&r.id).unwrap().clear(&r.boundary)?;
                save(
                    &cfg.output
                        .join(format!("calibration-{}-before-clear.json", stamp())),
                    &cal,
                )?;
                save(
                    &cfg.output.join(format!("calibration-{}.json", stamp())),
                    &next,
                )?;
                save(&path, &next)?;
                cal = next;
                verified = false;
                owner.clear();
                selected = 0;
                app.stop.store(true, Ordering::SeqCst);
                let mut s = app.state.lock().unwrap();
                s["calibration"] = json!(cal);
                s["samples"][r.id.to_string()] = json!(t);
                s["enabled_id"] = Value::Null;
                s["message"] = json!(format!(
                    "{} cleared for this axis; previous values saved in history. Re-enable to teach new poses.",
                    r.boundary
                ));
                return Ok(s.clone());
            }
            if owner != job.client || selected != r.id || !verified {
                return Err("Enable this motor in this tab first".into());
            }
            if r.sequence <= last_seq {
                return Err("Stale or duplicate command rejected".into());
            }
            last_seq = r.sequence;
            let current_session = app.state.lock().unwrap()["coordinate_session"].as_str().map(str::to_string);
            if r.action == "tune" {
                // Operator confirms the axis is mid-travel with room both ways.
                if !r.supported {
                    return Err("Confirm the motor is mid-travel with room to move both ways".into());
                }
                let t = b.stop(r.id)?;
                let position = t.position_continuous.ok_or("Missing encoder coordinate")?;
                let axis = usable(&cal.axes[&r.id], current_session.as_deref());
                let (lo, hi) = axis.encoder_bounds();
                let room = [lo.map(|l| position - l), hi.map(|h| h - position)].into_iter().flatten().min().unwrap_or(i32::MAX);
                let travel = (room - 60).min(200);
                if travel < 40 {
                    return Err(format!("Only {room} counts to the nearest saved pose; move toward the middle first (needs 100)"));
                }
                let duty = (r.drive_pwm as f64 / 1000.).clamp(0.05, 1.);
                // Arm before publishing `running`: a refusal must not leave it set.
                app.arm(job.stop_epoch, false)?;
                {
                    let mut s = app.state.lock().unwrap();
                    s["tuning"] = json!({"execution":app.execution,"simulated":app.execution.is_virtual_calibration(),"running":true,"motor_id":r.id,"stage":"Starting","travel_counts":travel});
                    s["message"] = json!(format!("Tuning {} — short moves within ±{travel} counts. Z stops.", cfg.roles[&r.id]));
                    let _ = job.reply.send(Ok(s.clone()));
                }
                let result = b.identify(r.id, travel, duty, cfg.sweep_tuning.period_s, &app.cancel, |stage| {
                    app.state.lock().unwrap()["tuning"]["stage"] = json!(stage);
                });
                verified = false;
                owner.clear();
                let outcome = result.and_then(|record| {
                    let steps: Vec<sim_runtime::acquisition::motor_identification::StepTrace> =
                        serde_json::from_value(record["steps"].clone()).map_err(|e| e.to_string())?;
                    let fits: Vec<_> = steps.iter().filter_map(sim_runtime::acquisition::motor_identification::fit_step).collect();
                    let breakaway: [f64; 2] = serde_json::from_value(record["breakaway_duty"].clone()).map_err(|e| e.to_string())?;
                    let name = format!("tune-{}-{}.json", r.id, stamp());
                    let tuning = sim_runtime::acquisition::motor_identification::design(
                        &fits, breakaway, record["loop_period_s"].as_f64().unwrap_or(cfg.sweep_tuning.period_s),
                        cfg.sweep_tuning.velocity_filter_s, &name)?;
                    let artifact = json!({"record":record,"fits":fits,"tuning":tuning,"role":cfg.roles[&r.id],
                        "execution":app.execution,"simulated":app.execution.is_virtual_calibration(),"provenance":"Open-loop PWM identification under this fixture's load and supply; commissioning estimate, not a validated joint model"});
                    fs::write(cfg.output.join(&name), serde_json::to_vec_pretty(&artifact).unwrap()).map_err(|e| e.to_string())?;
                    Ok(tuning)
                });
                let mut s = app.state.lock().unwrap();
                s["tuning"]["running"] = json!(false);
                s["enabled_id"] = Value::Null;
                match outcome {
                    Ok(tuning) => {
                        let mut next = cal.clone();
                        next.axes.get_mut(&r.id).unwrap().tuning = Some(tuning.clone());
                        save(&cfg.output.join(format!("calibration-{}.json", stamp())), &next)?;
                        save(&path, &next)?;
                        cal = next;
                        s["calibration"] = json!(cal);
                        s["tuning"]["result"] = json!(tuning);
                        s["message"] = json!(format!(
                            "Tuned {}: {:.0} counts/s per unit duty, lag {:.0} ms, friction {:.0}%. New gains kp {:.2}, ki {:.2}, kd {:.3}. Select it to use them.",
                            cfg.roles[&r.id], tuning.gain_counts_s_per_duty, tuning.time_constant_s * 1000.,
                            tuning.friction_duty * 100., tuning.pid.kp, tuning.pid.ki, tuning.pid.kd));
                    }
                    Err(e) => {
                        probe_link = virtual_mode;
                        s["tuning"]["error"] = json!(e);
                        s["message"] = json!(format!("Tuning stopped: {e}. Torque off; previous gains kept."));
                    }
                }
                return Ok(s.clone());
            }
            if r.action == "gait_start" {
                let reply = |s: &Value| {
                    let _ = job.reply.send(Ok(s.clone()));
                };
                let outcome = run_gait(&app, &cfg, b, &cal, &proven, current_session.as_deref(), &job.client, r, job.stop_epoch, reply);
                *app.gait.lock().unwrap() = None;
                app.stop.store(true, Ordering::SeqCst);
                verified = false;
                owner.clear();
                let mut s = app.state.lock().unwrap();
                s["busy"] = json!(false);
                s["gait"]["running"] = json!(false);
                s["enabled_id"] = Value::Null;
                match outcome {
                    Ok(message) => s["message"] = json!(format!("{message}. Torque off and stationary encoder verified.")),
                    Err(e) => {
                        probe_link = virtual_mode;
                        s["gait"]["error"] = json!(e);
                        s["message"] = json!(format!("Gait stopped: {e}. Torque off."));
                    }
                }
                return Ok(s.clone());
            }
            if r.action == "lab_step" {
                let reply = |s: &Value| {
                    let _ = job.reply.send(Ok(s.clone()));
                };
                let outcome = run_lab_step(&app, &cfg, b, &cal, &proven, current_session.as_deref(), r, job.stop_epoch, reply);
                verified = false;
                owner.clear();
                let mut s = app.state.lock().unwrap();
                s["lab"]["running"] = json!(false);
                s["busy"] = json!(false);
                s["enabled_id"] = Value::Null;
                match outcome {
                    Ok(result) => {
                        s["lab"]["result"] = result.clone();
                        s["message"] = json!(format!("Lab step finished: {}. Torque off.", result["headline"].as_str().unwrap_or("")));
                    }
                    Err(e) => {
                        probe_link = virtual_mode;
                        s["lab"]["error"] = json!(e);
                        s["message"] = json!(format!("Lab step stopped: {e}. Torque off."));
                    }
                }
                return Ok(s.clone());
            }
            if r.action == "campaign" {
                let reply = |s: &Value| {
                    let _ = job.reply.send(Ok(s.clone()));
                };
                let outcome = run_campaign(&app, &cfg, b, &cal, &proven, current_session.as_deref(), r, job.stop_epoch, reply);
                verified = false;
                owner.clear();
                let mut s = app.state.lock().unwrap();
                s["campaign"]["running"] = json!(false);
                s["busy"] = json!(false);
                s["enabled_id"] = Value::Null;
                match outcome {
                    Ok(summary) => {
                        s["campaign"]["result"] = summary.clone();
                        s["message"] = json!(format!("Campaign finished: {}. Results in {}. Nothing was promoted to CAD.", summary["headline"].as_str().unwrap_or(""), summary["directory"].as_str().unwrap_or("")));
                    }
                    Err(e) => {
                        probe_link = virtual_mode;
                        s["campaign"]["error"] = json!(e);
                        s["message"] = json!(format!("Campaign stopped: {e}. Torque off; completed stages are kept as receipts (Resume continues)."));
                    }
                }
                return Ok(s.clone());
            }
            if r.action == "sweep_start" || r.action == "motion_start" || r.action == "sweep_all" {
                let all = r.action == "sweep_all";
                let teaching = r.action == "motion_start" || all;
                if app.stop.load(Ordering::SeqCst) {
                    return Err("Stop is latched; enable this axis first".into());
                }
                let input = SweepInput {
                    speed_counts_s: if teaching || all { r.speed_counts_s } else { 5. },
                    pwm_limit: r.drive_pwm,
                };
                input.validate(&cfg.sweep_tuning)?;
                let axis = usable(&cal.axes[&r.id], current_session.as_deref());
                axis.validate()?;
                if (!teaching || all) && (axis.lower.is_none() || axis.upper.is_none()) {
                    return Err("Teach both poses before starting a sweep".into());
                }
                // Other motors join the session: held in place while jogging,
                // or swept together. Only proven, enabled, current motors join.
                let mut ids = vec![r.id];
                let mut skipped = Vec::new();
                if all || (teaching && r.hold_others) {
                    for k in cfg.roles.keys().copied().filter(|k| *k != r.id) {
                        let a = usable(&cal.axes[&k], current_session.as_deref());
                        let why = if a.disabled {
                            Some("disabled")
                        } else if all && (a.lower.is_none() || a.upper.is_none()) {
                            Some("poses not taught")
                        } else if !proven.contains(&k) {
                            Some("watchdogs not proven; select with hold enabled")
                        } else {
                            None
                        };
                        match why {
                            Some(w) => skipped.push(format!("{} ({w})", cfg.roles[&k])),
                            None => ids.push(k),
                        }
                    }
                }
                let axes: Vec<AxisCalibration> = ids.iter().map(|k| usable(&cal.axes[k], current_session.as_deref())).collect();
                app.arm(job.stop_epoch, false)?;
                if r.sequence <= app.cancel_sequence.load(Ordering::SeqCst)
                    || app.stop.load(Ordering::SeqCst)
                {
                    return Err("Sweep cancelled before starting".into());
                }
                let run_id = stamp() as u64;
                *app.sweep.lock().unwrap() = Some(BrowserSweep {
                    owner: job.client.clone(),
                    id: r.id,
                    run_id,
                    sequence: r.sequence,
                    input,
                    last_seen: Instant::now(),
                    motion: if teaching {
                        motion_request(r)?
                    } else {
                        MotionCommand::Sweep
                    },
                    teaching,
                    capture: None,
                });
                {
                    let mut s = app.state.lock().unwrap();
                    s["busy"] = json!(true);
                    s["sweep"] = json!({"running":true,"run_id":run_id,"motor_id":r.id,"motor_ids":ids,"skipped":skipped,"all":all,"axes":{},"samples":[],"speed_counts_s":input.speed_counts_s,"pwm_limit":r.drive_pwm,"drive_mode":r.drive_mode,"clearance_counts":r.clearance_counts,"teaching":teaching});
                    s["message"] = json!(if all {
                        format!("Sweeping {} together. Keep this tab active; Stop ends the sweep.", ids.iter().map(|k| cfg.roles[k].as_str()).collect::<Vec<_>>().join(", "))
                    } else {
                        "Starting continuous traversal. Keep this tab active; Stop ends the sweep.".to_string()
                    });
                    let _ = job.reply.send(Ok(s.clone()));
                }
                let mut history = std::collections::VecDeque::new();
                // Per axis: half-cycles at first sample and latest; sweep-all holds an axis after two.
                let progress = std::cell::RefCell::new(std::collections::BTreeMap::<u8, (u64, u64)>::new());
                let done = |k: &u8| progress.borrow().get(k).is_some_and(|(s, l)| *l >= *s + 2);
                let result=b.controlled_motion_multi(&ids,&axes,r.clearance_counts,&cfg.sweep_tuning,&app.cancel,teaching,r.drive_mode,|| {
                    if app.cancel.load(Ordering::SeqCst) || app.stop.load(Ordering::SeqCst) {return Ok(None);}
                    let lock=app.sweep.lock().unwrap();let ctl=lock.as_ref().ok_or("Sweep controls closed")?;
                    if ctl.last_seen.elapsed()>Duration::from_millis(1500) {return Err("Browser heartbeat lost; sweep stopped".into());}
                    let a=usable(&serde_json::from_value(app.state.lock().unwrap()["calibration"]["axes"][r.id.to_string()].clone()).map_err(|e|e.to_string())?,current_session.as_deref());
                    if matches!(ctl.motion,MotionCommand::Target(_)|MotionCommand::Sweep|MotionCommand::Learn) && (a.lower.is_none()||a.upper.is_none()){return Err("Teach both poses before learning, sweeping, or using the angle dial".into());}
                    let mut plan=vec![(ctl.input,ctl.motion,a)];
                    if all {
                        if ids.iter().all(|k|done(k)) {return Ok(None);}
                        if done(&r.id) {plan[0].1=MotionCommand::Hold;}
                    }
                    for k in &ids[1..] {
                        let ak=usable(&serde_json::from_value(app.state.lock().unwrap()["calibration"]["axes"][k.to_string()].clone()).map_err(|e|e.to_string())?,current_session.as_deref());
                        plan.push((ctl.input,if all && !done(k) {MotionCommand::Sweep} else {MotionCommand::Hold},ak));
                    }
                    Ok(Some(plan))
                },|id,t,sample| {
                    {let mut s=app.state.lock().unwrap();s["samples"][id.to_string()]=json!(t);s["sweep"]["axes"][id.to_string()]=json!(sample);}
                    {let mut p=progress.borrow_mut();let e=p.entry(id).or_insert((sample.half_cycles as u64,sample.half_cycles as u64));e.1=sample.half_cycles as u64;}
                    if id!=r.id {return Ok(());}
                    history.push_back(json!(sample));if history.len()>300 {history.pop_front();}
                    let capture={let mut lock=app.sweep.lock().unwrap();lock.as_mut().and_then(|ctl|ctl.capture.take())};
                    if let Some((boundary,joint_rad))=capture {
                        let stable=history.len()>=6 && history.iter().rev().take(6).all(|s|s["position_continuous"].as_i64().is_some_and(|p|(p-t.position_continuous.unwrap_or(t.position_raw as i32) as i64).abs()<=2));
                        if sample.holding && sample.velocity_counts_s.abs()<2. && (sample.target_raw-t.position_continuous.unwrap_or(t.position_raw as i32) as f64).abs()<3. && stable {
                            let mut next=cal.clone();let a=next.axes.get_mut(&r.id).unwrap();
                            match boundary.as_str(){"lower"=>a.lower=Some(t.position_continuous.ok_or("Missing continuous encoder coordinate")?),"upper"=>a.upper=Some(t.position_continuous.ok_or("Missing continuous encoder coordinate")?),"reference"=>a.reference=Some(t.position_continuous.ok_or("Missing continuous encoder coordinate")?),_=>return Err("Unknown pose".into())};
                            if a.lower.into_iter().chain(a.upper).any(|p|!(0..=4095).contains(&p)) {a.coordinate_session=Some(app.state.lock().unwrap()["coordinate_session"].as_str().unwrap().to_string());}
                            if boundary=="reference" {a.reference_session=a.reference.filter(|p|!(0..=4095).contains(p)).map(|_|app.state.lock().unwrap()["coordinate_session"].as_str().unwrap().to_string());a.reference_joint_rad=joint_rad;}
                            next.validate()?;
                            if next.axes[&r.id].reversed()!=cal.axes[&r.id].reversed(){return Err("Pose would reverse upper/lower direction; swap direction explicitly first".into());}
                            save(&cfg.output.join(format!("calibration-{}.json",stamp())),&next)?;save(&path,&next)?;cal=next;
                            let mut s=app.state.lock().unwrap();s["calibration"]=json!(cal);s["capture_message"]=json!(format!("Saved {boundary} pose"));
                        }else{app.state.lock().unwrap()["capture_message"]=json!("Still settling. Release Q/A, wait for Holding, then save the pose.");}
                    }
                    let mut s=app.state.lock().unwrap();s["samples"][r.id.to_string()]=json!(t);
                    s["sweep"]["samples"]=json!(history);s["sweep"]["latest"]=json!(sample);
                    if let Some(evidence)=&sample.adaptation.evidence {
                        let artifact=json!({"schema_version":1,"motor_id":r.id,"run_id":run_id,"axis":cal.axes[&r.id],"tuning":cfg.sweep_tuning,"evidence":evidence,"execution":app.execution,"simulated":app.execution.is_virtual_calibration(),"provenance":"Online observations under this session's load, effort ceiling and environment; not a physical safety certification; not auto-loaded next session"});
                        let artifact_path=cfg.output.join(format!("response-{}-{}.json",r.id,run_id));
                        let tmp=artifact_path.with_extension("tmp");fs::write(&tmp,serde_json::to_vec_pretty(&artifact).unwrap()).map_err(|e|e.to_string())?;fs::rename(tmp,artifact_path).map_err(|e|e.to_string())?;
                    }
                    s["message"]=json!(if all {"Sweeping enabled motors together · Z stops drive"} else if teaching {if sample.holding {"Holding position · Q/A to move · Z stops drive"}else{"Moving under feedback control · release to hold"}}else{"Sweeping between taught poses · Q/A takes over · Z stops drive"});
                    Ok(())
                });
                *app.sweep.lock().unwrap() = None;
                app.stop.store(true, Ordering::SeqCst);
                verified = false;
                owner.clear();
                {
                    let mut s = app.state.lock().unwrap();
                    s["busy"] = json!(false);
                    s["sweep"]["running"] = json!(false);
                    s["enabled_id"] = Value::Null;
                }
                let outcome = result?;
                probe_link = virtual_mode && outcome.motion_error.is_some();
                // Every axis kept reading through the session, so turn counts stay valid
                // unless the fault was a lost serial link.
                if outcome.motion_error.as_deref().is_some_and(readback_lost) {
                    for k in &ids {
                        b.reset_turn_tracking_for(*k);
                    }
                    if ids.iter().any(|k| multi_turn(&cal.axes[k])) {
                        app.state.lock().unwrap()["coordinate_session"] = json!(stamp().to_string());
                    }
                }
                let mut s = app.state.lock().unwrap();
                s["samples"][r.id.to_string()] = json!(outcome.telemetry);
                s["sweep"]["motion_error"] = json!(outcome.motion_error);
                s["message"] = json!(format!(
                    "{}. Torque off and stationary encoder verified.",
                    outcome.motion_error.as_deref().unwrap_or("Sweep stopped")
                ));
                return Ok(s.clone());
            }
            if r.action == "halt" {
                let t = b.stop(r.id)?;
                let mut s = app.state.lock().unwrap();
                s["samples"][r.id.to_string()] = json!(t);
                s["message"] = json!("Released. Torque off and stationary encoder verified.");
                return Ok(s.clone());
            }
            if r.action == "flip" {
                let mut next = cal.clone();
                let a = next.axes.get_mut(&r.id).unwrap();
                let reverse = !a.reversed();
                std::mem::swap(&mut a.lower, &mut a.upper);
                a.reverse = reverse;
                save(
                    &cfg.output.join(format!("calibration-{}.json", stamp())),
                    &next,
                )?;
                save(&path, &next)?;
                cal = next;
                let mut s = app.state.lock().unwrap();
                s["calibration"] = json!(cal);
                s["message"] =
                    json!("Upper/lower direction swapped. Physical encoder envelope preserved.");
                return Ok(s.clone());
            }
            if r.action == "direction" {
                if cal.axes[&r.id].lower.is_some() || cal.axes[&r.id].upper.is_some() {
                    return Err("Direction is fixed once a named limit is taught".into());
                }
                let mut next = cal.clone();
                next.axes.get_mut(&r.id).unwrap().reverse = r.reverse;
                save(
                    &cfg.output.join(format!("calibration-{}.json", stamp())),
                    &next,
                )?;
                save(&path, &next)?;
                cal = next;
                let mut s = app.state.lock().unwrap();
                s["calibration"] = json!(cal);
                s["message"] =
                    json!("Part direction saved. Upper/lower refer to the part's poses.");
                return Ok(s.clone());
            }
            if r.action == "jog" {
                app.state.lock().unwrap()["sweep"] = Value::Null;
                if app.stop.load(Ordering::SeqCst) {
                    return Err("Stop is latched; enable teaching again".into());
                }
                app.arm(job.stop_epoch, false)?;
                if r.sequence <= app.cancel_sequence.load(Ordering::SeqCst)
                    || app.stop.load(Ordering::SeqCst)
                {
                    return Ok(app.state.lock().unwrap().clone());
                }
                app.state.lock().unwrap()["busy"] = json!(true);
                let result = b.jog(r.id, &cal.axes[&r.id], r.delta, r.drive_pwm, &app.cancel);
                app.state.lock().unwrap()["busy"] = json!(false);
                let t = result?;
                if t.outside_jog_window || t.motion_error.is_some() {
                    verified = false;
                    owner.clear();
                    app.stop.store(true, Ordering::SeqCst);
                }
                let mut s = app.state.lock().unwrap();
                if t.outside_jog_window || t.motion_error.is_some() {
                    s["enabled_id"] = Value::Null;
                }
                s["samples"][r.id.to_string()] = json!(t.telemetry);
                s["last_jog"] = json!(t);
                s["message"] = json!(format!(
                    "{} Requested {:+} counts; actual {:+} counts at {}% PWM.{}{}",
                    t.reason,
                    t.requested_counts,
                    t.actual_counts,
                    t.drive_pwm as f64 / 10.,
                    if t.stop_reply_recoveries > 0 {
                        " Stop readback reply retried and verified; no motion command repeated."
                    } else {
                        ""
                    },
                    if t.outside_jog_window {
                        " Coast exceeded the jog window. Stopped; inspect clearance before enabling again."
                    } else {
                        ""
                    }
                ));
                return Ok(s.clone());
            }
            if r.action == "capture" {
                let t = b.stop(r.id)?;
                let mut next = cal.clone();
                let a = next.axes.get_mut(&r.id).unwrap();
                match r.boundary.as_str() {
                    "lower" => {
                        a.lower = Some(
                            t.position_continuous
                                .ok_or("Missing continuous encoder coordinate")?,
                        )
                    }
                    "upper" => {
                        a.upper = Some(
                            t.position_continuous
                                .ok_or("Missing continuous encoder coordinate")?,
                        )
                    }
                    "reference" => {
                        a.reference = Some(
                            t.position_continuous
                                .ok_or("Missing continuous encoder coordinate")?,
                        )
                    }
                    _ => return Err("Unknown boundary".into()),
                };
                if r.boundary == "reference" {
                    a.reference_session = a.reference.filter(|p| !(0..=4095).contains(p)).map(|_| {
                        app.state.lock().unwrap()["coordinate_session"].as_str().unwrap().to_string()
                    });
                    a.reference_joint_rad = r.reference_joint_rad;
                }
                if a.lower
                    .into_iter()
                    .chain(a.upper)
                    .any(|p| !(0..=4095).contains(&p))
                {
                    a.coordinate_session = Some(
                        app.state.lock().unwrap()["coordinate_session"]
                            .as_str()
                            .unwrap()
                            .to_string(),
                    );
                }
                next.validate()?;
                // Keep previous measurements as immutable versions before replacing the current record.
                save(
                    &cfg.output.join(format!("calibration-{}.json", stamp())),
                    &next,
                )?;
                save(&path, &next)?;
                cal = next;
                let mut s = app.state.lock().unwrap();
                s["calibration"] = json!(cal);
                s["samples"][r.id.to_string()] = json!(t);
                s["message"] = json!(format!(
                    "{} captured from stationary {} encoder and saved to disk.",
                    r.boundary, if app.execution.is_virtual_calibration() { "simulated" } else { "physical" }
                ));
                return Ok(s.clone());
            }
            Err("Unknown calibration action".into())
        })();
        // Link loss is judged only from a bus call's own error (the stop
        // below, or a readback probe), never from the command's error text,
        // which can be a filesystem failure. A dead socket fails these at
        // once (broken pipe) or within one 150 ms reply timeout.
        if let Err(e) = &result {
            idle_polling = false;
            app.stop.store(true, Ordering::SeqCst);
            verified = false;
            owner.clear();
            let stopped = bus.as_mut().map(|b| b.stop(if selected != 0 { selected } else { cfg.roles.keys().copied().find(|id| !cal.axes.get(id).is_some_and(|a| a.disabled)).unwrap_or(3) }));
            let link_lost = virtual_mode && matches!(&stopped, Some(Err(stop_error)) if transport_lost(stop_error));
            // The command's text says readback was lost only for a link or
            // frame fault (never device loss: its EIO may be a save failure);
            // the stop's own error may also say the adapter is gone.
            let lost = link_lost || link_readback_lost(strip_stop_retry(e))
                || matches!(&stopped, Some(Err(stop_error)) if readback_lost(strip_stop_retry(stop_error)));
            if let Some(b) = bus.as_mut() {
                if lost && selected != 0 {
                    b.reset_turn_tracking_for(selected);
                }
            }
            if link_lost {
                lose_bus(&app, &mut bus);
            }
            let mut s = app.state.lock().unwrap();
            if lost && cal.axes.get(&selected).is_some_and(multi_turn) {
                s["coordinate_session"] = json!(stamp().to_string());
            }
            s["enabled_id"] = Value::Null;
            if lost {
                // A virtual execution is revoked only with its bus (lose_bus).
                s["connected"] = json!(false);
            }
            s["busy"] = json!(false);
            s["error"] = json!(e);
            s["message"] = json!(format!(
                "{e}. {}",
                if link_lost {
                    "Virtual bench link lost; bus closed (the bench torques off on disconnect). Restart and reconnect explicitly"
                } else if stopped.as_ref().is_some_and(|v| v.is_ok()) {
                    "Stopped and readback verified"
                } else {
                    "Physical stop unverified; keep motor power off until resolved"
                }
            ));
        } else if probe_link && bus.as_mut().and_then(|b| cfg.roles.keys().find(|id| !cal.axes.get(*id).is_some_and(|a| a.disabled)).map(|id| b.feedback(*id))).is_some_and(|probe| probe.is_err_and(|e| transport_lost(&e))) {
            idle_polling = false;
            verified = false;
            owner.clear();
            lose_bus(&app, &mut bus);
            let mut s = app.state.lock().unwrap();
            s["error"] = json!("Virtual bench link lost; bus closed. Restart and reconnect explicitly.");
            let message = format!("{} Virtual bench link lost; restart and reconnect explicitly.", s["message"].as_str().unwrap_or(""));
            s["message"] = json!(message);
        } else {
            app.state.lock().unwrap()["error"] = Value::Null;
        }
        let _ = job.reply.send(result);
    }
}
fn reply(s: &mut TcpStream, status: u16, kind: &str, body: &[u8]) {
    let _ = write!(
        s,
        "HTTP/1.1 {status} Response\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nCross-Origin-Opener-Policy: same-origin\r\nCross-Origin-Embedder-Policy: require-corp\r\nContent-Security-Policy: frame-ancestors 'none'\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = s.write_all(body);
}
fn handle(mut stream: TcpStream, app: Arc<App>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let result = (|| -> R<(String, Vec<u8>)> {
        let mut data = Vec::new();
        let end = loop {
            let mut b = [0; 1024];
            let n = stream.read(&mut b).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("Incomplete request".into());
            }
            data.extend(&b[..n]);
            if data.len() > 16384 {
                return Err("Request too large".into());
            }
            if let Some(p) = data.windows(4).position(|v| v == b"\r\n\r\n") {
                break p + 4;
            }
        };
        let headers = String::from_utf8(data[..end].to_vec()).map_err(|e| e.to_string())?;
        let first = headers
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>();
        if first.len() != 3 {
            return Err("Bad request".into());
        }
        let (method, path) = (first[0], first[1].split('?').next().unwrap());
        let h = |key: &str| {
            headers
                .lines()
                .skip(1)
                .filter_map(|l| l.split_once(':'))
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.trim())
        };
        if h("host") != app.origin.strip_prefix("http://")
            || h("origin").is_some_and(|v| v != app.origin)
            || h("sec-fetch-site").is_some_and(|v| v != "same-origin" && v != "none")
        {
            return Err("Same-origin localhost access required".into());
        }
        if method == "GET" && !path.starts_with("/calibration/") {
            if path == "/actuator-motion-view.mjs" {
                return Ok((
                    "text/javascript".into(),
                    include_bytes!("../../../web/viewer/actuator-motion-view.mjs").to_vec(),
                ));
            }
            if path == "/calibration-mirror.mjs" {
                return Ok((
                    "text/javascript".into(),
                    include_bytes!("../../../web/viewer/calibration-mirror.mjs").to_vec(),
                ));
            }
            if path == "/calibration-ui.mjs" {
                return Ok((
                    "text/javascript".into(),
                    {
                        let source = include_str!("../../../web/viewer/calibration-ui.mjs");
                        if app.execution.is_virtual_calibration() {
                            let extra = format!("'X-Calibration-Server':{},'X-Calibration-Bench':{},'X-Calibration-Generation':'1',", json!(app.execution.server_instance), json!(app.execution.bench_instance));
                            source.replace("headers:{'X-Control-Token'", &format!("headers:{{{extra}'X-Control-Token'")).into_bytes()
                        } else { source.as_bytes().to_vec() }
                    },
                ));
            }
            let root = app.viewer.canonicalize().map_err(|e| e.to_string())?;
            let file = root
                .join(if path == "/" {
                    "index.html"
                } else {
                    path.trim_start_matches('/')
                })
                .canonicalize()
                .map_err(|e| e.to_string())?;
            if !file.starts_with(&root) {
                return Err("Outside viewer".into());
            }
            let kind = match file.extension().and_then(|s| s.to_str()).unwrap_or("") {
                "html" => "text/html",
                "mjs" | "js" => "text/javascript",
                "wasm" => "application/wasm",
                "json" => "application/json",
                "css" => "text/css",
                _ => "application/octet-stream",
            };
            let mut bytes = fs::read(&file).map_err(|e| e.to_string())?;
            if path == "/" {
                bytes=String::from_utf8(bytes).map_err(|e|e.to_string())?.replace("</body>",&format!("<meta name=\"calibration-token\" content=\"{}\"><script type=\"module\" src=\"/calibration-ui.mjs\"></script></body>",app.token)).into_bytes()
            }
            return Ok((kind.into(), bytes));
        }
        if h("x-control-token") != Some(app.token.as_str()) {
            return Err("Session token required".into());
        }
        if method == "GET" && path == "/calibration/status" {
            return Ok((
                "application/json".into(),
                app.state.lock().unwrap().to_string().into_bytes(),
            ));
        }
        if method == "GET" && path == "/calibration/gaits" {
            return Ok(("application/json".into(), serde_json::to_vec(&gait_catalog()?).unwrap()));
        }
        if method == "GET" && path == "/calibration/gait" {
            let query = first[1].split_once('?').map(|q| q.1).unwrap_or("");
            let rel = query.strip_prefix("path=").ok_or("gait path required")?;
            let rel = percent_decode(rel)?;
            return Ok(("application/json".into(), serde_json::to_vec(&gait_with_governor(&rel)?).unwrap()));
        }
        if method == "GET" && path == "/calibration/export" {
            return Ok(("application/json".into(), serde_json::to_vec_pretty(&export_document(&app)).unwrap()));
        }
        if method != "POST" || path != "/calibration/command" {
            return Err("Unknown endpoint".into());
        }
        let client = h("x-client-id").ok_or("Tab identity required")?;
        if client.len() != 36 || !client.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
            return Err("Invalid tab identity".into());
        }
        let length = h("content-length")
            .unwrap_or("0")
            .parse::<usize>()
            .map_err(|e| e.to_string())?;
        if length > 4096 || h("transfer-encoding").is_some() {
            return Err("Invalid body length".into());
        }
        while data.len() < end + length {
            let mut b = [0; 1024];
            let n = stream.read(&mut b).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("Incomplete body".into());
            }
            data.extend(&b[..n]);
        }
        let request: Request =
            serde_json::from_slice(&data[end..end + length]).map_err(|e| e.to_string())?;
        let execution = if request.action == "stop" {
            None
        } else if h("x-calibration-server").is_some() || h("x-calibration-bench").is_some() || h("x-calibration-generation").is_some() {
            let identity = ExecutionIdentity { schema_version: 1, kind: "virtual_calibration".into(),
                server_instance: h("x-calibration-server").ok_or(format!("{BINDING_REFUSED}: server identity required"))?.into(),
                bench_instance: h("x-calibration-bench").ok_or(format!("{BINDING_REFUSED}: bench identity required"))?.into() };
            let generation = h("x-calibration-generation").ok_or(format!("{BINDING_REFUSED}: connection generation required"))?.parse::<u64>().map_err(|_| format!("{BINDING_REFUSED}: invalid connection generation"))?;
            // Scope is checked after the binding, in check_execution (400).
            if identity != app.execution || !identity.is_virtual_calibration() || generation == 0 {
                return Err(format!("{BINDING_REFUSED}: identity mismatch"));
            }
            let mut generations = app.generations.lock().unwrap();
            let current = generations.entry(client.into()).or_insert(generation);
            if generation < *current { return Err(format!("{BINDING_REFUSED}: stale connection generation")); }
            if generation > *current {
                *current = generation;
                app.latch_stop();
            }
            Some((identity, generation))
        } else { None };
        app.check_execution(&request.action, client, execution.as_ref())?;
        let mut stop_epoch = *app.safety.lock().unwrap();
        if request.action == "capture_hold" {
            let mut lock = app.sweep.lock().unwrap();
            let ctl = lock
                .as_mut()
                .ok_or("Move then release to hold before saving")?;
            if app.stop.load(Ordering::SeqCst)
                || app.cancel.load(Ordering::SeqCst)
                || motion_request(&request)? != MotionCommand::Hold
                || !ctl.teaching
                || ctl.motion != MotionCommand::Hold
                || !matches!(request.boundary.as_str(), "upper" | "lower" | "reference")
            {
                return Err("Release to hold before saving a pose".into());
            }
            ctl.update(client, &request, &app.sweep_tuning)?;
            if ctl.motion != MotionCommand::Hold {
                return Err("Release before saving".into());
            }
            ctl.capture = Some((request.boundary.clone(), request.reference_joint_rad));
            return Ok(("application/json".into(), b"{\"ok\":true}".to_vec()));
        }
        if request.action == "gait_update" {
            let mut lock = app.gait.lock().unwrap();
            let lease = lock.as_mut().ok_or("No gait is playing on the leg")?;
            if lease.owner != client {
                return Err("Another tab owns the gait session".into());
            }
            if !(request.speed_scale > 0. && request.speed_scale <= 1.) {
                return Err("Speed scale must be in (0, 1]".into());
            }
            lease.speed_scale = request.speed_scale;
            lease.playing = request.playing;
            lease.last_seen = Instant::now();
            return Ok(("application/json".into(), b"{\"ok\":true}".to_vec()));
        }
        if request.action == "sweep_update" || request.action == "motion_update" {
            let mut lock = app.sweep.lock().unwrap();
            let control = lock.as_mut().ok_or("No continuous sweep is active")?;
            if app.stop.load(Ordering::SeqCst) || app.cancel.load(Ordering::SeqCst) {
                return Err("Sweep stop is latched".into());
            }
            control.update(client, &request, &app.sweep_tuning)?;
            return Ok(("application/json".into(), b"{\"ok\":true}".to_vec()));
        }
        if request.action == "clear" || request.action == "select" {
            stop_epoch = app.latch_stop();
        }
        if request.action == "stop" {
            app.latch_stop();
            // Independent of the ordinary queue. Worker/active loops observe
            // the latch; response means latched, not stationary readback.
            // The copy shows the latch at once (disabled, idle); the stored
            // state and its records are left to the worker's torque-off.
            let mut state = app.state.lock().unwrap().clone();
            state["enabled_id"] = Value::Null;
            state["busy"] = json!(false);
            state["stop_latched"] = json!(true);
            state["message"] = json!(STOP_LATCHED_MESSAGE);
            return Ok(("application/json".into(), state.to_string().into_bytes()));
        }
        if request.action == "halt" {
            // Release, not STOP: cancel the running/queued motion (even a jog
            // queued but not started) without bumping the STOP epoch, so other
            // axes keep their state and the queued halt itself still runs.
            app.cancel_sequence
                .fetch_max(request.sequence, Ordering::SeqCst);
            app.cancel.store(true, Ordering::SeqCst);
        }
        let (tx, rx) = mpsc::channel();
        app.jobs
            .try_send(Job {
                queued: Instant::now(),
                execution,
                stop_epoch,
                request,
                client: client.into(),
                reply: tx,
            })
            .map_err(|_| "Hardware busy; no command queued")?;
        let value = rx
            .recv_timeout(HTTP_WAIT)
            .map_err(|_| "Hardware response timed out")??;
        Ok(("application/json".into(), value.to_string().into_bytes()))
    })();
    match result {
        Ok((kind, bytes)) => reply(&mut stream, 200, &kind, &bytes),
        // Execution-binding refusals (identity, generation, lost virtual bench)
        // are 409 so clients revoke their pin; business refusals, including an
        // out-of-scope virtual command, stay 400.
        Err(e) => reply(
            &mut stream,
            if e.starts_with(BINDING_REFUSED) { BINDING_REFUSED_STATUS } else { 400 },
            "application/json",
            json!({"error":e}).to_string().as_bytes(),
        ),
    }
}
fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn percent_decode(s: &str) -> R<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                out.push(u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).map_err(|e| e.to_string())?, 16).map_err(|e| e.to_string())?);
                i += 3;
            }
            b'+' => { out.push(b' '); i += 1; }
            c => { out.push(c); i += 1; }
        }
    }
    String::from_utf8(out).map_err(|e| e.to_string())
}
/// A compiled gait inside the repository's examples (never an arbitrary file).
fn gait_file(rel: &str) -> R<std::path::PathBuf> {
    let root = repo_root().join("examples").canonicalize().map_err(|e| e.to_string())?;
    let file = repo_root().join(rel).canonicalize().map_err(|e| format!("{rel}: {e}"))?;
    if !file.starts_with(&root) || file.file_name().and_then(|n| n.to_str()) != Some("compiled.json") {
        return Err("Gaits are compiled.json files inside examples/".into());
    }
    Ok(file)
}
/// Completed gait-search trials, best first, with their simulated result.
fn gait_catalog() -> R<Value> {
    let base = repo_root().join("examples/full-robot/measured-actuator-integration");
    let mut rows = Vec::new();
    for study in fs::read_dir(&base).map_err(|e| e.to_string())?.flatten() {
        let comparison = study.path().join("comparison");
        let Ok(trials) = fs::read_dir(&comparison) else { continue };
        let measured = comparison.join("actuator-provenance.json").exists();
        for t in trials.flatten() {
            let dir = t.path();
            let (Ok(trial), true) = (fs::read(dir.join("trial.json")), dir.join("compiled.json").exists()) else { continue };
            let Ok(trial) = serde_json::from_slice::<Value>(&trial) else { continue };
            let outcome = &trial["observation"]["outcome"];
            if outcome["status"] != "complete" {
                continue;
            }
            // Only gaits that passed the search's own gates (tracking, upright).
            let Ok(evaluation) = fs::read(dir.join("evaluation.json")).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice::<Value>(&b).map_err(|e| e.to_string())) else { continue };
            if evaluation["rejection_reasons"].as_array().is_none_or(|r| !r.is_empty()) {
                continue;
            }
            let rel = dir.join("compiled.json").strip_prefix(repo_root()).map(|p| p.display().to_string()).unwrap_or_default();
            let rel = rel.trim_start_matches("./").to_string();
            rows.push(json!({
                "path": rel,
                "study": study.file_name().to_string_lossy(),
                "trial": t.file_name().to_string_lossy(),
                "objective": outcome["objective"],
                "speed_m_s": evaluation["eligible_speed_m_s"],
                "tracking_rms_rad_max": evaluation["tracking_rms_rad"].as_array().map(|v| v.iter().filter_map(|x| x.as_f64()).fold(0f64, f64::max)),
                "measured_actuators": measured,
                "values": trial["observation"]["values"],
            }));
        }
    }
    rows.sort_by(|a, b| {
        (b["measured_actuators"].as_bool(), b["speed_m_s"].as_f64().unwrap_or(f64::NEG_INFINITY))
            .partial_cmp(&(a["measured_actuators"].as_bool(), a["speed_m_s"].as_f64().unwrap_or(f64::NEG_INFINITY)))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut rows: Vec<Value> = rows.into_iter().take(60).collect();
    rows.extend(lab_catalog(&base));
    Ok(json!({"gaits": rows}))
}
/// Gait-lab results (`<lab>/results/<name>-<hash>/`): gaits that passed their
/// gates, fastest first, then pose sequences that passed their checks.
fn lab_catalog(base: &std::path::Path) -> Vec<Value> {
    let (mut gaits, mut poses) = (Vec::new(), Vec::new());
    for lab in fs::read_dir(base).into_iter().flatten().flatten() {
        let Ok(results) = fs::read_dir(lab.path().join("results")) else { continue };
        for r in results.flatten() {
            let dir = r.path();
            let Some(report) = fs::read_to_string(dir.join("report.yaml")).ok().and_then(|t| serde_norway::from_str::<Value>(&t).ok()) else { continue };
            if !dir.join("compiled.json").exists() {
                continue;
            }
            let rel = dir.join("compiled.json").strip_prefix(repo_root()).map(|p| p.display().to_string()).unwrap_or_default();
            let row = |kind: &str, name: &Value| json!({
                "path": rel.trim_start_matches("./"), "kind": kind, "study": lab.file_name().to_string_lossy(), "trial": name,
                "speed_m_s": report["speed_m_s"], "measured_actuators": true, "summary": report["summary"],
            });
            match (report["kind"].as_str(), report["status"].as_str()) {
                (Some("pose_sequence"), Some("ready")) => poses.push(row("pose_sequence", &report["sequence"])),
                (None, Some("passed")) if dir.join("spec-identity.json").exists() => gaits.push(row("lab_gait", &report["gait"])),
                _ => {}
            }
        }
    }
    gaits.sort_by(|a, b| b["speed_m_s"].as_f64().partial_cmp(&a["speed_m_s"].as_f64()).unwrap_or(std::cmp::Ordering::Equal));
    gaits.into_iter().chain(poses).collect()
}
/// A trial's compiled gait with the reference governor the simulation ran it
/// through attached (`playback_governor`; the rule is the shared
/// `gait_playback::compiled_with_governor`).
fn gait_with_governor(rel: &str) -> R<Value> {
    Ok(sim_runtime::gait_playback::compiled_with_governor(&gait_file(rel)?)?.0)
}
/// Per-motor statistics of one leg gait run.
fn gait_statistics(rows: &[Value], ids: &[u8], roles: &std::collections::BTreeMap<u8, String>, predicted: &std::collections::BTreeMap<u8, f64>, pwm_ceiling: f64) -> Value {
    let mut out = serde_json::Map::new();
    for id in ids {
        let r: Vec<&Value> = rows.iter().filter(|x| x["id"] == *id && x["playing"] == true && x["actual"].is_number()).collect();
        let f = |x: &Value, k: &str| x[k].as_f64().unwrap_or(f64::NAN);
        let n = r.len().max(1) as f64;
        let err: Vec<f64> = r.iter().map(|x| f(x, "actual") - f(x, "command")).collect();
        let raw: Vec<f64> = r.iter().map(|x| f(x, "actual") - f(x, "desired")).collect();
        let rms = |v: &[f64]| (v.iter().map(|e| e * e).sum::<f64>() / v.len().max(1) as f64).sqrt();
        let peak = |v: &[f64]| v.iter().fold(0f64, |m, e| m.max(e.abs()));
        let pwm: Vec<f64> = r.iter().map(|x| f(x, "pwm").abs()).collect();
        let governed = r.iter().filter(|x| (f(x, "command") - f(x, "desired")).abs() > 20.).count() as f64 / n;
        let clamped = r.iter().filter(|x| x["clamped"] == true).count() as f64 / n;
        // Measured acceleration over ~50 ms windows (belt-slip screen).
        let mut peak_acc = 0f64;
        for w in r.windows(3) {
            let dt = f(w[2], "wall_s") - f(w[0], "wall_s");
            if dt > 0.03 && dt < 0.2 {
                peak_acc = peak_acc.max(((f(w[2], "velocity") - f(w[0], "velocity")) / dt).abs());
            }
        }
        // Lag: shift (in samples) of the actual trace that best matches the command.
        let (cmd, act): (Vec<f64>, Vec<f64>) = r.iter().map(|x| (f(x, "command"), f(x, "actual"))).unzip();
        let lag = (0..20usize).min_by(|a, b| {
            let e = |k: usize| cmd.iter().zip(act.iter().skip(k)).map(|(c, a)| (a - c).powi(2)).sum::<f64>() / (cmd.len().saturating_sub(k)).max(1) as f64;
            e(*a).total_cmp(&e(*b))
        }).unwrap_or(0);
        let period = if r.len() > 1 { (f(r[r.len() - 1], "wall_s") - f(r[0], "wall_s")) / (r.len() - 1) as f64 } else { 0. };
        let volts: Vec<f64> = r.iter().map(|x| f(x, "voltage_v")).filter(|v| v.is_finite()).collect();
        let temps: Vec<f64> = r.iter().map(|x| f(x, "temperature_c")).filter(|v| v.is_finite()).collect();
        out.insert(id.to_string(), json!({
            "role": roles[id], "samples": r.len(),
            "tracking_rms_counts": rms(&err), "tracking_peak_counts": peak(&err),
            "tracking_rms_deg": rms(&err) * 360. / 4096., "tracking_peak_deg": peak(&err) * 360. / 4096.,
            "error_vs_raw_gait_rms_counts": rms(&raw),
            "simulated_tracking_rms_counts": predicted.get(id),
            "lag_s": lag as f64 * period,
            "mean_effort": pwm.iter().sum::<f64>() / n / 1000., "saturated_fraction": pwm.iter().filter(|p| **p >= 0.95 * pwm_ceiling).count() as f64 / n,
            "peak_measured_acceleration_counts_s2": peak_acc,
            "governor_limited_fraction": governed, "clamped_fraction": clamped,
            "minimum_voltage_v": volts.iter().cloned().fold(f64::INFINITY, f64::min),
            "maximum_temperature_c": temps.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        }));
    }
    Value::Object(out)
}
/// Recent leg gait runs (summaries only), newest first.
fn gait_run_history(cfg: &Config) -> Value {
    let dir = cfg.output.join("gait-runs");
    let mut runs: Vec<(String, Value)> = fs::read_dir(&dir).ok().into_iter().flatten().flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let v: Value = serde_json::from_slice(&fs::read(e.path()).ok()?).ok()?;
            Some((name.clone(), json!({"file": name, "gait": v["gait"], "effort": v["effort"], "speed_scale": v["speed_scale"], "gait_time_s": v["gait_time_s"], "statistics": v["statistics"], "outcome": v["outcome"]})))
        }).collect();
    runs.sort_by(|a, b| b.0.cmp(&a.0));
    json!(runs.into_iter().take(12).map(|r| r.1).collect::<Vec<_>>())
}
/// Play a gait on the physical leg. Each bound motor follows the gait's
/// governed reference (the same shared Rust sampler and reference governor
/// the simulation used), tightened to `effort` × that motor's measured
/// capability from the accepted actuator registry, and for the belt to the
/// campaign plan's belt acceleration limit. Motion stays inside the taught
/// poses (the desired reference is clamped before the governor), goes
/// through the shared feedback controller and the FPGA window, needs a live
/// browser lease, and starts from the leg's measured pose. Every period is
/// recorded; per-motor statistics are saved with the run.
#[allow(clippy::too_many_arguments)]
fn run_gait(
    app: &App,
    cfg: &Config,
    b: &mut CalibrationBus,
    cal: &Calibration,
    proven: &std::collections::BTreeSet<u8>,
    session: Option<&str>,
    client: &str,
    r: &Request,
    stop_epoch: u64,
    reply: impl FnOnce(&Value),
) -> R<String> {
    use sim_runtime::gait_playback::{Gait, GovernedGait, LegBinding};
    const RAD: f64 = std::f64::consts::TAU / 4096.;
    if !r.supported {
        return Err("Confirm the leg is suspended with clear space around every joint".into());
    }
    let effort = if r.effort > 0. { r.effort.clamp(0.05, 1.) } else { 0.5 };
    let compiled = gait_with_governor(&r.gait)?;
    let gait = Gait::from_compiled(&compiled, &r.gait)?;
    if r.bindings.is_empty() {
        return Err("Bind at least one motor to a CAD joint in the leg mirror".into());
    }
    let mut bindings = Vec::new();
    let mut axes = Vec::new();
    for g in &r.bindings {
        let a = usable(cal.axes.get(&g.id).ok_or("Unknown motor")?, session);
        let role = &cfg.roles[&g.id];
        if a.disabled {
            return Err(format!("{role} is disabled"));
        }
        if a.lower.is_none() || a.upper.is_none() {
            return Err(format!("Teach both poses of {role} first"));
        }
        if !proven.contains(&g.id) {
            return Err(format!("{role}: watchdogs not proven; select a motor with hold enabled"));
        }
        let reference = a.reference.ok_or(format!("{role}: save its sim alignment first"))?;
        if (reference < 0 || reference > 4095) && a.reference_session.as_deref() != session {
            return Err(format!("{role}: its sim alignment is from an earlier encoder session; re-align"));
        }
        // The saved alignment pose wins over the page's: the reference counts
        // were captured at that joint angle.
        let home_rad = a.reference_joint_rad.unwrap_or(g.home_rad);
        let binding = LegBinding { id: g.id, joint: g.joint.clone(), polarity: g.polarity, reference_counts: reference as f64, home_rad };
        binding.validate()?;
        if gait.index(&g.joint).is_none() {
            return Err(format!("the gait has no joint {}", g.joint));
        }
        bindings.push(binding);
        axes.push(a);
    }
    // The gait must fit the taught poses as mapped: a wrong sign or alignment
    // puts it outside, and every target would be pinned at a pose.
    let window = |a: &AxisCalibration| {
        let (lo, hi) = a.encoder_bounds();
        (lo.unwrap().min(hi.unwrap()) as f64 + 6., lo.unwrap().max(hi.unwrap()) as f64 - 6.)
    };
    let mut misfit = Vec::new();
    for (bd, a) in bindings.iter().zip(&axes) {
        let (lo, hi) = window(a);
        let i = gait.index(&bd.joint).unwrap();
        let fit = |polarity: f64| -> R<f64> {
            let flipped = LegBinding { polarity, ..bd.clone() };
            let mut inside = 0;
            for k in 0..200 {
                let q = gait.sample(gait.info.period_s * k as f64 / 200.)?[i];
                inside += usize::from((lo..=hi).contains(&flipped.counts(q)));
            }
            Ok(inside as f64 / 200.)
        };
        let (here, flipped) = (fit(bd.polarity)?, fit(-bd.polarity)?);
        if here < 0.95 {
            let role = &cfg.roles[&bd.id];
            misfit.push(if flipped >= 0.95 {
                format!("{role}: only {:.0}% of the gait fits its taught poses with this direction, {:.0}% with the opposite; its mirror sign is probably reversed (flip +/− in the leg mirror, check with Q)", here * 100., flipped * 100.)
            } else {
                format!("{role}: only {:.0}% of the gait fits its taught poses ({:.0}% reversed); re-save its sim alignment at the CAD home pose or widen its poses", here * 100., flipped * 100.)
            });
        }
    }
    if !misfit.is_empty() {
        return Err(misfit.join("; "));
    }
    // Per-motor limits: effort × measured capability (accepted registry, at the
    // measured supply), belt acceleration from the campaign plan.
    let registry = sim_runtime::actuator_registry::Registry::load(&repo_root().join("examples/actuators/hx30hm/accepted/registry.json"))?;
    let plan_limits: Value = cfg.campaign_plan.as_ref().and_then(|p| fs::read(p).ok()).and_then(|b| serde_json::from_slice::<Value>(&b).ok()).map(|v| v["limits"].clone()).unwrap_or(Value::Null);
    let supply = app.state.lock().unwrap()["samples"].as_object().and_then(|m| m.values().filter_map(|t| t["voltage_v"].as_f64()).reduce(f64::min)).unwrap_or(12.0);
    let mut governed = GovernedGait::new(gait.clone());
    let mut limits = serde_json::Map::new();
    for (bd, a) in bindings.iter().zip(&axes) {
        let i = gait.index(&bd.joint).unwrap();
        let family_name = registry.role_family(&bd.joint)?.to_string();
        let family = &registry.families[&family_name];
        let (full_speed, measured_acc) = sim_runtime::actuator_registry::family_limits(family, supply)?;
        let role = &cfg.roles[&bd.id];
        let belt_acc = plan_limits[role.as_str()]["max_acceleration_counts_s2"].as_f64().map(|c| c * RAD);
        let speed = effort * full_speed;
        let acc = measured_acc.map(|m| effort * m).unwrap_or(f64::INFINITY).min(belt_acc.unwrap_or(f64::INFINITY));
        governed.limit(i, speed, acc)?;
        let (lo, hi) = window(a);
        governed.clamp(i, bd.joint_rad(lo), bd.joint_rad(hi));
        let c = governed.config(i).unwrap();
        limits.insert(bd.id.to_string(), json!({"role": role, "family": family_name, "family_hash": family.content_hash(),
            "motor_full_drive_speed_rad_s": full_speed, "motor_measured_acceleration_rad_s2": measured_acc, "belt_acceleration_limit_rad_s2": belt_acc,
            "governor_speed_rad_s": c.maximum_speed_rad_s, "governor_acceleration_rad_s2": c.maximum_acceleration_rad_s2,
            "governor_speed_counts_s": c.maximum_speed_rad_s / RAD, "governor_acceleration_counts_s2": c.maximum_acceleration_rad_s2 / RAD}));
    }
    // Simulated tracking of the same gait (the search's evaluation), for comparison.
    let evaluation: Value = fs::read(gait_file(&r.gait)?.with_file_name("evaluation.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    let predicted: std::collections::BTreeMap<u8, f64> = bindings.iter().filter_map(|bd| {
        let i = gait.index(&bd.joint)?;
        Some((bd.id, evaluation["tracking_rms_rad"][i].as_f64()? / RAD))
    }).collect();
    let ids: Vec<u8> = bindings.iter().map(|b| b.id).collect();
    let speed_scale = if r.speed_scale > 0. { r.speed_scale.min(1.) } else { 1. };
    let pwm_ceiling = r.drive_pwm.clamp(1, 1000) as f64;
    // Arm before publishing or replying, so a refusal reaches the client.
    app.arm(stop_epoch, false)?;
    {
        let mut s = app.state.lock().unwrap();
        s["busy"] = json!(true);
        s["gait"] = json!({"running": true, "phase": "approach", "gait": r.gait, "t": 0., "speed_scale": speed_scale, "effort": effort,
            "motor_ids": ids, "bindings": bindings.iter().map(|b| json!(b)).collect::<Vec<_>>(), "limits": limits,
            "gait_governor": gait.info.governor, "targets": {}, "errors": {}, "clamped": 0, "statistics": {}});
        s["gait_runs"] = gait_run_history(cfg);
        s["message"] = json!("Moving the leg to the gait's first pose through the gait's governor. Z stops drive.");
        reply(&s);
    }
    *app.gait.lock().unwrap() = Some(GaitLease { owner: client.into(), speed_scale, playing: true, last_seen: Instant::now() });
    let run_started = Instant::now();
    writeln_log(cfg, json!({"event": "gait_start", "gait": r.gait, "bindings": bindings, "speed_scale": speed_scale, "effort": effort, "limits": limits}))?;
    let started = std::cell::Cell::new(false);
    let clock = std::cell::Cell::new((0f64, Instant::now()));
    let governed = std::cell::RefCell::new(governed);
    let positions = std::cell::RefCell::new(std::collections::BTreeMap::<u8, f64>::new());
    let commands = std::cell::RefCell::new(std::collections::BTreeMap::<u8, (f64, f64, f64, bool)>::new());
    let rows = std::cell::RefCell::new(Vec::<Value>::new());
    let clamped = std::cell::Cell::new(0u64);
    let tolerance = cfg.sweep_tuning.hold_deadband_counts.unwrap_or(8.).max(8.) + 4.;
    // Each motor's governed reference starts from its first reading in this session.
    let initialized = std::cell::RefCell::new(std::collections::BTreeSet::<u8>::new());
    let result = b.controlled_motion_multi(&ids, &axes, 20, &cfg.sweep_tuning, &app.cancel, false, r.drive_mode, || {
        if app.cancel.load(Ordering::SeqCst) || app.stop.load(Ordering::SeqCst) {
            return Ok(None);
        }
        let (scale, playing) = {
            let lock = app.gait.lock().unwrap();
            let lease = lock.as_ref().ok_or("Gait controls closed")?;
            if lease.last_seen.elapsed() > Duration::from_millis(1500) {
                return Err("Browser heartbeat lost; gait stopped".into());
            }
            (lease.speed_scale, lease.playing)
        };
        let (mut t, last) = clock.get();
        let now = Instant::now();
        let dt = (now - last).as_secs_f64().clamp(0.001, 0.2);
        if started.get() && playing {
            t += dt * scale;
        }
        clock.set((t, now));
        let mut g = governed.borrow_mut();
        for bd in &bindings {
            let first = if initialized.borrow().contains(&bd.id) { None } else { positions.borrow().get(&bd.id).copied() };
            if let Some(p) = first {
                g.start_from(gait.index(&bd.joint).unwrap(), bd.joint_rad(p));
                initialized.borrow_mut().insert(bd.id);
            }
        }
        if initialized.borrow().len() < bindings.len() {
            // First period: nothing read yet; hold until every motor has a reading.
            return Ok(Some(axes.iter().map(|a| (SweepInput { speed_counts_s: 60., pwm_limit: r.drive_pwm.clamp(1, 1000) }, MotionCommand::Hold, a.clone())).collect()));
        }
        let out = g.step(t, dt, scale)?;
        let desired = g.gait.sample(t)?;
        let desired_start = g.desired(0.)?;
        drop(g);
        // Start the clock once every motor has reached the governed first pose.
        if !started.get() {
            // Per motor: distance still to govern (reference to first pose),
            // and the motor's distance from the reference, in counts.
            let gaps: Vec<(u8, f64, f64)> = bindings.iter().map(|bd| {
                let i = gait.index(&bd.joint).unwrap();
                let goal = bd.counts(out[i].0);
                let first = bd.counts(desired_start[i]);
                (bd.id, (first - goal).abs(), positions.borrow().get(&bd.id).map_or(f64::INFINITY, |p| (p - goal).abs()))
            }).collect();
            if gaps.iter().all(|(_, left, off)| *left < 2. && *off < tolerance) {
                started.set(true);
            } else if run_started.elapsed() > Duration::from_secs(30) {
                return Err(format!("The leg did not reach the gait's first pose within 30 s ({})", gaps.iter().map(|(id, left, off)| format!("{}: reference {left:.0} counts from the first pose, motor {off:.0} counts from the reference", cfg.roles[id])).collect::<Vec<_>>().join("; ")));
            }
        }
        let mut plan = Vec::new();
        let mut shown = serde_json::Map::new();
        for (bd, a) in bindings.iter().zip(&axes) {
            let i = gait.index(&bd.joint).unwrap();
            let (q, v) = out[i];
            let goal = bd.counts(q);
            let raw = bd.counts(desired[i]);
            let (lo, hi) = window(a);
            let is_clamped = raw < lo || raw > hi;
            if is_clamped {
                clamped.set(clamped.get() + 1);
            }
            let velocity = bd.polarity * v / RAD;
            commands.borrow_mut().insert(bd.id, (goal, raw, velocity, is_clamped));
            let speed = (velocity.abs() + 60.).min(cfg.sweep_tuning.maximum_speed_counts_s);
            plan.push((SweepInput { speed_counts_s: speed, pwm_limit: r.drive_pwm.clamp(1, 1000) }, MotionCommand::Track(goal.clamp(lo, hi), velocity), a.clone()));
            shown.insert(bd.id.to_string(), json!(goal));
        }
        let mut s = app.state.lock().unwrap();
        s["gait"]["t"] = json!(t);
        s["gait"]["phase"] = json!(if started.get() { if playing { "playing" } else { "paused" } } else { "approach" });
        s["gait"]["speed_scale"] = json!(scale);
        s["gait"]["targets"] = Value::Object(shown);
        s["gait"]["clamped"] = json!(clamped.get());
        Ok(Some(plan))
    }, |id, t, sample| {
        let p = t.position_continuous.unwrap_or(t.position_raw as i32) as f64;
        positions.borrow_mut().insert(id, p);
        let (command, desired, velocity, is_clamped) = commands.borrow().get(&id).copied().unwrap_or((p, p, 0., false));
        {
            let mut rows = rows.borrow_mut();
            if rows.len() < 60_000 {
                rows.push(json!({"id": id, "wall_s": run_started.elapsed().as_secs_f64(), "gait_s": clock.get().0, "playing": started.get(),
                    "command": command, "desired": desired, "command_velocity": velocity, "clamped": is_clamped,
                    "actual": p, "velocity": sample.velocity_counts_s, "pwm": sample.pwm, "voltage_v": t.voltage_v, "temperature_c": t.temperature_c,
                    "warnings": sample.warnings}));
            }
        }
        let mut s = app.state.lock().unwrap();
        s["samples"][id.to_string()] = json!(t);
        s["gait"]["errors"][id.to_string()] = json!(p - command);
        if !sample.warnings.is_empty() {
            s["gait"]["warnings"] = json!(sample.warnings);
        }
        // Rolling statistics every ~1 s of samples.
        if rows.borrow().len() % 100 == 0 {
            s["gait"]["statistics"] = gait_statistics(&rows.borrow(), &ids, &cfg.roles, &predicted, pwm_ceiling);
        }
        Ok(())
    })?;
    let statistics = gait_statistics(&rows.borrow(), &ids, &cfg.roles, &predicted, pwm_ceiling);
    let outcome = result.motion_error.clone().unwrap_or_else(|| "stopped".into());
    let dir = cfg.output.join("gait-runs");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let record = json!({"version": 1, "gait": r.gait, "gait_governor": gait.info.governor, "effort": effort, "speed_scale": speed_scale,
        "pwm_ceiling": pwm_ceiling, "drive_mode": r.drive_mode, "bindings": bindings, "limits": limits, "registry": registry.identity(),
        "gait_time_s": clock.get().0, "wall_s": run_started.elapsed().as_secs_f64(), "outcome": outcome, "clamped_targets": clamped.get(),
        "statistics": statistics, "samples": *rows.borrow(),
        "scope": "Suspended leg (no ground contact); tracking error is actual minus the governed command sent to the shared controller."});
    fs::write(dir.join(format!("run-{}.json", stamp())), serde_json::to_vec(&record).unwrap()).map_err(|e| e.to_string())?;
    writeln_log(cfg, json!({"event": "gait_end", "gait": r.gait, "motion_error": result.motion_error, "clamped_targets": clamped.get(), "statistics": statistics}))?;
    {
        let mut s = app.state.lock().unwrap();
        s["gait"]["statistics"] = statistics;
        s["gait_runs"] = gait_run_history(cfg);
    }
    match result.motion_error {
        Some(e) => Err(e),
        None => Ok(format!("Gait stopped after {:.1} s of gait time; statistics saved", clock.get().0)),
    }
}
fn writeln_log(cfg: &Config, value: Value) -> R<()> {
    use std::io::Write as _;
    let mut f = fs::OpenOptions::new().create(true).append(true).open(cfg.output.join("gait.jsonl")).map_err(|e| e.to_string())?;
    let mut value = value;
    value["unix_ms"] = json!(stamp());
    writeln!(f, "{value}").map_err(|e| e.to_string())
}
/// A lesson's lab step (`sim-lab`): one taught, proven motor at a bounded
/// duty for a few seconds, through the campaign's guarded session (travel
/// window with braking margin, sag, temperature, stop). Needs the operator's
/// confirmation that the leg is supported; writes a receipt.
#[allow(clippy::too_many_arguments)]
fn run_lab_step(
    app: &App,
    cfg: &Config,
    b: &mut CalibrationBus,
    cal: &Calibration,
    proven: &std::collections::BTreeSet<u8>,
    session: Option<&str>,
    r: &Request,
    stop_epoch: u64,
    reply: impl FnOnce(&Value),
) -> R<Value> {
    use sim_runtime::acquisition::{calibration_serial::BusRig, characterization as ch};
    if !r.supported {
        return Err("Confirm the operator checklist (at the bench, leg supported, supervisor running)".into());
    }
    if app.stop.load(Ordering::SeqCst) {
        return Err("Stop is latched; clear it at the bench first".into());
    }
    let (id, role) = cfg.roles.iter().find(|(_, role)| role.as_str() == r.role || role.starts_with(&format!("{}/", r.role)) || role.ends_with(&format!("/{}", r.role))).map(|(i, role)| (*i, role.clone())).ok_or_else(|| format!("No motor has the role `{}`", r.role))?;
    let a = usable(&cal.axes[&id], session);
    let (lo, hi) = a.encoder_bounds();
    if a.disabled {
        return Err(format!("{role} is disabled"));
    }
    let (Some(lo), Some(hi)) = (lo, hi) else { return Err(format!("Teach both poses of {role} first")) };
    if !proven.contains(&id) {
        return Err(format!("Prove {role}'s watchdogs first (select it with hold enabled)"));
    }
    // Gates and drive limits from the campaign plan, when configured.
    let plan: Option<ch::Plan> = cfg.campaign_plan.as_ref().and_then(|p| fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok());
    let gates = plan.as_ref().map(|p| p.gates.clone()).unwrap_or_default();
    let limits: std::collections::BTreeMap<u8, ch::AxisLimits> = plan.as_ref().and_then(|p| p.limits.get(&role).cloned()).map(|l| [(id, l)].into()).unwrap_or_default();
    let axis = ch::Axis { id, role: role.clone(), lower: lo.min(hi) as f64, upper: lo.max(hi) as f64 };
    app.arm(stop_epoch, false)?;
    {
        let mut s = app.state.lock().unwrap();
        s["busy"] = json!(true);
        s["lab"] = json!({"running": true, "role": role, "duty": r.duty, "seconds": r.seconds});
        s["message"] = json!(format!("Lab step on {role}: duty {:.2} for {:.1} s. Stop ends it.", r.duty, r.seconds));
        reply(&s);
    }
    let windows = vec![(id, axis.lower as i32, axis.upper as i32)];
    let step = {
        let mut bus = BusRig::new(b, &windows, cfg.sweep_tuning.period_s, &app.cancel)?;
        let mut limited = ch::LimitedRig::new(&mut bus, limits.clone());
        let result = {
            let mut session = ch::Session::new(&mut limited, gates, vec![axis.clone()])?;
            session.limits = limits;
            ch::lab_step(&mut session, id, r.duty, r.seconds)
        };
        let _ = ch::Rig::stop(&mut limited);
        result?
    };
    const RAD: f64 = std::f64::consts::TAU / 4096.;
    let dir = cfg.output.join("labs");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("lab-{}-{}.json", role.replace('/', "-"), stamp()));
    let steady_rad_s = step.steady_counts_s.map(|c| c * RAD);
    let receipt = json!({"role": role, "motor_id": id, "duty": r.duty, "seconds": r.seconds, "steady_counts_s": step.steady_counts_s, "steady_rad_s": steady_rad_s, "stopped": step.stopped, "samples": step.samples, "window": [axis.lower, axis.upper], "supply_v": step.samples.first().map(|s| s.voltage_v)});
    fs::write(&path, serde_json::to_vec_pretty(&receipt).unwrap()).map_err(|e| e.to_string())?;
    writeln_log(cfg, json!({"event": "lab_step", "role": role, "duty": r.duty, "seconds": r.seconds, "steady_rad_s": steady_rad_s, "stopped": step.stopped, "receipt": path}))?;
    Ok(json!({"role": role, "steady_rad_s": steady_rad_s, "stopped": step.stopped, "seconds": step.seconds, "receipt": path, "supply_v": step.samples.first().map(|s| s.voltage_v),
        "headline": match steady_rad_s { Some(v) => format!("{role} ran at {v:.3} rad/s"), None => format!("{role} did not reach a steady speed") }}))
}
/// Campaign directory and the receipts to reuse. Arms first: a command a STOP
/// interrupted creates nothing, so it can never shadow the real interrupted
/// campaign. Resume picks the newest unfinished/resumable directory holding
/// at least one receipt; empty ones are ignored, and none is ever deleted.
fn campaign_directory(app: &App, root: &std::path::Path, resume: bool, stop_epoch: u64) -> R<(PathBuf, Vec<sim_runtime::acquisition::characterization::StageResult>)> {
    use sim_runtime::acquisition::characterization as ch;
    app.arm(stop_epoch, false)?;
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let receipt = |p: &std::path::Path| p.extension().is_some_and(|x| x == "json") && !p.to_string_lossy().ends_with(".execution.json");
    let previous = fs::read_dir(root).ok().into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| p.is_dir() && (!p.join("report.json").exists() || fs::read(p.join("resume-pending.json")).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()).is_some_and(|v| v["resumable"] == json!(true))))
        .filter(|p| fs::read_dir(p.join("receipts")).ok().into_iter().flatten().flatten().any(|e| receipt(&e.path()))).max();
    match (resume, previous) {
        (true, Some(dir)) => {
            let history = dir.join(format!("attempt-before-resume-{}", stamp()));
            fs::create_dir(&history).map_err(|e| e.to_string())?;
            for name in ["report.json", "summary.json", "promotion.json", "resume-pending.json"] {
                let previous = dir.join(name);
                if previous.exists() { fs::copy(&previous, history.join(name)).map_err(|e| e.to_string())?; }
            }
            let mut receipts: Vec<ch::StageResult> = Vec::new();
            let mut names: Vec<_> = fs::read_dir(dir.join("receipts")).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).filter(|p| !p.to_string_lossy().ends_with(".execution.json")).collect();
            names.sort();
            for n in names {
                receipts.push(serde_json::from_slice(&fs::read(&n).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?);
            }
            Ok((dir, receipts))
        }
        (true, None) => Err("No interrupted campaign with receipts to resume".into()),
        (false, _) => Ok((root.join(format!("campaign-{}", stamp())), Vec::new())),
    }
}
/// The characterization campaign on the connected leg (PLAN.md). Axes are the
/// enabled, watchdog-proven motors with both poses taught; each is rehearsed
/// with its tuned model (untuned axes are refused). Stage results are written
/// as receipts the moment they finish; a resumed campaign reuses them.
#[allow(clippy::too_many_arguments)]
fn run_campaign(
    app: &App,
    cfg: &Config,
    b: &mut CalibrationBus,
    cal: &Calibration,
    proven: &std::collections::BTreeSet<u8>,
    session: Option<&str>,
    r: &Request,
    stop_epoch: u64,
    reply: impl FnOnce(&Value),
) -> R<Value> {
    use sim_runtime::acquisition::{calibration_serial::BusRig, characterization as ch, virtual_bench::MotorModel};
    if !r.supported {
        return Err("Confirm the leg is suspended with clear space around every joint".into());
    }
    let plan_path = cfg.campaign_plan.clone().ok_or("No campaign_plan in the server configuration")?;
    let mut plan: ch::Plan = serde_json::from_slice(&fs::read(&plan_path).map_err(|e| format!("{}: {e}", plan_path.display()))?).map_err(|e| e.to_string())?;
    // Axes from taught poses; untaught, disabled, unproven or untuned motors are left out.
    let (mut axes, mut models, mut skipped) = (Vec::new(), std::collections::BTreeMap::new(), Vec::new());
    for (id, role) in &cfg.roles {
        let a = usable(&cal.axes[id], session);
        let (lo, hi) = a.encoder_bounds();
        let why = if a.disabled {
            Some("disabled")
        } else if lo.is_none() || hi.is_none() {
            Some("poses not taught")
        } else if !proven.contains(id) {
            Some("watchdogs not proven; select a motor with hold enabled")
        } else if a.tuning.is_none() {
            Some("not tuned; the campaign rehearses with the tuned model")
        } else {
            None
        };
        if let Some(w) = why {
            skipped.push(format!("{role} ({w})"));
            continue;
        }
        let (lo, hi) = (lo.unwrap().min(hi.unwrap()), lo.unwrap().max(hi.unwrap()));
        let t = a.tuning.as_ref().unwrap();
        axes.push(ch::Axis { id: *id, role: role.clone(), lower: lo as f64, upper: hi as f64 });
        models.insert(*id, MotorModel {
            speed_gain: t.gain_counts_s_per_duty,
            lag_s: t.time_constant_s.max(0.01),
            breakaway_duty: 0.5 * (t.breakaway_duty[0] + t.breakaway_duty[1]),
            moving_friction_duty: t.friction_duty,
            ..Default::default()
        });
    }
    if axes.is_empty() {
        return Err(format!("No motor is ready for the campaign: {}", skipped.join(", ")));
    }
    plan.axes = axes.clone();
    let (dir, resume) = campaign_directory(app, &cfg.output.join("campaigns"), r.resume, stop_epoch)?;
    fs::create_dir_all(dir.join("receipts")).map_err(|e| e.to_string())?;
    let plan_bytes = serde_json::to_vec_pretty(&plan).map_err(|e| e.to_string())?;
    if r.resume && fs::read(dir.join("plan.json")).map_err(|e| e.to_string())? != plan_bytes {
        return Err("Campaign plan or taught axes changed; start a new campaign instead of reusing receipts".into());
    }
    if !r.resume { fs::write(dir.join("plan.json"), &plan_bytes).map_err(|e| e.to_string())?; }
    fs::write(dir.join("resume-pending.json"), serde_json::to_vec_pretty(&json!({"execution":app.execution,"simulated":app.execution.is_virtual_calibration(),"resumable":true})).unwrap()).map_err(|e| e.to_string())?;
    {
        let mut s = app.state.lock().unwrap();
        s["busy"] = json!(true);
        s["campaign"] = json!({"execution":app.execution,"simulated":app.execution.is_virtual_calibration(),"running": true, "axes": axes, "skipped": skipped, "stage": "Starting", "completed": resume.iter().filter(|stage| stage.completed).count(), "receipt_count":resume.len(), "directory": dir, "log": []});
        s["message"] = json!(format!("Characterization campaign on {}. Stop ends it; completed stages are kept.", axes.iter().map(|a| a.role.as_str()).collect::<Vec<_>>().join(", ")));
        reply(&s);
    }
    let windows: Vec<(u8, i32, i32)> = axes.iter().map(|a| (a.id, a.lower as i32, a.upper as i32)).collect();
    let started = Instant::now();
    let receipt_error = std::cell::RefCell::new(None::<String>);
    let report = {
        let mut rig = BusRig::new(b, &windows, cfg.sweep_tuning.period_s, &app.cancel)?;
        let mut receipt_index = resume.len();
        let mut completed = resume.iter().filter(|stage| stage.completed).count();
        let result = ch::run_with(
            &plan,
            &mut rig,
            Some(ch::per_axis_predictor(models.clone())),
            // No CAD scene in this server: multi-axis combinations stay within
            // taught poses, which were reached together only if taught so.
            &|_| Ok(()),
            &mut |m| {
                let mut s = app.state.lock().unwrap();
                s["campaign"]["stage"] = json!(m);
                if let Some(log) = s["campaign"]["log"].as_array_mut() {
                    log.push(json!(m));
                }
            },
            &resume,
            &mut |stage| {
                let next_index = receipt_index + 1;
                let path = dir.join("receipts").join(format!("{next_index:03}-{}-{}.json", stage.stage, stage.id));
                if let Err(error) = fs::write(&path, serde_json::to_vec_pretty(stage).unwrap()) {
                    *receipt_error.borrow_mut() = Some(format!("{}: {error}", path.display()));
                    app.latch_stop();
                    return;
                }
                let provenance = path.with_extension("execution.json");
                if let Err(error) = fs::write(&provenance, serde_json::to_vec_pretty(&app.execution).unwrap()) {
                    *receipt_error.borrow_mut() = Some(format!("{}: {error}", provenance.display()));
                    app.latch_stop();
                    return;
                }
                receipt_index = next_index;
                let mut s = app.state.lock().unwrap();
                if stage.completed { completed += 1; }
                s["campaign"]["completed"] = json!(completed);
                s["campaign"]["receipt_count"] = json!(receipt_index);
                s["campaign"]["last"] = json!({"stage": stage.stage, "axis": stage.id, "completed": stage.completed, "abort": stage.abort});
            },
        );
        let _ = ch::Rig::stop(&mut rig);
        result
    };
    if let Some(error) = receipt_error.into_inner() { return Err(error); }
    let report = report?;
    let fitted_replay: Vec<Value> = report.fitted.iter().map(|(id, fits)| {
        let prior = &models[id];
        json!({"axis": id, "tuned_model_rms_counts": ch::replay_error(&report.samples, *id, prior, cfg.sweep_tuning.period_s),
               "fitted_model_rms_counts": ch::replay_error(&report.samples, *id, &ch::fitted_model(prior, fits), cfg.sweep_tuning.period_s)})
    }).collect();
    let coordinates: std::collections::BTreeMap<u8, Vec<String>> = cfg.roles.iter().map(|(id, role)| {
        let joint = if role.contains("knee") { "Foot" } else if role.contains("worm") { "Worm" } else { "Hip" };
        (*id, ["-Y", "+X", "+Y", "-X"].iter().map(|leg| format!("joint.{leg} | {joint} servo output")).collect())
    }).collect();
    let promotion = ch::promotion(&report, &json!({"source": "tuned per-motor models"}), &coordinates, &format!("{} campaign on {} ({})", if app.execution.is_virtual_calibration() { "SIMULATED virtual calibration" } else { "Physical hardware" }, cfg.fixture, dir.display()));
    let ranking = ch::select_tests(&report, &plan.sensitivity);
    let aborted: Vec<Value> = report.stages.iter().filter(|s| !s.completed).map(|s| json!({"stage": s.stage, "axis": s.id, "abort": s.abort})).collect();
    let summary = json!({
        "execution":app.execution,"simulated":app.execution.is_virtual_calibration(),
        "resumable": !aborted.is_empty() || app.cancel.load(Ordering::SeqCst),
        "directory": dir, "wall_s": started.elapsed().as_secs_f64(), "axes": axes, "skipped": skipped,
        "aborted": aborted, "replay": fitted_replay, "ranking": ranking.iter().take(8).collect::<Vec<_>>(),
        "headline": format!("{} stages, {} stopped by a gate", report.stages.len(), aborted.len()),
    });
    for (name, mut value) in [("report.json", serde_json::to_value(&report).unwrap()), ("promotion.json", promotion), ("summary.json", summary.clone())] {
        value["execution"] = json!(app.execution);
        value["simulated"] = json!(app.execution.is_virtual_calibration());
        fs::write(dir.join(name), serde_json::to_vec_pretty(&value).unwrap()).map_err(|e| e.to_string())?;
    }
    if summary["resumable"] == json!(false) {
        fs::write(dir.join("resume-pending.json"), serde_json::to_vec_pretty(&json!({"execution":app.execution,"simulated":app.execution.is_virtual_calibration(),"resumable":false})).unwrap()).map_err(|e| e.to_string())?;
    }
    Ok(summary)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 3 && !(args.len() == 5 && args[3] == "--virtual-bench") {
        return Err("serve_actuator_calibration CONFIG HTTP_PORT [--virtual-bench SOCKET]".into());
    }
    let virtual_socket = (args.len() == 5).then(|| PathBuf::from(&args[4]));
    let cfg: Config = serde_json::from_slice(&fs::read(&args[1])?)?;
    if cfg.roles.keys().copied().collect::<Vec<_>>() != vec![1, 2, 3] {
        return Err("This FPGA profile covers IDs 1,2,3".into());
    }
    cfg.sweep_tuning.validate()?;
    if virtual_socket.is_some() != (cfg.serial == "virtual-capability-only") {
        return Err("Virtual mode requires serial=virtual-capability-only and --virtual-bench; no fallback".into());
    }
    if virtual_socket.as_ref().is_some_and(|p| !p.is_absolute()) { return Err("Virtual capability socket must be absolute".into()); }
    if virtual_socket.is_some() && cfg.output.exists() && !cfg.output.join("execution.json").exists()
        && fs::read_dir(&cfg.output)?.next().is_some() {
        return Err("Virtual output contains unidentified prior artifacts; use a new empty output directory".into());
    }
    fs::create_dir_all(&cfg.output)?;
    let (initial_bus, bench_instance) = match &virtual_socket {
        Some(socket) => {
            let (bus, identity) = CalibrationBus::open_virtual(socket, None, &cfg.output.join("serial.jsonl"))?;
            (Some(bus), identity)
        }
        None => (None, String::new()),
    };
    let execution = ExecutionIdentity { schema_version: 1,
        kind: if virtual_socket.is_some() { "virtual_calibration" } else { "physical" }.into(),
        server_instance: sim_runtime::hardware_client::new_client_id(), bench_instance };
    let provenance_path = cfg.output.join("execution.json");
    if provenance_path.exists() {
        let previous: ExecutionIdentity = serde_json::from_slice(&fs::read(&provenance_path)?)?;
        if previous.kind != execution.kind || previous.bench_instance != execution.bench_instance {
            return Err("Output belongs to a different physical/virtual bench; use a new output directory".into());
        }
    }
    fs::write(cfg.output.join(format!("execution-{}.json", execution.server_instance)), serde_json::to_vec_pretty(&execution)?)?;
    fs::write(provenance_path, serde_json::to_vec_pretty(&execution)?)?;
    let listener = TcpListener::bind(format!("127.0.0.1:{}", args[2]))?;
    let origin = format!("http://{}", listener.local_addr()?);
    let mut bytes = [0; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let token = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let (tx, rx) = mpsc::sync_channel(1);
    let app = Arc::new(App {
        execution: execution.clone(), virtual_socket,
        generations: Mutex::new(std::collections::BTreeMap::new()), safety: Mutex::new(0),
        state: Mutex::new(
            json!({"execution":execution,"coordinate_session":stamp().to_string(),"maximum_speed_counts_s":cfg.sweep_tuning.maximum_speed_counts_s,"connected":false,"enabled_id":null,"busy":false,"samples":{},"message":"Starting","error":null,"output":cfg.output}),
        ),
        jobs: tx,
        stop: AtomicBool::new(true),
        cancel: AtomicBool::new(true),
        cancel_sequence: AtomicU64::new(0),
        origin: origin.clone(),
        token,
        viewer: cfg.viewer.clone(),
        sweep: Mutex::new(None),
        gait: Mutex::new(None),
        sweep_tuning: cfg.sweep_tuning.clone(),
    });
    app.state.lock().unwrap()["gait_runs"] = gait_run_history(&cfg);
    let a = app.clone();
    std::thread::spawn(move || worker(a, rx, cfg, initial_bus));
    println!("Calibration and robot viewer: {origin}");
    for stream in listener.incoming() {
        let s = stream?;
        let a = app.clone();
        std::thread::spawn(move || handle(s, a));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lab_results_join_the_gait_list() {
        let base = std::env::temp_dir().join(format!("lab-catalog-{}", std::process::id()));
        let results = base.join("gait-lab-test/results");
        let entry = |name: &str, report: &str, governor: bool| {
            let d = results.join(name);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("report.yaml"), report).unwrap();
            fs::write(d.join("compiled.json"), "{}").unwrap();
            if governor {
                fs::write(d.join("spec-identity.json"), "{}").unwrap();
            }
        };
        entry("slow", "gait: slow\nstatus: passed\nspeed_m_s: 0.1\n", true);
        entry("fast", "gait: fast\nstatus: passed\nspeed_m_s: 0.2\n", true);
        entry("fell", "gait: fell\nstatus: rejected\nspeed_m_s: null\n", true);
        entry("no-governor", "gait: old\nstatus: passed\nspeed_m_s: 0.3\n", false);
        entry("crouch", "kind: pose_sequence\nsequence: crouch\nstatus: ready\n", false);
        entry("blocked", "kind: pose_sequence\nsequence: over\nstatus: blocked\n", false);
        let rows = lab_catalog(&base);
        fs::remove_dir_all(&base).ok();
        let names: Vec<(&str, &str)> = rows.iter().map(|r| (r["kind"].as_str().unwrap(), r["trial"].as_str().unwrap())).collect();
        assert_eq!(names, [("lab_gait", "fast"), ("lab_gait", "slow"), ("pose_sequence", "crouch")], "passed gaits fastest first, then ready poses");
    }
    fn tuning() -> SweepTuning {
        serde_json::from_str::<Config>(include_str!(
            "../../../examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/server.json"
        ))
        .unwrap()
        .sweep_tuning
    }
    #[test]
    fn sweep_lease_cannot_be_revived_or_updated_by_another_run() {
        let mut lease = BrowserSweep {
            owner: "owner".into(),
            id: 2,
            run_id: 4,
            sequence: 1,
            input: SweepInput {
                speed_counts_s: 5.,
                pwm_limit: 100,
            },
            last_seen: Instant::now(),
            motion: MotionCommand::Sweep,
            teaching: false,
            capture: None,
        };
        let mut request:Request=serde_json::from_value(json!({"action":"sweep_update","id":2,"run_id":4,"sequence":2,"speed_counts_s":10.,"drive_pwm":200})).unwrap();
        assert!(lease.update("other", &request, &tuning()).is_err());
        request.run_id = 3;
        assert!(lease.update("owner", &request, &tuning()).is_err());
        request.run_id = 4;
        lease.update("owner", &request, &tuning()).unwrap();
        assert_eq!(lease.input.speed_counts_s, 10.);
        assert!(lease.update("owner", &request, &tuning()).is_err());
        request.sequence = 3;
        lease.last_seen = Instant::now() - Duration::from_secs(2);
        assert!(lease.update("owner", &request, &tuning()).is_err());
    }
    fn identity() -> ExecutionIdentity {
        ExecutionIdentity { schema_version:1, kind:"virtual_calibration".into(),
            server_instance:sim_runtime::hardware_client::new_client_id(), bench_instance:sim_runtime::hardware_client::new_client_id() }
    }
    fn app_fixture(execution: ExecutionIdentity) -> (Arc<App>, mpsc::Receiver<Job>) {
        let (tx, rx) = mpsc::sync_channel(1);
        let app = Arc::new(App { execution:execution.clone(), virtual_socket:None,
            generations:Mutex::new(std::collections::BTreeMap::new()), safety:Mutex::new(0),
            state:Mutex::new(json!({"execution":execution,"campaign":{"result":{"completed":3}},"tuning":{"result":{"kp":7}},"calibration":{"axes":{"1":{"lower":100,"upper":200}}}})),
            jobs:tx, stop:AtomicBool::new(false), cancel:AtomicBool::new(false), cancel_sequence:AtomicU64::new(0),
            origin:"http://127.0.0.1:4194".into(), token:"fixture-token".into(), viewer:PathBuf::new(),
            sweep:Mutex::new(None), gait:Mutex::new(None), sweep_tuning:tuning() });
        (app, rx)
    }
    fn wire_post(app: Arc<App>, client: &str, body: Value, pin: Option<(&ExecutionIdentity,u64)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || handle(listener.accept().unwrap().0, app));
        let mut stream = TcpStream::connect(address).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let body = body.to_string();
        let headers = pin.map(|(i,g)| format!("X-Calibration-Server: {}\r\nX-Calibration-Bench: {}\r\nX-Calibration-Generation: {g}\r\n", i.server_instance,i.bench_instance)).unwrap_or_default();
        write!(stream,"POST /calibration/command HTTP/1.1\r\nHost: 127.0.0.1:4194\r\nX-Control-Token: fixture-token\r\nX-Client-Id: {client}\r\n{headers}Content-Length: {}\r\n\r\n{body}",body.len()).unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        server.join().unwrap();
        reply
    }
    #[test]
    fn authoritative_handler_refuses_unbound_replaced_physical_and_out_of_scope() {
        let pin = identity();
        let client = sim_runtime::hardware_client::new_client_id();
        for kind in ["virtual_calibration", "physical"] {
            let mut active = pin.clone(); active.kind = kind.into();
            let (app, rx) = app_fixture(active);
            let mut replaced = pin.clone(); replaced.server_instance = sim_runtime::hardware_client::new_client_id();
            for proposed in [&pin, &replaced] {
                let answer = wire_post(app.clone(), &client, json!({"action":"jog","id":1,"delta":1}),Some((proposed,1)));
                if kind == "virtual_calibration" && proposed == &pin {
                    // Valid binding, out-of-scope action: an ordinary refusal that keeps the pin.
                    assert!(answer.starts_with("HTTP/1.1 400") && !answer.contains(BINDING_REFUSED) && answer.contains(&out_of_scope("jog")), "{answer}");
                } else {
                    assert!(answer.starts_with("HTTP/1.1 409") && answer.contains(BINDING_REFUSED), "binding refusal, not a business 400");
                }
            }
            let answer = wire_post(app.clone(),&client,json!({"action":"select","id":1}),Some((&replaced,1)));
            assert!(answer.starts_with("HTTP/1.1 409") && answer.contains(BINDING_REFUSED));
            assert!(rx.try_recv().is_err(), "refused requests never reach acquisition");
        }
        let (app, _) = app_fixture(pin);
        assert!(wire_post(app,&client,json!({"action":"inspect"}),None).starts_with("HTTP/1.1 409"), "virtual identity required is a binding refusal");
    }
    #[test]
    fn newer_generation_revokes_queued_work_and_stop_bypasses_full_queue_preserving_records() {
        let pin = identity();
        let client = sim_runtime::hardware_client::new_client_id();
        let (app, rx) = app_fixture(pin.clone());
        app.generations.lock().unwrap().insert(client.clone(),1);
        let (reply, _) = mpsc::channel();
        app.jobs.try_send(Job { queued:Instant::now(), execution:Some((pin.clone(),1)), stop_epoch:0,
            request:serde_json::from_value(json!({"action":"select","id":1})).unwrap(), client:client.clone(), reply }).unwrap();
        // This real inline consumer registers generation 2 before rejecting a
        // missing hold session, proving queued generation 1 is now invalid.
        assert!(wire_post(app.clone(),&client,json!({"action":"capture_hold","id":1}),Some((&pin,2))).starts_with("HTTP/1.1 400"));
        let queued = rx.try_recv().unwrap();
        assert!(app.check_execution(&queued.request.action,&queued.client,queued.execution.as_ref()).is_err_and(|e| e.starts_with(BINDING_REFUSED)));
        app.jobs.try_send(queued).unwrap();
        let records = app.state.lock().unwrap().clone();
        let epoch = *app.safety.lock().unwrap();
        let response = wire_post(app.clone(),&client,json!({"action":"stop","id":null}),None);
        assert!(response.starts_with("HTTP/1.1 200"));
        let body: Value = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert!(body["enabled_id"].is_null() && body["busy"] == json!(false) && body["stop_latched"] == json!(true), "early reply shows the axis disabled at once");
        assert_eq!(body["message"], json!(STOP_LATCHED_MESSAGE));
        assert!(app.stop.load(Ordering::SeqCst) && app.cancel.load(Ordering::SeqCst));
        assert!(app.arm(epoch,true).is_err(), "STOP after early reply cannot be cleared");
        assert_eq!(*app.state.lock().unwrap(),records,"STOP latch keeps taught/tune/campaign records");
    }
    fn physical() -> ExecutionIdentity {
        let mut identity = identity(); identity.kind = "physical".into(); identity
    }
    fn config_fixture(output: PathBuf) -> Config {
        let mut cfg: Config = serde_json::from_str(include_str!("../../../examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/server.json")).unwrap();
        // Never the real adapter path in server.json: nothing here may open hardware.
        cfg.serial = "/nonexistent/test-serial".into();
        cfg.output = output; cfg.campaign_plan = None; cfg
    }
    #[test]
    fn out_of_scope_with_a_valid_pin_is_a_400_that_never_runs_and_keeps_the_binding() {
        let pin = identity();
        let client = sim_runtime::hardware_client::new_client_id();
        let (app, rx) = app_fixture(pin.clone());
        for action in ["jog", "flip", "direction", "gait_start", "gait_update", "lab_step"] {
            let answer = wire_post(app.clone(), &client, json!({"action":action,"id":1}), Some((&pin, 1)));
            assert!(answer.starts_with("HTTP/1.1 400") && !answer.contains(BINDING_REFUSED) && answer.contains(&out_of_scope(action)), "{action}: {answer}");
            assert!(rx.try_recv().is_err(), "{action} never reaches acquisition");
            assert_eq!(app.check_execution(action, &client, Some(&(pin.clone(), 1))), Err(out_of_scope(action)));
        }
        assert_eq!(app.generations.lock().unwrap().get(&client), Some(&1), "the binding is unchanged");
        assert!(app.check_execution("select", &client, Some(&(pin.clone(), 1))).is_ok(), "in-scope commands still pass");
        // A wrong identity is a binding refusal whatever the action.
        let mut replaced = pin.clone(); replaced.bench_instance = sim_runtime::hardware_client::new_client_id();
        assert!(app.check_execution("jog", &client, Some(&(replaced, 1))).is_err_and(|e| e.starts_with(BINDING_REFUSED)));
        assert!(app.check_execution("jog", &client, Some(&(pin, 2))).is_err_and(|e| e.starts_with(BINDING_REFUSED)), "generation mismatch first");
    }
    #[test]
    fn session_binding_follows_the_live_coordinate_session() {
        let mut a = AxisCalibration::default();
        assert!(!bound_to_session(&a, Some("1")), "no multi-turn poses");
        a.coordinate_session = Some("1".into());
        assert!(bound_to_session(&a, Some("1")));
        assert!(!bound_to_session(&a, Some("2")), "already unusable after a re-stamp; no second re-stamp");
        assert!(usable(&AxisCalibration { lower: Some(5000), ..a.clone() }, Some("2")).lower.is_none(), "a re-stamp drops the stale poses");
        a.coordinate_session = None;
        a.reference_session = Some("2".into());
        assert!(bound_to_session(&a, Some("2")));
        assert!(!bound_to_session(&a, None));
    }
    #[test]
    fn stop_outcome_separates_absent_disabled_motors_from_unverified_torque_off() {
        const WRAP: &str = "Stop readback unverified after bounded retries: ";
        let timeout = |id: u8| format!("ID {id}: serial reply timeout (0 reply bytes received)");
        // (id, disabled, error, reset_turns, failure, disconnect, link_lost)
        let cases = [
            (1, false, format!("{WRAP}{}", timeout(1)), true, true, true, true),
            (1, false, format!("{WRAP}Stop not verified: cut motor supply power"), false, true, false, false),
            (1, false, format!("{WRAP}ID 1: device error 32"), false, true, false, false),
            (1, false, format!("{WRAP}bad checksum"), true, true, true, true),
            (1, false, "Device not configured (os error 6)".into(), true, true, true, false),
            (1, false, "Broken pipe (os error 32)".into(), false, true, false, true),
            // The one note: the disabled motor itself never answered.
            (2, true, timeout(2), true, false, false, false),
            (2, true, timeout(254), true, true, true, true),
            (2, true, timeout(3), true, true, true, true),
            (2, true, "Stop not verified: cut motor supply power".into(), false, true, false, false),
            (2, true, "ID 2: device error 32".into(), false, true, false, false),
            (2, true, "Broken pipe (os error 32)".into(), false, true, false, true),
            (2, true, "Device not configured (os error 6)".into(), true, true, true, false),
            (2, true, "Ambiguous half-turn encoder jump; reference must be re-established".into(), true, true, true, true),
            (2, true, "STOP sent; receive stream fault prevents physical verification. Cut motor power, then reconnect.".into(), true, true, true, true),
        ];
        for (id, disabled, error, reset_turns, failure, disconnect, link_lost) in cases {
            assert_eq!(stop_outcome(id, disabled, &error), StopOutcome { reset_turns, failure, disconnect, link_lost }, "{id} {disabled} {error}");
        }
    }
    #[test]
    fn device_loss_and_corrupt_frames_count_as_lost_readback() {
        // Exact texts from servo_bus.rs, calibration.rs and actuator_sweep.rs.
        let corrupt = ["bad framing or length", "foreign reply ID", "bad checksum", "unexpected payload width",
            "Invalid bridge stream frame header or length", "Invalid bridge stream frame checksum",
            "Ambiguous half-turn encoder jump; reference must be re-established", "ambiguous encoder wrap: observation gap exceeds speed bound"];
        for e in corrupt {
            assert!(readback_lost(e) && transport_lost(e), "{e}: turns reset, a virtual bus is dropped");
        }
        for e in ["Device not configured (os error 6)", "No such device or address (os error 6)", "Input/output error (os error 5)"] {
            assert!(readback_lost(e) && !transport_lost(e), "{e}: turns reset; never drops a virtual socket");
        }
        for e in ["Unknown motor ID", "ID 1: device error 32", "No space left on device (os error 28)", "Permission denied (os error 13)"] {
            assert!(!readback_lost(e), "{e}");
        }
    }
    #[test]
    fn virtual_export_is_labelled_and_physical_export_unchanged() {
        let (app, _rx) = app_fixture(identity());
        let doc = export_document(&app);
        assert_eq!(doc["execution"], serde_json::to_value(&app.execution).unwrap(), "the identity the viewer pins");
        assert_eq!(doc["simulated"], json!(true));
        assert_eq!(doc["axes"], app.state.lock().unwrap()["calibration"]["axes"]);
        let (app, _rx) = app_fixture(physical());
        assert_eq!(export_document(&app), app.state.lock().unwrap()["calibration"], "no new keys on a physical export");
    }
    /// A bench on a Unix socket (never a serial device): completes the
    /// handshake, then answers each request frame with `respond`.
    fn fake_bench(respond: fn(&[u8]) -> Option<Vec<u8>>) -> (CalibrationBus, std::thread::JoinHandle<()>, PathBuf) {
        use std::os::unix::net::UnixListener;
        // Short absolute base: macOS $TMPDIR exceeds the 104-byte sun_path limit.
        let dir = PathBuf::from(format!("/tmp/fb-{}-{}", std::process::id(), &sim_runtime::hardware_client::new_client_id()[..8]));
        fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("bench.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let instance = sim_runtime::hardware_client::new_client_id();
        let peer_instance = instance.clone();
        let peer = std::thread::spawn(move || {
            let mut peer = listener.accept().unwrap().0;
            peer.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            let mut greeting = [0; 25];
            peer.read_exact(&mut greeting).unwrap();
            assert_eq!(&greeting, b"HX-VIRTUAL-CALIBRATION/1\n");
            writeln!(peer, "{}", json!({"schema_version":1,"kind":"virtual_calibration","bench_instance":peer_instance})).unwrap();
            // Stay connected until the client closes, like the real bench.
            let (mut pending, mut buf) = (Vec::new(), [0u8; 256]);
            loop {
                match peer.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => pending.extend_from_slice(&buf[..n]),
                }
                while pending.len() >= 4 && pending.len() >= pending[3] as usize + 4 {
                    let total = pending[3] as usize + 4;
                    let frame: Vec<u8> = pending.drain(..total).collect();
                    if let Some(reply) = respond(&frame) {
                        if peer.write_all(&reply).is_err() { return; }
                    }
                }
            }
        });
        let (bus, opened) = CalibrationBus::open_virtual(&socket, Some(&instance), &dir.join("serial.jsonl")).unwrap();
        assert_eq!(opened, instance);
        (bus, peer, dir)
    }
    /// The FPGA (ID 254) answers every request with a valid calibration
    /// profile status; motors never answer.
    fn fpga_reply(frame: &[u8]) -> Option<Vec<u8>> {
        (frame[2] == 254).then(|| sim_runtime::acquisition::servo_bus::packet(254, 0, &[5, 0, 0, 0, 0, 0, 0, 0, 60, 90, 126, 208, 7]).unwrap())
    }
    fn run_observe_stop(app: &App, cfg: &Config, cal: &Calibration, bus: &mut Option<CalibrationBus>) {
        let handled = *app.safety.lock().unwrap();
        app.latch_stop();
        let (mut selected, mut verified, mut owner) = (1u8, true, "owner".to_string());
        assert_eq!(observe_stop(app, cfg, cal, bus, handled, &mut selected, &mut verified, &mut owner), handled + 1);
        assert!(selected == 0 && !verified && owner.is_empty(), "STOP clears ownership");
    }
    #[test]
    fn stop_readback_loss_restamps_a_bound_session_and_disabled_absent_motors_are_notes() {
        // FPGA answers; no motor ever does. Physical execution, so the bus is
        // kept and every axis is attempted (about 1.2 s of reply timeouts).
        let (bus, peer, dir) = fake_bench(fpga_reply);
        let (app, _rx) = app_fixture(physical());
        { let mut s = app.state.lock().unwrap(); s["coordinate_session"] = json!("S"); s["connected"] = json!(true); }
        let cfg = config_fixture(dir.clone());
        let mut cal = Calibration::default();
        cal.axes.insert(1, AxisCalibration { role: "knee".into(), lower: Some(100), coordinate_session: Some("S".into()), ..Default::default() });
        for id in [2, 3] {
            cal.axes.insert(id, AxisCalibration { role: cfg.roles[&id].clone(), disabled: true, ..Default::default() });
        }
        let mut bus = Some(bus);
        run_observe_stop(&app, &cfg, &cal, &mut bus);
        let s = app.state.lock().unwrap().clone();
        assert!(bus.is_some(), "physical keeps its bus");
        assert_ne!(s["coordinate_session"], json!("S"), "knee's turns were reset, so its bound poses are invalidated");
        assert_eq!(s["connected"], json!(false));
        let error = s["error"].as_str().unwrap();
        assert!(error.starts_with("knee: Stop readback unverified after bounded retries: ID 1: serial reply timeout") && !error.contains("worm"), "{error}");
        let message = s["message"].as_str().unwrap();
        assert!(message.contains("torque-off readback unverified") && message.contains("Note: disabled worm (ID 2)")
            && message.contains("disabled belt/hip (ID 3)"), "{message}");
        drop(bus);
        peer.join().unwrap();
        fs::remove_dir_all(dir).ok();
    }
    #[test]
    fn disabled_axis_that_answers_but_fails_to_stop_is_an_unverified_torque_off() {
        // The FPGA answers; disabled motor 2 answers its readback with a device error.
        let (bus, peer, dir) = fake_bench(|frame| fpga_reply(frame).or_else(||
            (frame[2] == 2).then(|| sim_runtime::acquisition::servo_bus::packet(2, 0x20, &[0; 15]).unwrap())));
        let (app, _rx) = app_fixture(physical());
        { let mut s = app.state.lock().unwrap(); s["coordinate_session"] = json!("S"); s["connected"] = json!(true); }
        let mut cfg = config_fixture(dir.clone());
        cfg.roles.retain(|id, _| *id == 2);
        let mut cal = Calibration::default();
        cal.axes.insert(2, AxisCalibration { role: "worm".into(), disabled: true, coordinate_session: Some("S".into()), ..Default::default() });
        let mut bus = Some(bus);
        run_observe_stop(&app, &cfg, &cal, &mut bus);
        let s = app.state.lock().unwrap().clone();
        assert_eq!(s["error"], json!("worm: ID 2: device error 32"));
        assert!(s["message"].as_str().unwrap().contains("torque-off readback unverified") && !s["message"].as_str().unwrap().contains("Note:"));
        assert_eq!(s["coordinate_session"], json!("S"), "it answered, so its turns were kept");
        assert_eq!(s["connected"], json!(true));
        drop(bus);
        peer.join().unwrap();
        fs::remove_dir_all(dir).ok();
    }
    #[test]
    fn job_captured_before_a_stop_is_refused_and_never_runs() {
        // No bus is open (None), so the worker's torque-off touches no device.
        let output = std::env::temp_dir().join(format!("stale-stop-{}-{}", std::process::id(), stamp()));
        let (app, rx) = app_fixture(physical());
        let epoch = *app.safety.lock().unwrap();
        let worker_app = app.clone();
        let cfg = config_fixture(output.clone());
        // The worker thread parks on its queue afterwards (App holds the sender).
        std::thread::spawn(move || worker(worker_app, rx, cfg, None));
        let (reply, answer) = mpsc::channel();
        app.latch_stop();
        app.jobs.send(Job { queued:Instant::now(), execution:None, stop_epoch:epoch,
            request:serde_json::from_value(json!({"action":"inspect"})).unwrap(), client:sim_runtime::hardware_client::new_client_id(), reply }).unwrap();
        let refused = answer.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(refused, Err(STOP_INTERRUPTED.to_string()));
        assert!(app.stop.load(Ordering::SeqCst) && app.state.lock().unwrap()["enabled_id"].is_null());
        assert!(app.arm(epoch, true).is_err(), "even an explicit rearm captured before STOP is refused");
        let (reply, answer) = mpsc::channel();
        app.jobs.send(Job { queued:Instant::now().checked_sub(HTTP_WAIT + Duration::from_secs(1)).unwrap(), execution:None, stop_epoch:*app.safety.lock().unwrap(),
            request:serde_json::from_value(json!({"action":"inspect"})).unwrap(), client:sim_runtime::hardware_client::new_client_id(), reply }).unwrap();
        assert_eq!(answer.recv_timeout(Duration::from_secs(3)).unwrap(), Err("Command expired before execution; nothing ran".to_string()));
        fs::remove_dir_all(&output).ok();
    }
    #[test]
    fn socket_failures_count_as_lost_links_and_virtual_never_opens_serial() {
        for lost in ["Broken pipe (os error 32)", "Connection reset by peer (os error 54)", "ID 1: serial reply timeout (0 reply bytes received)",
            "Stop readback unverified after bounded retries: ID 2: serial reply timeout (0 reply bytes received)"] {
            assert!(transport_lost(lost), "{lost}");
        }
        let binding = format!("{BINDING_REFUSED}: Virtual bench disconnected; restart and reconnect explicitly");
        for kept in ["Unknown motor ID", "Stop readback unverified after bounded retries: Stop not verified: cut motor supply power", binding.as_str(),
            "No space left on device (os error 28)", "Read-only file system (os error 30)", "Permission denied (os error 13)"] {
            assert!(!transport_lost(kept), "{kept}");
        }
        let (app, _rx) = app_fixture(identity());
        let cfg = config_fixture(std::env::temp_dir().join("never-opened"));
        assert!(app.open_bus(&cfg).is_err_and(|e| e.starts_with(BINDING_REFUSED)), "virtual execution without a socket refuses; no serial fallback");
    }
    #[test]
    fn campaign_refused_by_stop_creates_nothing_and_resume_ignores_empty_directories() {
        let root = std::env::temp_dir().join(format!("campaign-arm-{}-{}", std::process::id(), stamp())).join("campaigns");
        let (app, _rx) = app_fixture(physical());
        let epoch = *app.safety.lock().unwrap();
        app.latch_stop();
        assert_eq!(campaign_directory(&app, &root, false, epoch).unwrap_err(), STOP_INTERRUPTED);
        assert!(!root.exists(), "a refused campaign writes no directory");
        // An interrupted campaign with a receipt, then a newer empty one.
        let receipt = json!({"stage":"breakaway","id":1,"completed":true,"metrics":null,"samples":0,"simulated_s":0.0});
        fs::create_dir_all(root.join("campaign-1/receipts")).unwrap();
        fs::write(root.join("campaign-1/receipts/001-breakaway-1.json"), receipt.to_string()).unwrap();
        fs::create_dir_all(root.join("campaign-2/receipts")).unwrap();
        let epoch = *app.safety.lock().unwrap();
        app.arm(epoch, true).unwrap(); // the operator selects again after STOP
        let (dir, receipts) = campaign_directory(&app, &root, true, epoch).unwrap();
        assert_eq!(dir, root.join("campaign-1"));
        assert_eq!(receipts.len(), 1);
        assert!(root.join("campaign-2").exists(), "existing campaign directories are never deleted");
        fs::remove_dir_all(root.parent().unwrap()).ok();
    }

}

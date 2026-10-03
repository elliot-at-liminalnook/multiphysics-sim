//! Shared calibration application: serialized acquisition, durable records and
//! independently accessible STOP, cancellation and motion leases. No worker
//! thread, process or transport is created by this module.
use crate::acquisition::{
    calibration::{AxisCalibration, Calibration},
    calibration_serial::CalibrationBus,
    calibration_sweep::{DriveMode, MotionCommand, SweepInput, SweepTuning},
};
use crate::hardware::protocol::calibration::{
    BINDING_REFUSED, ExecutionIdentity, virtual_command_allowed,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
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
const STOP_LATCHED_MESSAGE: &str = "STOP latched; torque-off requested for all configured axes. Await release readback. Records retained.";
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub serial: String,
    #[serde(default)]
    pub viewer: PathBuf,
    pub output: PathBuf,
    pub fixture: String,
    pub roles: std::collections::BTreeMap<u8, String>,
    pub sweep_tuning: SweepTuning,
    /// Characterization campaign plan (PLAN.md); axes come from taught poses.
    #[serde(default)]
    pub campaign_plan: Option<PathBuf>,
    /// Explicit local simulation, never inferred from a serial URL.
    #[serde(default)]
    pub virtual_bench: bool,
    /// Stable simulated device name used for output provenance and exclusivity.
    #[serde(default)]
    pub bench_instance: String,
}
fn default_drive() -> u16 {
    25
}
fn nullable_motor_id<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
    Ok(Option::<u8>::deserialize(d)?.unwrap_or(0))
}
#[derive(Deserialize, Serialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub action: String,
    #[serde(default, deserialize_with = "nullable_motor_id")]
    pub id: u8,
    #[serde(default)]
    pub delta: i16,
    #[serde(default = "default_drive")]
    pub drive_pwm: u16,
    #[serde(default)]
    pub boundary: String,
    #[serde(default)]
    pub reverse: bool,
    #[serde(default)]
    pub supported: bool,
    #[serde(default)]
    pub sequence: u64,
    #[serde(default)]
    pub speed_counts_s: f64,
    #[serde(default)]
    pub clearance_counts: u16,
    #[serde(default)]
    pub run_id: u64,
    #[serde(default)]
    pub motion: String,
    #[serde(default)]
    pub target_raw: f64,
    #[serde(default)]
    pub disabled: bool,
    /// Energize the other enabled motors to hold position during a session.
    #[serde(default)]
    pub hold_others: bool,
    /// Servo drive for a session: pwm (host loop), servo_position or servo_speed.
    #[serde(default)]
    pub drive_mode: DriveMode,
    /// Campaign: reuse completed stages of the last interrupted campaign.
    #[serde(default)]
    pub resume: bool,
    /// Gait playback: repository path of a compiled gait (compiled.json).
    #[serde(default)]
    pub gait: String,
    /// Gait playback: display bindings of motors to CAD joints (the leg
    /// mirror's), with the CAD home angle from the same Rust mirror.
    #[serde(default)]
    pub bindings: Vec<GaitBinding>,
    /// Gait playback speed, a fraction of the gait's own timing.
    #[serde(default)]
    pub speed_scale: f64,
    #[serde(default)]
    pub playing: bool,
    /// Gait playback on the leg: fraction of each motor's measured capability.
    #[serde(default)]
    pub effort: f64,
    /// Saving `reference`: the CAD joint angle (rad) the leg mirror shows as the
    /// alignment pose. Omitted means the joint's CAD home.
    #[serde(default)]
    pub reference_joint_rad: Option<f64>,
    /// Lesson lab step: the joint role, open-loop duty and duration.
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub duty: f64,
    #[serde(default)]
    pub seconds: f64,
}
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct GaitBinding {
    pub id: u8,
    pub joint: String,
    pub polarity: f64,
    pub home_rad: f64,
}
/// Browser-held playback controls for a hardware gait session (lease).
struct GaitLease {
    owner: String,
    speed_scale: f64,
    playing: bool,
    last_seen: Instant,
}
/// Legacy compatibility deadline for any caller waiting for the worker reply.
const HTTP_WAIT: Duration = Duration::from_secs(8);
struct Job {
    /// When the caller queued it; older than [`HTTP_WAIT`] at pickup means the
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
    /// Pose to save once settled, awaited by its `capture_hold` request.
    capture: Option<PendingCapture>,
}
/// A `capture_hold` the hold session performs at its next sample of the axis.
/// The session answers `done` with the outcome: `Ok` once the calibration,
/// `capture_message` and the axis's sample are all in the state, or the
/// refusal (still settling, validation, save failure).
struct PendingCapture {
    boundary: String,
    /// The alignment joint angle, for `reference`.
    joint_rad: Option<f64>,
    /// The request's sequence: with the session's run, which capture this is.
    sequence: u64,
    done: mpsc::SyncSender<R<()>>,
}
/// How long `capture_hold` waits for the hold session to take its capture.
/// The motion lease is 1.5 s and a client cannot heartbeat while this
/// request is in flight, so the wait stays well under it.
const CAPTURE_WAIT: Duration = Duration::from_millis(1000);
/// Once the session has taken the capture, how much longer to wait for its
/// outcome (it is computed at once; this only bounds a slow disk).
const CAPTURE_OUTCOME_WAIT: Duration = Duration::from_secs(2);
/// Waits for the hold session's answer to the capture (`run_id`,
/// `sequence`) and returns the state to answer with (the full status, as
/// `capture` answers, so a client can adopt it). A capture the session never
/// took within [`CAPTURE_WAIT`] is withdrawn, so it cannot be saved later.
fn await_capture(app: &App, run_id: u64, sequence: u64, done: &mpsc::Receiver<R<()>>) -> R<Value> {
    let outcome = match done.recv_timeout(CAPTURE_WAIT) {
        Err(mpsc::RecvTimeoutError::Timeout) => {
            {
                let mut lock = app.sweep.lock().unwrap();
                if let Some(ctl) = lock.as_mut()
                    && ctl.run_id == run_id
                    && ctl.capture.as_ref().is_some_and(|c| c.sequence == sequence)
                {
                    ctl.capture = None;
                    return Err(
                        "Pose not saved: the hold session did not take the capture within 1 s"
                            .into(),
                    );
                }
            }
            // Taken (or the session ended, dropping it): its answer is due.
            done.recv_timeout(CAPTURE_OUTCOME_WAIT)
        }
        other => other,
    };
    match outcome {
        Ok(Ok(())) => Ok(app.state.lock().unwrap().clone()),
        Ok(Err(e)) => Err(e),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            // The session ended (dropping the capture); the worker publishes
            // its reason just after, so wait briefly for it before falling
            // back to the current message.
            let deadline = Instant::now() + Duration::from_millis(500);
            let detail = loop {
                let s = app.state.lock().unwrap();
                // This session's own reason only: the top-level `error` may
                // still hold an earlier job's failure.
                let reason = s["sweep"]["motion_error"].as_str().filter(|_| s["sweep"]["run_id"].as_u64() == Some(run_id)).map(str::to_string);
                if reason.is_some() || Instant::now() >= deadline {
                    break reason.or_else(|| s["message"].as_str().map(str::to_string)).unwrap_or_default();
                }
                drop(s);
                std::thread::sleep(Duration::from_millis(20));
            };
            Err(if detail.is_empty() {
                "Pose not saved: the hold session ended before the capture".to_string()
            } else {
                format!("Pose not saved: the hold session ended before the capture ({detail})")
            })
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            Err("Pose save not confirmed: the hold session took the capture but did not report within 3 s; check the saved poses".into())
        }
    }
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
    session.is_some()
        && (a.coordinate_session.as_deref() == session || a.reference_session.as_deref() == session)
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
    [
        "timeout",
        "readback",
        "receive",
        "serial",
        "bad checksum",
        "framing",
        "foreign reply",
        "bridge stream",
        "payload width",
        "ambiguous",
    ]
    .iter()
    .any(|k| e.contains(k))
}
/// OS errors of a vanished serial adapter: ENXIO after a USB unplug ("Device
/// not configured" on macOS, "No such device or address" on Linux) and EIO.
fn device_lost(e: &str) -> bool {
    let e = e.to_ascii_lowercase();
    [
        "device not configured",
        "no such device or address",
        "input/output error",
    ]
    .iter()
    .any(|k| e.contains(k))
}
/// `stop()`'s bounded-retry wrapper always says "readback"; the attempts'
/// own errors after it say whether readback was actually lost.
fn strip_stop_retry(e: &str) -> &str {
    e.strip_prefix("Stop readback unverified after bounded retries: ")
        .unwrap_or(e)
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
    link_readback_lost(e)
        || [
            "broken pipe",
            "connection reset",
            "connection aborted",
            "connection refused",
            "not connected",
            "resource temporarily unavailable",
            "unexpected end of file",
            "failed to fill whole buffer",
        ]
        .iter()
        .any(|k| lower.contains(k))
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
    doc.insert(
        "execution".into(),
        serde_json::to_value(&app.execution).unwrap(),
    );
    doc.insert("simulated".into(), json!(true));
    Value::Object(doc)
}
/// `value` (a JSON object) with the execution that produced it and whether it
/// is simulated, the keys `export_document` adds: gait run records, their log
/// events and the live gait status carry them in both modes (`simulated`
/// false and the physical identity on hardware).
fn labelled(app: &App, mut value: Value) -> Value {
    value["execution"] = json!(app.execution);
    value["simulated"] = json!(app.execution.is_virtual_calibration());
    value
}
/// What a virtual gait run cannot stand for (a gait run record's
/// `virtual_limits`), and the supply its governor limits assumed.
fn virtual_gait_limits(measured_supply: Option<f64>, supply: f64) -> String {
    let supply = match measured_supply {
        Some(v) => format!("the bench's reported supply, {v:.2} V"),
        None => format!("an assumed {supply:.1} V supply (the bench reported no usable voltage)"),
    };
    format!(
        "SIMULATED virtual bench run, not measured hardware: the bench's motor models stand in for the leg. \
        Its supply sag and winding heating are simplified bench models, not this leg's supply or windings; \
        belt slip, backlash, compliance, cable drag and serial/FPGA timing jitter are not emulated. \
        Governor limits come from the accepted physical actuator registry at {supply}; the bench's models are not registry families."
    )
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
    shutdown: AtomicBool,
    handles: std::sync::atomic::AtomicUsize,
    generations: Mutex<std::collections::BTreeMap<String, u64>>,
    safety: Mutex<u64>,
    state: Mutex<Value>,
    jobs: mpsc::SyncSender<Job>,
    stop: AtomicBool,
    cancel: AtomicBool,
    cancel_sequence: AtomicU64,
    sweep: Mutex<Option<BrowserSweep>>,
    gait: Mutex<Option<GaitLease>>,
    sweep_tuning: SweepTuning,
}
impl App {
    fn check_execution(
        &self,
        action: &str,
        client: &str,
        pin: Option<&(ExecutionIdentity, u64)>,
    ) -> R<()> {
        if action == "stop" {
            return Ok(());
        }
        if self.execution.is_virtual_calibration()
            && self.state.lock().unwrap()["execution"].is_null()
        {
            return Err(format!(
                "{BINDING_REFUSED}: Virtual bench disconnected; restart and reconnect explicitly"
            ));
        }
        match pin {
            Some((identity, generation)) => {
                if !self.execution.is_virtual_calibration()
                    || identity != &self.execution
                    || *generation == 0
                    || self.generations.lock().unwrap().get(client) != Some(generation)
                {
                    return Err(format!("{BINDING_REFUSED}: identity/generation mismatch"));
                }
                // The binding holds; refusing this command leaves it intact
                // (an ordinary 400, never the 409 that revokes the client's pin).
                if !virtual_command_allowed(action) {
                    return Err(out_of_scope(action));
                }
            }
            None if self.execution.is_virtual_calibration() => {
                return Err(format!(
                    "{BINDING_REFUSED}: Virtual execution identity required"
                ));
            }
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
        if self.execution.is_virtual_calibration() {
            Err(format!(
                "{BINDING_REFUSED}: virtual acquisition ended; reconnect explicitly"
            ))
        } else {
            CalibrationBus::open(&cfg.serial, &cfg.output.join("serial.jsonl"))
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
    let absent = disabled
        && attempt.starts_with(&format!(
            "ID {id}: serial reply timeout (0 reply bytes received)"
        ));
    StopOutcome {
        reset_turns: lost,
        failure: !absent,
        disconnect: lost && !absent,
        link_lost: transport_lost(error) && !absent,
    }
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
fn observe_stop(
    app: &App,
    cfg: &Config,
    cal: &Calibration,
    bus: &mut Option<CalibrationBus>,
    handled: u64,
    selected: &mut u8,
    verified: &mut bool,
    owner: &mut String,
) -> u64 {
    let epoch = *app.safety.lock().unwrap();
    if epoch == handled {
        return handled;
    }
    let virtual_mode = app.execution.is_virtual_calibration();
    let session = app.state.lock().unwrap()["coordinate_session"]
        .as_str()
        .map(str::to_string);
    let mut failures = Vec::new();
    let mut notes = Vec::new();
    let mut link_lost = false;
    let mut readback_gone = false;
    let mut restamp = false;
    if let Some(b) = bus.as_mut() {
        for id in cfg.roles.keys() {
            let axis = cal.axes.get(id);
            let disabled = axis.is_some_and(|a| a.disabled);
            let result = if disabled {
                b.stop_single_attempt(*id)
            } else {
                b.stop(*id)
            };
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
                        notes.push(format!(
                            "disabled {} (ID {id}): no readback expected ({error})",
                            cfg.roles[id]
                        ));
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
    let notes = if notes.is_empty() {
        String::new()
    } else {
        format!(" Note: {}.", notes.join("; "))
    };
    s["stop_epoch"] = json!(epoch);
    s["release"] = json!({"epoch":epoch,"verified":failures.is_empty() && bus.is_some(),"bus_open":bus.is_some(),"errors":failures,"notes":notes});
    if failures.is_empty() {
        s["message"] = json!(format!(
            "{}{notes}",
            if bus.is_some() {
                "STOP acted on; torque off and stationary readback verified for responding axes. Records retained."
            } else {
                "STOP acted on; no acquisition bus is open. Physical release readback unavailable. Records retained."
            }
        ));
    } else {
        s["error"] = json!(failures.join("; "));
        s["message"] = json!(format!(
            "STOP latched; torque-off readback unverified ({}). Cut motor power. Records retained.{notes}",
            failures.join("; ")
        ));
    }
    epoch
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
                out.push(
                    u8::from_str_radix(
                        std::str::from_utf8(&bytes[i + 1..i + 3]).map_err(|e| e.to_string())?,
                        16,
                    )
                    .map_err(|e| e.to_string())?,
                );
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|e| e.to_string())
}
mod session;
pub use session::{Handle, Service, Worker};

#[cfg(test)]
mod tests;

mod campaign;
mod gait;
mod worker;
use campaign::*;
use gait::*;
use worker::worker;

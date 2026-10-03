//! A `--robot FILE` driven by its external controller (RD3): the controller
//! binding beside the model (`sim_runtime::controller_binding`), loaded on
//! the reload worker, becomes a [`ControlledRun`]; the run thread builds it
//! as `sim_runtime::session::Session` (which starts the Python simloop
//! program and attaches it on the model's `control.external` seam) and
//! feeds it, once per seam period, the twist the shared drive limiter
//! (`kinematics::step`, on sim time) allows plus the request heartbeat
//! ([`TwistState`]). The UI side asks through `RunController::drive` and
//! reads [`DriveStatus`] and `drive_json`.
use serde_json::{Value, json};
use sim_domain_control::drive::kinematics::{self, ACCEL_UNITS, AXIS_NAMES, BodyTwist, Deadman, Limits, SPEED_UNITS};
use sim_domain_robot::PhysicalModel;
use sim_runtime::controller_binding::{COMMAND_CHANNELS, ControlledRobot};
use sim_runtime::session::{ExternalProgram, InputChannel, Scene};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use super::{Phase, RunController};
use super::protocol::Command;

/// What `robot_state.drive` and the recording sidecar call the controller.
pub const CONTROLLER_LABEL: &str = "external controller (Python, simloop)";
/// The seed of every drive session the viewer builds: no seed source exists
/// for a `--robot FILE` (the preset seed comes from presets.json), so it is
/// fixed, recorded in the recording and its sidecar, and replay uses the recorded one.
pub const DRIVE_SEED: u64 = 0;
pub const SEED_RULE: &str = "a --robot FILE drive session is built with seed 0 (no seed source exists for a file); the recording carries its seed and replay rebuilds with the recorded one";
/// Physical FILE runs' fidelity (`state_json.build`), extended by the controller.
pub const DRIVE_FIDELITY: &str = "the --robot FILE's PhysicalModel run by sim_runtime::session::Session: sim_runtime::physical::PhysicalRobot (shared registry, BuildOptions::default()) on the run thread, with the external controller (Python, simloop) attached on the model's control.external seam at the model's control period; the physics is the model file's (provenance as the file declares it; uncalibrated where it says so); the drive limiter and deadman run on sim time on the run thread and the controller applies its own (idempotent) limiter, deadman and wheel mixer";
pub const DEADMAN_RULE: &str = "the deadman counts simulation time since the last fresh drive request (each request raises command.heartbeat by one); a request older than the profile's timeout_s is lost and the stop rule applies (ramp at stop_decel, or immediate). While paused no simulation time passes, so the deadman cannot expire while paused; a held input must keep sending requests (each one is a fresh request) or the robot stops after timeout_s of simulated time. The viewer re-sends a held input once per frame, so at speed scale s and frame rate f a held input is refreshed every s/f simulated seconds: below f = s / timeout_s frames per second (16 fps at ×8 with timeout_s 0.5 s, 2 fps at ×1) the deadman can expire between frames and the robot stutters (stops and restarts) although the input is held. When the run thread is compute-limited it advances less simulated time per frame, which shortens that interval. A replay's recorded requests are not live: when a drive replay ends the request is zero and the deadman counts as expired, so the next Run stops the robot until a fresh request arrives";
pub const DRIVE_RULE: &str = "requests are body twists (forward m/s, lateral m/s, yaw rad/s) checked against the profile (kinematics::check_twist) on the UI thread and again on the run thread; once per seam period the run thread sends the limited twist (kinematics::step: acceleration limit, deadman on sim time) and the heartbeat on the four command channels; halt zeroes the request and the commanded twist at once. A nonzero request moves the robot only while it runs: it is refused while idle, paused, building before Run, failed or ended (it would otherwise wait with no age, since no simulation time passes, and drive the robot at the next Run with no input); a zero request (stop) or a halt is accepted in every phase that is not failed or ended, and one made before the first build is applied right after it (the newest one wins). The run thread applies every queued command in order before each seam period, so the newest request (a release, stop or halt included) governs the next period however far the run thread has fallen behind.";
/// Largest heartbeat the `command.heartbeat` channel carries (2^53, exact in
/// f64): the runtime's own bound on that input (`controller_binding::HEARTBEAT_MAX`), as an integer.
pub const HEARTBEAT_MAX: u64 = sim_runtime::controller_binding::HEARTBEAT_MAX as u64;
/// Why a nonzero drive request is refused while the run is not running ([`DRIVE_RULE`]).
pub const NOT_RUNNING: &str = "drive requests move the robot only while it runs; press Run (the Run button, or REST robot_run start) first (a stop or halt is accepted while paused)";

/// A `--robot FILE` with a loaded controller binding: what the run thread builds.
pub struct ControlledRun {
    /// The loaded model, as the file was parsed (never written).
    pub model: PhysicalModel,
    pub controlled: ControlledRobot,
    /// `controller_binding::scene(model, controlled, DRIVE_DURATION_S)`, built on the loader thread.
    pub scene: Scene,
    /// [`DRIVE_SEED`] (see [`SEED_RULE`]).
    pub seed: u64,
    /// The opened `.simrobot.json`.
    pub model_path: PathBuf,
    /// The workspace root saves resolve against (`crate::workspace::root()`), or why none was found.
    pub root: Result<PathBuf, String>,
}
impl ControlledRun {
    /// Builds the run's scene. Called on the loader thread (`robot::source::check`); no file access.
    pub fn new(model_path: &Path, model: PhysicalModel, controlled: ControlledRobot) -> Self {
        let scene = sim_runtime::controller_binding::scene(model.clone(), &controlled, sim_runtime::controller_binding::DRIVE_DURATION_S);
        let root = crate::workspace::root().map(Path::to_path_buf);
        Self { model, controlled, scene, seed: DRIVE_SEED, model_path: model_path.to_path_buf(), root }
    }
    /// The model file name without `.simrobot.json` (the recordings folder name).
    pub fn stem(&self) -> String {
        let name = self.model_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match name.strip_suffix(".simrobot.json") {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => self.model_path.file_stem().map_or_else(|| "robot".into(), |s| s.to_string_lossy().into_owned()),
        }
    }
    pub fn limits(&self) -> Limits {
        self.controlled.resolved.limits()
    }
    pub fn deadman(&self) -> Deadman {
        self.controlled.resolved.deadman()
    }
    /// The scene's external program (what Session::new starts).
    pub fn external(&self) -> Option<&ExternalProgram> {
        self.scene.controller.as_ref().and_then(|c| c.external.as_ref())
    }
}

/// Session inputs must be exactly the four command channels, in order, or the actions below would be misassigned.
pub(super) fn check_inputs(inputs: &[InputChannel]) -> Result<(), String> {
    if inputs.iter().map(|c| c.name.as_str()).eq(COMMAND_CHANNELS) {
        Ok(())
    } else {
        Err(format!("the drive session's command inputs are [{}]; the run thread sends [{}] (controller_binding::COMMAND_CHANNELS)", inputs.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", "), COMMAND_CHANNELS.join(", ")))
    }
}

/// The run thread's drive state: the latest request, the limited twist it
/// commanded and the heartbeat. Pure (no session), so it is unit-tested alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TwistState {
    pub request: BodyTwist,
    pub commanded: BodyTwist,
    /// Sim time (s) of the last fresh request (0 before any: the zero request at t = 0).
    pub last_request_s: f64,
    /// Fresh requests so far (`command.heartbeat`).
    pub heartbeat: u64,
    /// The last period's deadman verdict.
    pub expired: bool,
    /// The last request was a halt.
    pub halted: bool,
    /// The action last sent: `[forward, lateral, yaw, heartbeat]` (COMMAND_CHANNELS order).
    pub sent: [f64; 4],
    /// Seam periods stepped since the build.
    pub periods: u64,
}
impl Default for TwistState {
    fn default() -> Self {
        Self { request: BodyTwist::ZERO, commanded: BodyTwist::ZERO, last_request_s: 0.0, heartbeat: 0, expired: false, halted: false, sent: [0.0; 4], periods: 0 }
    }
}
impl TwistState {
    /// A fresh request at sim time `now_s` (`Command::Twist`): checked against
    /// the profile, heartbeat + 1; a halt zeroes the request and the commanded twist at once.
    pub fn request(&mut self, request: BodyTwist, halt: bool, now_s: f64, limits: &Limits) -> Result<(), String> {
        kinematics::check_twist(request, limits).map_err(|e| format!("drive request refused: {e}"))?;
        if !now_s.is_finite() {
            return Err(format!("drive request refused: the simulation time {now_s} is not finite"));
        }
        if self.heartbeat >= HEARTBEAT_MAX {
            return Err(format!("drive request refused: command.heartbeat is exhausted at {HEARTBEAT_MAX}; Reset starts a new session"));
        }
        self.heartbeat += 1;
        self.last_request_s = now_s;
        self.halted = halt;
        if halt {
            self.request = BodyTwist::ZERO;
            self.commanded = BodyTwist::ZERO;
        } else {
            self.request = request;
        }
        Ok(())
    }
    /// One seam period starting at sim time `now_s`: the shared rule
    /// (`kinematics::step` with age = now − last request) and the action to
    /// send. The result is clamped into ±max_speed (0 on an unsupported axis):
    /// mathematically a no-op, it removes the rounding of `p + (r − p)` that
    /// could otherwise exceed the session's input bounds by one ulp.
    pub fn advance(&mut self, now_s: f64, period_s: f64, limits: &Limits, deadman: &Deadman) -> Result<[f64; 4], String> {
        let age = now_s - self.last_request_s;
        let c = kinematics::step(self.commanded, self.request, period_s, age, limits, deadman).map_err(|e| format!("drive limiter at t = {now_s:.3} s: {e}"))?;
        let t = c.twist.to_array();
        let bounded: [f64; 3] = std::array::from_fn(|i| if limits.supported[i] { t[i].max(-limits.max_speed[i]).min(limits.max_speed[i]) } else { 0.0 });
        self.commanded = BodyTwist::from_array(bounded);
        self.expired = c.expired;
        self.sent = [bounded[0], bounded[1], bounded[2], self.heartbeat as f64];
        self.periods += 1;
        Ok(self.sent)
    }
    /// A recorded action re-sent by a replay: the state shows what was sent
    /// (the original request is not in the recording, so it is shown as the
    /// commanded twist). Called after the period stepped, with `now_s` the
    /// sim time at the START of that period: a live request is stamped with
    /// the time the next period starts at, so a replayed one is too.
    pub fn replayed(&mut self, action: &[f64], now_s: f64) {
        let at = |i: usize| action.get(i).copied().unwrap_or(0.0);
        let twist = BodyTwist::new(at(0), at(1), at(2));
        let heartbeat = at(3).max(0.0) as u64;
        if heartbeat != self.heartbeat || self.periods == 0 {
            self.last_request_s = now_s;
        }
        (self.request, self.commanded, self.heartbeat, self.halted) = (twist, twist, heartbeat, false);
        self.sent = [at(0), at(1), at(2), at(3)];
        self.periods += 1;
    }
    /// A drive replay ended (finished, failed or cancelled) at sim time
    /// `now_s`: the recorded requests were not live, so nothing may keep
    /// driving after it. The request is zero and the deadman counts as
    /// expired (the last request is stamped 2 × timeout_s before `now_s`, so
    /// the next period's age is past the timeout without relying on rounding
    /// at exactly timeout_s): the next Run ramps the commanded twist to zero
    /// by the stop rule until a fresh request arrives. The heartbeat and
    /// periods are kept (they count what the session received).
    pub fn replay_ended(&mut self, now_s: f64, deadman: &Deadman) {
        self.request = BodyTwist::ZERO;
        self.halted = false;
        self.expired = true;
        self.last_request_s = self.last_request_s.min(now_s - 2.0 * deadman.timeout_s);
    }
    pub fn status(&self, time_s: f64) -> DriveStatus {
        DriveStatus { request: self.request, commanded: self.commanded, heartbeat: self.heartbeat, age_s: time_s - self.last_request_s, expired: self.expired, halted: self.halted, time_s, periods: self.periods }
    }
}

/// What the run thread publishes about the drive, each chunk and after each request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DriveStatus {
    pub request: BodyTwist,
    pub commanded: BodyTwist,
    pub heartbeat: u64,
    /// Sim seconds since the last fresh request.
    pub age_s: f64,
    /// The deadman expired at the last period.
    pub expired: bool,
    pub halted: bool,
    /// Sim time (s) of this status.
    pub time_s: f64,
    pub periods: u64,
}
impl DriveStatus {
    pub fn json(&self) -> Value {
        json!({"request": twist_json(self.request), "commanded": twist_json(self.commanded), "heartbeat": self.heartbeat, "age_s": self.age_s,
            "expired": self.expired, "halted": self.halted, "time_s": self.time_s, "periods": self.periods})
    }
}
/// A twist with its units.
pub fn twist_json(t: BodyTwist) -> Value {
    json!({"forward_m_s": t.forward_m_s, "lateral_m_s": t.lateral_m_s, "yaw_rad_s": t.yaw_rad_s})
}

/// The resolved drive the host handed the controller (the trailing `--drive-json <json>`), parsed.
fn resolved_drive(p: &ExternalProgram) -> Option<Value> {
    let n = p.args.len();
    (n >= 2 && p.args[n - 2] == "--drive-json").then(|| serde_json::from_str::<Value>(&p.args[n - 1]).unwrap_or_else(|_| Value::String(p.args[n - 1].clone())))
}

/// Every difference between a recorded drive scene and the loaded run that
/// makes a replay a different experiment, each named with both values: the
/// controller identity (the shared `controller_binding::identity_of` and
/// `ControllerIdentity::differences`: script, its sha256, the simloop library's
/// sha256, args, profile and its sha256), the language, the resolved drive and the robot model. Empty when they match.
pub(super) fn differences(run: &ControlledRun, recorded: &Scene) -> Vec<String> {
    let mut out = Vec::new();
    let Some(current) = run.external() else {
        return vec!["the loaded run's scene has no external controller (controller_binding::scene did not set controller.external)".into()];
    };
    let Some(program) = recorded.controller.as_ref().filter(|c| c.external.is_some()) else {
        return vec!["the recording's scene has no external controller (scene.controller.external is absent): it is not a drive session of a bound robot".into()];
    };
    match sim_runtime::controller_binding::identity_of(program) {
        Err(e) => out.push(e),
        Ok(None) => out.push("the recording's controller is not an external program".into()),
        Ok(Some(theirs)) => out.extend(theirs.differences(&run.controlled.identity)),
    }
    let theirs = program.external.as_ref().expect("filtered above");
    if theirs.language != current.language {
        out.push(format!("controller language: recorded {:?}, current {:?}", theirs.language, current.language));
    }
    let (a, b) = (resolved_drive(theirs), resolved_drive(current));
    if a != b {
        // Compact: the resolved JSON is long; name the members that differ.
        let keys = |v: &Option<Value>| v.as_ref().and_then(Value::as_object).map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default();
        let mut names: Vec<String> = keys(&a).into_iter().chain(keys(&b)).collect();
        names.sort();
        names.dedup();
        let differing: Vec<String> = names.into_iter().filter(|k| a.as_ref().and_then(|d| d.get(k)) != b.as_ref().and_then(|d| d.get(k))).collect();
        out.push(format!("resolved drive (--drive-json): {} differ", if differing.is_empty() { "the values".to_string() } else { differing.join(", ") }));
    }
    let fp = |m: &PhysicalModel| sim_runtime::physics_context::fingerprint(&serde_json::to_value(m).unwrap_or(Value::Null));
    if fp(&recorded.robot) != fp(&run.model) {
        out.push(format!("robot model: the recorded scene's robot differs from the loaded {}", run.model_path.display()));
    }
    for (what, a, b) in [("seam period_s", recorded.period_s, run.scene.period_s)] {
        if a != b {
            out.push(format!("{what}: recorded {a}, current {b}"));
        }
    }
    out
}

impl RunController {
    /// The loaded controller binding's run (None without a binding, for a failed binding and for presets).
    pub fn controlled(&self) -> Option<&Arc<ControlledRun>> {
        self.controlled.as_ref()
    }
    /// Why the binding beside the model did not load (the run is failed with it; it never falls back to the hold run).
    pub fn binding_error(&self) -> Option<&str> {
        self.unbound.as_deref()
    }
    /// Why a drive request cannot be sent now (`Ok` when it would be sent).
    pub fn check_drive(&self) -> Result<(), String> {
        self.recorded_refusal("a drive request")?;
        if self.controlled.is_none() {
            return Err(match (&self.unbound, &self.preset) {
                (Some(e), _) => format!("drive request refused: the controller binding did not load ({e}); fix it and Reload"),
                (None, Some(p)) => format!("drive requests are for a `--robot FILE` with a controller binding; preset `{}` is driven by its motion requests (robot_input)", p.preset.id),
                (None, None) => "drive requests need a controller binding beside the model (<stem>.controller.json, controller_binding::binding_path_for); this `--robot FILE` runs the file's hold controller (servo-target jog)".into(),
            });
        }
        if let Some(why) = self.replay_block() {
            return Err(format!("drive request refused: {why}"));
        }
        match self.status.phase {
            Phase::Failed => Err(format!("drive request refused: the run failed ({}); Reset rebuilds the session", self.status.error.as_deref().unwrap_or("no error text"))),
            Phase::Ended => Err(format!("drive request refused: {}; Reset starts a new session", self.status.end.as_ref().and_then(|e| e["message"].as_str()).unwrap_or("the run ended"))),
            _ => Ok(()),
        }
    }
    /// Why this request cannot be sent now: [`Self::check_drive`], the
    /// profile (`kinematics::check_twist`), then the run rule ([`DRIVE_RULE`]):
    /// a nonzero request only while the run runs (`running` is the last
    /// Run/Pause sent, and commands are ordered, so the run thread sees Run
    /// before this request); a zero request or a halt in any other phase.
    pub fn check_drive_request(&self, request: BodyTwist, halt: bool) -> Result<(), String> {
        self.check_drive()?;
        let run = self.controlled.as_ref().expect("check_drive refuses without a binding");
        kinematics::check_twist(request, &run.limits()).map_err(|e| format!("drive request refused: {e} (profile {})", run.controlled.profile_path.display()))?;
        if !halt && !request.is_zero() && !self.running {
            // The phase as robot_state.run names it (snake_case).
            let phase = serde_json::to_value(self.status.phase).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            return Err(format!("drive request refused (phase {phase}): {NOT_RUNNING}"));
        }
        Ok(())
    }
    /// The one drive handler (Part D's `RobotAction::Drive`): checked by
    /// [`Self::check_drive_request`], then sent as `Command::Twist`; the run
    /// thread applies it at the current sim time. Refusals are kept for `drive_json`.
    pub fn drive(&mut self, request: BodyTwist, halt: bool) -> Result<(), String> {
        let checked = self.check_drive_request(request, halt);
        if let Err(e) = checked {
            self.twist_refusal = Some(e.clone());
            return Err(e);
        }
        self.thread.send(Command::Twist { request, halt }).map_err(|_| "the run thread has stopped".to_string())?;
        self.twist_refusal = None;
        self.twist_requested = Some((request, halt));
        Ok(())
    }
    /// The run thread's latest drive status (None before a controlled build).
    pub fn drive_state(&self) -> Option<&DriveStatus> {
        self.twist.as_ref()
    }
    /// `robot_state.drive`: the controller, profile, limits with units, kinematics, geometry with provenance, deadman, identity, request/commanded state and errors.
    pub fn drive_json(&self) -> Value {
        let available = self.check_drive().map(|_| ());
        // Whether a nonzero request would be accepted now (DRIVE_RULE: only while running; a stop or halt whenever available).
        let accepts_motion = available.is_ok() && self.running;
        let Some(run) = &self.controlled else {
            return json!({"label": CONTROLLER_LABEL, "bound": false, "available": false, "unavailable_reason": available.err(), "binding_error": self.unbound, "error": self.status.error});
        };
        let c = &run.controlled;
        let r = &c.resolved;
        let axes: Vec<Value> = (0..3).map(|i| json!({"axis": AXIS_NAMES[i], "supported": r.limits.supported[i],
            "max_speed": {"value": r.limits.max_speed[i], "unit": SPEED_UNITS[i]}, "max_accel": {"value": r.limits.max_accel[i], "unit": ACCEL_UNITS[i]},
            "stop_decel": {"value": r.limits.stop_decel[i], "unit": ACCEL_UNITS[i]}})).collect();
        let status = self.twist.as_ref();
        let mut out = json!({"label": CONTROLLER_LABEL, "bound": true, "fidelity": DRIVE_FIDELITY, "available": available.is_ok(), "unavailable_reason": available.err(),
            "binding": c.binding_path, "model": run.model_path, "profile": {"path": c.profile_path, "sha256": r.profile_sha256},
            "kinematics": r.kinematics, "geometry": serde_json::to_value(&r.geometry).unwrap_or(Value::Null), "limits": axes,
            "identity": serde_json::to_value(&c.identity).unwrap_or(Value::Null), "rule": DRIVE_RULE});
        out["deadman"] = json!({"timeout_s": {"value": r.deadman.timeout_s, "unit": "s"}, "on_loss": r.deadman.on_loss, "clock": DEADMAN_RULE,
            "expired": status.map(|s| s.expired), "age_s": status.map(|s| s.age_s)});
        out["session"] = json!({"seed": run.seed, "seed_rule": SEED_RULE, "period_s": run.scene.period_s, "duration_s": run.scene.duration_s, "channels": COMMAND_CHANNELS});
        out["requested"] = json!(self.twist_requested.map(|(t, halt)| json!({"twist": twist_json(t), "halt": halt})));
        out["status"] = json!(status.map(DriveStatus::json));
        out["accepts_motion"] = json!(accepts_motion);
        out["last_refusal"] = json!(self.twist_refusal);
        out["last_apply_error"] = json!(self.twist_error);
        out["error"] = json!(self.status.error);
        out
    }
}

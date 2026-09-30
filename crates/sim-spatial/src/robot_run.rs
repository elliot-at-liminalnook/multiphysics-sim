//! Robot-mode run thread: one worker owns the simulation and advances it in
//! fixed sim-time chunks, paced at most to real time. For `--robot FILE` it is
//! the shared `sim_runtime::physical::PhysicalRobot` (built from a clone of the
//! loaded model with `sim_runtime::registry()` and `BuildOptions::default()`,
//! as sim-app's cad scene does); for a preset (`robot_preset`) it is the shared
//! `EmbeddedEnvironment` (with a task) or `EmbeddedSession` built from the
//! preset's files unchanged. The UI thread only sends commands and applies the
//! published frames; it never builds, advances or locks the simulation.
use bevy::math::{DMat3, DQuat};
use serde::Serialize;
use serde_json::{Value, json};
use sim_domain_robot::PhysicalModel;
use crate::robot_motion::{self, Motion, MotionChannel};
use crate::robot_preset::PresetRun;
use crate::robot_recording::{self, Listed, Saved, Snapshot};
use sim_runtime::session::InputChannel;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// Sim time advanced per chunk (s): sim-app's 0.02 s grid. Step advances
/// exactly one chunk; running advances whole chunks.
pub const CHUNK_S: f64 = 0.02;
/// Wall-clock window over which the real-time factor is measured.
const RTF_WINDOW: Duration = Duration::from_secs(1);
pub const PACING: &str = "paced at most to real time: a chunk starts only when wall time since Run has caught up with sim time (lag beyond one chunk is dropped, never made up faster than real time); rtf = sim seconds / wall seconds over the last ~1 s of running, including pacing sleeps; null when not running";

/// One +/− jog press: rad on a revolute/continuous joint, m on a prismatic one.
pub const JOG_STEP_RAD: f64 = 0.05;
pub const JOG_STEP_M: f64 = 0.005;
pub const JOG_LABEL: &str = "servo target (PD hold from the export), not walking-controller teleop";
pub const JOG_SEMANTICS: &str = "a jog sets PhysicalRobot::set_target on the run thread (index resolved by the robot's joint_names with the joint./slide. prefix stripped, as physical.rs does). While paused it sets the target, which takes effect on the next run or step; before the first build it is queued and applied right after the build; Reset rebuilds from the file, so jogged targets return to the file's control targets. Targets outside the file's limits are refused, never clamped; a joint without limits in the file accepts any finite target (no limit in file). Nothing is written to the file.";

/// A joint the run thread holds a servo target for, as read from the file.
#[derive(Clone, Debug, Serialize)]
pub struct Servo {
    pub joint: String,
    pub unit: &'static str,
    /// The file's `limits`, or None ("no limit in file").
    pub limit: Option<[f64; 2]>,
    /// The file's control target: what a build starts from (physical.rs uses 0 when absent).
    pub file_target: f64,
}
impl Servo {
    pub fn step(&self) -> f64 {
        if self.unit == "m" { JOG_STEP_M } else { JOG_STEP_RAD }
    }
    pub fn limit_text(&self) -> String {
        self.limit.map_or("no limit in file".into(), |[lo, hi]| format!("limits [{lo}, {hi}] {} (file)", self.unit))
    }
}

/// Whether the file gives `joint` a servo target under `BuildOptions::default()`,
/// read from the file alone (works with no build and after a failed one). It
/// mirrors `PhysicalRobot::build`: only a joint driven by a motor whose
/// firmware is not `none` gets a position servo and a target, and a
/// `trajectory` with keyframes overwrites every target each control step.
pub fn servo(model: &PhysicalModel, joint: &str) -> Result<Servo, String> {
    let Some(j) = model.joint(joint) else {
        let known: Vec<&str> = model.joints.iter().filter(|j| servo(model, &j.name).is_ok()).map(|j| j.name.as_str()).collect();
        return Err(format!("unknown joint `{joint}`; joints with a servo target in this file: {}", if known.is_empty() { "none".into() } else { known.join(", ") }));
    };
    let control = &model.control;
    match control.mode.as_str() {
        "hold" => {}
        "trajectory" if control.trajectory.is_empty() => {}
        "trajectory" => return Err(format!("joint `{joint}`: the file's control mode is `trajectory` ({} keyframes), which sets every servo target each control step, so a jog would be overwritten; jogging needs mode `hold`", control.trajectory.len())),
        mode => return Err(format!("joint `{joint}`: the file's control mode `{mode}` is not supported by PhysicalRobot (hold | trajectory)")),
    }
    if matches!(j.kind.as_str(), "fixed") || j.is_loop() {
        return Err(format!("joint `{joint}` has no servo target: it is a `{}` joint with no driven degree of freedom", j.kind));
    }
    let Some(motor) = model.motors.iter().find(|m| m.joint.as_deref() == Some(joint)) else {
        return Err(format!("joint `{joint}` has no servo target: no motor in the file drives it (passive joint)"));
    };
    if motor.firmware.kind == "none" {
        return Err(format!("joint `{joint}` has no servo target: its motor `{}` has firmware `none` (powered directly), so the build makes no position servo for it", motor.name));
    }
    let unit = if j.kind == "prismatic" { "m" } else { "rad" };
    Ok(Servo { joint: joint.into(), unit, limit: j.limits, file_target: control.targets.get(joint).copied().unwrap_or(0.0) })
}

/// A requested target for `servo`: finite and within the file's limits; never clamped.
pub fn check_target(servo: &Servo, target: f64) -> Result<(), String> {
    let joint = &servo.joint;
    if !target.is_finite() {
        return Err(format!("joint `{joint}`: target {target} is not finite"));
    }
    match servo.limit {
        Some([lo, hi]) if target < lo || target > hi => Err(format!("joint `{joint}`: target {target} {} is outside the file's limits [{lo}, {hi}] {}; not clamped", servo.unit, servo.unit)),
        _ => Ok(()),
    }
}

/// Short joint name of a robot port (`joint.hip` → `hip`), as physical.rs strips it.
fn short(port: &str) -> &str {
    port.trim_start_matches("joint.").trim_start_matches("slide.")
}

#[derive(Clone, Copy, Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Nothing built yet; the static assembly pose is shown.
    Idle,
    Building,
    Running,
    Paused,
    /// A build or advance error stopped the run; Reset rebuilds.
    Failed,
    /// A preset reached its horizon, or its episode terminated/truncated
    /// (`run.end` says which and why); Reset rebuilds.
    Ended,
}

/// What the run thread builds: the loaded file, or a preset's parsed inputs.
#[derive(Clone)]
pub enum Source {
    Robot(PhysicalModel),
    Preset(Arc<PresetRun>),
}
impl Source {
    fn chunk_s(&self) -> f64 {
        match self {
            Source::Robot(_) => CHUNK_S,
            Source::Preset(p) => p.chunk_s(),
        }
    }
}

/// One published state of the robot, stamped with its generation.
#[derive(Clone, Debug)]
pub struct Frame {
    pub generation: u64,
    pub time: f64,
    /// Chunks of `CHUNK_S` advanced since the build.
    pub steps: u64,
    /// Nominal solver steps completed (presets; the session's own count).
    pub completed_steps: Option<u64>,
    /// Per link of the loaded model (by index): position of the link frame (at
    /// its com) and orientation, model frame; None when the frame has no pose
    /// of that name.
    pub poses: Vec<Option<([f64; 3], DQuat)>>,
    /// Link names in the simulation's frame that match no loaded link.
    pub unmatched: Vec<String>,
    pub joint_names: Vec<String>,
    pub joint_angles: Vec<f64>,
    /// Snapshot of the robot's servo targets when the frame was taken.
    pub targets: Vec<f64>,
    /// Presets: the held action (the session's input values, in `inputs()`
    /// order) when the frame was taken; empty for `--robot FILE`.
    pub inputs: Vec<f64>,
}
impl Frame {
    /// (target, measured angle) of a joint by its file name, if the robot has a target for it.
    pub fn servo(&self, joint: &str) -> Option<(f64, f64)> {
        let i = self.joint_names.iter().position(|n| short(n) == joint)?;
        Some((*self.targets.get(i)?, *self.joint_angles.get(i)?))
    }
}

/// The UI keeps a frame only if it belongs to the current generation.
pub fn accept(current_generation: u64, frame: &Frame) -> bool {
    frame.generation >= current_generation
}

#[derive(Clone, Debug)]
struct Status {
    phase: Phase,
    generation: u64,
    rtf: Option<f64>,
    error: Option<String>,
    /// Why a preset run ended (phase `ended`).
    end: Option<Value>,
}

/// What the worker publishes: its status and its latest frame.
struct Published {
    status: Status,
    frame: Option<Frame>,
    /// Why the last jog could not be applied on the run thread (cleared by the next applied jog or a reset).
    jog_error: Option<String>,
    /// A preset's typed inputs and motion config, resolved at build (None before a build and after Reset).
    drive: Option<Arc<Drive>>,
    /// Why the last motion request could not be applied on the run thread.
    motion_error: Option<String>,
    /// The last finished save (its request number, and the pair written or why not).
    save: Option<(u64, Result<Saved, String>)>,
    /// The latest replay state (stamped with its request number and generation).
    replay: Option<ReplayState>,
    /// The last finished listing of saved recordings (its request number, and the list or why not).
    listing: Option<(u64, Result<Vec<Listed>, String>)>,
}

#[derive(Clone, Copy, Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplayPhase {
    Idle,
    Replaying,
    /// Stopped between chunks by Cancel: never a verdict.
    Cancelled,
    /// The shared runtime's replay checks passed (robot_recording::VERDICT_RULE).
    Done,
    /// Refused before replacing the run, or the runtime reported an error.
    Failed,
}

/// `robot_state.replay`: one replay request and what the run thread made of it.
#[derive(Clone, Debug, Serialize)]
pub struct ReplayState {
    /// Request number (0 before any replay).
    pub seq: u64,
    /// Generation of the replayed run (a refused replay republishes the current run under it).
    pub generation: u64,
    pub path: Option<std::path::PathBuf>,
    pub phase: ReplayPhase,
    /// Progress in `unit`: returned actions (environment) or nominal steps (session).
    pub completed: u64,
    pub total: Option<u64>,
    pub unit: Option<&'static str>,
    /// The replayed runtime's own completed_steps, and the recording's.
    pub completed_steps: Option<u64>,
    pub recorded_completed_steps: Option<u64>,
    pub recorded_failure: Option<String>,
    pub kind: Option<String>,
    pub verdict: Option<String>,
    pub error: Option<String>,
    /// Whether the current run is this replay's simulation (false when refused before replacing it).
    pub replaced: bool,
    pub cancel_requested: bool,
    /// robot_recording::MEASURED_RULE (null without a sidecar final_frame).
    pub measured: Option<Value>,
    /// The sidecar's preset id and saved time, when it exists.
    pub sidecar: Option<Value>,
    /// Wall time from the prepared replay to its end (s).
    pub wall_s: Option<f64>,
}
impl ReplayState {
    fn new(seq: u64, generation: u64, path: Option<std::path::PathBuf>, phase: ReplayPhase) -> Self {
        Self { seq, generation, path, phase, completed: 0, total: None, unit: None, completed_steps: None, recorded_completed_steps: None, recorded_failure: None,
            kind: None, verdict: None, error: None, replaced: false, cancel_requested: false, measured: None, sidecar: None, wall_s: None }
    }
    fn file(&self) -> String {
        self.path.as_ref().and_then(|p| p.file_name()).map_or("the recording".into(), |n| n.to_string_lossy().into_owned())
    }
    fn progress(&self) -> String {
        format!("{}/{} {}", self.completed, self.total.map_or("?".into(), |t| t.to_string()), self.unit.unwrap_or("units"))
    }
}

/// A built preset's typed input channels (the session's `inputs()`), its
/// motion config (`robot_motion::config`) and declared packet heartbeat.
#[derive(Debug)]
pub struct Drive {
    pub inputs: Vec<InputChannel>,
    pub motion: Option<Motion>,
    pub heartbeat: Option<MotionChannel>,
}
impl Drive {
    /// As the browser does on load: an invalid declaration fails the build, naming it.
    fn resolve(run: &PresetRun, policy_contract: &Value, inputs: &[InputChannel]) -> Result<Self, String> {
        let entry = &run.preset.entry;
        Ok(Self { inputs: inputs.to_vec(), motion: robot_motion::config(entry, policy_contract, inputs)?, heartbeat: robot_motion::heartbeat(entry, inputs)? })
    }
}

/// A motion request: the one handler behind physical W/A/S/D/X, the
/// `system_ui` motion:* controls and inspector buttons, and REST `robot_input`.
#[derive(Clone, Debug, Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MotionRequest {
    /// One key latched (system_ui / REST / inspector button) until Stop, another key or a channel request.
    Key(char),
    /// The physically held keys (press/release); empty requests zero.
    HeldKeys(Vec<char>),
    /// Every motion channel to zero.
    Stop,
    /// Explicit values by motion channel name, over the current request.
    Channels(std::collections::BTreeMap<String, f64>),
}

enum Command {
    Start,
    Pause,
    Step,
    Reset { generation: u64 },
    Jog { joint: String, target: f64 },
    /// Validated motion values for the three motion channels (in `Motion::channels` order).
    Motion { values: [f64; 3] },
    /// Snapshot the shared recording and write it (on a writer thread) to `target`.
    SaveRecording { seq: u64, target: std::path::PathBuf, note: Option<String>, unix_ms: u128 },
    /// Read `path`, prepare it through the shared prepare_replay and advance it in chunks under `generation`.
    Replay { generation: u64, seq: u64, path: std::path::PathBuf },
    /// Stop the replay between chunks.
    CancelReplay,
    /// Tests only: set the whole held action through `Sim::set_action` (the motion handler's setter), for presets without a motion config.
    #[cfg(test)]
    SetInputs(Vec<f64>),
}

/// Run actions shared by the buttons, `system_ui` and REST `robot_run`.
#[derive(Clone, Copy, Serialize, serde::Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunAction {
    Start,
    Pause,
    Step,
    Reset,
}
impl RunAction {
    pub const ALL: [RunAction; 4] = [RunAction::Start, RunAction::Pause, RunAction::Step, RunAction::Reset];
    pub fn name(self) -> &'static str {
        match self {
            RunAction::Start => "start",
            RunAction::Pause => "pause",
            RunAction::Step => "step",
            RunAction::Reset => "reset",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            RunAction::Start => "Run",
            RunAction::Pause => "Pause",
            RunAction::Step => "Step",
            RunAction::Reset => "Reset",
        }
    }
    pub fn parse(name: &str) -> Result<Self, String> {
        Self::ALL.into_iter().find(|a| a.name() == name).ok_or_else(|| {
            let valid: Vec<&str> = Self::ALL.iter().map(|a| a.name()).collect();
            format!("unknown robot_run action `{name}`; valid actions: {}", valid.join(", "))
        })
    }
}

/// The UI side of the run thread: sends commands, accepts frames.
pub struct RunController {
    tx: mpsc::Sender<Command>,
    shared: Arc<Mutex<Published>>,
    /// Generation the UI expects; frames from older generations are discarded.
    generation: u64,
    /// Whether the UI last asked the robot to run (commands are ordered).
    running: bool,
    frame: Option<Frame>,
    status: Status,
    /// The loaded model, for file-based jog validation (never written).
    model: PhysicalModel,
    /// Targets requested by jogs in this generation (cleared by Reset).
    jogged: std::collections::BTreeMap<String, f64>,
    jog_error: Option<String>,
    /// The preset this controller runs (None for `--robot FILE`).
    preset: Option<Arc<PresetRun>>,
    chunk_s: f64,
    drive: Option<Arc<Drive>>,
    /// The motion values last sent in this generation (cleared by Reset).
    requested: Option<[f64; 3]>,
    /// The keys behind `requested`, and whether they are physically held (press/release) or latched.
    keys: Vec<char>,
    keys_physical: bool,
    motion_refusal: Option<String>,
    motion_error: Option<String>,
    /// Saves requested and finished (request numbers), the pending target, the last pair written and the last save error.
    save_requested: u64,
    save_done: u64,
    saving: Option<std::path::PathBuf>,
    saved: Option<Saved>,
    save_error: Option<String>,
    /// The latest replay state (local until the run thread publishes a newer one).
    replay: ReplayState,
    /// Recording listings requested and finished, and the last list or why not.
    list_requested: u64,
    list_done: u64,
    recordings: Vec<Listed>,
    list_error: Option<String>,
}

impl RunController {
    /// Spawns the (idle) run thread with its own clone of the loaded model.
    pub fn spawn(model: PhysicalModel) -> Self {
        Self::spawn_source(Source::Robot(model.clone()), model, None)
    }
    /// Spawns the (idle) run thread for a preset; `scene.robot` is the model
    /// the links and inspector show. Nothing is built until Run or Step.
    pub fn spawn_preset(run: Arc<PresetRun>) -> Self {
        let model = run.scene.robot.clone();
        let mut c = Self::spawn_source(Source::Preset(run.clone()), model, Some(run));
        c.refresh_recordings();
        c
    }
    fn spawn_source(source: Source, model: PhysicalModel, preset: Option<Arc<PresetRun>>) -> Self {
        let (tx, rx) = mpsc::channel();
        let status = Status { phase: Phase::Idle, generation: 0, rtf: None, error: None, end: None };
        let shared = Arc::new(Mutex::new(Published { status: status.clone(), frame: None, jog_error: None, drive: None, motion_error: None, save: None, replay: None, listing: None }));
        let out = shared.clone();
        let chunk_s = source.chunk_s();
        let links: Vec<String> = model.links.iter().map(|l| l.name.clone()).collect();
        std::thread::Builder::new()
            .name("robot-run".into())
            .spawn(move || worker(source, links, rx, out))
            .expect("spawn robot run thread");
        Self { tx, shared, generation: 0, running: false, frame: None, status, model, jogged: Default::default(), jog_error: None, preset, chunk_s,
            drive: None, requested: None, keys: Vec::new(), keys_physical: false, motion_refusal: None, motion_error: None,
            save_requested: 0, save_done: 0, saving: None, saved: None, save_error: None,
            replay: ReplayState::new(0, 0, None, ReplayPhase::Idle), list_requested: 0, list_done: 0, recordings: Vec::new(), list_error: None }
    }
    pub fn preset(&self) -> Option<&Arc<PresetRun>> {
        self.preset.as_ref()
    }
    pub fn chunk_s(&self) -> f64 {
        self.chunk_s
    }
    pub fn end(&self) -> Option<&Value> {
        self.status.end.as_ref()
    }

    /// The target a jog step starts from: this generation's last requested
    /// target, else the latest accepted frame's, else the file's control target.
    pub fn requested_target(&self, servo: &Servo) -> f64 {
        self.jogged.get(&servo.joint).copied().or_else(|| self.frame.as_ref().and_then(|f| f.servo(&servo.joint)).map(|(t, _)| t)).unwrap_or(servo.file_target)
    }

    /// Why a jog of `joint` to `target` is refused: file validation first
    /// (unknown joint, no servo target, non-finite, outside the file's
    /// limits), then run state. Works idle and after a failed build.
    pub fn check_jog(&self, joint: &str, target: f64) -> Result<Servo, String> {
        if let Some(p) = &self.preset {
            return Err(format!("joint `{joint}`: servo-target jog is for `--robot FILE`; preset `{}` is driven by its declared controller recipe ({}), so the viewer sets no joint target", p.preset.id, p.kind()));
        }
        let servo = servo(&self.model, joint)?;
        check_target(&servo, target)?;
        if self.status.phase == Phase::Failed {
            return Err(format!("joint `{joint}`: the run failed; Reset rebuilds the robot before it can be jogged"));
        }
        Ok(servo)
    }

    /// The one jog handler behind the +/− buttons, `system_ui` jog:* and REST `robot_jog`.
    pub fn jog(&mut self, joint: &str, target: f64) -> Result<(), String> {
        self.check_jog(joint, target)?;
        self.jogged.insert(joint.to_string(), target);
        self.tx.send(Command::Jog { joint: joint.to_string(), target }).map_err(|_| "the run thread has stopped".to_string())
    }

    /// The built preset's typed inputs and motion config (None before a build).
    pub fn drive(&self) -> Option<&Arc<Drive>> {
        self.drive.as_ref()
    }
    /// Whether physical motion keys are read: a built preset with a motion config.
    pub fn motion_keys_active(&self) -> bool {
        self.drive.as_ref().is_some_and(|d| d.motion.is_some()) && !matches!(self.status.phase, Phase::Failed | Phase::Ended)
    }
    pub fn keys_physical(&self) -> bool {
        self.keys_physical
    }
    /// The held value of session input `index` in the latest accepted frame.
    fn held(&self, index: usize) -> Option<f64> {
        self.frame.as_ref().and_then(|f| f.inputs.get(index).copied())
    }

    /// Why a motion request cannot be sent now, else the built drive and its motion config.
    fn check_motion(&self) -> Result<(Arc<Drive>, &Motion), String> {
        let Some(p) = &self.preset else {
            return Err("motion requests are for robot presets (their declared Rust controller); `--robot FILE` has servo-target jog".into());
        };
        let id = &p.preset.id;
        if let Some(why) = self.replay_block() {
            return Err(format!("preset `{id}`: motion request refused: {why}"));
        }
        match self.status.phase {
            Phase::Failed => return Err(format!("preset `{id}`: the run failed; Reset rebuilds the session before motion requests")),
            Phase::Ended => return Err(format!("preset `{id}`: {}; Reset rebuilds at t = 0 before motion requests", self.status.end.as_ref().and_then(|e| e["message"].as_str()).unwrap_or("the run ended"))),
            _ => {}
        }
        let drive = self.drive.as_ref().ok_or_else(|| format!("preset `{id}`: no built session yet; Run or Step builds {} before motion requests", p.kind()))?;
        let motion = drive.motion.as_ref().ok_or_else(|| format!("preset `{id}` has no motion config: its session's policy_contract has no step_reference and presets.json declares no motion_commands"))?;
        if let Some(h) = &drive.heartbeat {
            let x = self.held(h.index).unwrap_or(f64::NAN);
            if !(x < h.upper) {
                return Err(format!("preset `{id}`: motion packet sequence `{}` is exhausted at {x} (upper {}); reset the session", h.name, h.upper));
            }
        }
        Ok((drive.clone(), motion))
    }
    /// Validates `request` against the session's typed channel bounds, without sending it.
    fn motion_values(&self, request: &MotionRequest) -> Result<[f64; 3], String> {
        let (drive, motion) = self.check_motion()?;
        let current = self.requested.unwrap_or_else(|| std::array::from_fn(|i| self.held(motion.channels[i].index).unwrap_or(0.0)));
        match request {
            MotionRequest::Key(k) => motion.keys(&[*k]),
            MotionRequest::HeldKeys(keys) => motion.keys(keys),
            MotionRequest::Stop => motion.stop(),
            MotionRequest::Channels(map) if map.is_empty() => Err("robot_input needs at least one channel value, a key, or stop".into()),
            MotionRequest::Channels(map) => motion.values(current, &map.iter().map(|(k, v)| (k.clone(), *v)).collect::<Vec<_>>(), &drive.inputs),
        }
    }
    /// Why a motion request is refused now (`Ok` when it would be sent).
    pub fn check_motion_request(&self, request: &MotionRequest) -> Result<(), String> {
        self.motion_values(request).map(|_| ())
    }
    /// The one motion handler behind physical keys, `system_ui` motion:*, the
    /// inspector buttons and REST `robot_input`. Refusals name the channel and
    /// its bounds and are kept for `robot_state.motion.last_refusal`.
    pub fn motion(&mut self, request: MotionRequest) -> Result<(), String> {
        let values = match self.motion_values(&request) {
            Ok(v) => v,
            Err(e) => {
                self.motion_refusal = Some(e.clone());
                return Err(e);
            }
        };
        self.tx.send(Command::Motion { values }).map_err(|_| "the run thread has stopped".to_string())?;
        self.motion_refusal = None;
        self.requested = Some(values);
        (self.keys, self.keys_physical) = match request {
            MotionRequest::Key(k) => (vec![k], false),
            MotionRequest::HeldKeys(keys) => (robot_motion::KEYS.into_iter().filter(|k| keys.contains(k)).collect(), true),
            MotionRequest::Stop | MotionRequest::Channels(_) => (Vec::new(), false),
        };
        Ok(())
    }
    /// `robot_state.motion`: source, channels with bounds, requested and held
    /// values, active keys, heartbeat, last refusal and the motion-request label.
    pub fn motion_json(&self) -> Value {
        let Some(p) = &self.preset else { return Value::Null };
        let declared = json!({"motion_commands": p.preset.entry.get("motion_commands"), "motion_heartbeat": p.preset.entry.get("motion_heartbeat"), "motion_key_vectors": p.preset.entry.get("motion_key_vectors")});
        let available = self.check_motion().map(|_| ());
        let drive = self.drive.as_ref();
        let motion = drive.and_then(|d| d.motion.as_ref());
        let channels: Option<Vec<Value>> = motion.map(|m| {
            m.channels.iter().enumerate().map(|(i, c)| json!({"name": c.name, "index": c.index, "kind": c.kind, "unit": c.unit, "lower": c.lower, "upper": c.upper,
                "requested": self.requested.map(|r| r[i]), "held": self.held(c.index)})).collect()
        });
        let heartbeat = drive.and_then(|d| d.heartbeat.as_ref()).map(|h| json!({"channel": h.name, "index": h.index, "lower": h.lower, "upper": h.upper, "value": self.held(h.index), "rule": robot_motion::HEARTBEAT_RULE}));
        json!({"label": robot_motion::LABEL, "available": available.is_ok(), "unavailable_reason": available.err(),
            "source": motion.map(|m| m.source), "config": motion.map(Motion::json), "channels": channels,
            "requested": self.requested, "active_keys": self.keys.iter().map(|k| k.to_string()).collect::<Vec<_>>(), "keys_physical": self.keys_physical,
            "heartbeat": heartbeat, "last_refusal": self.motion_refusal, "last_apply_error": self.motion_error,
            "session_inputs": drive.map(|d| d.inputs.iter().map(|c| json!({"name": c.name, "lower": c.lower, "upper": c.upper, "initial": c.initial})).collect::<Vec<_>>()),
            "declared_in_presets_json": declared, "keys": robot_motion::KEY_SEMANTICS, "stop_key": robot_motion::STOP_KEY, "clamping": robot_motion::CLAMP_RULE,
            "values_rule": "requested: the values last sent this generation (null until a request; Reset clears them); held: the session's input value in the latest accepted frame. Non-motion channels keep their held values."})
    }

    /// Why a save cannot be requested now (`Ok` when it would be sent).
    /// Refusals name the reason; the target itself is checked by `save_recording`.
    pub fn check_save(&self) -> Result<&Arc<PresetRun>, String> {
        let Some(p) = &self.preset else {
            return Err("recordings are for robot presets (the shared EmbeddedSession/EmbeddedEnvironment recording); `--robot FILE` runs PhysicalRobot, which keeps no recording".into());
        };
        let id = &p.preset.id;
        if let Some(why) = self.replay_block() {
            return Err(format!("preset `{id}`: save refused: {why}"));
        }
        match self.status.phase {
            Phase::Idle => return Err(format!("preset `{id}`: no built session yet; Run or Step builds {} before a recording can be saved", p.kind())),
            Phase::Building => return Err(format!("preset `{id}`: the session is building; save once it is built")),
            _ => {}
        }
        if let Some(t) = &self.saving {
            return Err(format!("preset `{id}`: a save is still being written ({}); wait for it", t.display()));
        }
        Ok(p)
    }
    /// The one save handler behind the Save recording button, `system_ui`
    /// recording:save and REST `robot_save_recording`. The target is resolved
    /// here without file-system access (robot_recording::target); the run
    /// thread snapshots the shared recording and a writer thread writes the
    /// pair, reported in `recording_json` once done.
    pub fn save_recording(&mut self, path: Option<&str>, note: Option<&str>) -> Result<std::path::PathBuf, String> {
        let result = self.check_save().and_then(|p| {
            let unix_ms = robot_recording::now_ms();
            robot_recording::target(&p.root, &p.preset.id, path, unix_ms).map(|t| (t, unix_ms))
        });
        let (target, unix_ms) = match result {
            Ok(x) => x,
            Err(e) => {
                self.save_error = Some(e.clone());
                return Err(e);
            }
        };
        self.save_requested += 1;
        self.tx.send(Command::SaveRecording { seq: self.save_requested, target: target.clone(), note: note.map(str::to_string), unix_ms }).map_err(|_| "the run thread has stopped".to_string())?;
        self.saving = Some(target.clone());
        self.save_error = None;
        Ok(target)
    }
    /// `robot_state.recording`: availability, the pending target, the last pair written and the last error, with the rules.
    pub fn recording_json(&self) -> Value {
        let available = self.check_save().map(|_| ());
        json!({"available": available.is_ok(), "unavailable_reason": available.err(), "pending": self.saving, "last_saved": self.saved, "error": self.save_error,
            "saves_requested": self.save_requested, "saves_finished": self.save_done,
            "root": self.preset.as_ref().map(|p| &p.root), "location_rule": robot_recording::LOCATION_RULE, "file_rule": robot_recording::FILE_RULE,
            "replayable_rule": robot_recording::REPLAYABLE_RULE,
            "kind_rule": "the browser's kind for the same preset (web/worker.js: a task → EnvironmentSimulation.recording() = EmbeddedEnvironment::episode_recording(), kind sampled_environment_recording; otherwise EmbeddedSimulation.recording() = EmbeddedSession::recording(), kind embedded_session)"})
    }
    pub fn saved(&self) -> Option<&Saved> {
        self.saved.as_ref()
    }
    pub fn save_error(&self) -> Option<&str> {
        self.save_error.as_deref()
    }
    pub fn save_pending(&self) -> Option<&std::path::Path> {
        self.saving.as_deref()
    }

    /// Lists the saved recordings of the loaded preset on a lister thread (at
    /// open, after each finished save and on request); `recordings_json` once done.
    pub fn refresh_recordings(&mut self) {
        let Some(p) = self.preset.clone() else { return };
        self.list_requested += 1;
        let (seq, out) = (self.list_requested, self.shared.clone());
        let spawned = std::thread::Builder::new().name("robot-recording-list".into()).spawn(move || {
            let result = robot_recording::list(&p.root, &p.preset.id);
            out.lock().unwrap_or_else(|p| p.into_inner()).listing = Some((seq, result));
        });
        if let Err(e) = spawned {
            self.list_done = self.list_requested;
            self.list_error = Some(format!("could not start the recording lister: {e}"));
        }
    }
    pub fn recordings(&self) -> &[Listed] {
        &self.recordings
    }
    /// `robot_state.recordings`: the saved recordings of the loaded preset (null for `--robot FILE`).
    pub fn recordings_json(&self) -> Value {
        let Some(p) = &self.preset else { return Value::Null };
        json!({"dir": p.root.join(robot_recording::DIR).join(&p.preset.id), "files": self.recordings, "pending": self.list_done < self.list_requested, "error": self.list_error,
            "rule": "*.json (not *.meta.json) in runs/robot-presets/<preset-id>/ under the root, by file name (UTC stamp, oldest first); meta summarises the sidecar when it exists; listed off the UI thread at open, after each save and on robot_replay {action: \"list\"}"})
    }

    /// Why run controls, motion and Save are refused because of a replay (None when no replay holds them).
    fn replay_block(&self) -> Option<String> {
        let r = &self.replay;
        match r.phase {
            ReplayPhase::Replaying => Some(format!("replay of {} in progress ({}); Cancel or Reset", r.file(), r.progress())),
            ReplayPhase::Cancelled if r.replaced => Some(format!("the run is a cancelled partial replay of {} ({}); Reset starts a fresh run, or replay a recording", r.file(), r.progress())),
            _ => None,
        }
    }
    /// Why a replay cannot be started now (`Ok` when it would be sent).
    pub fn check_replay(&self) -> Result<&Arc<PresetRun>, String> {
        let Some(p) = &self.preset else {
            return Err("replay is for robot presets (the shared EmbeddedSession/EmbeddedEnvironment prepare_replay); `--robot FILE` runs PhysicalRobot, which has no recording or replay".into());
        };
        let id = &p.preset.id;
        if self.replay.phase == ReplayPhase::Replaying {
            return Err(format!("preset `{id}`: a replay of {} is in progress ({}); Cancel or Reset before another replay", self.replay.file(), self.replay.progress()));
        }
        if self.status.phase == Phase::Building {
            return Err(format!("preset `{id}`: the session is building; replay once it is built"));
        }
        if self.running {
            return Err(format!("preset `{id}`: the run is running; Pause before replaying (a replay replaces the current run)"));
        }
        Ok(p)
    }
    /// The one replay handler behind the inspector Replay buttons, `system_ui`
    /// replay:<file> and REST `robot_replay`. The run thread reads the file,
    /// prepares it through the shared prepare_replay and advances it in chunks
    /// (robot_recording::REPLAY_RULE); the verdict is in `replay_json`.
    pub fn replay(&mut self, file: Option<&str>, path: Option<&str>) -> Result<std::path::PathBuf, String> {
        let p = self.check_replay()?;
        let source = robot_recording::replay_source(&p.root, &p.preset.id, file, path)?;
        // Frames of the replaced run are stale once the replay (or its refusal) is published.
        self.generation += 1;
        self.running = false;
        self.requested = None;
        self.keys.clear();
        self.keys_physical = false;
        self.motion_refusal = None;
        self.motion_error = None;
        self.replay = ReplayState::new(self.replay.seq + 1, self.generation, Some(source.clone()), ReplayPhase::Replaying);
        self.tx.send(Command::Replay { generation: self.generation, seq: self.replay.seq, path: source.clone() }).map_err(|_| "the run thread has stopped".to_string())?;
        Ok(source)
    }
    /// Cancel: the run thread stops between chunks (phase cancelled, never done).
    pub fn cancel_replay(&mut self) -> Result<(), String> {
        self.check_cancel()?;
        self.tx.send(Command::CancelReplay).map_err(|_| "the run thread has stopped".to_string())?;
        self.replay.cancel_requested = true;
        Ok(())
    }
    pub fn check_cancel(&self) -> Result<(), String> {
        if self.replay.phase != ReplayPhase::Replaying {
            return Err(format!("no replay in progress to cancel (replay phase {:?})", self.replay.phase).to_lowercase());
        }
        if self.replay.cancel_requested {
            return Err(format!("cancel of {} already requested; it stops between chunks", self.replay.file()));
        }
        Ok(())
    }
    pub fn replay_state(&self) -> &ReplayState {
        &self.replay
    }
    /// `robot_state.replay`: path, phase, completed/total, verdict, error and measured, with the rules.
    pub fn replay_json(&self) -> Value {
        if self.preset.is_none() {
            return Value::Null;
        }
        let available = self.check_replay().map(|_| ());
        let mut v = json!(self.replay);
        v["available"] = json!(available.is_ok());
        v["unavailable_reason"] = json!(available.err());
        v["replay_rule"] = json!(robot_recording::REPLAY_RULE);
        v["verdict_rule"] = json!(robot_recording::VERDICT_RULE);
        v["identity_rule"] = json!(robot_recording::IDENTITY_RULE);
        v["pause_step_rule"] = json!("Pause and Step are refused during a replay (\"replay … in progress; Cancel or Reset\"): a replay re-executes the recorded schedule to its end or to Cancel, and pausing or stepping it would add a second, unrecorded control path; Cancel stops it between chunks and Reset returns to a fresh run");
        v
    }
    /// Tests only: the whole held action through the motion handler's setter.
    #[cfg(test)]
    fn set_inputs(&self, values: Vec<f64>) {
        self.tx.send(Command::SetInputs(values)).unwrap();
    }

    /// Why an action is unavailable now (`Ok` when it can be sent).
    pub fn check(&self, action: RunAction) -> Result<(), String> {
        if action != RunAction::Reset {
            if let Some(why) = self.replay_block() {
                return Err(why);
            }
        }
        let failed = self.status.phase == Phase::Failed;
        let ended = self.status.phase == Phase::Ended;
        let why = || self.status.end.as_ref().and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("the run ended").to_string();
        match action {
            RunAction::Start if failed => Err("the run failed; Reset rebuilds the robot before it can run again".into()),
            RunAction::Start if ended => Err(format!("{}; Reset rebuilds at t = 0 before it can run again", why())),
            RunAction::Start if self.running => Err("already running".into()),
            RunAction::Pause if !self.running || failed || ended => Err("not running".into()),
            RunAction::Step if failed => Err("the run failed; Reset rebuilds the robot before it can step".into()),
            RunAction::Step if ended => Err(format!("{}; Reset rebuilds at t = 0 before it can step", why())),
            RunAction::Step if self.running => Err(format!("step advances one {} s chunk only while paused; the robot is running — pause first", self.chunk_s)),
            _ => Ok(()),
        }
    }

    /// The one handler behind the buttons, `system_ui` and REST `robot_run`.
    pub fn act(&mut self, action: RunAction) -> Result<(), String> {
        self.check(action)?;
        let command = match action {
            RunAction::Start => {
                self.running = true;
                Command::Start
            }
            RunAction::Pause => {
                self.running = false;
                Command::Pause
            }
            RunAction::Step => Command::Step,
            RunAction::Reset => {
                // Reset leaves the robot paused at t = 0; every older frame is stale.
                self.running = false;
                self.generation += 1;
                self.frame = None;
                // The rebuild starts from the file's control targets again.
                self.jogged.clear();
                self.jog_error = None;
                // The rebuild starts from the session's initial inputs again.
                self.drive = None;
                self.requested = None;
                self.keys.clear();
                self.keys_physical = false;
                self.motion_error = None;
                // Reset ends any replay: a fresh run.
                self.replay = ReplayState::new(self.replay.seq, self.generation, None, ReplayPhase::Idle);
                self.status = Status { phase: Phase::Building, generation: self.generation, rtf: None, error: None, end: None };
                Command::Reset { generation: self.generation }
            }
        };
        self.tx.send(command).map_err(|_| "the run thread has stopped".to_string())
    }

    /// Takes the worker's latest status and frame; returns true when the
    /// displayed frame changed (a stale-generation frame is never accepted).
    pub fn poll(&mut self) -> bool {
        let shared = self.shared.clone();
        let published = shared.lock().unwrap_or_else(|p| p.into_inner());
        let mut relist = false;
        // Saves are file results, kept across generations.
        if let Some((seq, result)) = published.save.as_ref().filter(|(seq, _)| *seq > self.save_done) {
            self.save_done = *seq;
            if self.save_done >= self.save_requested {
                self.saving = None;
            }
            match result {
                Ok(saved) => {
                    self.saved = Some(saved.clone());
                    self.save_error = None;
                    relist = true;
                }
                Err(e) => self.save_error = Some(e.clone()),
            }
        }
        if let Some((seq, result)) = published.listing.as_ref().filter(|(seq, _)| *seq > self.list_done) {
            self.list_done = *seq;
            match result {
                Ok(list) => {
                    self.recordings = list.clone();
                    self.list_error = None;
                }
                Err(e) => self.list_error = Some(e.clone()),
            }
        }
        if let Some(r) = published.replay.as_ref().filter(|r| r.generation >= self.generation && r.seq >= self.replay.seq) {
            let cancel = self.replay.cancel_requested && self.replay.seq == r.seq;
            self.replay = r.clone();
            self.replay.cancel_requested |= cancel;
        }
        if published.status.generation >= self.generation {
            self.status = published.status.clone();
            self.jog_error = published.jog_error.clone();
            self.drive = published.drive.clone();
            self.motion_error = published.motion_error.clone();
            if matches!(self.status.phase, Phase::Failed | Phase::Ended) {
                self.running = false;
            }
        }
        let fresh = published.frame.as_ref().filter(|f| accept(self.generation, f) && self.frame.as_ref().is_none_or(|old| old.generation != f.generation || old.steps != f.steps || old.targets != f.targets || old.completed_steps != f.completed_steps || old.inputs != f.inputs));
        let changed = match fresh {
            Some(f) => {
                self.frame = Some(f.clone());
                true
            }
            None => false,
        };
        drop(published);
        if relist {
            self.refresh_recordings();
        }
        changed
    }

    pub fn frame(&self) -> Option<&Frame> {
        self.frame.as_ref()
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn phase(&self) -> Phase {
        self.status.phase
    }
    pub fn rtf(&self) -> Option<f64> {
        self.status.rtf
    }
    pub fn error(&self) -> Option<&str> {
        self.status.error.as_deref()
    }
    pub fn jog_error(&self) -> Option<&str> {
        self.jog_error.as_deref()
    }
    pub fn model(&self) -> &PhysicalModel {
        &self.model
    }

    /// One joint's jog state: file limit, requested target, and the target and
    /// measured value from the latest accepted frame (null before a build).
    pub fn jog_json(&self, joint: &str) -> Value {
        match servo(&self.model, joint) {
            Err(e) => json!({"joint": joint, "servo": false, "reason": e}),
            Ok(s) => {
                let latest = self.frame.as_ref().and_then(|f| f.servo(joint));
                json!({"joint": joint, "servo": true, "unit": s.unit, "limit": s.limit, "limit_text": s.limit_text(), "step": s.step(),
                    "file_target": s.file_target, "requested_target": self.jogged.get(joint), "target": latest.map(|l| l.0), "measured": latest.map(|l| l.1),
                    "frame_generation": self.frame.as_ref().map(|f| f.generation), "frame_time": self.frame.as_ref().map(|f| f.time)})
            }
        }
    }
    /// Frames need drawing while the worker is building or running.
    pub fn active(&self) -> bool {
        matches!(self.status.phase, Phase::Building | Phase::Running) || self.running || self.replay.phase == ReplayPhase::Replaying || self.saving.is_some() || self.list_done < self.list_requested
    }

    /// `robot_state.run`: phase, time, steps, chunk, rtf, generation, error
    /// and the latest accepted frame. time/steps are null until a frame of
    /// the current generation exists (idle before any run).
    pub fn state_json(&self, links: &[String]) -> Value {
        let f = self.frame.as_ref();
        let poses: Option<Vec<Value>> = f.map(|f| {
            f.poses.iter().enumerate().map(|(i, pose)| match pose {
                Some((p, q)) => json!({"link": links.get(i), "position": p, "quat_xyzw": [q.x, q.y, q.z, q.w]}),
                None => json!({"link": links.get(i), "position": null, "quat_xyzw": null}),
            }).collect()
        });
        let build = match &self.preset {
            None => "sim_runtime::physical::PhysicalRobot::build(model clone, sim_runtime::registry(), BuildOptions::default()) on the run thread".to_string(),
            Some(p) if p.task.is_some() => format!("sim_runtime::environment::EmbeddedEnvironment::new(scene, config, task, seed {}) from the preset's files unchanged, on the run thread", p.seed),
            Some(p) => format!("sim_runtime::embedded::EmbeddedSession::new(scene, config, seed {}, CaptureMode::Latest) from the preset's files unchanged, on the run thread", p.seed),
        };
        let without: Option<Vec<&String>> = f.map(|f| f.poses.iter().enumerate().filter(|(_, p)| p.is_none()).filter_map(|(i, _)| links.get(i)).collect());
        json!({"phase": self.status.phase, "time": f.map(|f| f.time), "steps": f.map(|f| f.steps), "chunk_s": self.chunk_s,
            "rtf": self.status.rtf, "generation": self.generation, "error": self.status.error, "end": self.status.end, "frame_generation": f.map(|f| f.generation),
            "completed_steps": f.and_then(|f| f.completed_steps),
            "joints": f.map(|f| &f.joint_names), "joint_angles": f.map(|f| &f.joint_angles), "targets": f.map(|f| &f.targets), "poses": poses,
            "unmatched_frame_links": f.map(|f| &f.unmatched), "links_without_pose": without,
            "build": build,
            "steps_unit": "chunks of chunk_s since the last build", "pacing": PACING, "poses_frame": "link frame at its com, model frame (Z up)"})
    }
}

fn frame(robot: &sim_runtime::physical::PhysicalRobot, generation: u64, steps: u64) -> Frame {
    let poses = robot
        .poses()
        .iter()
        .map(|(r, p)| {
            let cols: [f64; 9] = std::array::from_fn(|i| r.as_slice()[i]);
            Some(([p.x, p.y, p.z], DQuat::from_mat3(&DMat3::from_cols_array(&cols)).normalize()))
        })
        .collect();
    let targets = robot.targets.lock().unwrap_or_else(|p| p.into_inner()).clone();
    Frame { generation, time: robot.time(), steps, completed_steps: None, poses, unmatched: Vec::new(), joint_names: robot.joint_names.clone(), joint_angles: robot.joint_angles(), targets, inputs: Vec::new() }
}

/// A frame from a preset session's `interactive_frame()`: `poses[]` of
/// `{name, position_m, rotation}` (rotation row-major, the same link frames
/// `PhysicalRobot::poses` gives), mapped to the loaded links by name. Names
/// that match no link are kept in `unmatched`, never dropped silently.
fn preset_frame(v: &Value, links: &[String], generation: u64, steps: u64, step_s: f64, inputs: &[f64]) -> Result<Frame, String> {
    let completed = v.get("completed_steps").and_then(Value::as_u64).ok_or("session frame has no completed_steps")?;
    let mut poses = vec![None; links.len()];
    let mut unmatched = Vec::new();
    for (k, pose) in v.get("poses").and_then(Value::as_array).ok_or("session frame has no poses")?.iter().enumerate() {
        let name = pose.get("name").and_then(Value::as_str).ok_or_else(|| format!("session frame poses[{k}] has no name"))?;
        let num = |x: &Value| x.as_f64().ok_or_else(|| format!("session frame pose `{name}`: non-numeric value"));
        let p = pose.get("position_m").and_then(Value::as_array).filter(|a| a.len() == 3).ok_or_else(|| format!("session frame pose `{name}` has no position_m"))?;
        let r = pose.get("rotation").and_then(Value::as_array).filter(|a| a.len() == 3).ok_or_else(|| format!("session frame pose `{name}` has no 3×3 rotation"))?;
        let mut m = [[0.0; 3]; 3];
        for (i, row) in r.iter().enumerate() {
            let row = row.as_array().filter(|a| a.len() == 3).ok_or_else(|| format!("session frame pose `{name}`: rotation row {i} is not 3 numbers"))?;
            for j in 0..3 {
                m[i][j] = num(&row[j])?;
            }
        }
        let cols = DMat3::from_cols([m[0][0], m[1][0], m[2][0]].into(), [m[0][1], m[1][1], m[2][1]].into(), [m[0][2], m[1][2], m[2][2]].into());
        let pos = [num(&p[0])?, num(&p[1])?, num(&p[2])?];
        match links.iter().position(|l| l == name) {
            Some(i) => poses[i] = Some((pos, DQuat::from_mat3(&cols).normalize())),
            None => unmatched.push(name.to_string()),
        }
    }
    Ok(Frame { generation, time: completed as f64 * step_s, steps, completed_steps: Some(completed), poses, unmatched, joint_names: Vec::new(), joint_angles: Vec::new(), targets: Vec::new(), inputs: inputs.to_vec() })
}

/// A frame's time, step count and link poses as JSON (the sidecar's final frame).
fn frame_json(f: &Frame, links: &[String]) -> Value {
    let poses: Vec<Value> = f.poses.iter().enumerate().map(|(i, pose)| match pose {
        Some((p, q)) => json!({"link": links.get(i), "position": p, "quat_xyzw": [q.x, q.y, q.z, q.w]}),
        None => json!({"link": links.get(i), "position": null, "quat_xyzw": null}),
    }).collect();
    json!({"time": f.time, "completed_steps": f.completed_steps, "chunks": f.steps, "generation": f.generation, "inputs": f.inputs, "poses": poses})
}

/// The simulation the run thread owns.
enum Sim {
    Robot(sim_runtime::physical::PhysicalRobot),
    /// The held action: the session's own input values (see `robot_preset`'s action_rule).
    Environment { env: sim_runtime::environment::EmbeddedEnvironment, held: Vec<f64>, run: Arc<PresetRun>, drive: Arc<Drive> },
    Session { session: sim_runtime::embedded::EmbeddedSession, run: Arc<PresetRun>, drive: Arc<Drive> },
}
impl Sim {
    fn build(source: &Source, registry: &mut Option<sim_core::BehaviorRegistry>) -> Result<Sim, String> {
        use sim_runtime::embedded::{CaptureMode, EmbeddedSession};
        use sim_runtime::environment::EmbeddedEnvironment;
        match source {
            Source::Robot(model) => {
                let registry = registry.get_or_insert_with(sim_runtime::registry);
                sim_runtime::physical::PhysicalRobot::build(model.clone(), registry, &sim_runtime::physical::BuildOptions::default()).map(Sim::Robot)
            }
            Source::Preset(run) => match &run.task {
                Some(task) => {
                    let env = EmbeddedEnvironment::new(run.scene.clone(), run.config.clone(), task.clone(), run.seed)?;
                    // Held at the session's reset values, as EmbeddedEnvironment::prepare_replay does.
                    let held = env.inputs().iter().map(|c| c.initial).collect();
                    let drive = Arc::new(Drive::resolve(run, &env.metadata()["policy_contract"], env.inputs())?);
                    Ok(Sim::Environment { env, held, run: run.clone(), drive })
                }
                None => {
                    let session = EmbeddedSession::new(run.scene.clone(), run.config.clone(), run.seed, CaptureMode::Latest)?;
                    let drive = Arc::new(Drive::resolve(run, &session.policy_metadata(), session.inputs())?);
                    Ok(Sim::Session { session, run: run.clone(), drive })
                }
            },
        }
    }
    fn time(&self) -> f64 {
        match self {
            Sim::Robot(r) => r.time(),
            Sim::Environment { env, run, .. } => env.transition().completed_steps as f64 * run.config.step_s,
            Sim::Session { session, run, .. } => session.completed_steps() as f64 * run.config.step_s,
        }
    }
    /// Advances exactly one chunk: one action packet for a preset, which
    /// advances its declared heartbeat first (`robot_motion::next_packet`).
    fn advance(&mut self) -> Result<(), String> {
        match self {
            Sim::Robot(r) => r.advance(CHUNK_S),
            Sim::Environment { env, held, drive, .. } => {
                robot_motion::next_packet(drive.heartbeat.as_ref(), held)?;
                env.step(held).map(|_| ())
            }
            Sim::Session { session, run, drive } => {
                if drive.heartbeat.is_some() {
                    let mut action = session.input_values().to_vec();
                    robot_motion::next_packet(drive.heartbeat.as_ref(), &mut action)?;
                    session.set_inputs(&action)?;
                }
                session.advance(run.chunk_steps())
            }
        }
    }
    /// Sets the motion channels of the held action, through the session's own
    /// validating `set_inputs` (at the next `EmbeddedEnvironment::step` for an
    /// environment, now for a session). Other channels keep their held values.
    fn set_motion(&mut self, values: [f64; 3]) -> Result<(), String> {
        let (motion, current) = match self {
            Sim::Robot(_) => return Err("motion requests are for presets".into()),
            Sim::Environment { held, drive, .. } => (drive.motion.as_ref(), held.clone()),
            Sim::Session { session, drive, .. } => (drive.motion.as_ref(), session.input_values().to_vec()),
        };
        let motion = motion.ok_or("the preset has no motion config")?;
        let mut action = current;
        for (c, x) in motion.channels.iter().zip(values) {
            c.check(x, "requested value")?;
            *action.get_mut(c.index).ok_or_else(|| format!("motion channel `{}` index {} is outside the action", c.name, c.index))? = x;
        }
        self.set_action(action)
    }
    /// Sets the whole held action: validated by the session's `set_inputs` (now
    /// for a session; for an environment, bounds-checked here and validated
    /// again by `EmbeddedEnvironment::step`).
    fn set_action(&mut self, action: Vec<f64>) -> Result<(), String> {
        match self {
            Sim::Environment { env, held, .. } => {
                if action.len() != env.inputs().len() {
                    return Err(format!("action has {} values; the environment has {} inputs", action.len(), env.inputs().len()));
                }
                for (c, x) in env.inputs().iter().zip(&action) {
                    if !(x.is_finite() && *x >= c.lower && *x <= c.upper) {
                        return Err(format!("input `{}` = {x} is outside [{}, {}]", c.name, c.lower, c.upper));
                    }
                }
                *held = action;
            }
            Sim::Session { session, .. } => session.set_inputs(&action)?,
            Sim::Robot(_) => return Err("inputs are for presets".into()),
        }
        Ok(())
    }
    /// The runtime's own nominal completed_steps (presets).
    fn completed_steps(&self) -> Option<u64> {
        match self {
            Sim::Robot(_) => None,
            Sim::Environment { env, .. } => Some(env.transition().completed_steps as u64),
            Sim::Session { session, .. } => Some(session.completed_steps() as u64),
        }
    }
    /// One bounded replay chunk: the next returned action through
    /// `EmbeddedEnvironment::step`, or up to the preset's chunk of nominal steps
    /// through `EmbeddedSession::advance`. Returns the units advanced.
    fn advance_replay(&mut self, work: &mut ReplayWork) -> Result<u64, String> {
        match (self, work) {
            (Sim::Environment { env, held, .. }, ReplayWork::Actions(queue)) => {
                let action = queue.front().ok_or("no pending replay action")?.clone();
                env.step(&action)?;
                *held = action;
                queue.pop_front();
                Ok(1)
            }
            (Sim::Session { session, run, .. }, ReplayWork::Steps { remaining }) => {
                let n = (*remaining).min(run.chunk_steps());
                *remaining -= n;
                session.advance(n)?;
                Ok(n as u64)
            }
            _ => Err("replay work does not match the simulation kind".into()),
        }
    }
    /// The shared recording, as the browser saves it for this preset kind.
    fn snapshot(&self) -> Result<(Snapshot, &Arc<PresetRun>), String> {
        match self {
            Sim::Robot(_) => Err("`--robot FILE` runs PhysicalRobot, which keeps no recording".into()),
            Sim::Environment { env, run, .. } => Ok((Snapshot::Environment(env.episode_recording()), run)),
            Sim::Session { session, run, .. } => Ok((Snapshot::Session(session.recording()), run)),
        }
    }
    fn drive(&self) -> Option<Arc<Drive>> {
        match self {
            Sim::Robot(_) => None,
            Sim::Environment { drive, .. } | Sim::Session { drive, .. } => Some(drive.clone()),
        }
    }
    /// Why the run cannot continue without a reset (horizon or episode end).
    fn ended(&self) -> Option<Value> {
        match self {
            Sim::Robot(_) => None,
            Sim::Environment { env, run, .. } => {
                let t = env.transition();
                (t.terminated || t.truncated).then(|| {
                    let kind = if t.terminated { "terminated" } else { "horizon" };
                    let message = if t.terminated {
                        format!("episode terminated at t = {:.3} s: {}", t.completed_steps as f64 * run.config.step_s, t.termination_reasons.join("; "))
                    } else {
                        format!("horizon reached (episode truncated) at t = {:.3} s: {} of {} steps", t.completed_steps as f64 * run.config.step_s, t.completed_steps, run.config.steps)
                    };
                    json!({"kind": kind, "terminated": t.terminated, "truncated": t.truncated, "termination_reasons": t.termination_reasons, "completed_steps": t.completed_steps, "message": message})
                })
            }
            Sim::Session { session, run, .. } => (session.remaining_steps() == 0).then(|| {
                let n = session.completed_steps();
                json!({"kind": "horizon", "terminated": false, "truncated": true, "termination_reasons": [], "completed_steps": n,
                    "message": format!("horizon reached at t = {:.3} s: {n} of {} steps", n as f64 * run.config.step_s, run.config.steps)})
            }),
        }
    }
    fn frame(&self, links: &[String], generation: u64, steps: u64) -> Result<Frame, String> {
        match self {
            Sim::Robot(r) => Ok(frame(r, generation, steps)),
            Sim::Environment { env, run, held, .. } => preset_frame(&env.frame()?, links, generation, steps, run.config.step_s, held),
            Sim::Session { session, run, .. } => preset_frame(&session.interactive_frame()?, links, generation, steps, run.config.step_s, session.input_values()),
        }
    }
}

/// What a prepared replay still has to advance: the actions
/// `EmbeddedEnvironment::prepare_replay` returned, or the steps
/// `EmbeddedSession::prepare_replay` returned.
enum ReplayWork {
    Actions(VecDeque<Vec<f64>>),
    Steps { remaining: usize },
}
impl ReplayWork {
    fn done(&self) -> bool {
        match self {
            ReplayWork::Actions(q) => q.is_empty(),
            ReplayWork::Steps { remaining } => *remaining == 0,
        }
    }
}
/// A replay the run thread is advancing.
struct ActiveReplay {
    state: ReplayState,
    work: ReplayWork,
    final_frame: Option<Value>,
    started: Instant,
}

/// Reads `path` and prepares it through the shared runtime against the loaded
/// preset (robot_recording::REPLAY_RULE): the replacement simulation, its work
/// and the state so far. Refusals name the reason; runtime refusals are verbatim.
fn prepare_replay(source: &Source, current: Option<&Sim>, path: &std::path::Path, mut state: ReplayState) -> Result<(Sim, ActiveReplay), (String, ReplayState)> {
    use sim_runtime::embedded::{CaptureMode, EmbeddedRecording, EmbeddedSession};
    use sim_runtime::environment::{EmbeddedEnvironment, EnvironmentRecording};
    use sim_runtime::physics_context::fingerprint;
    let Source::Preset(run) = source else {
        return Err(("replay is for robot presets; `--robot FILE` runs PhysicalRobot, which has no recording or replay".into(), state));
    };
    let id = &run.preset.id;
    macro_rules! tryr {
        ($e:expr) => {
            match $e {
                Ok(x) => x,
                Err(e) => return Err((e, state)),
            }
        };
    }
    let text = tryr!(std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display())));
    let value: Value = tryr!(serde_json::from_str(&text).map_err(|e| format!("{}: not JSON: {e}", path.display())));
    let kind = value.get("kind").and_then(Value::as_str).unwrap_or("(no kind)").to_string();
    let expected = if run.task.is_some() { "sampled_environment_recording" } else { "embedded_session" };
    state.kind = Some(kind.clone());
    if kind != expected {
        return Err((format!("refused: {} is a `{kind}` recording, but preset `{id}` runs {} and replays `{expected}` recordings (the kind its Save and the browser's Download write)", path.display(), run.kind()), state));
    }
    let sidecar: Option<Value> = std::fs::read_to_string(robot_recording::meta_path(path)).ok().and_then(|t| serde_json::from_str(&t).ok());
    let final_frame = sidecar.as_ref().map(|m| m["final_frame"].clone()).filter(|f| !f.is_null());
    state.sidecar = sidecar.as_ref().map(|m| json!({"path": robot_recording::meta_path(path), "preset_id": m["preset"]["id"], "saved_utc": m["saved_utc"], "note": m["note"], "has_final_frame": final_frame.is_some()}));
    match &run.task {
        Some(task) => {
            let record: EnvironmentRecording = tryr!(serde_json::from_value(value).map_err(|e| format!("{}: not a shared EnvironmentRecording: {e}", path.display())));
            state.recorded_completed_steps = Some(record.runtime.completed_steps as u64);
            state.recorded_failure = record.error.clone().or(record.runtime.failure.clone());
            // prepare_replay compares the record with this (loaded) environment; build one if none is built yet.
            let built;
            let env = match current {
                Some(Sim::Environment { env, .. }) => env,
                _ => {
                    built = tryr!(EmbeddedEnvironment::new(run.scene.clone(), run.config.clone(), task.clone(), run.seed).map_err(|e| format!("building preset `{id}` to check the recording against: {e}")));
                    &built
                }
            };
            let (next, actions) = tryr!(env.prepare_replay(record).map_err(|e| format!("EmbeddedEnvironment::prepare_replay refused it: {e}")));
            let held = next.inputs().iter().map(|c| c.initial).collect();
            let drive = Arc::new(tryr!(Drive::resolve(run, &next.metadata()["policy_contract"], next.inputs())));
            state.total = Some(actions.len() as u64);
            state.unit = Some("actions");
            Ok((Sim::Environment { env: next, held, run: run.clone(), drive }, ActiveReplay { state, work: ReplayWork::Actions(actions.into()), final_frame, started: Instant::now() }))
        }
        None => {
            let record: EmbeddedRecording = tryr!(serde_json::from_value(value).map_err(|e| format!("{}: not a shared EmbeddedRecording: {e}", path.display())));
            state.recorded_completed_steps = Some(record.completed_steps as u64);
            state.recorded_failure = record.failure.clone();
            // robot_recording::IDENTITY_RULE: EmbeddedSession::prepare_replay does not compare with the loaded preset.
            for (what, a, b) in [("scene", json!(record.scene), json!(run.scene)), ("config (controller recipe)", json!(record.config), json!(run.config))] {
                if fingerprint(&a) != fingerprint(&b) {
                    return Err((format!("refused by the viewer identity check (as sim-web's): the recording's {what} differs from preset `{id}`'s; replay must match the loaded scene and controller recipe; load another preset to change them"), state));
                }
            }
            let (session, steps) = tryr!(EmbeddedSession::prepare_replay(record, CaptureMode::Latest).map_err(|e| format!("EmbeddedSession::prepare_replay refused it: {e}")));
            let drive = Arc::new(tryr!(Drive::resolve(run, &session.policy_metadata(), session.inputs())));
            state.total = Some(steps as u64);
            state.unit = Some("nominal steps");
            Ok((Sim::Session { session, run: run.clone(), drive }, ActiveReplay { state, work: ReplayWork::Steps { remaining: steps }, final_frame, started: Instant::now() }))
        }
    }
}

/// The verdict once the work is exhausted (`error` None) or the runtime
/// returned an error (robot_recording::VERDICT_RULE): only what it establishes.
fn finish_replay(sim: &Sim, r: &mut ActiveReplay, error: Option<String>) {
    let s = &mut r.state;
    s.completed_steps = sim.completed_steps();
    s.wall_s = Some(r.started.elapsed().as_secs_f64());
    let (done, recorded) = (s.completed_steps, s.recorded_completed_steps);
    let steps_match = done.is_some() && done == recorded;
    let counts = format!("completed_steps {} (recorded {})", done.map_or("?".into(), |n| n.to_string()), recorded.map_or("?".into(), |n| n.to_string()));
    let (phase, verdict, err) = match (sim, error) {
        (Sim::Environment { env, .. }, None) => match env.error() {
            Some(e) => (ReplayPhase::Failed, "failed: the environment reported an error after the replay".to_string(), Some(e.to_string())),
            None if steps_match => (ReplayPhase::Done, format!("passed the shared runtime's replay checks: EmbeddedEnvironment::prepare_replay accepted the recording against the loaded task, scene and config, and all {} returned actions stepped through EmbeddedEnvironment::step without error; {counts}; states are not compared by the runtime", s.completed), None),
            None => (ReplayPhase::Failed, format!("failed: {counts} differ"), Some(format!("replayed {counts}"))),
        },
        (Sim::Environment { .. }, Some(e)) => (ReplayPhase::Failed, "failed: EmbeddedEnvironment::step returned an error during the replay".into(), Some(e)),
        (_, None) if steps_match && s.recorded_failure.is_none() => (ReplayPhase::Done, format!("passed the shared runtime's replay checks: EmbeddedSession::prepare_replay rebuilt the recording (after the viewer identity check) and every returned step advanced with replay_expected satisfied (recorded inputs re-applied at their steps, no failure); {counts}; states are not compared by the runtime"), None),
        (_, None) => (ReplayPhase::Failed, format!("failed: {counts}{}", s.recorded_failure.as_ref().map_or(String::new(), |f| format!("; the recorded failure `{f}` did not occur"))), Some(format!("replayed {counts}"))),
        (_, Some(e)) if s.recorded_failure.as_deref() == Some(e.as_str()) && steps_match => (ReplayPhase::Done, format!("passed the shared runtime's replay checks: the recorded failure reproduced with the same message at the same step (replay_expected); {counts}; the run is failed as recorded"), None),
        (_, Some(e)) => (ReplayPhase::Failed, "failed: the shared runtime returned an error during the replay".into(), Some(e)),
    };
    s.phase = phase;
    s.verdict = Some(verdict);
    s.error = err;
}

/// Sets one servo target by the file's joint name. `set_target` ignores a bad
/// index silently, so an unresolved name is an error here instead.
fn apply_jog(robot: &sim_runtime::physical::PhysicalRobot, joint: &str, target: f64) -> Result<(), String> {
    let index = robot.joint_names.iter().position(|n| short(n) == joint).ok_or_else(|| format!("joint `{joint}`: the built robot has no servo target named `{joint}` (its targets: {})", robot.joint_names.join(", ")))?;
    if index >= robot.targets.lock().unwrap_or_else(|p| p.into_inner()).len() {
        return Err(format!("joint `{joint}`: the built robot lists it (index {index}) but holds no servo target for it"));
    }
    robot.set_target(index, target);
    Ok(())
}

/// The run thread. The simulation is built and advanced only here.
fn worker(source: Source, links: Vec<String>, rx: mpsc::Receiver<Command>, out: Arc<Mutex<Published>>) {
    let chunk_s = source.chunk_s();
    let mut registry = None;
    let mut sim: Option<Sim> = None;
    let mut generation = 0;
    let mut steps = 0;
    let mut failed = false;
    let mut ended = false;
    let mut running = false;
    // Pacing anchor (wall, sim) and the RTF window of (wall, sim) samples.
    let mut anchor = (Instant::now(), 0.0);
    let mut window: VecDeque<(Instant, f64)> = VecDeque::new();
    let set = |status: Status, frame: Option<Frame>| {
        let mut p = out.lock().unwrap_or_else(|p| p.into_inner());
        p.status = status;
        if let Some(f) = frame {
            p.frame = Some(f);
        }
    };
    let set_jog_error = |e: Option<String>| out.lock().unwrap_or_else(|p| p.into_inner()).jog_error = e;
    let set_motion_error = |e: Option<String>| out.lock().unwrap_or_else(|p| p.into_inner()).motion_error = e;
    let set_drive = |d: Option<Arc<Drive>>| out.lock().unwrap_or_else(|p| p.into_inner()).drive = d;
    let set_replay = |r: &ReplayState| out.lock().unwrap_or_else(|p| p.into_inner()).replay = Some(r.clone());
    // The replay being advanced, and the last replay request number seen.
    let mut replay: Option<ActiveReplay> = None;
    let mut replay_seq = 0;
    // Jogs received before the first build, applied right after it.
    let mut pending: Vec<(String, f64)> = Vec::new();
    let status = |phase, generation, rtf, error: Option<String>| Status { phase, generation, rtf, error, end: None };
    // A frame of the current state, or the failure it hit (the last good frame stays published).
    let publish = |sim: &Sim, phase: Phase, generation: u64, steps: u64, rtf: Option<f64>| -> bool {
        match sim.frame(&links, generation, steps) {
            Ok(f) => {
                let end = if phase == Phase::Ended { sim.ended() } else { None };
                set(Status { end, ..status(phase, generation, rtf, None) }, Some(f));
                true
            }
            Err(e) => {
                set(status(Phase::Failed, generation, None, Some(format!("frame failed at t = {:.3} s: {e}", sim.time()))), None);
                false
            }
        }
    };
    loop {
        let command = if running {
            match rx.try_recv() {
                Ok(c) => Some(c),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        } else {
            match rx.recv() {
                Ok(c) => Some(c),
                Err(_) => return,
            }
        };
        let mut build = |sim: &mut Option<Sim>, generation: u64, pending: &mut Vec<(String, f64)>| -> bool {
            if sim.is_some() {
                return true;
            }
            set(status(Phase::Building, generation, None, None), None);
            match Sim::build(&source, &mut registry) {
                Ok(s) => {
                    if let Sim::Robot(r) = &s {
                        for (joint, target) in pending.drain(..) {
                            if let Err(e) = apply_jog(r, &joint, target) {
                                set_jog_error(Some(e));
                            }
                        }
                    }
                    set_drive(s.drive());
                    let ok = publish(&s, Phase::Paused, generation, 0, None);
                    *sim = Some(s);
                    ok
                }
                Err(e) => {
                    set(status(Phase::Failed, generation, None, Some(format!("build failed: {e}"))), None);
                    false
                }
            }
        };
        match command {
            Some(Command::Reset { generation: g }) => {
                generation = g;
                // Reset ends any replay: a fresh run.
                replay = None;
                set_replay(&ReplayState::new(replay_seq, g, None, ReplayPhase::Idle));
                sim = None;
                pending.clear();
                set_jog_error(None);
                set_drive(None);
                set_motion_error(None);
                steps = 0;
                running = false;
                ended = false;
                failed = !build(&mut sim, generation, &mut pending);
            }
            Some(Command::Jog { joint, .. }) if failed => set_jog_error(Some(format!("joint `{joint}`: not applied; the run failed (Reset rebuilds)"))),
            Some(Command::Motion { .. }) if failed || ended => set_motion_error(Some("motion request not applied: the run failed or ended (Reset rebuilds)".into())),
            // Saved in every phase with a built simulation, failed and ended included (labelled by robot_recording::REPLAYABLE_RULE).
            Some(Command::SaveRecording { seq, target, note, unix_ms }) => {
                let result = sim.as_ref().ok_or_else(|| "no built session to record: the build failed or has not run (Reset rebuilds)".to_string()).and_then(|s| {
                    let (snapshot, run) = s.snapshot()?;
                    let last = s.frame(&links, generation, steps).ok().map(|f| frame_json(&f, &links));
                    let meta = robot_recording::meta(&snapshot, run, &target, note.as_deref(), unix_ms, generation, steps, last);
                    Ok((snapshot, meta, run.root.clone()))
                });
                match result {
                    // Serialising and writing happen on a writer thread: a full-robot scene is megabytes.
                    Ok((snapshot, meta, root)) => {
                        let writer_out = out.clone();
                        let spawned = std::thread::Builder::new().name("robot-recording-writer".into()).spawn(move || {
                            let result = robot_recording::write(&root, &target, &snapshot, meta);
                            writer_out.lock().unwrap_or_else(|p| p.into_inner()).save = Some((seq, result));
                        });
                        if let Err(e) = spawned {
                            out.lock().unwrap_or_else(|p| p.into_inner()).save = Some((seq, Err(format!("could not start the recording writer: {e}"))));
                        }
                    }
                    Err(e) => out.lock().unwrap_or_else(|p| p.into_inner()).save = Some((seq, Err(e))),
                }
            }
            Some(Command::Replay { generation: g, seq, path }) => {
                generation = g;
                replay_seq = seq;
                running = false;
                replay = None;
                set_motion_error(None);
                let state = ReplayState::new(seq, g, Some(path.clone()), ReplayPhase::Replaying);
                match prepare_replay(&source, sim.as_ref(), &path, state) {
                    Ok((next, mut r)) => {
                        r.state.replaced = true;
                        steps = 0;
                        ended = false;
                        pending.clear();
                        set_jog_error(None);
                        set_drive(next.drive());
                        r.state.completed_steps = next.completed_steps();
                        let s = sim.insert(next);
                        failed = !publish(s, Phase::Running, generation, 0, None);
                        if failed {
                            finish_replay(s, &mut r, Some("the replayed session's first frame failed".into()));
                            set_replay(&r.state);
                        } else if r.work.done() {
                            finish_replay(s, &mut r, None);
                            ended = s.ended().is_some();
                            failed = !publish(s, if ended { Phase::Ended } else { Phase::Paused }, generation, 0, None);
                            set_replay(&r.state);
                        } else {
                            set_replay(&r.state);
                            running = true;
                            anchor = (Instant::now(), s.time());
                            window.clear();
                            window.push_back(anchor);
                            replay = Some(r);
                        }
                    }
                    Err((e, mut state)) => {
                        // Refused before replacing anything: the current run is republished unchanged under the new generation.
                        state.phase = ReplayPhase::Failed;
                        state.verdict = Some("refused: not replayed; the current run is unchanged".into());
                        state.error = Some(e);
                        let previous = out.lock().unwrap_or_else(|p| p.into_inner()).status.clone();
                        let frame = sim.as_ref().and_then(|s| s.frame(&links, generation, steps).ok());
                        set(Status { generation, rtf: None, ..previous }, frame);
                        set_replay(&state);
                    }
                }
            }
            Some(Command::CancelReplay) => {
                if let Some(mut r) = replay.take() {
                    running = false;
                    r.state.phase = ReplayPhase::Cancelled;
                    r.state.cancel_requested = true;
                    r.state.wall_s = Some(r.started.elapsed().as_secs_f64());
                    r.state.verdict = Some(format!("cancelled at {}: not a verdict; the replay did not finish", r.state.progress()));
                    if let Some(s) = sim.as_ref() {
                        r.state.completed_steps = s.completed_steps();
                        failed = !publish(s, Phase::Paused, generation, steps, None);
                    }
                    set_replay(&r.state);
                }
            }
            Some(Command::Motion { .. }) if replay.is_some() => set_motion_error(Some("motion request not applied: a replay is in progress".into())),
            #[cfg(test)]
            Some(Command::SetInputs(values)) => match sim.as_mut().map(|s| s.set_action(values)) {
                Some(Ok(())) => {
                    failed = !publish(sim.as_ref().unwrap(), Phase::Paused, generation, steps, None);
                }
                Some(Err(e)) => set_motion_error(Some(e)),
                None => set_motion_error(Some("no built session".into())),
            },
            _ if failed || ended => {}
            Some(Command::Jog { joint, target }) => match sim.as_ref() {
                None => pending.push((joint, target)),
                Some(Sim::Robot(r)) => match apply_jog(r, &joint, target) {
                    Ok(()) => {
                        set_jog_error(None);
                        let phase = if running { Phase::Running } else { Phase::Paused };
                        let rtf = out.lock().unwrap_or_else(|p| p.into_inner()).status.rtf;
                        set(status(phase, generation, rtf, None), Some(frame(r, generation, steps)));
                    }
                    Err(e) => set_jog_error(Some(e)),
                },
                Some(_) => set_jog_error(Some(format!("joint `{joint}`: not applied; a preset is driven by its declared controller"))),
            },
            Some(Command::Motion { values }) => match sim.as_mut() {
                None => set_motion_error(Some("motion request not applied: no built session".into())),
                Some(s) => match s.set_motion(values) {
                    Ok(()) => {
                        set_motion_error(None);
                        if !running {
                            // Paused: republish so the held action shows the request now.
                            let rtf = out.lock().unwrap_or_else(|p| p.into_inner()).status.rtf;
                            failed = !publish(s, Phase::Paused, generation, steps, rtf);
                        }
                    }
                    Err(e) => set_motion_error(Some(format!("motion request not applied: {e}"))),
                },
            },
            Some(Command::Start) => {
                if build(&mut sim, generation, &mut pending) {
                    running = true;
                    let t = sim.as_ref().map_or(0.0, Sim::time);
                    anchor = (Instant::now(), t);
                    window.clear();
                    window.push_back(anchor);
                    set(status(Phase::Running, generation, None, None), None);
                } else {
                    failed = true;
                }
            }
            Some(Command::Pause) => {
                running = false;
                set(status(if sim.is_some() { Phase::Paused } else { Phase::Idle }, generation, None, None), None);
            }
            Some(Command::Step) => {
                if !build(&mut sim, generation, &mut pending) {
                    failed = true;
                    continue;
                }
                let s = sim.as_mut().unwrap();
                match s.advance() {
                    Ok(()) => {
                        steps += 1;
                        ended = s.ended().is_some();
                        failed = !publish(s, if ended { Phase::Ended } else { Phase::Paused }, generation, steps, None);
                    }
                    Err(e) => {
                        failed = true;
                        set(status(Phase::Failed, generation, None, Some(format!("advance failed at t = {:.3} s: {e}", s.time()))), None);
                    }
                }
            }
            None => {}
        }
        if !running || failed || ended {
            continue;
        }
        let Some(s) = sim.as_mut() else { continue };
        // Pace: never ahead of the wall clock; drop lag beyond one chunk.
        let wall = anchor.0.elapsed().as_secs_f64();
        let t = s.time() - anchor.1;
        if t > wall {
            std::thread::sleep(Duration::from_secs_f64((t - wall).min(0.005)));
            continue;
        }
        if wall - t > chunk_s {
            anchor = (Instant::now() - Duration::from_secs_f64(chunk_s), s.time());
        }
        let advanced = match replay.as_mut() {
            Some(r) => s.advance_replay(&mut r.work).map(|n| r.state.completed += n),
            None => s.advance(),
        };
        match advanced {
            Ok(()) => {
                steps += 1;
                if let Some(r) = replay.as_mut() {
                    r.state.completed_steps = s.completed_steps();
                    if !r.work.done() {
                        failed = !publish(s, Phase::Running, generation, steps, None);
                        if failed {
                            running = false;
                            let mut r = replay.take().unwrap();
                            finish_replay(s, &mut r, Some(format!("frame failed at t = {:.3} s", s.time())));
                            set_replay(&r.state);
                        } else {
                            set_replay(&r.state);
                        }
                        continue;
                    }
                    let mut r = replay.take().unwrap();
                    running = false;
                    finish_replay(s, &mut r, None);
                    ended = s.ended().is_some();
                    failed = !publish(s, if ended { Phase::Ended } else { Phase::Paused }, generation, steps, None);
                    if let (Some(recorded), Ok(f)) = (r.final_frame.as_ref(), s.frame(&links, generation, steps)) {
                        r.state.measured = robot_recording::measured(recorded, &frame_json(&f, &links));
                    }
                    set_replay(&r.state);
                    continue;
                }
                let now = Instant::now();
                window.push_back((now, s.time()));
                while window.len() > 2 && now.duration_since(window[1].0) >= RTF_WINDOW {
                    window.pop_front();
                }
                let (w0, s0) = window[0];
                let dw = now.duration_since(w0).as_secs_f64();
                let rtf = (dw > 0.0).then(|| (s.time() - s0) / dw);
                ended = s.ended().is_some();
                if ended {
                    running = false;
                }
                failed = !publish(s, if ended { Phase::Ended } else { Phase::Running }, generation, steps, rtf);
                if failed {
                    running = false;
                }
            }
            Err(e) => {
                running = false;
                failed = true;
                let frame = if let Some(mut r) = replay.take() {
                    finish_replay(s, &mut r, Some(e.clone()));
                    let f = s.frame(&links, generation, steps).ok();
                    if let (Some(recorded), Some(f)) = (r.final_frame.as_ref(), f.as_ref()) {
                        r.state.measured = robot_recording::measured(recorded, &frame_json(f, &links));
                    }
                    set_replay(&r.state);
                    f
                } else {
                    None
                };
                set(status(Phase::Failed, generation, None, Some(format!("advance failed at t = {:.3} s: {e}", s.time()))), frame);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wait(c: &mut RunController, what: &str, done: impl Fn(&RunController) -> bool) {
        let start = Instant::now();
        while !done(c) {
            assert!(start.elapsed() < Duration::from_secs(120), "timed out waiting for {what}: phase {:?}, {:?}", c.phase(), c.status.error);
            std::thread::sleep(Duration::from_millis(5));
            c.poll();
        }
    }
    #[test]
    fn run_thread_steps_one_chunk_resets_generation_and_rejects_stale_frames() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let model = crate::robot::load(&root.join("examples/wheeled-robot/baseline/robot.simrobot.json")).unwrap().model;
        let assembly: Vec<[f64; 3]> = model.links.iter().map(|l| l.com).collect();
        let mut c = RunController::spawn(model);
        assert_eq!(c.phase(), Phase::Idle);
        assert!(c.frame().is_none());
        // Step before any run builds, then advances exactly one chunk.
        c.act(RunAction::Step).unwrap();
        wait(&mut c, "first step", |c| c.frame().is_some_and(|f| f.steps == 1));
        let f = c.frame().unwrap();
        assert!((f.time - CHUNK_S).abs() < 1e-9, "t = {}", f.time);
        assert_eq!((f.generation, c.phase()), (0, Phase::Paused));
        c.act(RunAction::Step).unwrap();
        wait(&mut c, "second step", |c| c.frame().is_some_and(|f| f.steps == 2));
        assert!((c.frame().unwrap().time - 2.0 * CHUNK_S).abs() < 1e-9);
        // Step while running is an error, not a no-op.
        c.act(RunAction::Start).unwrap();
        let err = c.act(RunAction::Step).unwrap_err();
        assert!(err.contains("pause first"), "{err}");
        wait(&mut c, "running", |c| c.frame().is_some_and(|f| f.steps > 3));
        c.act(RunAction::Pause).unwrap();
        // Reset: generation + 1, t = 0 at the assembly pose; the old frame is stale.
        let old = c.frame().unwrap().clone();
        c.act(RunAction::Reset).unwrap();
        assert_eq!(c.generation(), 1);
        assert!(!accept(c.generation(), &old));
        assert!(c.frame().is_none_or(|f| f.generation == 1));
        wait(&mut c, "reset", |c| c.frame().is_some() && c.phase() == Phase::Paused);
        let f = c.frame().unwrap();
        assert_eq!((f.generation, f.steps, f.time), (1, 0, 0.0));
        for (pose, com) in f.poses.iter().zip(&assembly) {
            let (p, q) = pose.as_ref().expect("every wheeled link has a pose");
            assert!((0..3).all(|k| (p[k] - com[k]).abs() < 1e-9), "{p:?} vs {com:?}");
            assert!(q.angle_between(DQuat::IDENTITY) < 1e-9);
        }
        assert!(RunAction::parse("jump").unwrap_err().contains("`jump`"));
    }

    #[test]
    fn jog_validates_against_the_file_and_moves_the_servo_toward_its_target() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut model = crate::robot::load(&root.join("examples/wheeled-robot/baseline/robot.simrobot.json")).unwrap().model;
        // In memory only: give the driven left axle file limits.
        model.joints.iter_mut().find(|j| j.name == "left axle").unwrap().limits = Some([-0.3, 0.3]);
        let mut c = RunController::spawn(model);
        // File validation, idle, with no build.
        let e = c.jog("left axle", 0.5).unwrap_err();
        assert!(e.contains("`left axle`") && e.contains("[-0.3, 0.3]") && e.contains("0.5"), "{e}");
        let e = c.jog("elbow", 0.1).unwrap_err();
        assert!(e.contains("unknown joint `elbow`") && e.contains("left axle"), "{e}");
        let e = c.jog("passive axle", 0.1).unwrap_err();
        assert!(e.contains("`passive axle` has no servo target"), "{e}");
        assert!(c.jog("left axle", f64::NAN).unwrap_err().contains("not finite"));
        // right axle has no limits in the file: any finite target, labelled so.
        assert_eq!(servo(c.model(), "right axle").unwrap().limit_text(), "no limit in file");
        // Before the first build a jog is queued and applied right after the build.
        c.jog("left axle", 0.2).unwrap();
        c.act(RunAction::Step).unwrap();
        wait(&mut c, "first step", |c| c.frame().is_some_and(|f| f.steps == 1));
        let (target, a0) = c.frame().unwrap().servo("left axle").unwrap();
        assert_eq!(target, 0.2);
        assert_eq!(c.frame().unwrap().servo("right axle").unwrap().0, 0.0);
        for n in 2..=25 {
            c.act(RunAction::Step).unwrap();
            wait(&mut c, "steps", |c| c.frame().is_some_and(|f| f.steps == n));
        }
        let (_, a1) = c.frame().unwrap().servo("left axle").unwrap();
        assert!((0.2 - a1).abs() < (0.2 - a0).abs() && a1 > a0, "angle {a0} -> {a1}, target 0.2");
        // While paused, a jog sets the target in the next frame without stepping.
        let steps = c.frame().unwrap().steps;
        c.jog("left axle", -0.25).unwrap();
        wait(&mut c, "paused jog", |c| c.frame().is_some_and(|f| f.servo("left axle").unwrap().0 == -0.25));
        assert_eq!(c.frame().unwrap().steps, steps);
        // An out-of-limit jog is refused and leaves the target unchanged.
        let e = c.jog("left axle", -0.31).unwrap_err();
        assert!(e.contains("`left axle`") && e.contains("[-0.3, 0.3]") && e.contains("not clamped"), "{e}");
        c.act(RunAction::Step).unwrap();
        wait(&mut c, "step after refusal", |c| c.frame().is_some_and(|f| f.steps == steps + 1));
        assert_eq!(c.frame().unwrap().servo("left axle").unwrap().0, -0.25);
        assert!(c.jog_error().is_none());
        // Reset returns to the file's control target.
        c.act(RunAction::Reset).unwrap();
        wait(&mut c, "reset", |c| c.frame().is_some() && c.phase() == Phase::Paused);
        assert_eq!(c.frame().unwrap().servo("left axle").unwrap().0, 0.0);
    }

    fn preset(id: &str) -> Result<(crate::robot::Loaded, PresetRun), String> {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let p = crate::robot_preset::select(&root.join(crate::robot_preset::PRESETS), &root, id)?;
        crate::robot::load_preset(p, &root)
    }

    #[test]
    fn preset_session_steps_one_chunk_ends_at_its_horizon_and_resets() {
        // Refusals name the id (and the mode).
        let e = preset("pendulum-live").err().unwrap();
        assert!(e.contains("`pendulum-live`") && e.contains("mode `live`"), "{e}");
        let e = preset("no-such-preset").err().unwrap();
        assert!(e.contains("unknown robot preset `no-such-preset`"), "{e}");
        // The smallest embedded preset: pendulum scene + embedded config, no task → EmbeddedSession.
        let (loaded, run) = preset("pendulum-embedded").unwrap();
        assert_eq!((run.kind(), run.seed), ("EmbeddedSession", 0));
        assert!(loaded.geometry.iter().any(|g| g.is_some()));
        let chunk = run.chunk_steps() as u64;
        assert_eq!(chunk, run.config.report_every.clamp(1, 40) as u64);
        let horizon = run.config.steps as u64 / chunk;
        let mut c = RunController::spawn_preset(Arc::new(run));
        assert!(c.jog("pivot", 0.0).unwrap_err().contains("declared controller recipe"));
        c.act(RunAction::Step).unwrap();
        wait(&mut c, "first step", |c| c.frame().is_some_and(|f| f.steps == 1));
        let f = c.frame().unwrap();
        let step_s = c.preset().unwrap().config.step_s;
        assert_eq!(f.completed_steps, Some(chunk));
        assert!((f.time - c.chunk_s()).abs() < 1e-12 && (f.time - chunk as f64 * step_s).abs() < 1e-12, "t = {}", f.time);
        // Every frame link maps to a loaded link by name.
        assert!(f.unmatched.is_empty(), "unmatched {:?}", f.unmatched);
        assert!(f.poses.iter().all(Option::is_some));
        // Run to the horizon: a distinct ended phase with its reason; Run is then refused naming Reset.
        c.act(RunAction::Start).unwrap();
        wait(&mut c, "horizon", |c| c.phase() == Phase::Ended);
        let f = c.frame().unwrap();
        assert_eq!((f.steps, f.completed_steps), (horizon, Some(horizon * chunk)));
        let end = c.end().unwrap();
        assert_eq!(end["kind"], "horizon");
        assert!(end["message"].as_str().unwrap().contains("horizon reached"));
        assert!(c.act(RunAction::Start).unwrap_err().contains("Reset"));
        assert!(c.act(RunAction::Step).unwrap_err().contains("Reset"));
        // Reset rebuilds from the same parsed files and seed: t = 0, generation + 1.
        let old = c.frame().unwrap().clone();
        c.act(RunAction::Reset).unwrap();
        assert!(!accept(c.generation(), &old));
        wait(&mut c, "reset", |c| c.frame().is_some() && c.phase() == Phase::Paused);
        let f = c.frame().unwrap();
        assert_eq!((f.generation, f.steps, f.completed_steps, f.time), (1, 0, Some(0), 0.0));
        assert!(c.end().is_none());
        c.act(RunAction::Step).unwrap();
        wait(&mut c, "step after reset", |c| c.frame().is_some_and(|f| f.steps == 1));
        assert_eq!(c.frame().unwrap().completed_steps, Some(chunk));
    }

    /// The one motion handler on the full-robot robot-measured-400hz preset
    /// (motion_commands, key vectors and the packet heartbeat from presets.json):
    /// accept, refuse by name, Stop, and one heartbeat increment per packet.
    #[test]
    fn preset_motion_requests_validate_against_session_bounds_and_advance_the_heartbeat() {
        let (_, run) = preset("robot-measured-400hz").unwrap();
        let mut c = RunController::spawn_preset(Arc::new(run));
        let e = c.motion(MotionRequest::Key('w')).unwrap_err();
        assert!(e.contains("no built session"), "{e}");
        c.act(RunAction::Step).unwrap();
        wait(&mut c, "first step", |c| c.frame().is_some_and(|f| f.steps == 1) && c.drive().is_some());
        let drive = c.drive().unwrap().clone();
        let m = drive.motion.as_ref().expect("400hz declares motion_commands");
        assert_eq!(m.source, robot_motion::Source::Preset);
        let [fwd, lat, yaw] = m.channels.each_ref().map(|ch| ch.index);
        let hb = drive.heartbeat.as_ref().expect("400hz declares motion_heartbeat").index;
        let initial = drive.inputs[hb].initial;
        assert_eq!(c.frame().unwrap().inputs[hb], initial + 1.0, "one packet sent");
        // W: the declared vector [0.1, 0, 0] becomes the held forward_speed (paused: republished without stepping).
        c.motion(MotionRequest::Key('w')).unwrap();
        wait(&mut c, "W held", |c| c.frame().is_some_and(|f| f.inputs[fwd] == 0.1));
        let f = c.frame().unwrap();
        assert_eq!((f.steps, f.inputs[lat], f.inputs[yaw]), (1, 0.0, 0.0));
        assert_eq!(c.motion_json()["requested"], json!([0.1, 0.0, 0.0]));
        // Refusals name the channel (and bounds); nothing is clamped or sent.
        let f_ch = &m.channels[0];
        let e = c.motion(MotionRequest::Channels([(f_ch.name.clone(), f_ch.upper + 1.0)].into())).unwrap_err();
        assert!(e.contains("`command.forward_speed`") && e.contains(&f_ch.bounds()) && e.contains("not clamped"), "{e}");
        let e = c.motion(MotionRequest::Channels([("command.jump".to_string(), 0.0)].into())).unwrap_err();
        assert!(e.contains("unknown channel `command.jump`"), "{e}");
        let other = drive.inputs.iter().enumerate().find(|(i, _)| ![fwd, lat, yaw, hb].contains(i)).map(|(_, ch)| ch.name.clone()).expect("a non-motion input");
        let e = c.motion(MotionRequest::Channels([(other.clone(), 0.0)].into())).unwrap_err();
        assert!(e.contains(&format!("`{other}`")) && e.contains("not a motion command channel"), "{e}");
        let e = c.motion(MotionRequest::Channels([(f_ch.name.clone(), f64::NAN)].into())).unwrap_err();
        assert!(e.contains("not finite"), "{e}");
        assert_eq!(c.motion_json()["last_refusal"], json!(e));
        assert_eq!(c.motion_json()["requested"], json!([0.1, 0.0, 0.0]));
        // The heartbeat advances by one across one step; the request stays held.
        let before = c.frame().unwrap().inputs[hb];
        c.act(RunAction::Step).unwrap();
        wait(&mut c, "second step", |c| c.frame().is_some_and(|f| f.steps == 2));
        let f = c.frame().unwrap();
        assert_eq!((f.inputs[hb], f.inputs[fwd]), (before + 1.0, 0.1));
        // Stop: every motion channel to zero.
        c.motion(MotionRequest::Stop).unwrap();
        wait(&mut c, "stop", |c| c.frame().is_some_and(|f| f.inputs[fwd] == 0.0));
        let f = c.frame().unwrap();
        assert_eq!([f.inputs[fwd], f.inputs[lat], f.inputs[yaw]], [0.0; 3]);
        assert!(c.motion_json()["last_apply_error"].is_null());
    }

    /// Save through the one controller handler the button, system_ui and REST
    /// use: the files re-read as the shared recording types with the run's
    /// completed_steps; protected paths and existing files are refused.
    #[test]
    fn preset_recordings_save_the_shared_type_and_refuse_protected_paths_and_overwrites() {
        use sim_runtime::embedded::EmbeddedRecording;
        use sim_runtime::environment::EnvironmentRecording;
        let dir = std::env::temp_dir().join(format!("robot-recording-{}-{}", std::process::id(), robot_recording::now_ms()));
        let wait_save = |c: &mut RunController| wait(c, "save", |c| c.save_pending().is_none());
        // --robot FILE keeps no recording.
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let model = crate::robot::load(&root.join("examples/wheeled-robot/baseline/robot.simrobot.json")).unwrap().model;
        let e = RunController::spawn(model).save_recording(None, None).unwrap_err();
        assert!(e.contains("`--robot FILE`"), "{e}");
        // Environment preset (a task): EmbeddedEnvironment::episode_recording().
        let (_, run) = preset("pendulum-environment").unwrap();
        assert_eq!(run.kind(), "EmbeddedEnvironment");
        let mut c = RunController::spawn_preset(Arc::new(run));
        let e = c.save_recording(None, None).unwrap_err();
        assert!(e.contains("no built session yet"), "{e}");
        assert_eq!(c.recording_json()["error"], json!(e));
        for n in 1..=3 {
            c.act(RunAction::Step).unwrap();
            wait(&mut c, "step", |c| c.frame().is_some_and(|f| f.steps == n));
        }
        let steps = c.frame().unwrap().completed_steps.unwrap();
        assert!(steps > 0);
        let path = dir.join("env.json");
        assert_eq!(c.save_recording(Some(path.to_str().unwrap()), Some("three chunks")).unwrap(), path);
        wait_save(&mut c);
        let saved = c.saved().unwrap_or_else(|| panic!("not saved: {:?}", c.save_error())).clone();
        assert_eq!((saved.kind.as_str(), saved.completed_steps, saved.replayable, &saved.path), ("sampled_environment_recording", steps as usize, true, &path));
        let text = std::fs::read_to_string(&path).unwrap();
        let record: EnvironmentRecording = serde_json::from_str(&text).unwrap();
        assert_eq!((record.runtime.completed_steps, record.error.is_none()), (steps as usize, true));
        // The file is the shared type exactly as serde_json writes it.
        assert_eq!(serde_json::to_string(&record).unwrap(), text);
        let meta: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("env.meta.json")).unwrap()).unwrap();
        assert_eq!((meta["preset"]["id"].as_str(), meta["note"].as_str(), meta["completed_steps"].as_u64(), meta["seed"].as_u64()), (Some("pendulum-environment"), Some("three chunks"), Some(steps), Some(0)));
        assert_eq!(meta["runtime_identity"], json!(record.runtime.runtime_identity));
        let state = c.recording_json();
        assert_eq!((state["last_saved"]["completed_steps"].as_u64(), state["last_saved"]["path"].as_str()), (Some(steps), path.to_str()));
        // Never overwritten: the second save to the same path is refused by the writer and the file is unchanged.
        c.act(RunAction::Step).unwrap();
        wait(&mut c, "step", |c| c.frame().is_some_and(|f| f.steps == 4));
        c.save_recording(Some(path.to_str().unwrap()), None).unwrap();
        wait_save(&mut c);
        let e = c.save_error().unwrap();
        assert!(e.contains("already exists") && e.contains("never overwritten"), "{e}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert_eq!(c.saved().unwrap().completed_steps, steps as usize);
        // Protected directories, relative or through `..`, are refused before anything is sent.
        for p in ["examples/interactive/x.json", "runs/../cad/x.json", "web/x.json"] {
            let e = c.save_recording(Some(p), None).unwrap_err();
            assert!(e.contains("refused") && e.contains("never written under examples/, cad/ or web/"), "{p}: {e}");
            assert!(!c.preset().unwrap().root.join(p).exists());
        }
        assert!(c.save_recording(Some(dir.join("x.meta.json").to_str().unwrap()), None).unwrap_err().contains(".meta.json"));
        // The default location rule, without writing.
        let t = robot_recording::target(&c.preset().unwrap().root, "pendulum-environment", None, 1_790_748_502_729).unwrap();
        assert!(t.ends_with("runs/robot-presets/pendulum-environment/20260930T060822.729Z.json"), "{}", t.display());
        // Session preset (no task): EmbeddedSession::recording().
        let (_, run) = preset("pendulum-embedded").unwrap();
        let mut c = RunController::spawn_preset(Arc::new(run));
        c.act(RunAction::Step).unwrap();
        wait(&mut c, "step", |c| c.frame().is_some_and(|f| f.steps == 1));
        let steps = c.frame().unwrap().completed_steps.unwrap() as usize;
        let path = dir.join("session.json");
        c.save_recording(Some(path.to_str().unwrap()), None).unwrap();
        wait_save(&mut c);
        let record: EmbeddedRecording = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!((record.kind.as_str(), record.version, record.completed_steps), ("embedded_session", 3, steps));
        assert_eq!((c.saved().unwrap().kind.as_str(), c.saved().unwrap().replayable), ("embedded_session", true));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Replay through the one controller handler the buttons, system_ui and
    /// REST use, on both kinds: a run with an input change saved and replayed
    /// reaches done with the runtime's verdict and the same completed_steps;
    /// refusals during a replay name it; Cancel gives cancelled (never done);
    /// mismatches are refused with the runtime's message (environment) or the
    /// labelled identity check (session). Inputs are changed through
    /// `Sim::set_action` (the motion handler's setter): the pendulum presets
    /// have typed inputs but no motion config.
    #[test]
    fn preset_replay_reaches_the_runtime_verdict_cancels_and_refuses_mismatches() {
        let dir = std::env::temp_dir().join(format!("robot-replay-{}-{}", std::process::id(), robot_recording::now_ms()));
        let changed = |c: &RunController| -> Vec<f64> { c.drive().unwrap().inputs.iter().map(|ch| ch.initial + 0.5 * (ch.upper - ch.initial)).collect() };
        // Steps `n` chunks, changing every input after the first, then saves to `name`.
        let record = |c: &mut RunController, n: u64, name: &str| -> (std::path::PathBuf, u64) {
            c.act(RunAction::Step).unwrap();
            wait(c, "step", |c| c.frame().is_some_and(|f| f.steps == 1) && c.drive().is_some());
            let values = changed(c);
            c.set_inputs(values.clone());
            wait(c, "inputs", |c| c.frame().is_some_and(|f| f.inputs == values));
            for k in 2..=n {
                c.act(RunAction::Step).unwrap();
                wait(c, "step", |c| c.frame().is_some_and(|f| f.steps == k));
            }
            let path = dir.join(name);
            c.save_recording(Some(path.to_str().unwrap()), Some("input changed after chunk 1")).unwrap();
            wait(c, "save", |c| c.save_pending().is_none());
            assert!(c.save_error().is_none(), "{:?}", c.save_error());
            (path, c.frame().unwrap().completed_steps.unwrap())
        };
        let replayed = |c: &mut RunController| wait(c, "replay end", |c| c.replay_state().phase != ReplayPhase::Replaying);

        // Environment preset (pendulum-environment): EmbeddedEnvironment::prepare_replay.
        let (_, run) = preset("pendulum-environment").unwrap();
        let mut c = RunController::spawn_preset(Arc::new(run));
        let (env_path, steps) = record(&mut c, 3, "env.json");
        let text = std::fs::read_to_string(&env_path).unwrap();
        let rec: sim_runtime::environment::EnvironmentRecording = serde_json::from_str(&text).unwrap();
        assert!(!rec.runtime.input_events.is_empty(), "the input change is in the recording");
        let generation = c.generation();
        c.replay(None, Some(env_path.to_str().unwrap())).unwrap();
        assert_eq!((c.generation(), c.replay_state().phase), (generation + 1, ReplayPhase::Replaying));
        // Refused during the replay, naming it.
        for e in [c.check_motion_request(&MotionRequest::Key('w')).unwrap_err(), c.save_recording(None, None).unwrap_err(), c.replay(None, Some(env_path.to_str().unwrap())).unwrap_err(),
            c.act(RunAction::Pause).unwrap_err(), c.act(RunAction::Step).unwrap_err(), c.act(RunAction::Start).unwrap_err()] {
            assert!(e.contains("replay") && e.contains("in progress"), "{e}");
        }
        replayed(&mut c);
        let r = c.replay_state().clone();
        assert_eq!(r.phase, ReplayPhase::Done, "{r:?}");
        assert!(r.replaced && r.verdict.as_deref().unwrap().starts_with("passed the shared runtime's replay checks: EmbeddedEnvironment::prepare_replay"), "{r:?}");
        assert_eq!((r.completed_steps, r.recorded_completed_steps, r.completed, r.total, r.unit), (Some(steps), Some(steps), 3, Some(3), Some("actions")));
        let m = r.measured.as_ref().expect("the sidecar has final_frame");
        assert_eq!(m["label"], "measured difference, not a pass criterion");
        // The replayed run is the current paused run at the recorded state and inputs; Run works again.
        wait(&mut c, "replayed frame", |c| c.frame().is_some_and(|f| f.generation == generation + 1 && f.completed_steps == Some(steps)));
        assert_eq!((c.phase(), c.frame().unwrap().inputs.clone()), (Phase::Paused, changed(&c)));
        assert!(c.check(RunAction::Start).is_ok() && c.check_save().is_ok());
        // Mismatch: the same recording with a changed task is refused verbatim by the runtime; the run is unchanged.
        let mut v: Value = serde_json::from_str(&text).unwrap();
        v["task"]["rewards"][0]["weight_per_s"] = json!(3.0);
        let other = dir.join("env-other-task.json");
        std::fs::write(&other, v.to_string()).unwrap();
        c.replay(None, Some(other.to_str().unwrap())).unwrap();
        replayed(&mut c);
        let r = c.replay_state().clone();
        assert_eq!((r.phase, r.replaced), (ReplayPhase::Failed, false));
        assert_eq!(r.error.as_deref(), Some("EmbeddedEnvironment::prepare_replay refused it: replay must match loaded robot, controller and task"));
        wait(&mut c, "republished", |c| c.frame().is_some_and(|f| f.generation == c.generation()));
        assert_eq!((c.phase(), c.frame().unwrap().completed_steps), (Phase::Paused, Some(steps)));

        // Session preset (pendulum-policy): viewer identity check, then EmbeddedSession::prepare_replay.
        let (_, run) = preset("pendulum-policy").unwrap();
        assert_eq!(run.kind(), "EmbeddedSession");
        let mut c = RunController::spawn_preset(Arc::new(run));
        let (path, steps) = record(&mut c, 6, "session.json");
        // A recording of the other kind is refused naming both kinds.
        c.replay(None, Some(env_path.to_str().unwrap())).unwrap();
        replayed(&mut c);
        let e = c.replay_state().error.clone().unwrap();
        assert!(e.contains("`sampled_environment_recording`") && e.contains("`embedded_session`"), "{e}");
        c.replay(None, Some(path.to_str().unwrap())).unwrap();
        replayed(&mut c);
        let r = c.replay_state().clone();
        assert_eq!(r.phase, ReplayPhase::Done, "{r:?}");
        assert!(r.verdict.as_deref().unwrap().contains("replay_expected satisfied"), "{r:?}");
        assert_eq!((r.completed_steps, r.recorded_completed_steps, r.completed, r.total), (Some(steps), Some(steps), steps, Some(steps)));
        wait(&mut c, "replayed frame", |c| c.frame().is_some_and(|f| f.completed_steps == Some(steps)));
        assert_eq!(c.frame().unwrap().inputs, changed(&c));
        // Cancel: stops between chunks; cancelled, never done; the partial run is held until Reset.
        c.replay(None, Some(path.to_str().unwrap())).unwrap();
        c.cancel_replay().unwrap();
        replayed(&mut c);
        let r = c.replay_state().clone();
        assert_eq!(r.phase, ReplayPhase::Cancelled, "{r:?}");
        assert!(r.completed < steps && r.verdict.as_deref().unwrap().starts_with("cancelled"), "{r:?}");
        assert!(c.act(RunAction::Start).unwrap_err().contains("cancelled partial replay"));
        assert!(c.cancel_replay().unwrap_err().contains("no replay in progress"));
        c.act(RunAction::Reset).unwrap();
        assert_eq!(c.replay_state().phase, ReplayPhase::Idle);
        wait(&mut c, "reset", |c| c.frame().is_some_and(|f| f.completed_steps == Some(0)) && c.phase() == Phase::Paused);
        assert!(c.check(RunAction::Start).is_ok());
        // Another preset's recording (pendulum-embedded: another config) is refused by the labelled identity check.
        let (_, run) = preset("pendulum-embedded").unwrap();
        let mut e = RunController::spawn_preset(Arc::new(run));
        let (embedded, _) = { e.act(RunAction::Step).unwrap(); wait(&mut e, "step", |c| c.frame().is_some()); let p = dir.join("embedded.json"); e.save_recording(Some(p.to_str().unwrap()), None).unwrap(); wait(&mut e, "save", |c| c.save_pending().is_none()); (p, ()) };
        c.replay(None, Some(embedded.to_str().unwrap())).unwrap();
        replayed(&mut c);
        let e = c.replay_state().error.clone().unwrap();
        assert!(e.starts_with("refused by the viewer identity check (as sim-web's): the recording's config (controller recipe) differs from preset `pendulum-policy`'s"), "{e}");
        // --robot FILE: replay refused naming the mode.
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let model = crate::robot::load(&root.join("examples/wheeled-robot/baseline/robot.simrobot.json")).unwrap().model;
        assert!(RunController::spawn(model).replay(None, Some("x.json")).unwrap_err().contains("`--robot FILE`"));
        // Listing: *.json but not *.meta.json, with the sidecar summary.
        let listed_root = dir.join("root");
        let preset_dir = listed_root.join(robot_recording::DIR).join("pendulum-policy");
        std::fs::create_dir_all(&preset_dir).unwrap();
        for f in ["session.json", "session.meta.json"] {
            std::fs::copy(dir.join(f), preset_dir.join(f)).unwrap();
        }
        let list = robot_recording::list(&listed_root, "pendulum-policy").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].file.as_str(), list[0].meta.as_ref().unwrap()["completed_steps"].as_u64()), ("session.json", Some(steps)));
        assert!(robot_recording::list(&listed_root, "none").unwrap().is_empty());
        assert!(robot_recording::replay_source(&listed_root, "p", Some("../x.json"), None).unwrap_err().contains("bare file name"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Timing only (debug build): `ROBOT_PRESET=<id> cargo test -p sim-spatial --lib robot_run::tests::measure_full_robot_preset -- --ignored --nocapture`
    /// (default robot-measured-400hz). Builds on this thread, as the run thread does.
    #[test]
    #[ignore]
    fn measure_full_robot_preset() {
        let id = std::env::var("ROBOT_PRESET").unwrap_or_else(|_| "robot-measured-400hz".into());
        println!("preset {id}");
        let t0 = Instant::now();
        let (loaded, run) = preset(&id).unwrap();
        let parse = t0.elapsed().as_secs_f64();
        println!("parse+load: {parse:.2} s, {} links, {} chunk steps × {} s = {} s, {}", loaded.model.links.len(), run.chunk_steps(), run.config.step_s, run.chunk_s(), run.kind());
        let run = Arc::new(run);
        let t1 = Instant::now();
        let mut sim = match Sim::build(&Source::Preset(run.clone()), &mut None) {
            Ok(sim) => sim,
            Err(e) => panic!("build failed after {:.2} s: {e}", t1.elapsed().as_secs_f64()),
        };
        println!("build: {:.2} s; inputs {:?}", t1.elapsed().as_secs_f64(), match &sim { Sim::Environment { env, held, .. } => (env.inputs().iter().map(|c| c.name.clone()).collect::<Vec<_>>(), held.clone()), _ => (vec![], vec![]) });
        let links: Vec<String> = loaded.model.links.iter().map(|l| l.name.clone()).collect();
        let f = sim.frame(&links, 0, 0).unwrap();
        println!("frame: unmatched {:?}, links without pose {:?}", f.unmatched, f.poses.iter().zip(&links).filter(|(p, _)| p.is_none()).map(|(_, l)| l).collect::<Vec<_>>());
        for k in 1..=5 {
            let t = Instant::now();
            sim.advance().unwrap();
            let wall = t.elapsed().as_secs_f64();
            let frame_t = Instant::now();
            let f = sim.frame(&links, 0, k).unwrap();
            println!("chunk {k}: advance {wall:.3} s, frame {:.3} s, sim t {:.4} s, rtf {:.4}, ended {:?}", frame_t.elapsed().as_secs_f64(), f.time, run.chunk_s() / wall, sim.ended());
        }
    }
}

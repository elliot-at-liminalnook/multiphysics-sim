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
        Self::spawn_source(Source::Preset(run.clone()), model, Some(run))
    }
    fn spawn_source(source: Source, model: PhysicalModel, preset: Option<Arc<PresetRun>>) -> Self {
        let (tx, rx) = mpsc::channel();
        let status = Status { phase: Phase::Idle, generation: 0, rtf: None, error: None, end: None };
        let shared = Arc::new(Mutex::new(Published { status: status.clone(), frame: None, jog_error: None, drive: None, motion_error: None }));
        let out = shared.clone();
        let chunk_s = source.chunk_s();
        let links: Vec<String> = model.links.iter().map(|l| l.name.clone()).collect();
        std::thread::Builder::new()
            .name("robot-run".into())
            .spawn(move || worker(source, links, rx, out))
            .expect("spawn robot run thread");
        Self { tx, shared, generation: 0, running: false, frame: None, status, model, jogged: Default::default(), jog_error: None, preset, chunk_s,
            drive: None, requested: None, keys: Vec::new(), keys_physical: false, motion_refusal: None, motion_error: None }
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

    /// Why an action is unavailable now (`Ok` when it can be sent).
    pub fn check(&self, action: RunAction) -> Result<(), String> {
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
                self.status = Status { phase: Phase::Building, generation: self.generation, rtf: None, error: None, end: None };
                Command::Reset { generation: self.generation }
            }
        };
        self.tx.send(command).map_err(|_| "the run thread has stopped".to_string())
    }

    /// Takes the worker's latest status and frame; returns true when the
    /// displayed frame changed (a stale-generation frame is never accepted).
    pub fn poll(&mut self) -> bool {
        let published = self.shared.lock().unwrap_or_else(|p| p.into_inner());
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
        match fresh {
            Some(f) => {
                self.frame = Some(f.clone());
                true
            }
            None => false,
        }
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
        matches!(self.status.phase, Phase::Building | Phase::Running) || self.running
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
        match self {
            Sim::Environment { held, .. } => *held = action,
            Sim::Session { session, .. } => session.set_inputs(&action)?,
            Sim::Robot(_) => {}
        }
        Ok(())
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
        match s.advance() {
            Ok(()) => {
                steps += 1;
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
                set(status(Phase::Failed, generation, None, Some(format!("advance failed at t = {:.3} s: {e}", s.time()))), None);
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

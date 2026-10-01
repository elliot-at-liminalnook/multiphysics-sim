//! Robot mode for a planar (v2) `.simrobot.json` (`--robot FILE`): RoboCAD's
//! planar summary (bodies with mass, centre, planar inertia and section
//! outline in mm in the CAD working plane, revolute joints with pivots).
//! `PhysicalModel::parse` refuses these files, so robot mode reads them as
//! sim-phenomena's `CadModel` from the same bytes (version rule:
//! `sim_domain_robot::model::simrobot_version`) and runs them through the one
//! shared planar build, `cad_robot::build_planar` (`CadRobot`): no physics
//! of its own, and never the v3 physical model.
//!
//! One run thread (`jobs::RunThread`, "robot-run (planar v2)") builds the
//! `CadRobot` from the loaded model and advances it on simulation time with
//! the planar viewer's rules ([`PACING`]); the UI thread sends commands and
//! draws the generation-stamped frames it publishes ([`PlanarFrame`]) as
//! gizmos: outlines, centres of mass, chain-tip contact points, a ground grid.
use crate::jobs::{RunThread, Stamped};
use crate::robot_run::{RunAction, SpeedRequest, speed_target};
use bevy::prelude::*;
use serde::Serialize;
use serde_json::{Value, json};
use sim_phenomena::scenarios::cad_robot::{CadModel, CadRobot, PLANAR_BANDWIDTH_HZ, PLANAR_DAMPING_RATIO, build_planar};
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// `robot_state.format.name` for a planar file.
pub const FORMAT_NAME: &str = "planar v2 summary";
/// What a planar run is, stated from `CadRobot::build` (cad_robot.rs); shown
/// verbatim in `robot_state.format.fidelity` and at the top of the inspector.
pub const FIDELITY: &str = "planar v2 summary: a 2-D multibody in the CAD working plane built by sim-phenomena's CadRobot (cad_robot::build_planar) — a root body (fixed when the CAD marks it ground, else a free planar rigid body under gravity resting on two compliant point contacts at its lowest outline points), serial chains of links through the revolute and continuous joints (other joint types are treated as fixed, with a warning; joints not reachable from the root are not simulated), every joint held at its target (the CAD pose until moved) by a PD servo on the seam with gains sized from the inertia it carries for a 6 Hz bandwidth and damping ratio 1, the target clamped to the CAD limits with soft end stops, the torque to the motor's stall-torque / no-load-speed line when the joint has a motor, and every joint's torque through a 100 Hz servo element capped at 50 N·m; a compliant point contact (friction 0.8) at every chain tip; backward Euler with a 1 ms step. Uncalibrated, no collision between links; not the v3 physical model (no 3-D inertia, collision geometry, joint friction or backlash, motor electrical models, sensors or stress results).";
/// The header's short form of [`FIDELITY`] (the full text is in Source and `robot_state.format`).
pub const HEADER_LABEL: &str = "planar v2 summary · uncalibrated · not the v3 physical model";
/// The run thread's name.
pub const THREAD: &str = "robot-run (planar v2)";
/// Simulation time advanced per grid step (s).
pub const GRID_S: f64 = 0.02;
/// Real time counted per tick at most (s), before the speed scale.
pub const MAX_REAL_DT_S: f64 = 0.05;
/// The run thread's tick (about 60 Hz).
pub const TICK: Duration = Duration::from_micros(16_667);
/// Arrow-key target steps (rad): plain and with Shift.
pub const KEY_STEP_RAD: f64 = 0.01;
pub const KEY_STEP_SHIFT_RAD: f64 = 0.05;
pub const PACING: &str = "planar v2 pacing (the planar viewer's rule, kept unchanged): every ~60 Hz tick adds min(real time since the last tick, 0.05 s) × speed_scale to an accumulator; when it holds at least one 0.02 s grid step the robot advances exactly one grid step and the accumulator is capped at one grid step, so a slow model never stalls the window and at most one grid step runs per tick (at most ~1.2 × real time at 60 Hz, whatever the scale). Step advances one grid step while paused. achieved_rate = sim seconds / wall seconds since Run or the last speed change; compute_s_per_sim_s = wall seconds spent inside CadRobot::advance per simulated second.";
pub const JOG_RULE: &str = "planar v2 joint targets: CadRobot::set_target on the run thread; a delta is added to the joint's current target on the thread (the arrow keys: ↑/↓ ±0.01 rad, Shift ±0.05 rad; the jog buttons ±0.05 rad), target sets it. Targets are not refused at the CAD limits: the PD law clamps them to the limits (with soft end stops), as the planar build does. Reset rebuilds from the loaded model, so targets return to the CAD pose. Nothing is written.";

// Every refusal of an action without a v2 meaning, by name: "planar v2 file: <what> is not available (<why>)".
pub const MOTION: &str = "planar v2 file: motion requests (WASD keys, motion:*, robot_input) are not available (they drive a robot preset's Rust controller; a planar v2 file has no PhysicalModel and no preset)";
pub const SAVE_RECORDING: &str = "planar v2 file: save recording is not available (it saves a robot preset's session recording; a planar v2 run has none)";
pub const REPLAY: &str = "planar v2 file: replay and the recordings list are not available (they re-execute a robot preset's saved session recordings; a planar v2 run has none)";
pub const GAIT: &str = "planar v2 file: gait preview is not available (it poses a robot preset's scene with the shared KinematicMirror; no PhysicalModel — it needs a v3 export in a preset)";
pub const RECORDED: &str = "planar v2 file: the recorded timeline is not available (it plays a recorded preset's capture)";
pub const JOINT_FRAMES: &str = "planar v2 file: the joint-frame overlay is not available (it reads PhysicalRobot::joint_frames; it needs a v3 export)";
pub const DEFLECTIONS: &str = "planar v2 file: the deflection overlay is not available (it reads PhysicalRobot::deflections of flexible links; it needs a v3 export)";
pub const GRAPHS: &str = "planar v2 file: graphs are not available (robot_graphs charts a PhysicalRobot run's or a preset session's frames; no planar series is defined; it needs a v3 export)";
/// Refusal of the stress overlay (keys S and H) for a planar file.
pub const STRESS: &str = "planar v2 file: the stress overlay is not available (stress needs a v3 physical export: `sim-cad run` writes a .simresult.json only for v3 files)";
/// Why the hardware mirror cannot pose a planar file (for `robot::hardware::mirror::scene_of`).
pub const NO_MIRROR: &str = "planar v2 file: the leg mirror is not available (it poses a robot preset's scene; a planar v2 summary has no scene or PhysicalModel)";
/// `robot_state.unavailable` for a planar file.
pub const UNAVAILABLE: [(&str, &str); 10] = [
    ("motion", MOTION),
    ("save_recording", SAVE_RECORDING),
    ("replay", REPLAY),
    ("gait_preview", GAIT),
    ("recorded", RECORDED),
    ("overlay_joints", JOINT_FRAMES),
    ("overlay_deflections", DEFLECTIONS),
    ("overlay_stress", STRESS),
    ("graphs", GRAPHS),
    ("leg_mirror", NO_MIRROR),
];

/// A planar file as read on the worker thread (never built there: the run thread builds it).
pub struct PlanarLoaded {
    pub model: CadModel,
    /// The file's `version` as stored (None when absent: read as 2).
    pub declared_version: Option<u64>,
    /// The version the shared rule reads (`simrobot_version`).
    pub version: u32,
    /// The file's `plane` block as stored (null when absent).
    pub plane: Value,
    /// The file's `unit` as stored.
    pub unit: Option<String>,
}

/// Reads a planar file from the bytes the worker read (`raw` is the same bytes
/// as JSON). Refused, naming the path: a unit other than mm (the planar build
/// reads millimetres), no bodies, or a revolute/continuous joint naming an
/// unknown body (the build's own errors, checked here so a failed reload keeps
/// the last good model; the build drops other joint types before it looks).
pub fn load_bytes(path: &Path, bytes: &[u8], raw: &Value) -> Result<PlanarLoaded, String> {
    let name = path.display();
    let model: CadModel = serde_json::from_slice(bytes).map_err(|e| format!("{name}: planar v2 file: {e}"))?;
    let unit = raw.get("unit").and_then(Value::as_str).map(str::to_string);
    if let Some(u) = unit.as_deref().filter(|u| *u != "mm") {
        return Err(format!("{name}: planar v2 file declares unit `{u}`; the planar build reads millimetres (unit `mm`) only"));
    }
    if model.bodies.is_empty() {
        return Err(format!("{name}: planar v2 file: the model has no bodies"));
    }
    if let Some(j) = model.joints.iter().filter(|j| matches!(j.kind.as_str(), "revolute" | "continuous")).find(|j| !model.bodies.iter().any(|b| b.name == j.child)) {
        return Err(format!("{name}: planar v2 file: joint {} names an unknown body {}", j.name, j.child));
    }
    Ok(PlanarLoaded {
        declared_version: raw.get("version").and_then(Value::as_u64),
        version: sim_domain_robot::model::simrobot_version(raw),
        plane: raw.get("plane").cloned().unwrap_or(Value::Null),
        unit,
        model,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanarPhase {
    /// The run thread is building the robot (at open and after Reset).
    #[default]
    Building,
    Running,
    Paused,
    /// The build or an advance failed (`error`, verbatim); Reset rebuilds.
    Failed,
}
impl PlanarPhase {
    pub fn name(self) -> &'static str {
        match self {
            PlanarPhase::Building => "building",
            PlanarPhase::Running => "running",
            PlanarPhase::Paused => "paused",
            PlanarPhase::Failed => "failed",
        }
    }
}

/// Commands to the planar run thread.
#[derive(Clone, Debug, PartialEq)]
pub enum PlanarCommand {
    Start,
    Pause,
    /// One grid step (paused).
    Step,
    /// Rebuild from the loaded model at `generation`, paused at t = 0.
    Reset { generation: u64 },
    Speed(f64),
    /// Add `delta` to joint `joint`'s current target (index in the built joint order).
    Nudge { joint: usize, delta: f64 },
    SetTarget { joint: usize, target: f64 },
}

/// One body's pose: world (m, in the plane, ground at y = 0) and angle (rad).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PlanarPose {
    pub body: usize,
    pub com: [f64; 2],
    pub angle: f64,
}

/// What the run thread publishes: status and the built robot's state, stamped
/// with the generation of the command it follows.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PlanarFrame {
    pub generation: u64,
    /// Publication counter of this thread (the UI copies only a new one).
    pub seq: u64,
    pub phase: PlanarPhase,
    /// A robot is built (false while building and after a failed build).
    pub built: bool,
    pub time: f64,
    /// Grid steps advanced in this generation.
    pub steps: u64,
    /// Built joint order (chain order), as `CadRobot::joint_names`.
    pub joint_names: Vec<String>,
    pub joint_angles: Vec<f64>,
    pub targets: Vec<f64>,
    /// `CadRobot::outlines`: per body index, a closed polyline (m).
    pub outlines: Vec<(usize, Vec<[f64; 2]>)>,
    /// `CadRobot::poses`: root first, then per link.
    pub poses: Vec<PlanarPose>,
    /// Chain tips (the tip contact points), read through the runtime (m).
    pub tips: Vec<[f64; 2]>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
    pub root: Option<String>,
    pub root_fixed: bool,
    pub bodies: usize,
    pub joints: usize,
    pub speed_scale: f64,
    pub sim_s: f64,
    pub compute_s: f64,
    pub achieved_rate: Option<f64>,
}
impl Stamped for PlanarFrame {
    fn generation(&self) -> u64 {
        self.generation
    }
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "a panic".into())
}

/// The run thread's state.
struct Worker {
    model: CadModel,
    out: Arc<Mutex<PlanarFrame>>,
    generation: u64,
    seq: u64,
    robot: Option<CadRobot>,
    phase: PlanarPhase,
    error: Option<String>,
    warnings: Vec<String>,
    speed: f64,
    accumulator: f64,
    last_tick: Option<Instant>,
    steps: u64,
    sim_s: f64,
    compute_s: f64,
    /// (wall, sim seconds) at Run or the last speed change, for `achieved_rate`.
    anchor: Option<(Instant, f64)>,
    achieved: Option<f64>,
}
impl Worker {
    fn run(mut self, rx: mpsc::Receiver<PlanarCommand>) {
        self.build();
        loop {
            let command = if self.phase == PlanarPhase::Running {
                let wait = TICK.saturating_sub(self.last_tick.map_or(TICK, |t| t.elapsed()));
                match rx.recv_timeout(wait) {
                    Ok(c) => Some(c),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            } else {
                // Idle: blocked on the channel, which wakes at once when it closes.
                match rx.recv() {
                    Ok(c) => Some(c),
                    Err(_) => return,
                }
            };
            if let Some(c) = command {
                self.command(c);
            }
            // A stream of commands (a held arrow key) must not starve the ticks.
            if self.phase == PlanarPhase::Running && self.last_tick.is_none_or(|t| t.elapsed() >= TICK) {
                self.tick();
            }
        }
    }
    fn build(&mut self) {
        self.robot = None;
        self.phase = PlanarPhase::Building;
        self.error = None;
        self.warnings.clear();
        (self.steps, self.sim_s, self.compute_s, self.accumulator) = (0, 0.0, 0.0, 0.0);
        (self.anchor, self.achieved, self.last_tick) = (None, None, None);
        self.publish();
        let model = self.model.clone();
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || build_planar(model))) {
            Ok(Ok(robot)) => {
                self.warnings = robot.warnings.clone();
                self.robot = Some(robot);
                self.phase = PlanarPhase::Paused;
            }
            Ok(Err(e)) => self.fail(format!("build failed: {e}")),
            Err(p) => self.fail(format!("build failed: the planar build panicked: {}", panic_text(p.as_ref()))),
        }
        self.publish();
    }
    fn fail(&mut self, error: String) {
        self.phase = PlanarPhase::Failed;
        self.error = Some(error);
        self.achieved = None;
        self.anchor = None;
    }
    /// One grid step; false (and failed, the error verbatim) when the advance errs.
    fn advance(&mut self) -> bool {
        let started = Instant::now();
        let result = {
            let Some(robot) = self.robot.as_mut() else { return false };
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| robot.advance(GRID_S)))
        };
        self.compute_s += started.elapsed().as_secs_f64();
        match result {
            Ok(Ok(())) => {
                self.sim_s += GRID_S;
                self.steps += 1;
                true
            }
            // The robot stays: its last pose is still drawn.
            Ok(Err(e)) => {
                self.fail(format!("simulation stopped: {e} (Reset rebuilds from the loaded model)"));
                false
            }
            Err(p) => {
                self.fail(format!("simulation stopped: the advance panicked: {} (Reset rebuilds from the loaded model)", panic_text(p.as_ref())));
                false
            }
        }
    }
    fn tick(&mut self) {
        let now = Instant::now();
        let real = self.last_tick.map_or(0.0, |t| now.duration_since(t).as_secs_f64());
        self.last_tick = Some(now);
        self.accumulator += real.min(MAX_REAL_DT_S) * self.speed;
        let whole = (self.accumulator / GRID_S).floor();
        if whole < 1.0 {
            return;
        }
        self.accumulator -= whole * GRID_S;
        // A slow model must not stall: at most one grid step per tick.
        self.accumulator = self.accumulator.min(GRID_S);
        if self.advance() {
            if let Some((wall, sim)) = self.anchor {
                let elapsed = now.duration_since(wall).as_secs_f64();
                if elapsed >= 0.25 {
                    self.achieved = Some((self.sim_s - sim) / elapsed);
                }
            }
        }
        self.publish();
    }
    fn command(&mut self, command: PlanarCommand) {
        match command {
            PlanarCommand::Start if self.phase == PlanarPhase::Paused => {
                self.phase = PlanarPhase::Running;
                let now = Instant::now();
                self.last_tick = Some(now);
                self.anchor = Some((now, self.sim_s));
                self.achieved = None;
            }
            PlanarCommand::Pause if self.phase == PlanarPhase::Running => {
                self.phase = PlanarPhase::Paused;
                self.anchor = None;
                self.achieved = None;
            }
            PlanarCommand::Step if self.phase == PlanarPhase::Paused => {
                self.advance();
            }
            PlanarCommand::Reset { generation } => {
                self.generation = generation;
                self.build();
                return;
            }
            PlanarCommand::Speed(scale) => {
                self.speed = scale;
                if self.phase == PlanarPhase::Running {
                    self.anchor = Some((Instant::now(), self.sim_s));
                    self.achieved = None;
                }
            }
            PlanarCommand::Nudge { joint, delta } => {
                if let Some(r) = &self.robot {
                    let current = r.targets.lock().unwrap_or_else(|p| p.into_inner()).get(joint).copied();
                    if let Some(current) = current {
                        r.set_target(joint, current + delta);
                    }
                }
            }
            PlanarCommand::SetTarget { joint, target } => {
                if let Some(r) = &self.robot {
                    r.set_target(joint, target);
                }
            }
            // Start while running, Pause or Step while not paused: nothing to do (the UI checks first).
            _ => return,
        }
        self.publish();
    }
    fn publish(&mut self) {
        self.seq += 1;
        let mut f = PlanarFrame {
            generation: self.generation,
            seq: self.seq,
            phase: self.phase,
            warnings: self.warnings.clone(),
            error: self.error.clone(),
            speed_scale: self.speed,
            steps: self.steps,
            sim_s: self.sim_s,
            compute_s: self.compute_s,
            achieved_rate: self.achieved,
            bodies: self.model.bodies.len(),
            ..Default::default()
        };
        if let Some(r) = &self.robot {
            f.built = true;
            f.time = r.runtime.time;
            f.joint_names = r.joint_names.clone();
            f.joint_angles = r.joint_angles();
            f.targets = r.targets.lock().unwrap_or_else(|p| p.into_inner()).clone();
            f.outlines = r.outlines();
            f.poses = r.poses().into_iter().map(|(body, com, angle)| PlanarPose { body, com, angle }).collect();
            // The tip contact points, as the planar viewer drew them.
            f.tips = r.chains.iter().map(|c| [r.runtime.get(c.tip[0]), r.runtime.get(c.tip[1])]).collect();
            f.root = r.model.bodies.get(r.root).map(|b| b.name.clone());
            f.root_fixed = r.root_fixed;
            f.joints = r.joint_names.len();
        }
        *self.out.lock().unwrap_or_else(|p| p.into_inner()) = f;
    }
}

/// The UI side of the planar run thread: sends commands, accepts frames of the current generation.
pub struct PlanarRun {
    thread: RunThread<PlanarCommand, PlanarFrame>,
    generation: u64,
    /// Whether the UI last asked the robot to run (commands are ordered).
    running: bool,
    speed: f64,
    frame: Option<PlanarFrame>,
    seen: u64,
}
impl PlanarRun {
    /// Spawns the run thread; it builds the robot at once and waits paused at t = 0.
    pub fn spawn(model: CadModel, generation: u64, speed: f64) -> Self {
        let initial = PlanarFrame { generation, speed_scale: speed, bodies: model.bodies.len(), ..Default::default() };
        let thread = RunThread::spawn(THREAD, initial, move |rx, out| {
            let worker = Worker {
                model,
                out,
                generation,
                seq: 0,
                robot: None,
                phase: PlanarPhase::Building,
                error: None,
                warnings: Vec::new(),
                speed,
                accumulator: 0.0,
                last_tick: None,
                steps: 0,
                sim_s: 0.0,
                compute_s: 0.0,
                anchor: None,
                achieved: None,
            };
            worker.run(rx)
        });
        Self { thread, generation, running: false, speed, frame: None, seen: 0 }
    }
    /// Takes the thread's latest frame (never one of an older generation);
    /// true when the accepted frame changed.
    pub fn poll(&mut self) -> bool {
        let latest = {
            let s = self.thread.lock();
            if s.generation >= self.generation && s.seq != self.seen { Some(s.clone()) } else { None }
        };
        let Some(f) = latest else { return false };
        self.seen = f.seq;
        if f.phase == PlanarPhase::Failed {
            self.running = false;
        }
        self.frame = Some(f);
        true
    }
    pub fn frame(&self) -> Option<&PlanarFrame> {
        self.frame.as_ref()
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn speed_scale(&self) -> f64 {
        self.speed
    }
    pub fn phase(&self) -> PlanarPhase {
        match &self.frame {
            None => PlanarPhase::Building,
            // Requested but not yet seen by the thread: report the request.
            Some(f) if f.phase == PlanarPhase::Paused && self.running => PlanarPhase::Running,
            Some(f) => f.phase,
        }
    }
    /// Frames are expected (redraw while true).
    pub fn active(&self) -> bool {
        self.running || matches!(self.phase(), PlanarPhase::Building)
    }
    fn failed(&self) -> bool {
        self.frame.as_ref().is_some_and(|f| f.phase == PlanarPhase::Failed)
    }
    /// Why a run control is refused now.
    pub fn check(&self, action: RunAction) -> Result<(), String> {
        match action {
            RunAction::Start if self.failed() => Err("the planar run failed; Reset rebuilds from the loaded model before it can run again".into()),
            RunAction::Start if self.running => Err("already running".into()),
            RunAction::Pause if !self.running => Err("not running".into()),
            RunAction::Step if self.failed() => Err("the planar run failed; Reset rebuilds from the loaded model before it can step".into()),
            RunAction::Step if self.running => Err(format!("step advances one {GRID_S} s grid step only while paused; the planar run is running — pause first")),
            _ => Ok(()),
        }
    }
    /// The one run handler for a planar file (buttons, keys, `system_ui`, REST `robot_run`).
    pub fn act(&mut self, action: RunAction) -> Result<(), String> {
        self.check(action)?;
        let command = match action {
            RunAction::Start => {
                self.running = true;
                PlanarCommand::Start
            }
            RunAction::Pause => {
                self.running = false;
                PlanarCommand::Pause
            }
            RunAction::Step => PlanarCommand::Step,
            RunAction::Reset => {
                // Rebuild from the loaded model, paused at t = 0; older frames are stale.
                self.running = false;
                self.generation += 1;
                self.frame = None;
                PlanarCommand::Reset { generation: self.generation }
            }
        };
        self.send(command)
    }
    fn send(&self, command: PlanarCommand) -> Result<(), String> {
        self.thread.send(command).map_err(|_| format!("the {THREAD} thread has stopped"))
    }
    pub fn check_speed(&self, request: SpeedRequest) -> Result<f64, String> {
        speed_target(self.speed, request)
    }
    pub fn speed(&mut self, request: SpeedRequest) -> Result<(), String> {
        let scale = self.check_speed(request)?;
        self.speed = scale;
        self.send(PlanarCommand::Speed(scale))
    }
    /// The built joint index of `joint` (by name), or why a target move is refused.
    pub fn check_joint(&self, joint: &str, value: f64) -> Result<usize, String> {
        if !value.is_finite() {
            return Err(format!("joint `{joint}`: the target move must be finite"));
        }
        if self.failed() {
            return Err(format!("joint `{joint}`: the planar run failed; Reset rebuilds from the loaded model"));
        }
        let f = self.frame.as_ref().filter(|f| f.built).ok_or_else(|| format!("joint `{joint}`: the planar robot is still building"))?;
        f.joint_names.iter().position(|n| n == joint).ok_or_else(|| {
            format!("unknown joint `{joint}`; simulated joints of this planar file: {}", if f.joint_names.is_empty() { "none".into() } else { f.joint_names.join(", ") })
        })
    }
    pub fn nudge(&self, joint: &str, delta: f64) -> Result<(), String> {
        let index = self.check_joint(joint, delta)?;
        self.send(PlanarCommand::Nudge { joint: index, delta })
    }
    pub fn set_target(&self, joint: &str, target: f64) -> Result<(), String> {
        let index = self.check_joint(joint, target)?;
        self.send(PlanarCommand::SetTarget { joint: index, target })
    }
    /// `robot_state.run` for a planar file.
    pub fn json(&self) -> Value {
        let f = self.frame.as_ref();
        json!({"thread": THREAD, "model": "sim_phenomena::scenarios::cad_robot::CadRobot (build_planar)", "phase": self.phase().name(), "requested_running": self.running,
            "generation": self.generation, "time": f.filter(|f| f.built).map(|f| f.time), "steps": f.map(|f| f.steps), "grid_s": GRID_S, "speed_scale": self.speed,
            "achieved_rate": f.and_then(|f| f.achieved_rate), "compute_s_per_sim_s": f.filter(|f| f.sim_s > 0.0).map(|f| f.compute_s / f.sim_s),
            "error": f.and_then(|f| f.error.clone()), "pacing": PACING})
    }
}

/// Robot mode's state for a planar file: the loaded summary, its run, the
/// arrow keys' joint and the contacts overlay.
pub struct PlanarView {
    pub loaded: PlanarLoaded,
    pub run: PlanarRun,
    /// The joint ←/→ select and ↑/↓ move (index in the built joint order).
    pub selected_joint: usize,
    /// Red chain-tip dots (key C, overlay:contacts, robot_overlay contacts).
    pub contacts: bool,
    /// Frame the camera on the next built frame: Some(true) also moves the focus (an open), Some(false) only sets the extent (a reload keeps the camera).
    pub frame_camera: Option<bool>,
}
impl PlanarView {
    /// Spawns the run thread for a loaded file (`generation`, speed and contacts carried over by the caller).
    pub fn new(loaded: PlanarLoaded, generation: u64, speed: f64, contacts: bool, move_camera: bool) -> Self {
        let run = PlanarRun::spawn(loaded.model.clone(), generation, speed);
        Self { loaded, run, selected_joint: 0, contacts, frame_camera: Some(move_camera) }
    }
    /// The built joint names (empty until the first built frame).
    pub fn joint_names(&self) -> &[String] {
        self.run.frame().filter(|f| f.built).map(|f| f.joint_names.as_slice()).unwrap_or(&[])
    }
    pub fn selected_joint_name(&self) -> Option<&str> {
        self.joint_names().get(self.selected_joint).map(String::as_str)
    }
    /// `robot_state.format` for a planar file.
    pub fn format_json(&self) -> Value {
        json!({"version": self.loaded.version, "declared_version": self.loaded.declared_version, "name": FORMAT_NAME, "fidelity": FIDELITY,
            "build": {"function": "sim_phenomena::scenarios::cad_robot::build_planar", "bandwidth_hz": PLANAR_BANDWIDTH_HZ, "damping_ratio": PLANAR_DAMPING_RATIO},
            "warnings": self.run.frame().map(|f| f.warnings.clone()).unwrap_or_default()})
    }
    /// `robot_state.planar`: the summary as read, the latest frame and the rules.
    pub fn json(&self, selected: Option<usize>) -> Value {
        let m = &self.loaded.model;
        let bodies: Vec<Value> = m.bodies.iter().enumerate().map(|(i, b)| json!({"index": i, "id": b.id, "name": b.name, "material": b.material, "mass_kg": b.mass_kg, "com_mm": b.com,
            "inertia_zz_kg_m2": b.inertia_zz, "ground": b.ground, "outline_loops": b.outline.len(), "bbox_mm": b.bbox})).collect();
        let simulated = self.joint_names();
        let joints: Vec<Value> = m.joints.iter().map(|j| json!({"name": j.name, "type": j.kind, "parent": j.parent, "child": j.child, "pivot_mm": j.pivot2, "limits_rad": j.limits,
            "damping": j.damping, "axis_sign": j.axis_sign, "simulated": simulated.contains(&j.name),
            "motor": j.motor.as_ref().map(|x| json!({"name": x.name, "spec": x.spec, "stall_torque_n_m": x.stall_torque, "no_load_speed_rad_s": x.no_load_speed, "rotor_inertia_kg_m2": x.rotor_inertia}))})).collect();
        let f = self.run.frame().filter(|f| f.built);
        let frame = f.map(|f| json!({"generation": f.generation, "time": f.time, "steps": f.steps, "joint_names": f.joint_names, "joint_angles_rad": f.joint_angles, "targets_rad": f.targets,
            "poses": f.poses, "tips_m": f.tips, "outline_count": f.outlines.len(), "root": f.root, "root_fixed": f.root_fixed}));
        let selected_body = selected.and_then(|i| bodies.get(i).cloned());
        json!({"unit": self.loaded.unit, "plane": self.loaded.plane, "source": m.source, "body_count": m.bodies.len(), "joint_count": m.joints.len(), "simulated_joint_count": simulated.len(),
            "bodies": bodies, "joints": joints, "selected_body": selected_body, "selected_joint": self.selected_joint_name(), "frame": frame,
            "frame_axes": "x right, y up in the CAD working plane (m), ground at y = 0 (the model's lowest point); drawn in the display frame as (x, y, 0)", "jog_rule": JOG_RULE})
    }
    /// `robot_state.overlays` for a planar file.
    pub fn overlays_json(&self) -> Value {
        json!({"contacts": {"on": self.contacts, "available": true, "count": self.run.frame().map(|f| f.tips.len()), "what": "chain-tip contact points (CadRobot chains[].tip), red dots"},
            "joints": {"available": false, "reason": JOINT_FRAMES},
            "deflections": {"available": false, "reason": DEFLECTIONS},
            "stress": {"available": false, "reason": STRESS}})
    }
}

/// The palette of body outlines (as the planar viewer drew them).
const PALETTE: [Color; 6] = [Color::srgb(0.9, 0.55, 0.2), Color::srgb(0.3, 0.7, 0.95), Color::srgb(0.4, 0.85, 0.5), Color::srgb(0.9, 0.35, 0.4), Color::srgb(0.8, 0.8, 0.3), Color::srgb(0.7, 0.5, 0.9)];

/// The plane's point (x right, y up, m) in the display frame. Robot mode's
/// display frame is Y up with the model's Z-up frame rotated by RobotRoot
/// (model (x, y, z) → display (x, z, −y)); the planar working plane is the
/// model's XZ plane (RoboCAD `Plane.xz()`, the export default), so a plane
/// point (u, v) is model (u, 0, v) and display (u, v, 0).
pub fn display(p: [f64; 2]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, 0.0)
}

/// Gizmos for the latest accepted frame: ground grid, outlines (closed, one
/// colour per body; the selected body's in the accent colour), centres of
/// mass, and red chain-tip dots while contacts are on.
pub fn draw(view: &PlanarView, selected: Option<usize>, gizmos: &mut Gizmos) {
    // Ground at display y = 0: the planar build puts the model's lowest point there.
    gizmos.grid(Isometry3d::new(Vec3::ZERO, Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)), UVec2::splat(40), Vec2::splat(0.05), Color::srgba(0.45, 0.50, 0.58, 0.35));
    let Some(f) = view.run.frame().filter(|f| f.built) else { return };
    for (body, points) in &f.outlines {
        let color = if selected == Some(*body) { crate::ui_kit::ACCENT } else { PALETTE[body % PALETTE.len()] };
        let points: Vec<Vec3> = points.iter().copied().map(display).collect();
        if let (Some(first), Some(last)) = (points.first().copied(), points.last().copied()) {
            gizmos.linestrip(points, color);
            gizmos.line(last, first, color);
        }
    }
    for pose in &f.poses {
        let color = if selected == Some(pose.body) { crate::ui_kit::ACCENT } else { Color::WHITE };
        gizmos.circle(Isometry3d::from_translation(display(pose.com) + Vec3::Z * 0.001), 0.004, color);
    }
    if view.contacts {
        for tip in &f.tips {
            gizmos.circle(Isometry3d::from_translation(display(*tip) + Vec3::Z * 0.001), 0.006, Color::srgb(1.0, 0.3, 0.3));
        }
    }
}

/// Display-frame bounds of a frame's outlines, centres and tips (None when empty).
pub fn bounds(f: &PlanarFrame) -> Option<(Vec3, Vec3)> {
    let points = f.outlines.iter().flat_map(|(_, p)| p.iter().copied()).chain(f.poses.iter().map(|p| p.com)).chain(f.tips.iter().copied());
    let (lo, hi) = points.map(display).fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(lo, hi), p| (lo.min(p), hi.max(p)));
    lo.x.is_finite().then_some((lo, hi))
}

/// The header's run line for a planar file.
pub fn run_line(view: &PlanarView) -> String {
    let r = &view.run;
    let f = r.frame().filter(|f| f.built);
    let time = f.map_or("t —".to_string(), |f| format!("t {:.2} s · {} grid steps", f.time, f.steps));
    let rate = f.and_then(|f| f.achieved_rate).map_or(String::new(), |x| format!(" · {x:.2}× real time achieved"));
    let error = r.frame().and_then(|f| f.error.as_deref()).map_or(String::new(), |e| format!(" · {}", clip(e, 50)));
    format!("{} · {time} · ×{}{rate} · gen {} · planar v2{error}", r.phase().name(), r.speed_scale(), r.generation())
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

/// The inspector text for a planar file (the section names robot mode's tabs: Link → bodies, Joints, Drives → motors, Source).
pub fn inspector_text(view: &PlanarView, section: &str, selected: Option<usize>, watch: &str) -> String {
    let m = &view.loaded.model;
    let f = view.run.frame().filter(|f| f.built);
    let mut t = format!("PLANAR V2 SUMMARY (not the v3 physical model)\n{FIDELITY}\n\n");
    if let Some(e) = view.run.frame().and_then(|f| f.error.as_deref()) {
        t += &format!("RUN FAILED: {e}\n\n");
    }
    for w in f.map(|f| f.warnings.as_slice()).unwrap_or_default() {
        t += &format!("warning (planar build): {w}\n");
    }
    match section {
        "joints" => {
            let names = view.joint_names();
            t += &format!("JOINTS — {} in file, {} simulated (CAD working plane; pivots in mm, angles in rad)\n{JOG_RULE}\nKeys: ←/→ select joint · ↑/↓ move its target (Shift: ×5)\n", m.joints.len(), names.len());
            for j in &m.joints {
                let built = names.iter().position(|n| n == &j.name);
                let marker = if built.is_some() && built == Some(view.selected_joint) { "▸ " } else { "  " };
                t += &format!("\n{marker}{} — {} · {} → {}\n    pivot {:?} mm · axis sign {} · damping {}\n", j.name, j.kind, j.parent.as_deref().unwrap_or("(root)"), j.child, j.pivot2, j.axis_sign, j.damping);
                t += &match &j.limits {
                    Some(l) => format!("    limits (rad, file): {}\n", l.iter().map(|x| x.map_or("none".into(), |v| format!("{v}"))).collect::<Vec<_>>().join(", ")),
                    None => "    limits: none in file\n".into(),
                };
                t += &match (built, f) {
                    (Some(i), Some(f)) => format!("    angle {:+.2}° → target {:+.2}° (relative to the parent link)\n", f.joint_angles.get(i).copied().unwrap_or(f64::NAN).to_degrees(), f.targets.get(i).copied().unwrap_or(f64::NAN).to_degrees()),
                    (None, Some(_)) => "    not simulated (fixed type, or not reachable from the root)\n".into(),
                    _ => "    (building)\n".into(),
                };
            }
        }
        "drives" => {
            t += "MOTORS (per joint, as exported: torque and speed at the joint, gear ratio applied)\n";
            let mut any = false;
            for j in &m.joints {
                if let Some(x) = &j.motor {
                    any = true;
                    t += &format!("• {} on {} — spec {} · stall {} N·m · no-load {} rad/s · rotor inertia {} kg·m²\n", x.name, j.name, if x.spec.is_empty() { "(none)" } else { x.spec.as_str() }, x.stall_torque, x.no_load_speed, x.rotor_inertia);
                }
            }
            if !any {
                t += "none in file (joints without a motor are held by the PD servo without a torque-speed cap)\n";
            }
        }
        "source" => {
            t += watch;
            t += &format!("FILE (as read)\nversion: {} (read as {} by simrobot_version)\nunit: {}\nplane: {}\nsource: {}\n",
                view.loaded.declared_version.map_or("absent".into(), |v| v.to_string()), view.loaded.version, view.loaded.unit.as_deref().unwrap_or("absent (read as mm)"),
                view.loaded.plane, m.source.as_deref().unwrap_or("not recorded"));
            t += &format!("\nRUN\n{PACING}\n");
        }
        _ => match selected.and_then(|i| Some((i, m.bodies.get(i)?))) {
            None => {
                let root = f.and_then(|f| f.root.clone()).map_or("—".into(), |r| format!("{r}{}", if f.is_some_and(|f| f.root_fixed) { " (fixed)" } else { " (free)" }));
                t += &format!("Select a body in the list.\n\n{} bodies · {} joints in file · root {root}\n", m.bodies.len(), m.joints.len());
            }
            Some((i, b)) => {
                t += &format!("{}   (body {} of {})\n\nmass: {} kg\ncom: {:?} mm (working plane)\ninertia about the plane normal: {} kg·m²\nmaterial: {}\nground: {}\noutline: {} loop(s)\n",
                    b.name, i + 1, m.bodies.len(), b.mass_kg, b.com, b.inertia_zz, b.material.as_deref().unwrap_or("none recorded"), if b.ground { "yes (fixed root)" } else { "no" }, b.outline.len());
                if let Some(p) = f.and_then(|f| f.poses.iter().find(|p| p.body == i)) {
                    t += &format!("simulated com ({:.4}, {:.4}) m · angle {:+.2}° from the CAD pose\n", p.com[0], p.com[1], p.angle.to_degrees());
                }
            }
        },
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ground block and one hanging link: the smallest planar export.
    const FILE: &str = r#"{"format":"simrobot","version":2,"unit":"mm","bodies":[
        {"id":"g","name":"ground","mass_kg":1.0,"com":[0,220],"inertia_zz":0.001,"ground":true,"outline":[[[-25,200],[25,200],[25,240],[-25,240]]]},
        {"id":"t","name":"thigh","mass_kg":0.03,"com":[0,140],"inertia_zz":3.6e-5,"outline":[[[-8,80],[8,80],[8,200],[-8,200]]]}],
      "joints":[{"name":"hip","type":"revolute","child":"thigh","parent":"ground","pivot2":[0,200],"limits":[-1.0,1.0]}],"source":"/tmp/robot.rcad"}"#;

    fn until(run: &mut PlanarRun, what: &str, done: impl Fn(&PlanarFrame) -> bool) -> PlanarFrame {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            run.poll();
            if let Some(f) = run.frame().filter(|f| done(f)) {
                return f.clone();
            }
            assert!(Instant::now() < deadline, "{what}: no such frame");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn gains_match_the_fidelity_text() {
        assert_eq!((PLANAR_BANDWIDTH_HZ, PLANAR_DAMPING_RATIO), (6.0, 1.0), "update FIDELITY with the gains");
        assert!(FIDELITY.contains("6 Hz") && FIDELITY.contains("damping ratio 1"));
    }

    /// The robot-mode loader routes a v2 file to the planar summary (never
    /// PhysicalModel) and a v3 file to the physical loader; the worker check does the same.
    #[test]
    fn version_routes_the_file() {
        let path = Path::new("/tmp/planar.simrobot.json");
        match crate::robot::load_file_bytes(path, FILE.as_bytes()).unwrap() {
            crate::robot_source::FileModel::Planar(p) => {
                assert_eq!((p.version, p.declared_version, p.model.bodies.len()), (2, Some(2), 2));
            }
            crate::robot_source::FileModel::Physical(_) => panic!("a v2 file must not load as physical"),
        }
        let unit = FILE.replace(r#""unit":"mm""#, r#""unit":"in""#);
        assert!(crate::robot::load_file_bytes(path, unit.as_bytes()).err().unwrap().contains("unit `in`"));
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let v3 = root.join("examples/wheeled-robot/baseline/robot.simrobot.json");
        let bytes = std::fs::read(&v3).unwrap();
        assert!(matches!(crate::robot::load_file_bytes(&v3, &bytes).unwrap(), crate::robot_source::FileModel::Physical(_)));
        let dir = std::env::temp_dir().join(format!("sim-spatial-planar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("robot.simrobot.json");
        std::fs::write(&file, FILE).unwrap();
        let checked = crate::robot_source::check(&file, None);
        assert_eq!(checked.outcome.name(), "loaded");
        assert!(matches!(checked.outcome, crate::robot_source::Outcome::Loaded(crate::robot_source::FileModel::Planar(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The run thread builds through the shared planar build, steps one grid,
    /// moves a target from the current one, runs, and Reset rebuilds at t = 0
    /// under the next generation.
    #[test]
    fn run_thread_builds_steps_moves_and_resets() {
        let loaded = load_bytes(Path::new("/tmp/planar.simrobot.json"), FILE.as_bytes(), &serde_json::from_str(FILE).unwrap()).unwrap();
        let mut run = PlanarRun::spawn(loaded.model, 0, 1.0);
        let built = until(&mut run, "built", |f| f.built);
        assert_eq!(built.joint_names, ["hip"]);
        assert!(!built.outlines.is_empty() && built.tips.len() == 1 && built.root_fixed);
        assert_eq!(built.phase, PlanarPhase::Paused);
        assert!(run.check(RunAction::Pause).is_err());
        run.act(RunAction::Step).unwrap();
        let stepped = until(&mut run, "stepped", |f| f.steps == 1);
        assert!((stepped.time - GRID_S).abs() < 1e-9, "{}", stepped.time);
        let before = stepped.targets[0];
        run.nudge("hip", 0.05).unwrap();
        until(&mut run, "nudged", |f| f.targets.first().is_some_and(|t| (t - (before + 0.05)).abs() < 1e-12));
        assert!(run.nudge("knee", 0.05).unwrap_err().contains("unknown joint `knee`"));
        assert!(run.set_target("hip", f64::NAN).is_err());
        run.act(RunAction::Start).unwrap();
        assert!(run.check(RunAction::Step).is_err());
        until(&mut run, "running", |f| f.steps >= 3);
        run.act(RunAction::Pause).unwrap();
        run.act(RunAction::Reset).unwrap();
        assert!(run.frame().is_none());
        let reset = until(&mut run, "reset", |f| f.built && f.generation == 1);
        assert_eq!((reset.steps, reset.time), (0, 0.0));
        assert!((reset.targets[0] - built.targets[0]).abs() < 1e-12, "Reset returns targets to the CAD pose");
        let started = Instant::now();
        drop(run);
        assert!(started.elapsed() < Duration::from_secs(1), "the run thread stops when its channel closes");
    }
}

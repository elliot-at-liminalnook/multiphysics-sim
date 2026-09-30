//! Robot-mode run thread: one worker owns the shared
//! `sim_runtime::physical::PhysicalRobot` (built from a clone of the loaded
//! model with `sim_runtime::registry()` and `BuildOptions::default()`, as
//! sim-app's cad scene does) and advances it in fixed sim-time chunks, paced
//! at most to real time. The UI thread only sends commands and applies the
//! published frames; it never builds, advances or locks the robot.
use bevy::math::{DMat3, DQuat};
use serde::Serialize;
use serde_json::{Value, json};
use sim_domain_robot::PhysicalModel;
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
}

/// One published state of the robot, stamped with its generation.
#[derive(Clone, Debug)]
pub struct Frame {
    pub generation: u64,
    pub time: f64,
    /// Chunks of `CHUNK_S` advanced since the build.
    pub steps: u64,
    /// Per link: position of the link frame (at its com) and orientation, model frame.
    pub poses: Vec<([f64; 3], DQuat)>,
    pub joint_names: Vec<String>,
    pub joint_angles: Vec<f64>,
    /// Snapshot of the robot's servo targets when the frame was taken.
    pub targets: Vec<f64>,
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
}

/// What the worker publishes: its status and its latest frame.
struct Published {
    status: Status,
    frame: Option<Frame>,
    /// Why the last jog could not be applied on the run thread (cleared by the next applied jog or a reset).
    jog_error: Option<String>,
}

enum Command {
    Start,
    Pause,
    Step,
    Reset { generation: u64 },
    Jog { joint: String, target: f64 },
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
}

impl RunController {
    /// Spawns the (idle) run thread with its own clone of the loaded model.
    pub fn spawn(model: PhysicalModel) -> Self {
        let (tx, rx) = mpsc::channel();
        let status = Status { phase: Phase::Idle, generation: 0, rtf: None, error: None };
        let shared = Arc::new(Mutex::new(Published { status: status.clone(), frame: None, jog_error: None }));
        let out = shared.clone();
        let own = model.clone();
        std::thread::Builder::new()
            .name("robot-run".into())
            .spawn(move || worker(own, rx, out))
            .expect("spawn robot run thread");
        Self { tx, shared, generation: 0, running: false, frame: None, status, model, jogged: Default::default(), jog_error: None }
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

    /// Why an action is unavailable now (`Ok` when it can be sent).
    pub fn check(&self, action: RunAction) -> Result<(), String> {
        let failed = self.status.phase == Phase::Failed;
        match action {
            RunAction::Start if failed => Err("the run failed; Reset rebuilds the robot before it can run again".into()),
            RunAction::Start if self.running => Err("already running".into()),
            RunAction::Pause if !self.running || failed => Err("not running".into()),
            RunAction::Step if failed => Err("the run failed; Reset rebuilds the robot before it can step".into()),
            RunAction::Step if self.running => Err(format!("step advances one {CHUNK_S} s chunk only while paused; the robot is running — pause first")),
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
                self.status = Status { phase: Phase::Building, generation: self.generation, rtf: None, error: None };
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
            if self.status.phase == Phase::Failed {
                self.running = false;
            }
        }
        let fresh = published.frame.as_ref().filter(|f| accept(self.generation, f) && self.frame.as_ref().is_none_or(|old| old.generation != f.generation || old.steps != f.steps || old.targets != f.targets));
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
        let poses: Option<Vec<Value>> = f.map(|f| f.poses.iter().enumerate().map(|(i, (p, q))| json!({"link": links.get(i), "position": p, "quat_xyzw": [q.x, q.y, q.z, q.w]})).collect());
        json!({"phase": self.status.phase, "time": f.map(|f| f.time), "steps": f.map(|f| f.steps), "chunk_s": CHUNK_S,
            "rtf": self.status.rtf, "generation": self.generation, "error": self.status.error, "frame_generation": f.map(|f| f.generation),
            "joints": f.map(|f| &f.joint_names), "joint_angles": f.map(|f| &f.joint_angles), "targets": f.map(|f| &f.targets), "poses": poses,
            "build": "sim_runtime::physical::PhysicalRobot::build(model clone, sim_runtime::registry(), BuildOptions::default()) on the run thread",
            "steps_unit": "chunks of chunk_s since the last build", "pacing": PACING, "poses_frame": "link frame at its com, model frame (Z up)"})
    }
}

fn frame(robot: &sim_runtime::physical::PhysicalRobot, generation: u64, steps: u64) -> Frame {
    let poses = robot
        .poses()
        .iter()
        .map(|(r, p)| {
            let cols: [f64; 9] = std::array::from_fn(|i| r.as_slice()[i]);
            ([p.x, p.y, p.z], DQuat::from_mat3(&DMat3::from_cols_array(&cols)).normalize())
        })
        .collect();
    let targets = robot.targets.lock().unwrap_or_else(|p| p.into_inner()).clone();
    Frame { generation, time: robot.time(), steps, poses, joint_names: robot.joint_names.clone(), joint_angles: robot.joint_angles(), targets }
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

/// The run thread. The robot is built and advanced only here.
fn worker(model: PhysicalModel, rx: mpsc::Receiver<Command>, out: Arc<Mutex<Published>>) {
    use sim_runtime::physical::{BuildOptions, PhysicalRobot};
    let registry = sim_runtime::registry();
    let mut robot: Option<PhysicalRobot> = None;
    let mut generation = 0;
    let mut steps = 0;
    let mut failed = false;
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
    // Jogs received before the first build, applied right after it.
    let mut pending: Vec<(String, f64)> = Vec::new();
    let status = |phase, generation, rtf, error: Option<String>| Status { phase, generation, rtf, error };
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
        let build = |robot: &mut Option<PhysicalRobot>, generation: u64, pending: &mut Vec<(String, f64)>| -> bool {
            if robot.is_some() {
                return true;
            }
            set(status(Phase::Building, generation, None, None), None);
            match PhysicalRobot::build(model.clone(), &registry, &BuildOptions::default()) {
                Ok(r) => {
                    for (joint, target) in pending.drain(..) {
                        if let Err(e) = apply_jog(&r, &joint, target) {
                            set_jog_error(Some(e));
                        }
                    }
                    set(status(Phase::Paused, generation, None, None), Some(frame(&r, generation, 0)));
                    *robot = Some(r);
                    true
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
                robot = None;
                pending.clear();
                set_jog_error(None);
                steps = 0;
                running = false;
                failed = !build(&mut robot, generation, &mut pending);
            }
            Some(Command::Jog { joint, .. }) if failed => set_jog_error(Some(format!("joint `{joint}`: not applied; the run failed (Reset rebuilds)"))),
            _ if failed => {}
            Some(Command::Jog { joint, target }) => match robot.as_ref() {
                None => pending.push((joint, target)),
                Some(r) => match apply_jog(r, &joint, target) {
                    Ok(()) => {
                        set_jog_error(None);
                        let phase = if running { Phase::Running } else { Phase::Paused };
                        let rtf = out.lock().unwrap_or_else(|p| p.into_inner()).status.rtf;
                        set(status(phase, generation, rtf, None), Some(frame(r, generation, steps)));
                    }
                    Err(e) => set_jog_error(Some(e)),
                },
            },
            Some(Command::Start) => {
                if build(&mut robot, generation, &mut pending) {
                    running = true;
                    let t = robot.as_ref().map_or(0.0, |r| r.time());
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
                set(status(if robot.is_some() { Phase::Paused } else { Phase::Idle }, generation, None, None), None);
            }
            Some(Command::Step) => {
                if !build(&mut robot, generation, &mut pending) {
                    failed = true;
                    continue;
                }
                let r = robot.as_mut().unwrap();
                match r.advance(CHUNK_S) {
                    Ok(()) => {
                        steps += 1;
                        set(status(Phase::Paused, generation, None, None), Some(frame(r, generation, steps)));
                    }
                    Err(e) => {
                        failed = true;
                        set(status(Phase::Failed, generation, None, Some(format!("advance failed at t = {:.3} s: {e}", r.time()))), None);
                    }
                }
            }
            None => {}
        }
        if !running || failed {
            continue;
        }
        let Some(r) = robot.as_mut() else { continue };
        // Pace: never ahead of the wall clock; drop lag beyond one chunk.
        let wall = anchor.0.elapsed().as_secs_f64();
        let sim = r.time() - anchor.1;
        if sim > wall {
            std::thread::sleep(Duration::from_secs_f64((sim - wall).min(0.005)));
            continue;
        }
        if wall - sim > CHUNK_S {
            anchor = (Instant::now() - Duration::from_secs_f64(CHUNK_S), r.time());
        }
        match r.advance(CHUNK_S) {
            Ok(()) => {
                steps += 1;
                let now = Instant::now();
                window.push_back((now, r.time()));
                while window.len() > 2 && now.duration_since(window[1].0) >= RTF_WINDOW {
                    window.pop_front();
                }
                let (w0, s0) = window[0];
                let dw = now.duration_since(w0).as_secs_f64();
                let rtf = (dw > 0.0).then(|| (r.time() - s0) / dw);
                set(status(Phase::Running, generation, rtf, None), Some(frame(r, generation, steps)));
            }
            Err(e) => {
                running = false;
                failed = true;
                set(status(Phase::Failed, generation, None, Some(format!("advance failed at t = {:.3} s: {e}", r.time()))), None);
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
        for ((p, q), com) in f.poses.iter().zip(&assembly) {
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
}

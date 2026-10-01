//! The planar run thread ([`PlanarCommand`] in, [`PlanarFrame`] out) and its UI side, [`PlanarRun`].
use super::{GRID_S, MAX_REAL_DT_S, PACING, PlanarFrame, PlanarPhase, PlanarPose, THREAD, TICK};
use crate::jobs::RunThread;
use crate::robot::run::{RunAction, SpeedRequest, speed_target};
use serde_json::{Value, json};
use sim_phenomena::scenarios::cad_robot::{CadModel, CadRobot, build_planar};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

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
            // robot_run's key names, so a client reading robot_state.run works for either generation.
            "chunk_s": GRID_S, "rtf": f.and_then(|f| f.achieved_rate), "compute_limited": Value::Null,
            "compute_limited_rule": "null for a planar v2 run: its pacing caps it near 1.2 × real time whatever the scale (pacing), so a shortfall against speed_scale is that cap, not compute; compute_s_per_sim_s is the compute measure",
            "error": f.and_then(|f| f.error.clone()), "pacing": PACING})
    }
}

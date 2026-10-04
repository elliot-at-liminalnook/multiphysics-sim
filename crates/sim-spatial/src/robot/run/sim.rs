//! The simulation the run thread owns: a PhysicalRobot, an EmbeddedEnvironment, an EmbeddedSession or a drive Session.
use serde_json::{Value, json};
use sim_domain_control::drive::kinematics::BodyTwist;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use crate::robot::motion;
use crate::robot::preset::PresetRun;
use crate::robot::recording::{self, Snapshot};
use sim_runtime::drive_host::{DriveHost, DriveStatus};
use super::controlled::ControlledRun;
use super::frames::{environment_names, frame, live_frame, preset_frame};
use super::protocol::{Drive, Source};
use super::replay::ReplayWork;
use super::{CHUNK_S, Frame, OverlayFlags};

/// Why a drive session refuses jogs, motion requests and held inputs.
pub(super) const OWNS_TARGETS: &str = "the external controller owns the wheel targets";

/// The simulation the run thread owns.
pub(super) enum Sim {
    Robot(sim_runtime::physical::PhysicalRobot),
    /// The held action: the session's own input values (see `robot_preset`'s action_rule).
    /// `names`: the session's coordinate names, for the frames' named motor targets.
    Environment { env: sim_runtime::environment::EmbeddedEnvironment, held: Vec<f64>, run: Arc<PresetRun>, drive: Arc<Drive>, names: Arc<Vec<String>> },
    Session { session: sim_runtime::embedded::EmbeddedSession, run: Arc<PresetRun>, drive: Arc<Drive>, names: Arc<Vec<String>> },
    /// A `live` preset: the shared `sim_runtime::session::Session` (the
    /// scene's own controller), one held action per controller period, as
    /// the browser worker's `Simulation`; `periods` counts the steps taken.
    Live { session: sim_runtime::session::Session, held: Vec<f64>, run: Arc<PresetRun>, drive: Arc<Drive>, periods: u64 },
    /// A `--robot FILE` with its controller binding: the shared drive host
    /// (the Session with the external controller on the seam, fed one twist
    /// action per period by its TwistState; shared with Build's robot systems).
    Controlled { host: DriveHost, run: Arc<ControlledRun> },
}
impl Sim {
    pub(super) fn build(source: &Source, registry: &mut Option<sim_core::BehaviorRegistry>) -> Result<Sim, String> {
        use sim_runtime::embedded::{CaptureMode, EmbeddedSession};
        use sim_runtime::environment::EmbeddedEnvironment;
        match source {
            Source::Robot(model) => {
                let registry = registry.get_or_insert_with(sim_runtime::registry);
                sim_runtime::physical::PhysicalRobot::build(model.clone(), registry, &sim_runtime::physical::BuildOptions::default()).map(Sim::Robot)
            }
            Source::Preset(run) => match (&run.config, &run.task) {
                (None, _) => {
                    let session = sim_runtime::session::Session::new(run.scene.clone(), run.seed)?;
                    let held: Vec<f64> = session.inputs.iter().map(|c| c.initial).collect();
                    let drive = Arc::new(Drive::resolve(run, serde_json::json!({}), &session.inputs)?);
                    Ok(Sim::Live { session, held, run: run.clone(), drive, periods: 0 })
                }
                (Some(config), Some(task)) => {
                    let env = EmbeddedEnvironment::new(run.scene.clone(), config.clone(), task.clone(), run.seed)?;
                    // Held at the session's reset values, as EmbeddedEnvironment::prepare_replay does.
                    let held = env.inputs().iter().map(|c| c.initial).collect();
                    let drive = Arc::new(Drive::resolve(run, env.metadata(), env.inputs())?);
                    let names = environment_names(&env);
                    Ok(Sim::Environment { env, held, run: run.clone(), drive, names })
                }
                (Some(config), None) => {
                    let session = EmbeddedSession::new(run.scene.clone(), config.clone(), run.seed, CaptureMode::Latest)?;
                    let drive = Arc::new(Drive::resolve(run, Drive::session_metadata(&session), session.inputs())?);
                    let names = Arc::new(session.coordinate_names().to_vec());
                    Ok(Sim::Session { session, run: run.clone(), drive, names })
                }
            },
            // Session::new starts the controller program (sim_couple::python) and
            // attaches it on the seam; this runs on the run thread (worker::build).
            Source::Controlled(run) => {
                let host = DriveHost::new(run.scene.clone(), run.seed, &run.controlled)?;
                Ok(Sim::Controlled { host, run: run.clone() })
            }
            // Never a fallback hold run: the binding's error is the build's.
            Source::Unbound { error, .. } => Err(error.clone()),
        }
    }
    pub(super) fn time(&self) -> f64 {
        match self {
            Sim::Robot(r) => r.time(),
            Sim::Environment { env, run, .. } => env.transition().completed_steps as f64 * run.step_s(),
            Sim::Session { session, run, .. } => session.completed_steps() as f64 * run.step_s(),
            Sim::Live { session, .. } => session.robot.time(),
            Sim::Controlled { host, .. } => host.time(),
        }
    }
    /// Advances exactly one chunk: one action packet for a preset, which
    /// advances its declared heartbeat first (`motion::next_packet`).
    pub(super) fn advance(&mut self) -> Result<(), String> {
        match self {
            Sim::Robot(r) => r.advance(CHUNK_S),
            Sim::Environment { env, held, drive, .. } => {
                motion::next_packet(drive.heartbeat.as_ref(), held)?;
                env.step(held).map(|_| ())
            }
            Sim::Live { session, held, drive, periods, .. } => {
                motion::next_packet(drive.heartbeat.as_ref(), held)?;
                session.step(held)?;
                *periods += 1;
                Ok(())
            }
            Sim::Session { session, run, drive, .. } => {
                if drive.heartbeat.is_some() {
                    let mut action = session.input_values().to_vec();
                    motion::next_packet(drive.heartbeat.as_ref(), &mut action)?;
                    session.set_inputs(&action)?;
                }
                session.advance(run.chunk_steps())
            }
            // One seam period: the shared limiter and deadman on sim time, then
            // Session::step (DriveHost::step commits the twist state only when the step ran).
            Sim::Controlled { host, .. } => host.step().map(|_| ()),
        }
    }
    /// A drive session's fresh request at the current sim time (`Command::Twist`).
    pub(super) fn twist(&mut self, request: BodyTwist, halt: bool) -> Result<DriveStatus, String> {
        match self {
            Sim::Controlled { host, .. } => {
                host.request(request, halt)?;
                Ok(host.status())
            }
            _ => Err("drive requests are for a --robot FILE with a controller binding".into()),
        }
    }
    /// A drive replay ended (done, failed or cancelled): its recorded requests
    /// stop governing the session (`TwistState::replay_ended`). Other kinds: nothing.
    pub(super) fn end_drive_replay(&mut self) {
        if let Sim::Controlled { host, .. } = self {
            host.replay_ended();
        }
    }
    /// The run paused (`Command::Pause`, the user's Pause from any origin;
    /// not a failure, the horizon or a replay's end, which stop running for
    /// their own reasons): a drive session's live request is invalidated
    /// (`DriveHost::pause`, `sim_runtime::drive_host::PAUSE_RULE`), so after
    /// Run the profile's on-loss rule applies until a fresh request arrives.
    /// Other kinds: nothing.
    pub(super) fn pause_drive(&mut self) {
        if let Sim::Controlled { host, .. } = self {
            host.pause();
        }
    }
    /// The simulated seconds a drive replay of `total` recorded actions
    /// covers (one action per seam period), for a cancelled replay's verdict;
    /// None for other kinds, whose units are not all one fixed step.
    pub(super) fn replay_span_s(&self, total: u64) -> Option<f64> {
        match self {
            Sim::Controlled { host, .. } => Some(total as f64 * host.session.scene.period_s),
            Sim::Live { session, .. } => Some(total as f64 * session.scene.period_s),
            _ => None,
        }
    }
    /// A drive session's status now (None for other kinds).
    pub(super) fn drive_status(&self) -> Option<DriveStatus> {
        match self {
            Sim::Controlled { host, .. } => Some(host.status()),
            _ => None,
        }
    }
    /// Sets the motion channels of the held action, through the session's own
    /// validating `set_inputs` (at the next `EmbeddedEnvironment::step` for an
    /// environment, now for a session). Other channels keep their held values.
    pub(super) fn set_motion(&mut self, values: [f64; 3]) -> Result<(), String> {
        let (motion, current) = match self {
            Sim::Robot(_) => return Err("motion requests are for presets".into()),
            Sim::Controlled { .. } => return Err(format!("motion requests are refused: {OWNS_TARGETS}; send drive requests")),
            Sim::Environment { held, drive, .. } | Sim::Live { held, drive, .. } => (drive.motion.as_ref(), held.clone()),
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
    /// The held action (the session's input values; empty for kinds without inputs).
    pub(super) fn held(&self) -> Vec<f64> {
        match self {
            Sim::Environment { held, .. } | Sim::Live { held, .. } => held.clone(),
            Sim::Session { session, .. } => session.input_values().to_vec(),
            Sim::Robot(_) | Sim::Controlled { .. } => Vec::new(),
        }
    }
    /// Sets the whole held action: validated by the session's `set_inputs` (now
    /// for a session; for an environment, bounds-checked here and validated
    /// again by `EmbeddedEnvironment::step`).
    pub(super) fn set_action(&mut self, action: Vec<f64>) -> Result<(), String> {
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
            Sim::Live { session, held, .. } => {
                if action.len() != session.inputs.len() {
                    return Err(format!("action has {} values; the session has {} inputs", action.len(), session.inputs.len()));
                }
                for (c, x) in session.inputs.iter().zip(&action) {
                    if !(x.is_finite() && *x >= c.lower && *x <= c.upper) {
                        return Err(format!("input `{}` = {x} is outside [{}, {}]", c.name, c.lower, c.upper));
                    }
                }
                *held = action;
            }
            Sim::Robot(_) => return Err("inputs are for presets".into()),
            Sim::Controlled { .. } => return Err(format!("held inputs are refused: {OWNS_TARGETS}; send drive requests")),
        }
        Ok(())
    }
    /// The runtime's own nominal completed_steps (presets).
    pub(super) fn completed_steps(&self) -> Option<u64> {
        match self {
            Sim::Robot(_) => None,
            Sim::Environment { env, .. } => Some(env.transition().completed_steps as u64),
            Sim::Session { session, .. } => Some(session.completed_steps() as u64),
            Sim::Live { periods, .. } => Some(*periods),
            // Seam periods stepped (one recorded action each).
            Sim::Controlled { host, .. } => Some(host.twist.periods),
        }
    }
    /// One bounded replay chunk: the next returned action through
    /// `EmbeddedEnvironment::step`, or up to the preset's chunk of nominal steps
    /// through `EmbeddedSession::advance`. Returns the units advanced.
    pub(super) fn advance_replay(&mut self, work: &mut ReplayWork) -> Result<u64, String> {
        match (self, work) {
            (Sim::Environment { env, held, .. }, ReplayWork::Actions(queue)) => {
                let action = queue.front().ok_or("no pending replay action")?.clone();
                env.step(&action)?;
                *held = action;
                queue.pop_front();
                Ok(1)
            }
            (Sim::Live { session, held, periods, .. }, ReplayWork::Live(queue)) => {
                let action = queue.front().ok_or("no pending replay action")?.clone();
                session.step(&action)?;
                *held = action;
                *periods += 1;
                queue.pop_front();
                Ok(1)
            }
            (Sim::Session { session, run, .. }, ReplayWork::Steps { remaining }) => {
                let n = (*remaining).min(run.chunk_steps());
                *remaining -= n;
                session.advance(n)?;
                Ok(n as u64)
            }
            // One recorded action per chunk (one seam period), exactly as recorded.
            (Sim::Controlled { host, .. }, ReplayWork::Drive(queue)) => {
                let action = queue.front().ok_or("no pending replay action")?.clone();
                // Stamped at the period's start, as a live request is (TwistState::replayed).
                host.step_recorded(&action)?;
                queue.pop_front();
                Ok(1)
            }
            _ => Err("replay work does not match the simulation kind".into()),
        }
    }
    /// The shared recording (as the browser saves it for a preset kind; the
    /// drive Session's own `recording()` for a controlled file), its sidecar
    /// and the root the writer checks the target against.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn save(&self, target: &Path, note: Option<&str>, unix_ms: u128, generation: u64, chunks: u64, last: Option<Value>) -> Result<(Snapshot, Value, PathBuf), String> {
        let (snapshot, meta, root) = match self {
            Sim::Robot(_) => return Err("`--robot FILE` runs PhysicalRobot, which keeps no recording (a file with a controller binding records its drive session)".into()),
            Sim::Environment { env, run, .. } => {
                let s = Snapshot::Environment(env.episode_recording());
                let meta = recording::meta(&s, run, target, note, unix_ms, generation, chunks, last);
                (s, meta, run.root.clone())
            }
            Sim::Session { session, run, .. } => {
                let s = Snapshot::Session(session.recording());
                let meta = recording::meta(&s, run, target, note, unix_ms, generation, chunks, last);
                (s, meta, run.root.clone())
            }
            Sim::Live { session, run, .. } => {
                let s = Snapshot::Live { recording: session.recording(), failure: session.frame().error };
                let meta = recording::meta(&s, run, target, note, unix_ms, generation, chunks, last);
                (s, meta, run.root.clone())
            }
            Sim::Controlled { host, run } => {
                let root = run.root.clone().map_err(|e| format!("drive recordings resolve against the workspace root: {e}"))?;
                let s = Snapshot::Drive { recording: host.session.recording(), failure: host.session.frame().error };
                let meta = recording::drive_meta(&s, run, target, note, unix_ms, generation, chunks, last);
                (s, meta, root)
            }
        };
        Ok((snapshot, meta, root))
    }
    pub(super) fn drive(&self) -> Option<Arc<Drive>> {
        match self {
            Sim::Robot(_) | Sim::Controlled { .. } => None,
            Sim::Environment { drive, .. } | Sim::Session { drive, .. } | Sim::Live { drive, .. } => Some(drive.clone()),
        }
    }
    /// Why the run cannot continue without a reset (horizon or episode end).
    pub(super) fn ended(&self) -> Option<Value> {
        match self {
            Sim::Robot(_) => None,
            Sim::Environment { env, run, .. } => {
                let t = env.transition();
                (t.terminated || t.truncated).then(|| {
                    let kind = if t.terminated { "terminated" } else { "horizon" };
                    let message = if t.terminated {
                        format!("episode terminated at t = {:.3} s: {}", t.completed_steps as f64 * run.step_s(), t.termination_reasons.join("; "))
                    } else {
                        format!("horizon reached (episode truncated) at t = {:.3} s: {} of {} steps", t.completed_steps as f64 * run.step_s(), t.completed_steps, run.requested_steps())
                    };
                    json!({"kind": kind, "terminated": t.terminated, "truncated": t.truncated, "termination_reasons": t.termination_reasons, "completed_steps": t.completed_steps, "message": message})
                })
            }
            Sim::Session { session, run, .. } => (session.remaining_steps() == 0).then(|| {
                let n = session.completed_steps();
                json!({"kind": "horizon", "terminated": false, "truncated": true, "termination_reasons": [], "completed_steps": n,
                    "message": format!("horizon reached at t = {:.3} s: {n} of {} steps", n as f64 * run.step_s(), run.requested_steps())})
            }),
            // Session::step refuses at the scene's duration.
            Sim::Live { session, periods, .. } => (session.robot.time() >= session.scene.duration_s - 1e-10).then(|| {
                json!({"kind": "horizon", "terminated": false, "truncated": true, "termination_reasons": [], "completed_steps": periods,
                    "message": format!("horizon reached at t = {:.3} s: {periods} periods of {} s (the scene's duration_s {} s); Reset starts a new episode", session.robot.time(), session.scene.period_s, session.scene.duration_s)})
            }),
            // Session::step refuses past duration_s (controller_binding::DRIVE_DURATION_S).
            Sim::Controlled { host, .. } => host.ended().then(|| {
                let (t, periods, scene) = (host.time(), host.twist.periods, &host.session.scene);
                json!({"kind": "horizon", "terminated": false, "truncated": true, "termination_reasons": [], "completed_steps": periods,
                    "message": format!("drive session horizon reached at t = {t:.3} s: {periods} periods of {} s; a drive session lasts controller_binding::DRIVE_DURATION_S = {} s of simulated time; Reset starts a new session", scene.period_s, scene.duration_s)})
            }),
        }
    }
    pub(super) fn frame(&self, links: &[String], generation: u64, steps: u64, flags: OverlayFlags) -> Result<Frame, String> {
        match self {
            Sim::Robot(r) => Ok(frame(r, generation, steps, flags)),
            // As the browser worker's EnvironmentSimulation.frame(): the transition as `learning`, and done/error.
            Sim::Environment { env, run, held, names, .. } => {
                let mut v = env.frame()?;
                let t = env.transition();
                v["learning"] = json!(t);
                v["done"] = json!(t.terminated || t.truncated || env.error().is_some());
                v["error"] = json!(env.error());
                preset_frame(&v, links, generation, steps, run.step_s(), held, names)
            }
            Sim::Session { session, run, names, .. } => preset_frame(&session.interactive_frame()?, links, generation, steps, run.step_s(), session.input_values(), names),
            Sim::Live { session, held, periods, .. } => live_frame(&session.frame(), links, generation, steps, *periods, held),
            // PhysicalRobot's own frame (with overlays), plus the action last sent and the periods stepped.
            Sim::Controlled { host, .. } => {
                let mut f = frame(&host.session.robot, generation, steps, flags);
                f.inputs = host.twist.sent.to_vec();
                f.completed_steps = Some(host.twist.periods);
                Ok(f)
            }
        }
    }
}

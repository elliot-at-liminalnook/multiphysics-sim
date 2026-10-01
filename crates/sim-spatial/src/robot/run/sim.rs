//! The simulation the run thread owns: a PhysicalRobot, an EmbeddedEnvironment or an EmbeddedSession.
use serde_json::{Value, json};
use std::sync::Arc;
use crate::robot::motion;
use crate::robot::preset::PresetRun;
use crate::robot::recording::Snapshot;
use super::frames::{environment_names, frame, preset_frame};
use super::protocol::{Drive, Source};
use super::replay::ReplayWork;
use super::{CHUNK_S, Frame, OverlayFlags};

/// The simulation the run thread owns.
pub(super) enum Sim {
    Robot(sim_runtime::physical::PhysicalRobot),
    /// The held action: the session's own input values (see `robot_preset`'s action_rule).
    /// `names`: the session's coordinate names, for the frames' named motor targets.
    Environment { env: sim_runtime::environment::EmbeddedEnvironment, held: Vec<f64>, run: Arc<PresetRun>, drive: Arc<Drive>, names: Arc<Vec<String>> },
    Session { session: sim_runtime::embedded::EmbeddedSession, run: Arc<PresetRun>, drive: Arc<Drive>, names: Arc<Vec<String>> },
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
            Source::Preset(run) => match &run.task {
                Some(task) => {
                    let env = EmbeddedEnvironment::new(run.scene.clone(), run.config.clone(), task.clone(), run.seed)?;
                    // Held at the session's reset values, as EmbeddedEnvironment::prepare_replay does.
                    let held = env.inputs().iter().map(|c| c.initial).collect();
                    let drive = Arc::new(Drive::resolve(run, &env.metadata()["policy_contract"], env.inputs())?);
                    let names = environment_names(&env);
                    Ok(Sim::Environment { env, held, run: run.clone(), drive, names })
                }
                None => {
                    let session = EmbeddedSession::new(run.scene.clone(), run.config.clone(), run.seed, CaptureMode::Latest)?;
                    let drive = Arc::new(Drive::resolve(run, &session.policy_metadata(), session.inputs())?);
                    let names = Arc::new(session.coordinate_names().to_vec());
                    Ok(Sim::Session { session, run: run.clone(), drive, names })
                }
            },
        }
    }
    pub(super) fn time(&self) -> f64 {
        match self {
            Sim::Robot(r) => r.time(),
            Sim::Environment { env, run, .. } => env.transition().completed_steps as f64 * run.config.step_s,
            Sim::Session { session, run, .. } => session.completed_steps() as f64 * run.config.step_s,
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
            Sim::Session { session, run, drive, .. } => {
                if drive.heartbeat.is_some() {
                    let mut action = session.input_values().to_vec();
                    motion::next_packet(drive.heartbeat.as_ref(), &mut action)?;
                    session.set_inputs(&action)?;
                }
                session.advance(run.chunk_steps())
            }
        }
    }
    /// Sets the motion channels of the held action, through the session's own
    /// validating `set_inputs` (at the next `EmbeddedEnvironment::step` for an
    /// environment, now for a session). Other channels keep their held values.
    pub(super) fn set_motion(&mut self, values: [f64; 3]) -> Result<(), String> {
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
            Sim::Robot(_) => return Err("inputs are for presets".into()),
        }
        Ok(())
    }
    /// The runtime's own nominal completed_steps (presets).
    pub(super) fn completed_steps(&self) -> Option<u64> {
        match self {
            Sim::Robot(_) => None,
            Sim::Environment { env, .. } => Some(env.transition().completed_steps as u64),
            Sim::Session { session, .. } => Some(session.completed_steps() as u64),
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
    pub(super) fn snapshot(&self) -> Result<(Snapshot, &Arc<PresetRun>), String> {
        match self {
            Sim::Robot(_) => Err("`--robot FILE` runs PhysicalRobot, which keeps no recording".into()),
            Sim::Environment { env, run, .. } => Ok((Snapshot::Environment(env.episode_recording()), run)),
            Sim::Session { session, run, .. } => Ok((Snapshot::Session(session.recording()), run)),
        }
    }
    pub(super) fn drive(&self) -> Option<Arc<Drive>> {
        match self {
            Sim::Robot(_) => None,
            Sim::Environment { drive, .. } | Sim::Session { drive, .. } => Some(drive.clone()),
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
    pub(super) fn frame(&self, links: &[String], generation: u64, steps: u64, flags: OverlayFlags) -> Result<Frame, String> {
        match self {
            Sim::Robot(r) => Ok(frame(r, generation, steps, flags)),
            Sim::Environment { env, run, held, names, .. } => preset_frame(&env.frame()?, links, generation, steps, run.config.step_s, held, names),
            Sim::Session { session, run, names, .. } => preset_frame(&session.interactive_frame()?, links, generation, steps, run.config.step_s, session.input_values(), names),
        }
    }
}

//! Reproducible motion evaluations over the ordinary sampled environment.
//! Checkpoints replay recorded inputs, with bounded host-driven advancement;
//! they are not approximate snapshots of an incomplete physical state.
use crate::{
    embedded::Config,
    environment::{EmbeddedEnvironment, EnvironmentRecording, Task, Transition},
    motion_parameters::MotionParameterization,
    physics_context::RuntimeIdentity,
    session::Scene,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sim_domain_control::motion_parameters::Values;
use std::collections::{BTreeSet, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Objective {
    NetSpeed,
    RewardRate,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentSpec {
    pub version: u32,
    pub scene: Scene,
    pub config: Config,
    pub task: Task,
    pub parameterization: MotionParameterization,
    pub source_actions: Vec<Vec<f64>>,
    pub baseline: Values,
    pub seed: u64,
    pub objective: Objective,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Experiment {
    pub version: u32,
    pub runtime: RuntimeIdentity,
    pub context_id: String,
    pub spec: ExperimentSpec,
}

fn identity(domain: &str, value: &impl Serialize) -> Result<String, String> {
    // Reuse the physics profile's exact canonical JSON encoding, including
    // harmless integer/float and signed-zero spelling changes across hosts.
    let value = serde_json::to_value(value).map_err(|e| e.to_string())?;
    let bytes = crate::physics_context::fingerprint(&value);
    let mut hash = blake3::Hasher::new();
    hash.update(domain.as_bytes());
    hash.update(&[0]);
    hash.update(bytes.as_bytes());
    Ok(hash.finalize().to_hex().to_string())
}

impl Experiment {
    pub fn bind(spec: ExperimentSpec) -> Result<Self, String> {
        if spec.version != 1 {
            return Err("unsupported experiment specification".into());
        }
        if spec.objective == Objective::NetSpeed
            && spec.task.progress.is_none()
            && spec.task.speed.is_none()
        {
            return Err(
                "net-speed optimization requires an explicit progress or legacy speed task".into(),
            );
        }
        let variant =
            spec.parameterization
                .materialize(&spec.scene, &spec.source_actions, &spec.baseline)?;
        let environment = EmbeddedEnvironment::new(
            variant.scene,
            spec.config.clone(),
            spec.task.clone(),
            spec.seed,
        )?;
        let intervals = environment.action_intervals();
        if intervals == 0 || variant.actions.len() != intervals {
            return Err(
                "experiment requires exactly one held command row per task interval".into(),
            );
        }
        drop(environment);
        let runtime = RuntimeIdentity::current();
        let context_id = identity("sim-motion-experiment-v1", &(&runtime, &spec))?;
        Ok(Self {
            version: 1,
            runtime,
            context_id,
            spec,
        })
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.spec.version != 1
            || self.runtime != RuntimeIdentity::current()
            || self.context_id
                != identity("sim-motion-experiment-v1", &(&self.runtime, &self.spec))?
        {
            return Err("experiment context or runtime source identity changed".into());
        }
        Ok(())
    }
    pub fn propose(&self, values: Values, method: String) -> Result<Proposal, String> {
        self.validate()?;
        self.spec.parameterization.space.validate(&values)?;
        if method.trim().is_empty() {
            return Err("proposal method is required".into());
        }
        let id = identity("sim-motion-trial-v1", &(&self.context_id, &values))?;
        Ok(Proposal { id, values, method })
    }
    fn check_proposal(&self, proposal: &Proposal) -> Result<(), String> {
        if self
            .propose(proposal.values.clone(), proposal.method.clone())?
            .id
            != proposal.id
        {
            return Err("proposal identity mismatch".into());
        }
        Ok(())
    }
    pub fn start(&self, proposal: Proposal) -> Result<Evaluation, String> {
        self.check_proposal(&proposal)?;
        let variant = self.spec.parameterization.materialize(
            &self.spec.scene,
            &self.spec.source_actions,
            &proposal.values,
        )?;
        let environment = EmbeddedEnvironment::new(
            variant.scene,
            self.spec.config.clone(),
            self.spec.task.clone(),
            self.spec.seed,
        )?;
        Ok(Evaluation {
            experiment: self.clone(),
            proposal,
            environment,
            actions: variant.actions,
            next_action: 0,
            reward_sum: 0.,
            replay: VecDeque::new(),
            expected: None,
        })
    }
    pub fn resume(&self, checkpoint: Checkpoint) -> Result<Evaluation, String> {
        checkpoint.validate(self)?;
        if checkpoint.recording.error.is_some() || checkpoint.recording.runtime.failure.is_some() {
            return Err(
                "a failed physics evaluation cannot resume; retain its failed result".into(),
            );
        }
        let mut evaluation = self.start(checkpoint.proposal.clone())?;
        let (environment, replay) = evaluation
            .environment
            .prepare_replay(checkpoint.recording.clone())?;
        if replay.len() > evaluation.actions.len() || replay != evaluation.actions[..replay.len()] {
            return Err("checkpoint commands differ from proposed motion".into());
        }
        evaluation.environment = environment;
        evaluation.replay = replay.into();
        evaluation.expected = Some(checkpoint);
        if evaluation.replay.is_empty() {
            evaluation.verify_replay()?;
        }
        Ok(evaluation)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub id: String,
    pub values: Values,
    pub method: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Running,
    Replaying,
    Complete,
    Terminated,
    Failed,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub version: u32,
    pub context_id: String,
    pub proposal: Proposal,
    pub recording: EnvironmentRecording,
    pub final_transition: Transition,
    /// Complete frame with only host wall-time counters removed.
    pub frame: Value,
    pub reward_sum: f64,
}
impl Checkpoint {
    pub fn validate(&self, experiment: &Experiment) -> Result<(), String> {
        experiment.check_proposal(&self.proposal)?;
        if self.version != 1
            || self.context_id != experiment.context_id
            || self.recording.version != 1
            || self.recording.kind != "sampled_environment_recording"
            || self.recording.runtime.version != 3
            || self.recording.runtime.kind != "embedded_session"
            || !self.reward_sum.is_finite()
            || self.recording.runtime.runtime_identity.as_ref() != Some(&experiment.runtime)
            || self.recording.runtime.seed != experiment.spec.seed
            || self.final_transition.completed_steps > self.recording.runtime.completed_steps
            || self.recording.runtime.completed_steps > experiment.spec.config.steps
        {
            return Err("invalid experiment checkpoint context or progress".into());
        }
        let t = &self.final_transition;
        let physics_time =
            self.recording.runtime.completed_steps as f64 * experiment.spec.config.step_s;
        if !t.time_s.is_finite()
            || !t.elapsed_s.is_finite()
            || !t.reward.is_finite()
            || t.observations.iter().any(|v| !v.is_finite())
            || (t.time_s / experiment.spec.config.step_s - t.completed_steps as f64).abs() > 1e-7
            || self
                .frame
                .get("time_s")
                .and_then(Value::as_f64)
                .is_none_or(|time| {
                    !time.is_finite()
                        || (time - physics_time).abs() > experiment.spec.config.step_s * 1e-7
                })
            || self.frame.get("completed_steps").and_then(Value::as_u64)
                != Some(self.recording.runtime.completed_steps as u64)
            || (self.recording.error.is_none()
                && self.recording.runtime.failure.is_none()
                && t.completed_steps != self.recording.runtime.completed_steps)
        {
            return Err("checkpoint frame and task clocks disagree".into());
        }
        let variant = experiment.spec.parameterization.materialize(
            &experiment.spec.scene,
            &experiment.spec.source_actions,
            &self.proposal.values,
        )?;
        if identity("scene", &self.recording.runtime.scene)? != identity("scene", &variant.scene)?
            || identity("config", &self.recording.runtime.config)?
                != identity("config", &experiment.spec.config)?
            || identity("task", &self.recording.task)? != identity("task", &experiment.spec.task)?
        {
            return Err("checkpoint robot, motion, physics or task differs".into());
        }
        // A failed action may commit physical substeps beyond the last task
        // endpoint. Only the observed prefix can be matched to held commands.
        let count = (t.time_s / experiment.spec.task.period_s).round() as usize;
        if count > variant.actions.len() {
            return Err("checkpoint exceeds command schedule".into());
        }
        if count > 0 {
            let mut record = self.recording.runtime.clone();
            record.completed_steps = t.completed_steps;
            record.failure = None;
            record
                .input_events
                .retain(|event| event.at_step < t.completed_steps);
            let frames = (0..=count)
                .map(|i| serde_json::json!({"time_s":i as f64 * experiment.spec.task.period_s}))
                .collect::<Vec<_>>();
            let inputs = &record
                .scene
                .controller
                .as_ref()
                .ok_or("missing checkpoint controller")?
                .inputs;
            if crate::forecast_actions::from_recording(&record, &frames, inputs)?
                != variant.actions[..count]
            {
                return Err("checkpoint commands differ from proposed motion".into());
            }
        }
        Ok(())
    }
    pub fn status(&self) -> Status {
        if self.recording.error.is_some() || self.recording.runtime.failure.is_some() {
            Status::Failed
        } else if self.final_transition.terminated {
            Status::Terminated
        } else if self.final_transition.truncated
            && self.final_transition.completed_steps == self.recording.runtime.config.steps
            && self.recording.runtime.completed_steps == self.recording.runtime.config.steps
        {
            Status::Complete
        } else {
            Status::Running
        }
    }
    /// Only complete, nonterminated, nonfailed episodes have an eligible score.
    pub fn score(&self, objective: Objective) -> Result<Option<f64>, String> {
        if self.status() != Status::Complete {
            return Ok(None);
        }
        let score = match objective {
            Objective::NetSpeed => self
                .final_transition
                .progress
                .as_ref()
                .map(|p| p.net_speed_m_s)
                .or_else(|| {
                    self.final_transition
                        .speed
                        .as_ref()
                        .map(|p| p.net_speed_m_s)
                })
                .ok_or("missing completed net-speed measurement")?,
            Objective::RewardRate => self.reward_sum / self.final_transition.time_s,
        };
        if !score.is_finite() {
            return Err("nonfinite experiment score".into());
        }
        Ok(Some(score))
    }
}

pub struct Evaluation {
    experiment: Experiment,
    proposal: Proposal,
    environment: EmbeddedEnvironment,
    actions: Vec<Vec<f64>>,
    next_action: usize,
    reward_sum: f64,
    replay: VecDeque<Vec<f64>>,
    expected: Option<Checkpoint>,
}
impl Evaluation {
    pub fn status(&self) -> Status {
        if self.environment.error().is_some() {
            Status::Failed
        } else if self.expected.is_some() {
            Status::Replaying
        } else if self.environment.transition().terminated {
            Status::Terminated
        } else if self.environment.transition().truncated {
            Status::Complete
        } else {
            Status::Running
        }
    }
    pub fn frame(&self) -> Result<Value, String> {
        self.environment.frame()
    }
    pub fn metadata(&self) -> Value {
        serde_json::json!({"version":1,"context_id":self.experiment.context_id,"proposal":self.proposal,
            "objective":self.experiment.spec.objective,"motion_parameters":self.experiment.spec.parameterization.metadata(),
            "environment":self.environment.metadata(),"resume":"Exact recorded-input replay before new actions; score is unavailable until a complete nonfailed episode."})
    }
    fn stable_frame(&self) -> Result<Value, String> {
        let mut frame = self.environment.frame()?;
        frame
            .as_object_mut()
            .ok_or("invalid environment frame")?
            .remove("stepping_wall_s");
        Ok(frame)
    }
    fn verify_replay(&mut self) -> Result<(), String> {
        let Some(expected) = &self.expected else {
            return Ok(());
        };
        if self.environment.transition() != &expected.final_transition
            || self.reward_sum != expected.reward_sum
            || crate::physics_context::fingerprint(&self.stable_frame()?)
                != crate::physics_context::fingerprint(&expected.frame)
        {
            return Err(
                "checkpoint replay differs from saved physical/task state; continuation refused"
                    .into(),
            );
        }
        self.expected = None;
        Ok(())
    }
    /// A host can pause/cancel between calls without discarding the candidate.
    /// The budget counts replay and new action intervals together.
    pub fn advance(&mut self, maximum_actions: usize) -> Result<Status, String> {
        if maximum_actions == 0 {
            return Err("positive experiment advance budget required".into());
        }
        for _ in 0..maximum_actions {
            if !matches!(self.status(), Status::Running | Status::Replaying) {
                break;
            }
            let replaying = self.expected.is_some();
            let action = if replaying {
                self.replay
                    .front()
                    .ok_or("missing checkpoint replay command")?
            } else {
                self.actions
                    .get(self.next_action)
                    .ok_or("motion command schedule exhausted")?
            };
            let transition = match self.environment.step(action) {
                Ok(transition) => transition,
                Err(error) => {
                    if replaying {
                        return Err(format!("checkpoint replay failed: {error}"));
                    } else {
                        return Ok(Status::Failed);
                    }
                }
            };
            self.reward_sum += transition.reward;
            if !self.reward_sum.is_finite() {
                return Err("experiment return overflow".into());
            }
            self.next_action += 1;
            if replaying {
                self.replay.pop_front();
                if self.replay.is_empty() {
                    self.verify_replay()?;
                }
            }
        }
        Ok(self.status())
    }
    pub fn checkpoint(&self) -> Result<Checkpoint, String> {
        // Preserve the original checkpoint while rebuilding its physical state.
        // A host crash during replay never moves the saved progress backwards.
        if let Some(expected) = &self.expected {
            return Ok(expected.clone());
        }
        Ok(Checkpoint {
            version: 1,
            context_id: self.experiment.context_id.clone(),
            proposal: self.proposal.clone(),
            recording: self.environment.episode_recording(),
            final_transition: self.environment.transition().clone(),
            frame: self.stable_frame()?,
            reward_sum: self.reward_sum,
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trial {
    pub proposal: Proposal,
    pub checkpoint: Option<Checkpoint>,
    pub preparation_failure: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Journal {
    pub version: u32,
    pub experiment: Experiment,
    pub trials: Vec<Trial>,
}
impl Journal {
    pub fn new(experiment: Experiment) -> Self {
        Self {
            version: 1,
            experiment,
            trials: vec![],
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        self.experiment.validate()?;
        if self.version != 1 {
            return Err("unsupported experiment journal".into());
        }
        let mut ids = BTreeSet::new();
        for trial in &self.trials {
            self.experiment.check_proposal(&trial.proposal)?;
            if !ids.insert(&trial.proposal.id)
                || (trial.checkpoint.is_some() && trial.preparation_failure.is_some())
                || trial
                    .preparation_failure
                    .as_ref()
                    .is_some_and(|e| e.trim().is_empty())
            {
                return Err("invalid or duplicate trial record".into());
            }
            if let Some(checkpoint) = &trial.checkpoint {
                checkpoint.validate(&self.experiment)?;
                if checkpoint.proposal != trial.proposal {
                    return Err("checkpoint belongs to a different proposal".into());
                }
                checkpoint.score(self.experiment.spec.objective)?;
            }
        }
        Ok(())
    }
    pub fn submit(&mut self, proposal: Proposal) -> Result<(), String> {
        self.validate()?;
        self.experiment.check_proposal(&proposal)?;
        if self.trials.iter().any(|t| t.proposal.id == proposal.id) {
            return Err("duplicate motion proposal".into());
        }
        self.trials.push(Trial {
            proposal,
            checkpoint: None,
            preparation_failure: None,
        });
        Ok(())
    }
    pub fn record(&mut self, checkpoint: Checkpoint) -> Result<(), String> {
        checkpoint.validate(&self.experiment)?;
        let trial = self
            .trials
            .iter_mut()
            .find(|t| t.proposal.id == checkpoint.proposal.id)
            .ok_or("unknown trial checkpoint")?;
        if trial.proposal != checkpoint.proposal
            || trial.preparation_failure.is_some()
            || trial.checkpoint.as_ref().is_some_and(|c| {
                c.status() != Status::Running
                    || c.recording.runtime.completed_steps
                        > checkpoint.recording.runtime.completed_steps
            })
        {
            return Err("cannot replace a terminal trial or regress its progress".into());
        }
        checkpoint.score(self.experiment.spec.objective)?;
        trial.checkpoint = Some(checkpoint);
        Ok(())
    }
    pub fn preparation_failed(&mut self, id: &str, reason: String) -> Result<(), String> {
        if reason.trim().is_empty() {
            return Err("failed preparation needs a reason".into());
        }
        let trial = self
            .trials
            .iter_mut()
            .find(|t| t.proposal.id == id)
            .ok_or("unknown failed proposal")?;
        if trial.checkpoint.is_some() || trial.preparation_failure.is_some() {
            return Err("trial already started or failed".into());
        }
        trial.preparation_failure = Some(reason);
        Ok(())
    }
    pub fn pending(&self) -> Option<&Trial> {
        self.trials.iter().find(|t| {
            t.preparation_failure.is_none()
                && t.checkpoint
                    .as_ref()
                    .is_none_or(|c| c.status() == Status::Running)
        })
    }
    pub fn best(&self) -> Result<Option<(&Proposal, f64)>, String> {
        self.validate()?;
        let mut best = None;
        for trial in &self.trials {
            if let Some(c) = &trial.checkpoint {
                if let Some(score) = c.score(self.experiment.spec.objective)? {
                    if best.is_none_or(|(_, s)| score > s) {
                        best = Some((&trial.proposal, score));
                    }
                }
            }
        }
        Ok(best)
    }
}

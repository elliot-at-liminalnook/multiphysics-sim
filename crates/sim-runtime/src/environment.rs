//! Sampled teacher environments over the production embedded runtime.
//! Actions are held controller inputs, not torques applied around the actuator.
//! Task termination is sampled at transition endpoints; physical collision and
//! joint constraints remain the runtime's responsibility.
use crate::embedded::{CaptureMode, Config, EmbeddedRecording, EmbeddedSession};
use crate::session::{InputChannel, Scene};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_core::QuantityKind;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    X,
    Y,
    Z,
}
impl Axis {
    fn index(&self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }
}

/// Named sources determine units. No arbitrary JSON pointer can masquerade as a
/// physical quantity or silently read an older policy-observation timestamp.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObservationSource {
    CoordinatePosition { coordinate: String },
    CoordinateVelocity { coordinate: String },
    ReferencePosition { coordinate: String },
    BodyPosition { link: String, axis: Axis },
    BodyVelocity { link: String, axis: Axis },
    MotorCurrent { motor: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub name: String,
    pub source: ObservationSource,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    Constant { value: f64 },
    Observation { name: String },
}

/// Negative squared error rate, integrated with endpoint quadrature over each
/// action interval. Scale has the observation's SI unit; weight is per second.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RewardTerm {
    pub name: String,
    pub observation: String,
    pub target: Target,
    pub scale: f64,
    pub weight_per_s: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminationBound {
    pub observation: String,
    pub lower: f64,
    pub upper: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub version: u32,
    /// Required acknowledgement: these channels do not establish real sensors.
    pub observation_source: String,
    pub period_s: f64,
    pub observations: Vec<Observation>,
    pub rewards: Vec<RewardTerm>,
    pub termination_bounds: Vec<TerminationBound>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RewardValue {
    pub name: String,
    pub value: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Transition {
    pub time_s: f64,
    pub elapsed_s: f64,
    pub observations: Vec<f64>,
    pub reward: f64,
    pub reward_terms: Vec<RewardValue>,
    /// A declared task bound failed; this does not mean the robot succeeded.
    pub terminated: bool,
    /// The configured time horizon was reached (distinct from a task failure).
    pub truncated: bool,
    pub termination_reasons: Vec<String>,
    pub completed_steps: usize,
}

struct Binding {
    pointer: String,
    kind: QuantityKind,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentRecording {
    pub version: u32,
    pub kind: String,
    pub task: Task,
    pub runtime: EmbeddedRecording,
    pub error: Option<String>,
}

/// Shared native/WASM adapter. Solver errors are errors, never valid learning
/// transitions. After a failed advance, reset is required to continue.
pub struct EmbeddedEnvironment {
    session: EmbeddedSession,
    task: Task,
    stride: usize,
    bindings: Vec<Binding>,
    reward_indices: Vec<(usize, Option<usize>)>,
    bound_indices: Vec<usize>,
    latest: Transition,
    fault: Option<String>,
}

fn stride(period: f64, step: f64) -> Result<usize, String> {
    let n = period / step;
    if !period.is_finite()
        || period <= 0.0
        || !n.is_finite()
        || n < 1.0
        || n > 1_000_000.0
        || (n - n.round()).abs() > 1e-8
    {
        return Err(
            "environment period must be an integer multiple of physics and controller periods"
                .into(),
        );
    }
    Ok(n.round() as usize)
}

impl EmbeddedEnvironment {
    pub fn new(scene: Scene, config: Config, task: Task, seed: u64) -> Result<Self, String> {
        let n = stride(task.period_s, config.step_s)?;
        stride(task.period_s, scene.period_s)?;
        if task.version != 1
            || task.observation_source != "ideal_runtime_teacher_only"
            || task.observations.is_empty()
            || config.policy.is_none()
            || config.steps % n != 0
            || config.profile_solver
        {
            return Err("environment requires task v1, ideal_runtime_teacher_only, observations, sampled policy, a whole-transition horizon and profiling disabled".into());
        }
        let session = EmbeddedSession::new(scene, config, seed, CaptureMode::Latest)?;
        if session.inputs().is_empty() {
            return Err("environment requires declared controller action inputs".into());
        }
        let frame = session.frame()?;
        let mut names = BTreeSet::new();
        let mut bindings = Vec::new();
        for o in &task.observations {
            if o.name.trim().is_empty() || !names.insert(&o.name) {
                return Err("observation names must be nonempty and unique".into());
            }
            let coordinate = |name: &str| {
                session
                    .coordinate_names()
                    .iter()
                    .position(|x| x == name)
                    .ok_or_else(|| format!("unknown independent coordinate {name}"))
            };
            let body = |name: &str| {
                frame["poses"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .position(|p| p["name"] == name)
                    .ok_or_else(|| format!("unknown body {name}"))
            };
            use ObservationSource::*;
            let (pointer, kind) = match &o.source {
                CoordinatePosition { coordinate: name } => (
                    format!(
                        "/joint_positions/{}",
                        session.joint_indices()[coordinate(name)?]
                    ),
                    QuantityKind::Angle,
                ),
                CoordinateVelocity { coordinate: name } => (
                    format!(
                        "/joint_velocities/{}",
                        session.joint_indices()[coordinate(name)?]
                    ),
                    QuantityKind::AngularVelocity,
                ),
                ReferencePosition { coordinate: name } => (
                    format!("/reference_targets_rad/{}", coordinate(name)?),
                    QuantityKind::Angle,
                ),
                BodyPosition { link, axis } => (
                    format!("/poses/{}/position_m/{}", body(link)?, axis.index()),
                    QuantityKind::Length,
                ),
                BodyVelocity { link, axis } => (
                    format!("/poses/{}/velocity_m_s/{}", body(link)?, axis.index()),
                    QuantityKind::LinearVelocity,
                ),
                MotorCurrent { motor } => {
                    let index = session
                        .scene()
                        .robot
                        .motors
                        .iter()
                        .position(|m| &m.name == motor)
                        .ok_or_else(|| format!("unknown motor {motor}"))?;
                    (
                        format!("/motor_readings/{index}/current_a"),
                        QuantityKind::Current,
                    )
                }
            };
            bindings.push(Binding { pointer, kind });
        }
        let index = |name: &str| {
            task.observations
                .iter()
                .position(|o| o.name == name)
                .ok_or_else(|| format!("unknown task observation {name}"))
        };
        let mut reward_names = BTreeSet::new();
        let mut reward_indices = Vec::new();
        for r in &task.rewards {
            if r.name.trim().is_empty()
                || !reward_names.insert(&r.name)
                || !r.scale.is_finite()
                || r.scale <= 0.0
                || !r.weight_per_s.is_finite()
                || r.weight_per_s < 0.0
            {
                return Err("reward names must be unique; scales positive finite; weights nonnegative finite".into());
            }
            let i = index(&r.observation)?;
            let j = match &r.target {
                Target::Constant { value } if value.is_finite() => None,
                Target::Observation { name } => {
                    let j = index(name)?;
                    if bindings[i].kind != bindings[j].kind {
                        return Err("reward target units must match observation units".into());
                    }
                    Some(j)
                }
                _ => return Err("reward target must be finite".into()),
            };
            reward_indices.push((i, j));
        }
        let mut bound_indices = Vec::new();
        for b in &task.termination_bounds {
            if !b.lower.is_finite() || !b.upper.is_finite() || b.lower > b.upper {
                return Err("task bounds must be ordered and finite".into());
            }
            bound_indices.push(index(&b.observation)?);
        }
        let mut result = Self {
            session,
            task,
            stride: n,
            bindings,
            reward_indices,
            bound_indices,
            latest: Transition {
                time_s: 0.0,
                elapsed_s: 0.0,
                observations: vec![],
                reward: 0.0,
                reward_terms: vec![],
                terminated: false,
                truncated: false,
                termination_reasons: vec![],
                completed_steps: 0,
            },
            fault: None,
        };
        result.latest = result.observe(&frame, 0.0)?;
        Ok(result)
    }

    fn observe(&self, frame: &Value, elapsed_s: f64) -> Result<Transition, String> {
        let observations: Vec<f64> = self
            .bindings
            .iter()
            .zip(&self.task.observations)
            .map(|(b, o)| {
                frame
                    .pointer(&b.pointer)
                    .and_then(Value::as_f64)
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| format!("missing or nonfinite endpoint observation {}", o.name))
            })
            .collect::<Result<_, _>>()?;
        let mut reward_terms = Vec::new();
        for (r, &(i, j)) in self.task.rewards.iter().zip(&self.reward_indices) {
            let target = match (&r.target, j) {
                (_, Some(j)) => observations[j],
                (Target::Constant { value }, None) => *value,
                _ => unreachable!(),
            };
            let value = if elapsed_s == 0.0 {
                0.0
            } else {
                -((observations[i] - target) / r.scale).powi(2) * r.weight_per_s * elapsed_s
            };
            if !value.is_finite() {
                return Err(format!("nonfinite reward {}", r.name));
            }
            reward_terms.push(RewardValue {
                name: r.name.clone(),
                value,
            });
        }
        let reward = reward_terms.iter().map(|r| r.value).sum::<f64>();
        if !reward.is_finite() {
            return Err("nonfinite total reward".into());
        }
        let termination_reasons: Vec<_> = self
            .task
            .termination_bounds
            .iter()
            .zip(&self.bound_indices)
            .filter(|(b, i)| observations[**i] < b.lower || observations[**i] > b.upper)
            .map(|(b, _)| format!("{} outside [{}, {}]", b.observation, b.lower, b.upper))
            .collect();
        Ok(Transition {
            time_s: frame["time_s"].as_f64().ok_or("missing endpoint time")?,
            elapsed_s,
            observations,
            reward,
            reward_terms,
            terminated: !termination_reasons.is_empty(),
            truncated: self.session.remaining_steps() == 0,
            termination_reasons,
            completed_steps: self.session.completed_steps(),
        })
    }

    pub fn step(&mut self, action: &[f64]) -> Result<Transition, String> {
        if let Some(e) = &self.fault {
            return Err(format!("environment failed; reset required: {e}"));
        }
        if self.latest.terminated || self.latest.truncated {
            return Err("episode ended; reset required".into());
        }
        // Input validation precedes every mutation and physics advance.
        self.session.set_inputs(action)?;
        let result = self
            .session
            .advance(self.stride)
            .and_then(|()| self.session.frame())
            .and_then(|f| self.observe(&f, self.task.period_s));
        match result {
            Ok(t) => {
                self.latest = t.clone();
                Ok(t)
            }
            Err(e) => {
                self.fault = Some(e.clone());
                Err(e)
            }
        }
    }

    /// Rebuild and validate before replacing the old episode. No hidden warmup.
    pub fn reset(&mut self, seed: u64) -> Result<Transition, String> {
        let next = Self::new(
            self.session.scene().clone(),
            self.session.config().clone(),
            self.task.clone(),
            seed,
        )?;
        *self = next;
        Ok(self.latest.clone())
    }
    pub fn transition(&self) -> &Transition {
        &self.latest
    }
    pub fn inputs(&self) -> &[InputChannel] {
        self.session.inputs()
    }
    pub fn frame(&self) -> Result<Value, String> {
        self.session.interactive_frame()
    }
    pub fn error(&self) -> Option<&str> {
        self.fault.as_deref()
    }
    pub fn recording(&self) -> EmbeddedRecording {
        self.session.recording()
    }
    pub fn episode_recording(&self) -> EnvironmentRecording {
        EnvironmentRecording {
            version: 1,
            kind: "sampled_environment_recording".into(),
            task: self.task.clone(),
            runtime: self.recording(),
            error: self.fault.clone(),
        }
    }
    /// Replay reconstructs physics from the seed and every held action. It is
    /// proportional to episode length, not an instantaneous state checkpoint.
    /// Hosts execute one returned action at a time for progress/cancellation.
    pub fn prepare_replay(
        &self,
        record: EnvironmentRecording,
    ) -> Result<(Self, Vec<Vec<f64>>), String> {
        if record.version != 1
            || record.kind != "sampled_environment_recording"
            || record.error.is_some()
            || record.runtime.failure.is_some()
            || record.runtime.version != 3
            || record.runtime.kind != "embedded_session"
            || record.runtime.completed_steps > self.session.config().steps
            || record.runtime.completed_steps % self.stride != 0
        {
            return Err("only valid completed-transition environment prefixes can be replayed; failed attempts remain diagnostic records".into());
        }
        let equal = |a: Value, b: Value| -> Result<(), String> {
            if a == b {
                Ok(())
            } else {
                Err("replay must match loaded robot, controller and task".into())
            }
        };
        equal(json!(record.task), json!(self.task))?;
        equal(json!(record.runtime.scene), json!(self.session.scene()))?;
        equal(json!(record.runtime.config), json!(self.session.config()))?;
        let count = record.runtime.completed_steps / self.stride;
        // The production recorder stores changes only. Reconstruct unchanged
        // intervals from the declared initial action and preceding change.
        for (i, event) in record.runtime.input_events.iter().enumerate() {
            if event.at_step % self.stride != 0
                || event.at_step >= record.runtime.completed_steps
                || (i > 0 && event.at_step <= record.runtime.input_events[i - 1].at_step)
                || event.values.len() != self.inputs().len()
                || event
                    .values
                    .iter()
                    .zip(self.inputs())
                    .any(|(v, c)| !v.is_finite() || *v < c.lower || *v > c.upper)
            {
                return Err("invalid environment replay action schedule".into());
            }
        }
        let mut actions = Vec::with_capacity(count);
        let mut held = self.inputs().iter().map(|c| c.initial).collect::<Vec<_>>();
        let mut events = record.runtime.input_events.iter().peekable();
        for i in 0..count {
            if events.peek().is_some_and(|e| e.at_step == i * self.stride) {
                held = events.next().unwrap().values.clone();
            }
            actions.push(held.clone());
        }
        let next = Self::new(
            record.runtime.scene,
            record.runtime.config,
            record.task,
            record.runtime.seed,
        )?;
        Ok((next, actions))
    }
    pub fn metadata(&self) -> Value {
        json!({"coordinate_names":self.session.coordinate_names(),"joint_indices":self.session.joint_indices(),
            "step_s":self.session.config().step_s,"steps":self.session.config().steps,
            "report_every":self.stride,"policy_contract":self.session.policy_metadata(),
            "environment_contract":self.contract()})
    }
    pub fn task(&self) -> &Task {
        &self.task
    }
    pub fn contract(&self) -> Value {
        json!({"version":1,"period_s":self.task.period_s,"deployable":false,
            "observation_source":self.task.observation_source,"actions":self.inputs(),
            "observations":self.task.observations.iter().zip(&self.bindings).map(|(o,b)|
                json!({"name":o.name,"kind":b.kind,"unit":b.kind.unit(),"source":o.source})).collect::<Vec<_>>(),
            "reward":"sum of negative scaled squared endpoint errors times elapsed simulation seconds",
            "termination_sampling":"action transition endpoints; not physical travel stops"})
    }
}

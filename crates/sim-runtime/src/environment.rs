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
    /// Projection of world linear velocity onto an axis of the current body frame.
    BodyLocalVelocity { link: String, axis: Axis },
    /// World angular velocity, not Euler-angle derivatives.
    BodyAngularVelocity { link: String, axis: Axis },
    /// A body basis vector expressed in world coordinates (dimensionless).
    BodyAxis { link: String, body_axis: Axis, world_axis: Axis },
    /// Sum of terrain-contact forces on a named link, in world coordinates.
    FloorForce { link: String, axis: Axis },
    /// The currently held, validated action input; units come from its declaration.
    ControllerInput { name: String },
    /// Current held neural motor-angle correction, zero before the first sample.
    NeuralCorrection { actuator: String },
    MotorCurrent { motor: String },
    MotorTorque { motor: String },
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
    /// Optional positive reward rate for elapsed simulated time (1/s).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub survival_reward_per_s: f64,
    /// Subtracted once when a sampled task bound terminates a transition.
    /// Not applied at reset, timeout alone, or numerical failure.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub termination_penalty: f64,
    /// Optional executed stepping/body tracking objective, separate from policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub walking: Option<crate::walking_task::WalkingTaskConfig>,
}
fn is_zero(value: &f64) -> bool { *value == 0.0 }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RewardValue {
    pub name: String,
    pub value: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Transition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub walking: Option<crate::walking_task::WalkingObservation>,
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
    source: EndpointSource,
    kind: QuantityKind,
}
enum EndpointSource {
    Scalar(String),
    Projection { vector: String, rotation: String, column: usize },
    Input(usize),
    Neural(String),
    FloorForce { link: usize, axis: usize },
}
impl Binding {
    fn read(&self, frame: &Value, session: &EmbeddedSession) -> Option<f64> {
        match &self.source {
            EndpointSource::Input(index) => session.input_values().get(*index).copied(),
            EndpointSource::Neural(target) => session.neural_correction(target),
            EndpointSource::Scalar(pointer) => frame.pointer(pointer)?.as_f64(),
            EndpointSource::Projection { vector, rotation, column } => {
                let vector = frame.pointer(vector)?;
                let rotation = frame.pointer(rotation)?;
                let mut value = 0.0;
                for row in 0..3 {
                    value += vector.get(row)?.as_f64()? * rotation.get(row)?.get(*column)?.as_f64()?;
                }
                Some(value)
            }
            EndpointSource::FloorForce { link, axis } => {
                let mut force = 0.0;
                for contact in frame.get("contacts")?.as_array()? {
                    if contact.get("link")?.as_u64()? as usize == *link && contact.get("other")?.is_null() {
                        force += contact.get("force_n")?.get(*axis)?.as_f64()?;
                    }
                }
                Some(force)
            }
        }
    }
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
    walking: Option<crate::walking_task::WalkingMonitor>,
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
            let mut projection = None;
            let mut input_index = None;
            let mut floor_force = None;
            let mut neural = None;
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
                BodyLocalVelocity { link, axis } => {
                    let i = body(link)?;
                    projection = Some((format!("/poses/{i}/rotation"), axis.index()));
                    (format!("/poses/{i}/velocity_m_s"), QuantityKind::LinearVelocity)
                }
                BodyAngularVelocity { link, axis } => (
                    format!("/poses/{}/angular_velocity_rad_s/{}", body(link)?, axis.index()),
                    QuantityKind::AngularVelocity,
                ),
                BodyAxis { link, body_axis, world_axis } => (
                    format!("/poses/{}/rotation/{}/{}", body(link)?, world_axis.index(), body_axis.index()),
                    QuantityKind::Dimensionless,
                ),
                FloorForce { link, axis } => {
                    floor_force = Some((body(link)?, axis.index()));
                    ("/contacts".into(), QuantityKind::Force)
                }
                ControllerInput { name } => {
                    let i = session.inputs().iter().position(|c| &c.name == name)
                        .ok_or_else(|| format!("unknown controller input {name}"))?;
                    input_index = Some(i);
                    (format!("/policy_inputs/{i}"), session.inputs()[i].kind)
                }
                NeuralCorrection { actuator } => {
                    session.neural_correction(actuator).ok_or_else(|| format!("unknown neural correction {actuator}"))?;
                    neural = Some(actuator.clone());
                    (String::new(), QuantityKind::Angle)
                }
                MotorCurrent { motor } | MotorTorque { motor } => {
                    let index = session
                        .scene()
                        .robot
                        .motors
                        .iter()
                        .position(|m| &m.name == motor)
                        .ok_or_else(|| format!("unknown motor {motor}"))?;
                    if matches!(&o.source, MotorTorque { .. }) {
                        (format!("/motor_readings/{index}/shaft_torque_nm"), QuantityKind::Torque)
                    } else {
                        (format!("/motor_readings/{index}/current_a"), QuantityKind::Current)
                    }
                }
            };
            let source = if let Some(target) = neural { EndpointSource::Neural(target) }
                else if let Some(index) = input_index { EndpointSource::Input(index) }
                else if let Some((rotation, column)) = projection { EndpointSource::Projection { vector: pointer, rotation, column } }
                else if let Some((link, axis)) = floor_force { EndpointSource::FloorForce { link, axis } }
                else { EndpointSource::Scalar(pointer) };
            bindings.push(Binding { source, kind });
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
                || !reward_names.insert(r.name.clone())
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
        for (name, value) in [("task.survival", task.survival_reward_per_s), ("task.termination", task.termination_penalty)] {
            if !value.is_finite() || value < 0.0 || (value != 0.0 && reward_names.contains(name)) {
                return Err("task survival/termination rewards must be finite, nonnegative and have distinct reserved names".into());
            }
        }
        for b in &task.termination_bounds {
            if !b.lower.is_finite() || !b.upper.is_finite() || b.lower > b.upper {
                return Err("task bounds must be ordered and finite".into());
            }
            bound_indices.push(index(&b.observation)?);
        }
        let walking=task.walking.clone().map(|config|{
            let policy=session.config().policy.as_ref().ok_or("walking task requires policy")?;
            if policy.step_reference.is_none() || !session.scene().options.contact
                || session.scene().robot.gravity[0]!=0. || session.scene().robot.gravity[1]!=0. || session.scene().robot.gravity[2]>=0.
                || ["walking.body_tracking","walking.step_outcome"].iter().any(|n|reward_names.contains(*n)) {
                return Err("walking task requires online steps, world -Z gravity, contact and distinct reward names".into());
            }
            let body=policy.body_feedback.as_ref().ok_or("walking task requires body reference binding")?;
            let feet=policy.point_feedback.as_ref().ok_or("walking task requires foot reference bindings")?;
            crate::walking_task::WalkingMonitor::new(config,task.period_s,body.reference_link.clone(),feet.markers.iter().map(|m|m.link.clone()).collect(),session.articulated())
        }).transpose()?;
        let mut result = Self {
            walking,
            session,
            task,
            stride: n,
            bindings,
            reward_indices,
            bound_indices,
            latest: Transition {
                walking: None,
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

    fn observe(&mut self, frame: &Value, elapsed_s: f64) -> Result<Transition, String> {
        let observations: Vec<f64> = self
            .bindings
            .iter()
            .zip(&self.task.observations)
            .map(|(b, o)| {
                b.read(frame, &self.session)
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
        let termination_reasons: Vec<_> = self
            .task
            .termination_bounds
            .iter()
            .zip(&self.bound_indices)
            .filter(|(b, i)| observations[**i] < b.lower || observations[**i] > b.upper)
            .map(|(b, _)| format!("{} outside [{}, {}]", b.observation, b.lower, b.upper))
            .collect();
        if self.task.survival_reward_per_s != 0.0 {
            reward_terms.push(RewardValue { name: "task.survival".into(), value: self.task.survival_reward_per_s * elapsed_s });
        }
        if self.task.termination_penalty != 0.0 {
            reward_terms.push(RewardValue { name: "task.termination".into(),
                value: if elapsed_s > 0.0 && !termination_reasons.is_empty() { -self.task.termination_penalty } else { 0.0 } });
        }
        let walking=self.walking.as_mut().map(|w|w.observe(self.session.articulated(),frame,elapsed_s,
            self.session.remaining_steps()==0||!termination_reasons.is_empty())).transpose()?;
        if let Some(w)=&walking {
            reward_terms.push(RewardValue{name:"walking.body_tracking".into(),value:w.body_reward});
            reward_terms.push(RewardValue{name:"walking.step_outcome".into(),value:w.step_reward});
        }
        let reward = reward_terms.iter().map(|r| r.value).sum::<f64>();
        if !reward.is_finite() {
            return Err("nonfinite total reward".into());
        }
        Ok(Transition {
            walking,
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
    /// Read-only accepted-step diagnostics; no extra work enters the control loop.
    pub fn implicit_step_diagnostics(
        &self,
    ) -> &[sim_domain_robot::articulated::embedding::ImplicitStepDiagnostics] {
        self.session.implicit_step_diagnostics()
    }
    pub fn interval_diagnostics(&self) -> &[sim_dynamics::hybrid::HybridDiagnostics] {
        self.session.interval_diagnostics()
    }
    /// Host-only bounded-run diagnostics; does not alter the recorded recipe.
    pub fn retain_solver_diagnostics(&mut self, enabled: bool) {
        self.session.retain_solver_diagnostics(enabled);
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
        let mut contract = json!({"version":1,"period_s":self.task.period_s,"deployable":false,
            "observation_source":self.task.observation_source,"actions":self.inputs(),
            "observations":self.task.observations.iter().zip(&self.bindings).map(|(o,b)|
                json!({"name":o.name,"kind":b.kind,"unit":b.kind.unit(),"source":o.source})).collect::<Vec<_>>(),
            "reward":"sum of negative scaled squared endpoint errors times elapsed simulation seconds",
            "termination_sampling":"action transition endpoints; not physical travel stops"});
        if self.task.survival_reward_per_s != 0.0 || self.task.termination_penalty != 0.0 {
            contract["reward"] = json!("survival rate minus scaled squared endpoint errors, times elapsed simulation seconds; subtract termination penalty once for sampled task failure, not reset, timeout alone or numerical failure");
            contract["survival_reward_per_s"] = json!(self.task.survival_reward_per_s);
            contract["termination_penalty"] = json!(self.task.termination_penalty);
        }
        if let Some(w)=&self.task.walking {
            contract["walking_task"]=json!({"config":w,
                "body_error_unit":"m","body_error_frame":"world",
                "reference":"planning reference held over the current control interval; reference_sample records its controller sample",
                "reward":"additional capped squared body-position error rate; one qualified-step bonus or failed-step penalty per observed swing outcome; interrupted swings fail",
                "observation_scope":"privileged task diagnostics in transition.walking, not added to the actor sensor vector",
                "qualification":"same sampled CAD-surface clearance, unloading and support checker as offline lift acceptance; not between-sample contact accuracy or complete walking acceptance"});
        }
        contract
    }
}

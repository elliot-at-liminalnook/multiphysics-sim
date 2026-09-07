//! Explicit, teacher-only sampled control over the existing Coupler contract.
//! Joint observations are ideal runtime state, not invented CAD sensors.
use crate::body_feedback::{BodyFeedback, BodyFeedbackConfig};
use crate::point_feedback::{PointFeedback, PointFeedbackConfig};
use crate::session::{ControllerProgram, InputChannel};
use crate::task_observation::{TaskObservationConfig, TaskObserver};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sim_core::{Channel, Contract, Coupler, QuantityKind};
use sim_domain_robot::{Articulated, Generalized};
use sim_script::{RhaiController, parameter_map};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyConfig {
    /// Explicit acknowledgement; hardware observation bindings are not supplied.
    pub observation_source: ObservationSource,
    /// Software command envelope in named independent coordinates (rad),
    /// intersected with any authored CAD limits. Not physical travel stops.
    pub target_bounds_rad: BTreeMap<String, [f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_observations: Option<TaskObservationConfig>,
    /// Ideal body-state feedback suggestion; it does not replace servo physics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_feedback: Option<BodyFeedbackConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point_feedback: Option<PointFeedbackConfig>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationSource {
    IdealJointStateDiagnostics,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputEvent {
    pub at_step: usize,
    pub values: Vec<f64>,
}

pub(crate) struct SampledPolicy {
    policy: Box<dyn Coupler>,
    task_observer: Option<TaskObserver>,
    body_feedback: Option<BodyFeedback>,
    point_feedback: Option<PointFeedback>,
    contract: Contract,
    inputs: Vec<InputChannel>,
    values: Vec<f64>,
    indices: Vec<usize>,
    limits: Vec<[f64; 2]>,
    stride: usize,
    pub targets: Vec<f64>,
    telemetry: serde_json::Value,
}
impl SampledPolicy {
    pub fn new(
        art: &Articulated,
        names: &[String],
        indices: &[usize],
        initial: &[f64],
        config: &PolicyConfig,
        program: &ControllerProgram,
        period: f64,
        step: f64,
        seed: u64,
    ) -> Result<Self, String> {
        let n = period / step;
        if !period.is_finite()
            || period <= 0.0
            || !n.is_finite()
            || n < 1.0
            || n > 1e9
            || (n - n.round()).abs() > 1e-8
        {
            return Err(
                "policy period must be a positive integer number of nominal physics steps".into(),
            );
        }
        if names.len() != initial.len()
            || names.len() != indices.len()
            || config.target_bounds_rad.len() != names.len()
        {
            return Err(
                "one named command bound and initial target per independent coordinate required"
                    .into(),
            );
        }
        let dofs: Vec<_> = art.dofs().map(|(_, d)| d).collect();
        let mut sensors = Vec::new();
        let mut actuators = Vec::new();
        let mut limits = Vec::new();
        for (name, &index) in names.iter().zip(indices) {
            let d = dofs.get(index).ok_or("invalid policy coordinate")?;
            if !matches!(d.kind, sim_domain_robot::articulated::DofKind::Revolute) {
                return Err("servo policy expects angular coordinates".into());
            }
            let bound = config
                .target_bounds_rad
                .get(name)
                .ok_or_else(|| format!("missing software command envelope for {name}"))?;
            let lo = bound[0].max(d.lower.unwrap_or(f64::NEG_INFINITY));
            let hi = bound[1].min(d.upper.unwrap_or(f64::INFINITY));
            if bound.iter().any(|v| !v.is_finite()) || lo > hi {
                return Err(format!(
                    "invalid software/CAD command intersection for {name}"
                ));
            }
            limits.push([lo, hi]);
            let joint = name.strip_prefix("joint.").unwrap_or(name);
            sensors.push(Channel {
                name: format!("{joint}.angle"),
                kind: QuantityKind::Angle,
            });
            sensors.push(Channel {
                name: format!("{joint}.angular_velocity"),
                kind: QuantityKind::AngularVelocity,
            });
            sensors.push(Channel {
                name: format!("{joint}.reference"),
                kind: QuantityKind::Angle,
            });
            actuators.push(Channel {
                name: format!("{joint}.target"),
                kind: QuantityKind::Angle,
            });
        }
        let task_observer = config
            .task_observations
            .clone()
            .map(|c| TaskObserver::new(art, c))
            .transpose()?;
        if let Some(observer) = &task_observer {
            sensors.extend_from_slice(observer.channels());
        }
        let body_feedback = config
            .body_feedback
            .clone()
            .map(|c| BodyFeedback::new(art, c))
            .transpose()?;
        if body_feedback.is_some() {
            sensors.extend(names.iter().map(|name| Channel {
                name: format!(
                    "{}.body_correction",
                    name.strip_prefix("joint.").unwrap_or(name)
                ),
                kind: QuantityKind::Angle,
            }));
        }
        let point_feedback = config
            .point_feedback
            .clone()
            .map(|c| PointFeedback::new(art, c))
            .transpose()?;
        if point_feedback.is_some() {
            sensors.extend(names.iter().map(|name| Channel {
                name: format!(
                    "{}.point_correction",
                    name.strip_prefix("joint.").unwrap_or(name)
                ),
                kind: QuantityKind::Angle,
            }));
        }
        let mut seen: BTreeSet<_> = sensors.iter().map(|s| s.name.clone()).collect();
        for input in &program.inputs {
            if input.name.trim().is_empty()
                || !seen.insert(input.name.clone())
                || !input.lower.is_finite()
                || !input.upper.is_finite()
                || !input.initial.is_finite()
                || input.lower > input.upper
                || input.initial < input.lower
                || input.initial > input.upper
            {
                return Err(format!("invalid or duplicate policy input {}", input.name));
            }
            sensors.push(Channel {
                name: input.name.clone(),
                kind: input.kind,
            });
        }
        if initial
            .iter()
            .zip(&limits)
            .any(|(x, b)| !x.is_finite() || *x < b[0] || *x > b[1])
        {
            return Err("initial policy target violates software/CAD bounds".into());
        }
        let contract = Contract {
            element: "embedded.policy".into(),
            period,
            sensors,
            actuators,
        };
        let mut policy = RhaiController::with_seed(
            program.sources.clone(),
            parameter_map(&program.parameters).map_err(|e| e.to_string())?,
            seed,
        )
        .map_err(|e| e.to_string())?;
        policy.open(&contract).map_err(|e| e.to_string())?;
        Ok(Self {
            policy: Box::new(policy),
            task_observer,
            body_feedback,
            point_feedback,
            contract,
            inputs: program.inputs.clone(),
            values: program.inputs.iter().map(|i| i.initial).collect(),
            indices: indices.to_vec(),
            limits,
            stride: n.round() as usize,
            targets: initial.to_vec(),
            telemetry: json!(null),
        })
    }
    pub fn inputs(&self) -> &[InputChannel] {
        &self.inputs
    }
    pub fn values(&self) -> &[f64] {
        &self.values
    }
    pub fn validate_inputs(&self, values: &[f64]) -> Result<(), String> {
        if values.len() != self.inputs.len()
            || values
                .iter()
                .zip(&self.inputs)
                .any(|(x, c)| !x.is_finite() || *x < c.lower || *x > c.upper)
        {
            return Err("policy inputs must match declared count, units and bounds".into());
        }
        Ok(())
    }
    pub fn set_inputs(&mut self, values: &[f64]) -> Result<(), String> {
        self.validate_inputs(values)?;
        self.values.copy_from_slice(values);
        Ok(())
    }
    pub fn sample(
        &mut self,
        index: usize,
        time: f64,
        g: &Generalized,
        art: &Articulated,
        reference: &[f64],
        map: &sim_domain_robot::articulated::embedding::RigidEmbedding<'_>,
        reference_time: f64,
        reference_advancing: bool,
    ) -> Result<(), String> {
        if index % self.stride != 0 {
            return Ok(());
        }
        if reference.len() != self.indices.len() {
            return Err("policy reference dimension mismatch".into());
        }
        let mut sensors = Vec::new();
        for (&i, &r) in self.indices.iter().zip(reference) {
            sensors.extend([g.q[i], g.qd[i], r]);
        }
        if let Some(observer) = &self.task_observer {
            sensors.extend(observer.observe(art, g)?);
        }
        let body_feedback = self
            .body_feedback
            .as_ref()
            .map(|feedback| feedback.sample(art, map, g, reference_time, reference_advancing))
            .transpose()?;
        if let Some(sample) = &body_feedback {
            sensors.extend(&sample.correction_rad);
        }
        let point_feedback = self
            .point_feedback
            .as_ref()
            .map(|feedback| feedback.sample(art, map, g, reference_time))
            .transpose()?;
        if let Some(sample) = &point_feedback {
            sensors.extend(&sample.correction_rad);
        }
        sensors.extend(&self.values);
        if sensors.iter().any(|x| !x.is_finite()) {
            return Err("nonfinite policy observation".into());
        }
        let mut targets = self.targets.clone();
        self.policy
            .sample(time, &sensors, &mut targets)
            .map_err(|e| e.to_string())?;
        if targets
            .iter()
            .zip(&self.limits)
            .any(|(x, b)| !x.is_finite() || *x < b[0] || *x > b[1])
        {
            return Err("policy output violates software/CAD command bounds".into());
        }
        self.telemetry = json!({"time_s":time,"observations":self.contract.sensors.iter().zip(&sensors).map(|(c,v)|(c.name.clone(),*v)).collect::<BTreeMap<_,_>>(),"targets":self.contract.actuators.iter().zip(&targets).map(|(c,v)|(c.name.clone(),*v)).collect::<BTreeMap<_,_>>(),"observation_source":"ideal_joint_state_diagnostics"});
        if let Some(sample) = body_feedback {
            self.telemetry["body_feedback"] = json!(sample);
        }
        if let Some(sample) = point_feedback {
            self.telemetry["point_feedback"] = json!(sample);
        }
        self.targets = targets;
        Ok(())
    }
    pub fn telemetry(&self) -> &serde_json::Value {
        &self.telemetry
    }
    pub fn metadata(&self) -> serde_json::Value {
        let channels = |channels: &[Channel]| {
            channels
                .iter()
                .map(|c| json!({"name":c.name,"kind":c.kind,"unit":c.unit()}))
                .collect::<Vec<_>>()
        };
        let mut metadata = json!({"period_s":self.contract.period,"observation_source":"ideal_joint_state_diagnostics","deployable":false,"observations":channels(&self.contract.sensors),"actuators":channels(&self.contract.actuators),"software_target_bounds_rad":self.limits,"timing":"Sample committed state before the next physics interval; targets are held until subsequent firmware sampling. Rendering does not set either clock."});
        if let Some(observer) = &self.task_observer {
            metadata["task_observations"] = json!({"config":observer.config(),"coordinate_frame":"Body gravity direction, absolute COM velocity and angular velocity resolved in reference-link axes. Marker position and its time derivative relative to reference-link COM/axes. Floor force in world axes; link resultant excluding internal contacts, not force at marker."});
        }
        if let Some(feedback) = &self.body_feedback {
            metadata["body_feedback"] = json!({"config":feedback.config(),"scope":"Bounded angular target suggestions from ideal body COM and floor loads, using shared closure/point Jacobians. No pose mutation, contact guarantee, orientation feedback or deployable sensing. A policy must explicitly consume body_correction channels."});
        }
        if let Some(feedback) = &self.point_feedback {
            metadata["point_feedback"] = json!({"config":feedback.config(),"scope":"Bounded angular target suggestions from ideal world marker positions and explicit phase activation. Shared linkage Jacobians; no pose mutation, contact guarantee or deployable sensing. Policy must consume point_correction channels."});
        }
        metadata
    }
}
impl Drop for SampledPolicy {
    fn drop(&mut self) {
        self.policy.close();
    }
}

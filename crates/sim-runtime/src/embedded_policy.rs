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
    /// Software command envelope in named actuator coordinates (rad),
    /// intersected with any authored CAD limits. Not physical travel stops.
    pub target_bounds_rad: BTreeMap<String, [f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_observations: Option<TaskObservationConfig>,
    /// Authored scheduled IMUs. Each adds six physical channels, availability
    /// and sample age to the shared Rhai/neural observation contract.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imu_observations: Vec<String>,
    /// Ideal body-state feedback suggestion; it does not replace servo physics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_feedback: Option<BodyFeedbackConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point_feedback: Option<PointFeedbackConfig>,
    /// Online geometric references; executed through ordinary Rhai motor targets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_reference: Option<crate::step_reference::StepReferenceConfig>,
    /// Bounded neural angle corrections applied after baseline Rhai feedback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neural_residual: Option<sim_domain_control::neural::Network>,
    /// Explicitly saturate the combined Rhai + neural request at the existing
    /// software/CAD command bounds. Does not change actuator torque or dynamics.
    #[serde(default, skip_serializing_if = "is_false")]
    pub neural_command_saturation: bool,
    /// Seeded Gaussian samples in normalized neural-output coordinates.
    /// Raw likelihoods are recorded before the ordinary actuator saturation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neural_exploration: Option<sim_domain_control::ppo::GaussianExploration>,
    /// Causal dynamics predictions of a held baseline command, or the explicitly
    /// optimized sequence when forecast_action_search is configured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trajectory_forecast: Option<crate::predictive_policy::ForecastBundle>,
    /// Replan future actuator targets from learned dynamics before neural corrections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forecast_action_search: Option<crate::predictive_control::ForecastActionConfig>,
    /// Omit optional body/point motor-feedback suggestions when the controller
    /// does not consume them. Planner definitions remain available separately.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback_observations: Option<bool>,
}
fn is_false(value: &bool) -> bool { !value }
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
    imu_observer: crate::imu_observation::ImuObserver,
    body_feedback: Option<BodyFeedback>,
    point_feedback: Option<PointFeedback>,
    step_reference: Option<crate::step_reference::OnlineStepReference>,
    contract: Contract,
    inputs: Vec<InputChannel>,
    values: Vec<f64>,
    indices: Vec<usize>,
    limits: Vec<[f64; 2]>,
    stride: usize,
    pub targets: Vec<f64>,
    telemetry: serde_json::Value,
    neural: Option<sim_domain_control::neural::BoundNetwork>,
    neural_command_saturation: bool,
    neural_exploration: Option<sim_domain_control::ppo::GaussianSampler>,
    trajectory_forecast: Option<crate::predictive_policy::OnlineForecast>,
    forecast_action_search: Option<crate::predictive_control::ForecastActionPlanner>,
    neural_sensors: Vec<Channel>,
    corrections: Vec<f64>,
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
                "one named command bound and initial target per actuator coordinate required"
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
        let imu_observer = crate::imu_observation::ImuObserver::new(art, &config.imu_observations)?;
        sensors.extend_from_slice(imu_observer.channels());
        let body_feedback = config
            .body_feedback
            .clone()
            .filter(|_| config.feedback_observations != Some(false))
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
            .filter(|_| config.feedback_observations != Some(false))
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
        let trajectory_forecast = config.trajectory_forecast.clone().map(|bundle|
            crate::predictive_policy::OnlineForecast::new(bundle, art, period, &contract.actuators)
        ).transpose()?;
        let forecast_action_search=config.forecast_action_search.clone().map(|c|
            crate::predictive_control::ForecastActionPlanner::new(c,
                config.trajectory_forecast.clone().ok_or("forecast action search requires explicit dynamics models")?,
                &contract.sensors,&limits,period)).transpose()?;
        let mut neural_sensors = contract.sensors.clone();
        if let Some(forecast) = &trajectory_forecast {
            if let Some(actor)=config.neural_residual.as_ref(){
                if !actor.features.iter().any(|f| f.source == "forecast.valid" && f.subtract.is_none()) {
                    return Err("predictive actor must consume forecast.valid for missing startup history".into());
                }
            }else if forecast_action_search.is_none(){return Err("trajectory forecasts require an explicit neural policy or action search".into());}
            neural_sensors.extend(forecast.channels()?);
        }
        let neural = config.neural_residual.clone().map(|n| {
            imu_observer.validate_network(&n)?;
            n.bind(&neural_sensors, &contract.actuators)
        }).transpose()?;
        if config.neural_command_saturation && neural.is_none() {
            return Err("neural command saturation requires an explicit neural policy".into());
        }
        let neural_exploration=config.neural_exploration.clone().map(|c| {
            let n=neural.as_ref().ok_or("Gaussian exploration requires an explicit neural policy")?;
            if !config.neural_command_saturation {return Err("Gaussian exploration requires explicit command saturation".into());}
            sim_domain_control::ppo::GaussianSampler::new(c,n.definition().outputs.len(),seed^0x504f4c494359)
        }).transpose()?;
        let mut policy = RhaiController::with_seed(
            program.sources.clone(),
            parameter_map(&program.parameters).map_err(|e| e.to_string())?,
            seed,
        )
        .map_err(|e| e.to_string())?;
        policy.open(&contract).map_err(|e| e.to_string())?;
        let step_reference = config
            .step_reference
            .clone()
            .map(|c| {
                crate::step_reference::OnlineStepReference::new(
                    art,
                    c,
                    config
                        .body_feedback
                        .as_ref()
                        .ok_or("stepping requires body feedback")?,
                    config
                        .point_feedback
                        .as_ref()
                        .ok_or("stepping requires point feedback")?,
                    &program.inputs,
                    &limits,
                    period,
                )
            })
            .transpose()?;
        Ok(Self {
            policy: Box::new(policy),
            task_observer,
            imu_observer,
            body_feedback,
            point_feedback,
            step_reference,
            contract,
            inputs: program.inputs.clone(),
            values: program.inputs.iter().map(|i| i.initial).collect(),
            indices: indices.to_vec(),
            limits,
            stride: n.round() as usize,
            targets: initial.to_vec(),
            telemetry: json!(null),
            neural,
            neural_command_saturation: config.neural_command_saturation,
            neural_exploration,
            trajectory_forecast,
            forecast_action_search,
            neural_sensors,
            corrections: vec![0.0; initial.len()],
        })
    }
    pub fn inputs(&self) -> &[InputChannel] {
        &self.inputs
    }
    pub fn values(&self) -> &[f64] {
        &self.values
    }
    pub fn correction(&self, target: &str) -> Option<f64> {
        self.neural.as_ref()?;
        self.contract.actuators.iter().position(|c| c.name == target).map(|i| self.corrections[i])
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
        let online = self
            .step_reference
            .as_mut()
            .map(|r| {
                sim_solve::profile::POLICY_REFERENCE
                    .time(|| r.sample(art, map, g, time, &self.values))
            })
            .transpose()?;
        let reference = online
            .as_ref()
            .map_or(reference, |p| p.coordinates.as_slice());
        let mut sensors = Vec::new();
        for (&i, &r) in self.indices.iter().zip(reference) {
            sensors.extend([g.q[i], g.qd[i], r]);
        }
        if let Some(observer) = &self.task_observer {
            sensors
                .extend(sim_solve::profile::POLICY_OBSERVATIONS.time(|| observer.observe(art, g))?);
        }
        sensors.extend(self.imu_observer.observe(art, g, time)?);
        let body_feedback = self
            .body_feedback
            .as_ref()
            .map(|feedback| {
                sim_solve::profile::POLICY_BODY_FEEDBACK.time(|| match &online {
                    Some(p) => {
                        feedback.sample_target(art, map, g, time, p.reference.body_world_m, [0.; 3])
                    }
                    None => feedback.sample(art, map, g, reference_time, reference_advancing),
                })
            })
            .transpose()?;
        if let Some(sample) = &body_feedback {
            sensors.extend(&sample.correction_rad);
        }
        let point_feedback = self
            .point_feedback
            .as_ref()
            .map(|feedback| {
                sim_solve::profile::POLICY_POINT_FEEDBACK.time(|| match &online {
                    Some(p) => feedback.sample_target(
                        art,
                        map,
                        g,
                        time,
                        &p.feedback_feet_world_m,
                        &vec![1.; p.reference.feet_world_m.len()],
                    ),
                    None => feedback.sample(art, map, g, reference_time),
                })
            })
            .transpose()?;
        if let Some(sample) = &point_feedback {
            sensors.extend(&sample.correction_rad);
        }
        sensors.extend(&self.values);
        if sensors.iter().any(|x| !x.is_finite()) {
            return Err("nonfinite policy observation".into());
        }
        let mut targets = self.targets.clone();
        sim_solve::profile::POLICY_SCRIPT
            .time(|| self.policy.sample(time, &sensors, &mut targets))
            .map_err(|e| e.to_string())?;
        if targets.iter().any(|target| !target.is_finite()) {
            return Err("nonfinite baseline command before trajectory prediction".into());
        }
        let mut forecast = self.trajectory_forecast.as_mut().map(|f| {
            let proposal: Vec<_> = targets.iter().zip(&self.limits)
                .map(|(v,b)| v.clamp(b[0],b[1])).collect();
            f.sample(art, g, time, &self.targets, &proposal)
        }).transpose()?;
        let action_search=self.forecast_action_search.as_mut().map(|search|{
            let forecast=forecast.as_mut().ok_or("missing online forecast")?;
            let motion=crate::motion_data::MotionSnapshot::from_state(art,g,time);
            let report=search.sample(forecast,&sensors,&motion)?;
            if report.active {targets=forecast.proposed_targets_rad[0].clone();}
            Ok::<_,String>(report)
        }).transpose()?;
        let mut neural_observations = sensors.clone();
        if let Some(forecast) = &forecast { neural_observations.extend(forecast.values()); }
        let mut neural_decision=None;
        let corrections = self.neural.as_ref().map(|n| {
            if let Some(sampler)=&mut self.neural_exploration {
                let inputs=n.normalize(&neural_observations)?;
                let means=n.definition().normalized_output(&inputs,false)?;
                let (raw_actions,log_probability)=sampler.sample(&means)?;
                let corrections=n.scale_outputs(&raw_actions)?;
                neural_decision=Some(sim_domain_control::ppo::GaussianDecision{inputs,means,raw_actions,log_probability});
                Ok(corrections)
            } else { n.sample(&neural_observations) }
        }).transpose()?;
        if let Some(corrections) = &corrections {
            for (target, correction) in targets.iter_mut().zip(corrections) {
                if *correction != 0.0 { *target += correction; }
            }
        }
        let requested_targets = if self.neural_command_saturation {
            if targets.iter().any(|target| !target.is_finite()) {
                return Err("nonfinite combined neural command before saturation".into());
            }
            let requested = targets.clone();
            for (target, bounds) in targets.iter_mut().zip(&self.limits) {
                *target = target.clamp(bounds[0], bounds[1]);
            }
            Some(requested)
        } else { None };
        if let Some((index, (value, bounds))) = targets
            .iter()
            .zip(&self.limits)
            .enumerate()
            .find(|(_, (x, b))| !x.is_finite() || **x < b[0] || **x > b[1])
        {
            return Err(format!(
                "policy output violates software/CAD command bounds at {time} s: {} requested {value} rad; allowed [{}, {}] rad",
                self.contract.actuators[index].name, bounds[0], bounds[1]
            ));
        }
        self.telemetry = json!({"time_s":time,"observations":self.contract.sensors.iter().zip(&sensors).map(|(c,v)|(c.name.clone(),*v)).collect::<BTreeMap<_,_>>(),"targets":self.contract.actuators.iter().zip(&targets).map(|(c,v)|(c.name.clone(),*v)).collect::<BTreeMap<_,_>>(),"observation_source":"ideal_joint_state_diagnostics"});
        if let Some(decision)=neural_decision {self.telemetry["neural_decision"]=json!(decision);}
        if let Some(report)=action_search {self.telemetry["forecast_action_search"]=json!(report);}
        if let Some(forecast) = forecast {
            self.telemetry["trajectory_forecast"] = json!(forecast);
            self.telemetry["neural_observations"] = json!(self.neural_sensors.iter().zip(&neural_observations)
                .map(|(c,v)|(c.name.clone(),*v)).collect::<BTreeMap<_,_>>());
        }
        if let Some(requested) = requested_targets {
            let count = requested.iter().zip(&targets).filter(|(a,b)| a != b).count();
            self.telemetry["neural_command_saturation"] = json!({"requested_targets_rad":requested,"saturated_commands":count});
        }
        if let Some(corrections) = corrections {
            self.telemetry["neural_residual"] = json!(self.contract.actuators.iter().zip(&corrections).map(|(c,v)| (c.name.clone(),*v)).collect::<BTreeMap<_,_>>());
            self.corrections = corrections;
        }
        if let Some(p) = online {
            self.telemetry["step_reference"] = json!({"reference":p.reference,"coordinates":p.coordinates,"maximum_marker_error_m":p.maximum_marker_error_m,"static_support":p.support,"feedback_feet_world_m":p.feedback_feet_world_m,"preload_extension_m":p.preload_extension_m,"measured_support_force_n":p.measured_support_force_n});
        }
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
        if !self.imu_observer.channels().is_empty() {
            metadata["authored_imu_observations"] = self.imu_observer.metadata().clone();
        }
        if let Some(neural) = &self.neural {
            metadata["neural_residual"] = json!({"definition":neural.definition(),"scope":"Bounded corrections after Rhai feedback, before software/CAD command validation. Pure Rust inference from current sampled observations; no physics bypass."});
            if self.neural_command_saturation {
                metadata["neural_residual"]["command_saturation"] = json!("Combined finite Rhai/neural requests are saturated at existing software/CAD command bounds before the same servo dynamics. Neural correction telemetry records the requested correction; policy targets record the applied command.");
            }
            if self.neural_exploration.is_some() {
                metadata["neural_residual"]["exploration"] = json!("Seeded Gaussian normalized actions before actuator saturation; neural_decision records input, mean, raw action and raw-action log probability. Episode seed and exploration config reproduce the draws.");
            }
        }
        if self.trajectory_forecast.is_some() {
            metadata["trajectory_forecast"] = json!({"observations":channels(&self.neural_sensors),
                "scope":"Causal separate horizon heads consume current Rust dynamics, backward finite-interval acceleration, previous applied commands and the current clamped Rhai proposal held for that horizon. No recorded future state or command is read. Forecast valid is zero until a previous sampled state exists; startup forecast values are placeholders. Forecasts describe the baseline proposal, before neural corrections; prediction error under learner commands requires separate validation."});
        }
        if let Some(search)=&self.forecast_action_search{
            metadata["trajectory_forecast"]["scope"]=json!("Causal separate horizon heads use current Rust dynamics, backward finite-interval acceleration, previous applied targets and the optimized future-action prefix for each horizon. Only the first target is applied before replanning. Forecasts precede neural corrections; longer-horizon prediction error requires checking which proposed future targets were actually executed.");
            metadata["forecast_action_search"]=json!({"config":search.config(),"scope":"Receding-horizon learned displacement search in requested horizontal travel direction. Existing software/CAD actuator bounds apply; command lease and zero requests return to the baseline policy. Forecast telemetry describes the optimized sequence before neural corrections. Only its first action is applied; subsequent actions are replanned from new dynamics. No model prediction certifies contact, speed or absence of falls."});
            if search.config().reference_proposal.is_some(){
                metadata["forecast_action_search"]["reference_scope"]=json!("An authored actuator trajectory on episode simulation time is compared with the held command and shifted previous plan. The highest predicted objective initializes optimization; every actuator variable remains free within existing bounds. No future physical state is read and no reference-tracking reward is added.");
            }
            if search.config().objective==crate::predictive_control::ForecastActionObjective::EpisodeNetDisplacement{
                metadata["forecast_action_search"]["scope"]=json!("Speed-discovery planning maximizes predicted increase in XY distance from the reset origin using the evaluator's shared endpoint metric. Every candidate endpoint is transformed from the current reference-link frame to world coordinates. Request direction only chooses a subgradient at zero distance; zero requests and expired command leases return to baseline control. Existing actuator bounds and physical execution remain unchanged. A short-horizon forecast is not a full-episode speed or fall certificate.");
            }
        }
        if let Some(r) = &self.step_reference {
            let scope = if r.config().sequence.update_command_before_lift {
                "Online support sequence and bounded CAD inverse kinematics. Non-reversing commands are reconsidered before lift-off after support qualification. Stops cancel unstarted swings and recenter without moving planted foot references; airborne swings finish landing. Translation reversals retain the committed transfer before selecting a new stance. Every reference still passes CAD geometry/placement checks. Ideal floor loads qualify lift, landing and recenter transitions."
            } else {
                "Online support sequence and bounded CAD inverse kinematics. Commands latch at foot-transfer boundaries; geometric references never mutate physical state. Ideal floor loads qualify lift/landing transitions."
            };
            metadata["step_reference"] = json!({"config":r.config(),"scope":scope});
        }
        if let Some(observer) = &self.task_observer {
            metadata["task_observations"] = json!({"config":observer.config(),"coordinate_frame":"Body gravity direction, absolute COM velocity and angular velocity resolved in reference-link axes. Marker position and its time derivative relative to reference-link COM/axes. Floor force in world axes; link resultant excluding internal contacts, not force at marker."});
            if observer.config().heading_world_z {
                metadata["task_observations"]["heading_world_z"] = json!("Ideal atan2(R_yx,R_xx), radians about world Z; undefined for a vertical reference X axis. Explicit simulation observation, not a calibrated hardware sensor.");
            }
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

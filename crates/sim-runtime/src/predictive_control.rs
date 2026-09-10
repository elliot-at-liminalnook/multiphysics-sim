//! Receding-horizon actuator proposals through a learned dynamics model. The
//! model ranks proposals; the ordinary runtime still owns every physical step.
use crate::{
    motion_data::{LinkMotion, MotionSnapshot},
    motion_forecast::MotionAxis,
    predictive_policy::{ForecastBundle, ForecastObservation},
};
use serde::{Deserialize, Serialize};
use sim_core::{Channel, QuantityKind as Q};
use sim_domain_control::{
    command_lease::{CommandLease, CommandLeaseConfig, CommandLeaseState},
    optimization::{ProjectedAscentResult, ProjectedAscentSettings, projected_ascent},
    trajectory::{Trajectory, TrajectoryConfig},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForecastActionObjective {
    #[default]
    RequestedDirection,
    /// Speed-discovery mode: maximize predicted increase in distance from the
    /// episode origin. Request direction only chooses a subgradient at zero.
    EpisodeNetDisplacement,
}
impl ForecastActionObjective {
    fn is_requested(&self) -> bool {
        *self == Self::RequestedDirection
    }
}

/// Transform an anchored forecast into the same XY endpoint objective used by
/// SpeedMonitor. Returns progress in metres and its reference-frame derivative.
pub fn forecast_net_progress(
    origin: [f64; 2],
    anchor: &LinkMotion,
    position_reference: [f64; 3],
) -> Result<(f64, [f64; 3]), String> {
    let pose = crate::session::LinkPose {
        name: anchor.name.clone(),
        position_m: anchor.position_m,
        rotation: anchor.rotation,
    };
    if !pose.valid_rigid_transform() || position_reference.iter().any(|x| !x.is_finite()) {
        return Err("invalid forecast endpoint transform".into());
    }
    let endpoint = std::array::from_fn(|i| {
        anchor.position_m[i]
            + (0..3)
                .map(|j| anchor.rotation[i][j] * position_reference[j])
                .sum::<f64>()
    });
    let after = crate::speed_task::net_displacement(origin, endpoint)?;
    let before =
        crate::speed_task::net_displacement(origin, [anchor.position_m[0], anchor.position_m[1]])?;
    let gradient = std::array::from_fn(|j| {
        (0..2)
            .map(|i| after.position_gradient[i] * anchor.rotation[i][j])
            .sum()
    });
    Ok((after.distance_m - before.distance_m, gradient))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForecastActionReference {
    pub expected_cad_sha256: String,
    pub actuators: Vec<Channel>,
    /// An authored actuator plan on episode simulation time. This supplies
    /// candidate commands, never future observations or a tracking objective.
    pub trajectory: TrajectoryConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForecastActionConfig {
    #[serde(default, skip_serializing_if = "ForecastActionObjective::is_requested")]
    pub objective: ForecastActionObjective,
    pub optimizer: ProjectedAscentSettings,
    pub forward_speed_input: String,
    pub lateral_speed_input: String,
    pub packet_sequence_input: String,
    pub command_lease: CommandLeaseConfig,
    /// Rotate requested planar travel in the forecast reference-link frame.
    pub heading_offset_rad: f64,
    pub warm_start: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_proposal: Option<ForecastActionReference>,
}
fn is_false(value: &bool) -> bool {
    !value
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForecastActionReport {
    #[serde(default)]
    pub objective: ForecastActionObjective,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode_origin_xy_m: Option<[f64; 2]>,
    pub active: bool,
    pub reason: String,
    pub lease: CommandLeaseState,
    pub direction_reference: [f64; 3],
    pub held_proposal_displacement_m: Option<f64>,
    pub warm_start_selected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_proposal_displacement_m: Option<f64>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub reference_proposal_selected: bool,
    pub optimization: Option<ProjectedAscentResult>,
}
pub(crate) struct ForecastActionPlanner {
    config: ForecastActionConfig,
    bundle: ForecastBundle,
    lease: CommandLease,
    state: CommandLeaseState,
    command_indices: [usize; 3],
    action_indices: Vec<usize>,
    bounds: Vec<[f64; 2]>,
    position_indices: [usize; 3],
    gravity_indices: [usize; 3],
    previous_plan: Option<Vec<Vec<f64>>>,
    origin: Option<[f64; 2]>,
    reference: Option<Trajectory>,
}
impl ForecastActionPlanner {
    pub fn config(&self) -> &ForecastActionConfig {
        &self.config
    }
    pub fn new(
        config: ForecastActionConfig,
        bundle: ForecastBundle,
        sensors: &[Channel],
        limits: &[[f64; 2]],
        period: f64,
    ) -> Result<Self, String> {
        bundle.validate()?;
        if !config.heading_offset_rad.is_finite()
            || (config.command_lease.period_s - period).abs() > 1e-12
            || config.optimizer.iterations == 0
            || config.optimizer.backtracks == 0
            || !config.optimizer.step_fraction.is_finite()
            || config.optimizer.step_fraction <= 0.
        {
            return Err("invalid forecast action-search clock/settings".into());
        }
        let bind = |name: &str, kind: Q| {
            let found = sensors
                .iter()
                .enumerate()
                .filter(|(_, c)| c.name == name && c.kind == kind)
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            if found.len() != 1 {
                Err(format!(
                    "missing/ambiguous typed predictive-control input {name}"
                ))
            } else {
                Ok(found[0])
            }
        };
        let command_indices = [
            bind(&config.forward_speed_input, Q::LinearVelocity)?,
            bind(&config.lateral_speed_input, Q::LinearVelocity)?,
            bind(&config.packet_sequence_input, Q::Dimensionless)?,
        ];
        if command_indices[0] == command_indices[1] {
            return Err("distinct planar command inputs required".into());
        }
        let head = bundle.heads.last().unwrap();
        let recipe = &head.recipe;
        let horizon = recipe.horizons_steps[0];
        if !recipe.controller_inputs.is_empty() || limits.is_empty() || limits.len() != recipe.actuator_targets.len()
            || limits
                .iter()
                .any(|b| !b[0].is_finite() || !b[1].is_finite() || b[0] >= b[1])
        {
            return Err("forecast search requires existing nonempty actuator bounds".into());
        }
        let reference = config
            .reference_proposal
            .as_ref()
            .map(|r| {
                if r.expected_cad_sha256 != recipe.expected_cad_sha256
                    || r.actuators.len() != recipe.actuator_targets.len()
                    || r.actuators
                        .iter()
                        .zip(&recipe.actuator_targets)
                        .any(|(a, name)| &a.name != name || a.kind != Q::Angle)
                {
                    return Err("forecast reference CAD/typed actuator contract mismatch".into());
                }
                let trajectory = Trajectory::new(r.trajectory.clone())?;
                if trajectory.dimension() != limits.len()
                    || r.trajectory.keyframes.iter().any(|k| {
                        k.values
                            .iter()
                            .zip(limits)
                            .any(|(v, b)| *v < b[0] || *v > b[1])
                    })
                {
                    return Err(
                        "forecast reference must obey existing actuator dimensions/bounds".into(),
                    );
                }
                Ok::<_, String>(trajectory)
            })
            .transpose()?;
        let mut action_indices = vec![];
        let mut bounds = vec![];
        for step in 1..=horizon {
            for (target, bound) in recipe.actuator_targets.iter().zip(limits) {
                let name = format!("action.{step}.{target}");
                action_indices.push(
                    head.network
                        .features
                        .iter()
                        .position(|f| f.source == name)
                        .ok_or("missing future action feature")?,
                );
                bounds.push(*bound);
            }
        }
        let mut position_indices = [0; 3];
        let mut gravity_indices = [0; 3];
        for axis in 0..3 {
            let matching = recipe
                .axes
                .iter()
                .filter_map(|a| match a {
                    MotionAxis::Link {
                        name,
                        link,
                        axis: i,
                    } if link == &recipe.reference_link && *i == axis => Some(name),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if matching.len() != 1 {
                return Err(
                    "forecast search requires all three reference-link position axes exactly once"
                        .into(),
                );
            }
            let target = format!("h{horizon}.{}.position", matching[0]);
            position_indices[axis] = head
                .network
                .outputs
                .iter()
                .position(|o| o.target == target && o.kind == Q::Length)
                .ok_or("missing reference displacement output")?;
            let name = format!("body.gravity.{}", ["x", "y", "z"][axis]);
            gravity_indices[axis] = head
                .network
                .features
                .iter()
                .position(|f| f.source == name && f.kind == Q::Dimensionless)
                .ok_or("missing gravity input")?;
        }
        let lease = CommandLease::new(config.command_lease.clone())?;
        let state = lease.initial();
        Ok(Self {
            config,
            bundle,
            lease,
            state,
            command_indices,
            action_indices,
            bounds,
            position_indices,
            gravity_indices,
            previous_plan: None,
            origin: None,
            reference,
        })
    }
    pub fn sample(
        &mut self,
        forecast: &mut ForecastObservation,
        sensors: &[f64],
        motion: &MotionSnapshot,
    ) -> Result<ForecastActionReport, String> {
        let anchor = motion
            .poses
            .iter()
            .find(|p| p.name == self.bundle.heads[0].recipe.reference_link)
            .ok_or("missing planning reference pose")?;
        if self.config.objective == ForecastActionObjective::EpisodeNetDisplacement
            && self.origin.is_none()
        {
            self.origin = Some([anchor.position_m[0], anchor.position_m[1]]);
        }
        let command = self.command_indices.map(|i| {
            sensors
                .get(i)
                .copied()
                .ok_or("missing predictive-control input")
        });
        let [forward, lateral, sequence] = command;
        let (forward, lateral, sequence) = (forward?, lateral?, sequence?);
        if !forward.is_finite() || !lateral.is_finite() {
            return Err("nonfinite predictive motion request".into());
        }
        self.state = self
            .lease
            .update(self.state.sequence, self.state.age_s, sequence)?;
        let mut report = ForecastActionReport {
            objective: self.config.objective,
            episode_origin_xy_m: self.origin,
            active: false,
            reason: "startup_history".into(),
            lease: self.state,
            direction_reference: [0.; 3],
            held_proposal_displacement_m: None,
            warm_start_selected: false,
            reference_proposal_displacement_m: None,
            reference_proposal_selected: false,
            optimization: None,
        };
        if !self.state.fresh || forward == 0. && lateral == 0. || !forecast.history_valid {
            report.reason = if !self.state.fresh {
                "expired_command"
            } else if forward == 0. && lateral == 0. {
                "zero_motion_request"
            } else {
                "startup_history"
            }
            .into();
            self.previous_plan = None;
            return Ok(report);
        }
        let head = self.bundle.heads.last().unwrap();
        let mut direction = [
            forward * self.config.heading_offset_rad.cos()
                - lateral * self.config.heading_offset_rad.sin(),
            forward * self.config.heading_offset_rad.sin()
                + lateral * self.config.heading_offset_rad.cos(),
            0.,
        ];
        let gravity = self.gravity_indices.map(|i| forecast.inputs[i]);
        let norm2 = gravity.iter().map(|v| v * v).sum::<f64>();
        if !norm2.is_finite() || (norm2 - 1.).abs() > 1e-6 {
            return Err("invalid reference gravity direction".into());
        }
        let vertical = direction
            .iter()
            .zip(gravity)
            .map(|(d, g)| d * g)
            .sum::<f64>();
        for i in 0..3 {
            direction[i] -= vertical * gravity[i];
        }
        let norm = direction.iter().map(|v| v * v).sum::<f64>().sqrt();
        if !norm.is_finite() {
            return Err("nonfinite horizontal motion request".into());
        }
        if norm <= 1e-12 {
            if self.config.objective == ForecastActionObjective::RequestedDirection {
                report.reason = "degenerate_horizontal_request".into();
                self.previous_plan = None;
                return Ok(report);
            }
            // Net distance is still defined when the requested body direction
            // projects to zero. Its endpoint derivative does not need this ray.
            direction = [0.; 3];
        } else {
            for d in &mut direction {
                *d /= norm;
            }
        }
        report.direction_reference = direction;
        let raw = forecast.inputs.clone();
        let prior = forecast.priors.last().ok_or("missing forecast prior")?;
        let objective = |a: &[f64]| -> Result<(f64, Vec<f64>), String> {
            let mut input = raw.clone();
            for (&i, &v) in self.action_indices.iter().zip(a) {
                input[i] = v;
            }
            let prediction = head.predict(&input, prior)?;
            let mut derivative = vec![0.; head.network.outputs.len()];
            let score = match self.config.objective {
                ForecastActionObjective::RequestedDirection => {
                    for i in 0..3 {
                        derivative[self.position_indices[i]] = direction[i];
                    }
                    prediction.iter().zip(&derivative).map(|(x, d)| x * d).sum()
                }
                ForecastActionObjective::EpisodeNetDisplacement => {
                    let position = self.position_indices.map(|i| prediction[i]);
                    let (score, mut gradient) = forecast_net_progress(
                        self.origin.ok_or("missing episode origin")?,
                        anchor,
                        position,
                    )?;
                    // At exactly zero endpoint distance, select the requested
                    // horizontal unit vector from the norm's subdifferential.
                    if gradient == [0.; 3] {
                        gradient = direction;
                    }
                    for i in 0..3 {
                        derivative[self.position_indices[i]] = gradient[i];
                    }
                    score
                }
            };
            let g = head.input_gradient(&input, &derivative)?;
            Ok((score, self.action_indices.iter().map(|&i| g[i]).collect()))
        };
        let mut initial = self
            .action_indices
            .iter()
            .map(|&i| raw[i])
            .collect::<Vec<_>>();
        let held = objective(&initial)?.0;
        report.held_proposal_displacement_m = Some(held);
        let mut initial_score = held;
        if self.config.warm_start {
            if let Some(previous) = &self.previous_plan {
                let warm = previous
                    .iter()
                    .skip(1)
                    .chain(forecast.proposed_targets_rad.last())
                    .flatten()
                    .copied()
                    .collect::<Vec<_>>();
                if warm.len() == initial.len() {
                    let score = objective(&warm)?.0;
                    if score > initial_score {
                        initial = warm;
                        initial_score = score;
                        report.warm_start_selected = true;
                    }
                }
            }
        }
        if let Some(reference) = &self.reference {
            let mut proposal = Vec::with_capacity(initial.len());
            for step in 0..head.recipe.horizons_steps[0] {
                // Action1 executes in [t,t+period], so the first reference
                // command is sampled at t, not at the end of that interval.
                proposal.extend(
                    reference
                        .sample(forecast.time_s + step as f64 * head.recipe.period_s)?
                        .values,
                );
            }
            let score = objective(&proposal)?.0;
            report.reference_proposal_displacement_m = Some(score);
            if score > initial_score {
                initial = proposal;
                report.reference_proposal_selected = true;
                report.warm_start_selected = false;
            }
        }
        let result = projected_ascent(&initial, &self.bounds, &self.config.optimizer, objective)?;
        for (&i, &v) in self.action_indices.iter().zip(&result.parameters) {
            forecast.inputs[i] = v;
        }
        forecast.proposed_targets_rad = result
            .parameters
            .chunks(head.recipe.actuator_targets.len())
            .map(|s| s.to_vec())
            .collect();
        for (i, h) in self.bundle.heads.iter().enumerate() {
            forecast.predictions[i] = h.predict(
                &forecast.inputs[..h.network.features.len()],
                &forecast.priors[i],
            )?;
        }
        self.previous_plan = Some(forecast.proposed_targets_rad.clone());
        report.active = true;
        report.reason = "planned".into();
        report.optimization = Some(result);
        Ok(report)
    }
}

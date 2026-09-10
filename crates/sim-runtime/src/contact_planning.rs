//! Offline phase-based motion optimization using CAD kinematics and inverse dynamics.
//! The search uses weighted residuals; independent physical tolerances determine
//! feasibility. Neither a solver stop nor a sampled plan certifies a live gait.
use crate::{
    contact_audit::{FloorClearance, sampled_floor_clearances, sampled_inter_link_penetrations},
    session::LinkPose,
    tracking::CaptureConfig,
};
use nalgebra::{UnitQuaternion, Vector3 as V};
use serde::{Deserialize, Serialize};
use sim_domain_control::contact_phase::{ContactPhaseConfig, ContactPhaseMotion};
use sim_domain_robot::{
    Articulated, Generalized,
    articulated::embedding::{
        CoordinateInterval, EmbeddedPoint, EmbeddingConfig, PlanePlacementConfig,
        PointMotionTarget, RigidEmbedding,
    },
    effective_servo::EffectiveServo,
    math::{quat_parts, rotation_vector_motion},
    motion_capability::{ConstrainedForceConfig, PointForceLoadMap, constrained_point_forces},
};
use sim_solve::least_squares::{
    LeastSquaresConfig, LeastSquaresResult, VariableBound, bounded_least_squares,
};
use std::collections::BTreeMap;

mod joint;
pub use joint::JointDomainRestoration;
#[cfg(feature = "conic")]
pub use joint::{ContactOrderNeighbor, JointForceConicOptimization};
#[cfg(all(feature = "native-ipopt", not(target_arch = "wasm32")))]
pub use joint::{JointIpoptConfig, JointIpoptOptimization};
pub use joint::{
    ContactEvent, ContactSurfaceFrame, ContactSurfacePoint, ForceKnotBinding, JointCacheStatistics,
    JointContactDecision, JointContactMotion, JointContactOptimization, JointContactReport,
    JointContactVariable, JointForceFrameSystem, JointForceTiming,
};

/// Offline mesh limit; reference compilation may need finer grids than search.
const MAX_CONTACT_SAMPLES: usize = 10_000;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContactSampling {
    /// Compatibility for archived diagnostics. May miss short contact intervals.
    #[default]
    UniformAndFootMidpoints,
    /// Add three checks in every constant-contact interval, including near edges.
    ContactIntervals,
}
fn unit_phase_rate() -> f64 {
    1.0
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContactClock {
    pub phase_rate: f64,
    pub phase_acceleration_per_s: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContactPlanRecipe {
    pub expected_cad_sha256: String,
    pub independent_coordinates: Vec<String>,
    pub initial_coordinates: Vec<f64>,
    pub initial_base_translation_m: [f64; 3],
    pub joint_search_bounds: Vec<CoordinateInterval>,
    pub embedding: EmbeddingConfig,
    pub placement: PlanePlacementConfig,
    pub actuators: BTreeMap<String, BTreeMap<String, f64>>,
    /// Explicit software/CAD command bounds for the nominal effective-servo reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub servo_command_limits: Option<sim_domain_robot::effective_servo::ServoCommandLimits>,
    pub uniform_samples: usize,
    #[serde(default)]
    pub sampling: ContactSampling,
    /// Explicit numerical collocation phases, independent of the uniform grid.
    #[serde(default)]
    pub additional_phases: Vec<f64>,
    /// Include body spline knots, where acceleration can have extrema.
    #[serde(default)]
    pub include_body_knots: bool,
    /// Prescribed reference-clock rate (negative for reverse, zero for static load diagnostics).
    #[serde(default = "unit_phase_rate")]
    pub phase_rate: f64,
    #[serde(default)]
    pub phase_acceleration_per_s: f64,
    /// Additional operating conditions that must satisfy the same physical gates.
    /// Their constraint residuals are appended; the primary clock alone defines
    /// the speed objective. This supports forward/reverse or ramp diagnostics.
    #[serde(default)]
    pub additional_clocks: Vec<ContactClock>,
    /// Numerical merit function only; physical feasibility continues to use
    /// the exact clipped runtime torque capacity and declared tolerance.
    #[serde(default)]
    pub extend_motoring_penalty_past_no_load: bool,
    pub force_allocation: ConstrainedForceConfig,
    pub moment_length_scale_m: f64,
    pub force_tolerance_n: f64,
    pub moment_tolerance_nm: f64,
    pub torque_tolerance_nm: f64,
    pub penetration_tolerance_m: f64,
    pub velocity_tolerance_m_s: f64,
    pub acceleration_tolerance_m_s2: f64,
    pub direction_world: [f64; 3],
    pub target_speed_m_s: f64,
    pub speed_residual_scale_m_s: f64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContactDecision {
    Period,
    Displacement {
        axis: usize,
    },
    /// Scalar travel along the recipe's unit world direction, in metres.
    DisplacementAlongDirection,
    FootPhase {
        foot: usize,
    },
    FootStance {
        foot: usize,
    },
    FootCenter {
        foot: usize,
        axis: usize,
    },
    FootSwing {
        foot: usize,
        axis: usize,
    },
    /// Additional-step indices are zero-based within `additional_steps`;
    /// phase/duration units are cycle fractions, centers and excursions metres.
    AdditionalStepPhase { foot: usize, step: usize },
    AdditionalStepStance { foot: usize, step: usize },
    AdditionalStepCenter { foot: usize, step: usize, axis: usize },
    AdditionalStepSwing { foot: usize, step: usize, axis: usize },
    BodyControl {
        control: usize,
        channel: usize,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContactVariable {
    pub decision: ContactDecision,
    pub bound: VariableBound,
}
#[derive(Clone, Debug, Serialize)]
pub struct ContactPlanFrame {
    pub time_s: f64,
    pub clock: ContactClock,
    pub residual_weight: f64,
    pub planned_contacts: Vec<bool>,
    pub coordinates: Vec<f64>,
    pub reduced_velocity: Vec<f64>,
    pub reduced_acceleration: Vec<f64>,
    pub support_forces_world_n: Vec<[f64; 3]>,
    pub wrench_residual: Vec<f64>,
    pub motor_torques_nm: Vec<f64>,
    pub torque_capacity_margin_nm: Vec<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub servo_command: Option<sim_domain_robot::effective_servo::ServoCommandCheck>,
    pub maximum_inter_link_penetration_m: f64,
    pub maximum_floor_penetration_m: f64,
    pub floor_clearances: Vec<FloorClearance>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ContactPlanReport {
    pub sampling: ContactSampling,
    pub speed_m_s: f64,
    pub residuals: Vec<f64>,
    pub sampled_feasible: bool,
    pub maximum_force_error_n: f64,
    pub maximum_moment_error_nm: f64,
    pub minimum_torque_margin_nm: f64,
    pub maximum_penetration_m: f64,
    pub frames: Vec<ContactPlanFrame>,
}

impl ContactPlanReport {
    fn append_operating_case(&mut self, other: Self) {
        // Each single-clock evaluation has one leading speed residual. Extra
        // cases constrain this same motion, rather than asking reverse to move
        // toward the primary positive speed target.
        self.residuals.extend(other.residuals.into_iter().skip(1));
        self.sampled_feasible &= other.sampled_feasible;
        self.maximum_force_error_n = self.maximum_force_error_n.max(other.maximum_force_error_n);
        self.maximum_moment_error_nm = self
            .maximum_moment_error_nm
            .max(other.maximum_moment_error_nm);
        self.minimum_torque_margin_nm = self
            .minimum_torque_margin_nm
            .min(other.minimum_torque_margin_nm);
        self.maximum_penetration_m = self.maximum_penetration_m.max(other.maximum_penetration_m);
        self.frames.extend(other.frames);
    }
}
#[derive(Debug, Serialize)]
pub struct FeasibleContactPlan {
    pub motion: ContactPhaseConfig,
    pub report: ContactPlanReport,
}
#[derive(Debug, Serialize)]
pub struct ContactOptimization {
    pub motion: ContactPhaseConfig,
    pub search: LeastSquaresResult,
    pub report: ContactPlanReport,
    /// Fastest physically admissible sampled candidate seen during any valid
    /// evaluation, including derivative probes and rejected least-squares trials.
    /// Re-evaluated for this report; still needs independent dense/dynamic audits.
    pub best_sampled_feasible: Option<FeasibleContactPlan>,
    pub scope: &'static str,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AdaptiveContactConfig {
    pub maximum_rounds: usize,
    pub audit_uniform_samples: usize,
    pub maximum_added_phases_per_round: usize,
    pub minimum_phase_separation: f64,
}
#[derive(Debug, Serialize)]
pub struct AdaptiveContactRound {
    pub optimization_recipe: ContactPlanRecipe,
    pub optimization: ContactOptimization,
    pub selected_motion: ContactPhaseConfig,
    pub audit_recipe: ContactPlanRecipe,
    pub audit: ContactPlanReport,
    pub added_phases: Vec<f64>,
}
#[derive(Debug, Serialize)]
pub struct AdaptiveContactOptimization {
    pub rounds: Vec<AdaptiveContactRound>,
    pub dense_feasible_motion: Option<ContactPhaseConfig>,
    pub termination: String,
    pub scope: &'static str,
}
pub struct ContactPlanner<'a> {
    art: &'a Articulated,
    seed: Generalized,
    map: RigidEmbedding<'a>,
    points: Vec<EmbeddedPoint>,
    motors: Vec<EffectiveServo>,
    recipe: ContactPlanRecipe,
    coordinate_frame: String,
    joint_cache: std::cell::RefCell<joint::JointCache>,
}
impl<'a> ContactPlanner<'a> {
    /// Caller supplies a separate analysis model with contact forces disabled.
    /// Surface geometry remains active for read-only overlap checks. Planned
    /// support forces must not be counted again by the contact-force evaluator.
    pub fn new(
        art: &'a Articulated,
        seed: &Generalized,
        markers: &CaptureConfig,
        recipe: ContactPlanRecipe,
    ) -> Result<Self, String> {
        if art.contact_on
            || art.bases.len() != 1
            || art.bases[0].grounded
            || art.model.world.terrain.is_some()
            || recipe.expected_cad_sha256.is_empty()
            || art.model.source["cad_sha256"].as_str() != Some(recipe.expected_cad_sha256.as_str())
            || markers.expected_cad_sha256.as_deref() != Some(recipe.expected_cad_sha256.as_str())
            || markers.coordinate_frame.is_empty()
            || markers.markers.is_empty()
            || recipe.uniform_samples < 4
            || recipe.uniform_samples > MAX_CONTACT_SAMPLES
            || recipe.additional_phases.len() > 10000
            || !recipe.phase_rate.is_finite()
            || !recipe.phase_acceleration_per_s.is_finite()
            || recipe.additional_clocks.len() > 16
            || recipe
                .additional_clocks
                .iter()
                .any(|c| !c.phase_rate.is_finite() || !c.phase_acceleration_per_s.is_finite())
            || recipe
                .additional_phases
                .iter()
                .any(|p| !p.is_finite() || !(0.0..1.0).contains(p))
            || recipe
                .initial_base_translation_m
                .iter()
                .any(|v| !v.is_finite())
            || [
                recipe.moment_length_scale_m,
                recipe.force_tolerance_n,
                recipe.moment_tolerance_nm,
                recipe.torque_tolerance_nm,
                recipe.penetration_tolerance_m,
                recipe.velocity_tolerance_m_s,
                recipe.acceleration_tolerance_m_s2,
                recipe.target_speed_m_s,
                recipe.speed_residual_scale_m_s,
            ]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0)
            || recipe.direction_world.iter().any(|v| !v.is_finite())
            || (V::from(recipe.direction_world).norm() - 1.0).abs() > 1e-9
        {
            return Err("explicit matched CAD/markers, finite positive tolerances and one floating-base flat-world analysis model required".into());
        }
        if let Some(limits) = &recipe.servo_command_limits {
            limits.validate(recipe.independent_coordinates.len())?;
        }
        crate::tracking::validate_markers(&markers.markers)?;
        let map = RigidEmbedding::new(
            art,
            &recipe.independent_coordinates,
            recipe.embedding.clone(),
        )?;
        let mut seed = seed.clone();
        let base = art.bases[0].state;
        for i in 0..3 {
            seed.states[base + i] += recipe.initial_base_translation_m[i];
        }
        seed = map
            .solve(
                &seed,
                &recipe.initial_coordinates,
                &vec![0.0; map.reduced_dimension()],
            )?
            .generalized;
        let points = markers
            .markers
            .iter()
            .map(|m| {
                let found = art
                    .links
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| l.name == m.link)
                    .map(|(i, _)| i)
                    .collect::<Vec<_>>();
                if found.len() != 1 {
                    return Err(format!("ambiguous marker link {}", m.link));
                }
                Ok(EmbeddedPoint {
                    link: found[0],
                    local_point_m: m.local_point_m,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let motors = recipe
            .independent_coordinates
            .iter()
            .map(|n| {
                EffectiveServo::new(
                    recipe
                        .actuators
                        .get(n)
                        .ok_or_else(|| format!("missing actuator {n}"))?,
                )
                .map_err(|e| e.to_string())
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self {
            art,
            seed,
            map,
            points,
            motors,
            recipe,
            coordinate_frame: markers.coordinate_frame.clone(),
            joint_cache: Default::default(),
        })
    }
    pub fn evaluate(&self, config: &ContactPhaseConfig) -> Result<ContactPlanReport, String> {
        let mut report = self.evaluate_clock(
            config,
            ContactClock {
                phase_rate: self.recipe.phase_rate,
                phase_acceleration_per_s: self.recipe.phase_acceleration_per_s,
            },
        )?;
        for &clock in &self.recipe.additional_clocks {
            report.append_operating_case(self.evaluate_clock(config, clock)?);
        }
        Ok(report)
    }
    fn evaluate_clock(
        &self,
        config: &ContactPhaseConfig,
        clock: ContactClock,
    ) -> Result<ContactPlanReport, String> {
        self.evaluate_clock_with_forces(config, clock, None, &[], None)
    }
    fn evaluate_clock_with_forces(
        &self,
        config: &ContactPhaseConfig,
        clock: ContactClock,
        explicit: Option<&joint::PreparedForces>,
        extra_phases: &[f64],
        mut load_maps: Option<&mut Vec<(f64, PointForceLoadMap)>>,
    ) -> Result<ContactPlanReport, String> {
        let reference = ContactPhaseMotion::new(config.clone())?;
        if config.feet.len() != self.points.len()
            || config.displacement_world_m[2] != 0.0
            || config.feet.iter().flat_map(|f| f.steps()).any(|f| {
                (f.center_world_m[2] - self.art.floor_z).abs() > 1e-9
                    || f.swing_offset_world_m[2] < 0.0
            })
        {
            return Err("one foot sequence per marker, all footholds on declared flat floor and nonnegative swing height required".into());
        }
        let mut phases = (0..self.recipe.uniform_samples)
            .map(|i| (i as f64 + 0.5) / self.recipe.uniform_samples as f64)
            .collect::<Vec<_>>();
        phases.extend(reference.phase_midpoints());
        phases.extend_from_slice(&self.recipe.additional_phases);
        phases.extend_from_slice(extra_phases);
        if let Some(forces) = explicit {
            phases.extend_from_slice(&forces.knot_phases);
        }
        if self.recipe.include_body_knots {
            phases.extend(
                config
                    .body
                    .keyframes
                    .iter()
                    .take(config.body.keyframes.len() - 1)
                    .map(|k| k.time_s / config.period_s),
            );
        }
        let count = phases.len();
        let mut phases = phases
            .into_iter()
            .map(|p| (p, 1.0 / count as f64))
            .collect::<Vec<_>>();
        if matches!(self.recipe.sampling, ContactSampling::ContactIntervals) {
            for (_, weight) in &mut phases {
                *weight *= 0.5;
            }
            for interval in reference.contact_intervals() {
                let duration = interval.end_phase - interval.start_phase;
                for fraction in [1e-4, 0.5, 1.0 - 1e-4] {
                    phases.push((
                        (interval.start_phase + fraction * duration).rem_euclid(1.0),
                        duration / 6.0,
                    ));
                }
            }
        }
        // Fixed residual dimension even when events coincide. Duration weights
        // prevent a newly opened tiny interval from causing a finite cost jump;
        // the independent maximum-residual gates still inspect every interval.
        let speed = V::from(config.displacement_world_m).dot(&V::from(self.recipe.direction_world))
            / config.period_s
            * clock.phase_rate;
        let mut report = ContactPlanReport {
            sampling: self.recipe.sampling,
            speed_m_s: speed,
            residuals: vec![
                (self.recipe.target_speed_m_s - speed) / self.recipe.speed_residual_scale_m_s,
            ],
            sampled_feasible: true,
            maximum_force_error_n: 0.0,
            maximum_moment_error_nm: 0.0,
            minimum_torque_margin_nm: f64::INFINITY,
            maximum_penetration_m: 0.0,
            frames: vec![],
        };
        let base = self.art.bases[0].state;
        for (phase, weight) in phases {
            if weight == 0.0 && explicit.is_none() {
                report
                    .residuals
                    .extend(std::iter::repeat_n(0.0, 8 + self.motors.len()));
                continue;
            }
            let scale = weight.sqrt();
            let time = phase * config.period_s;
            let sample =
                reference.sample_retimed(time, clock.phase_rate, clock.phase_acceleration_per_s)?;
            let seed = self.contact_sample_seed(&sample.body);
            let phi = V::from_column_slice(&sample.body.values[3..6]);
            let (omega, alpha) = rotation_vector_motion(
                phi,
                V::from_column_slice(&sample.body.rates[3..6]),
                V::from_column_slice(&sample.body.accelerations[3..6]),
            )?;
            let velocity = sample.body.rates[..3]
                .iter()
                .chain(omega.iter())
                .copied()
                .collect::<Vec<_>>();
            let acceleration = sample.body.accelerations[..3]
                .iter()
                .chain(alpha.iter())
                .copied()
                .collect::<Vec<_>>();
            let targets = self
                .points
                .iter()
                .zip(&sample.feet)
                .map(|(p, f)| PointMotionTarget {
                    point: p.clone(),
                    position_world_m: f.position_world_m,
                    velocity_world_m_s: f.velocity_world_m_s,
                    acceleration_world_m_s2: f.acceleration_world_m_s2,
                })
                .collect::<Vec<_>>();
            let fit = self.map.follow_points(
                &seed,
                &targets,
                &velocity,
                &acceleration,
                &self.recipe.joint_search_bounds,
                &self.recipe.placement,
                self.recipe.velocity_tolerance_m_s,
                self.recipe.acceleration_tolerance_m_s2,
            ).map_err(|e| format!("contact phase {phase:.17}, time {time:.17} s, clock rate {} acceleration {}: {e}",clock.phase_rate,clock.phase_acceleration_per_s))?;
            let required = self
                .map
                .prepare_dynamics(&fit.motion)?
                .required_reduced_forces(&fit.reduced_acceleration)?;
            let (_, points) = self.map.point_jacobians(
                &fit.motion.generalized,
                &fit.coordinates,
                &self.points,
            )?;
            let weights = sample
                .feet
                .iter()
                .map(|f| if f.in_contact { 1.0 } else { 0.0 })
                .collect::<Vec<_>>();
            let positions = points
                .iter()
                .map(|(p, _)| (*p).into())
                .collect::<Vec<[f64; 3]>>();
            let center = std::array::from_fn(|i| seed.states[base + i]);
            let mut explicit_torques = None;
            let (forces, residual) = if let Some(explicit) = explicit {
                let forces = explicit.sample(config, phase)?;
                let map = PointForceLoadMap::new(
                    &positions,
                    center,
                    points.iter().map(|(_, j)| j.clone()).collect(),
                    required.as_slice().to_vec(),
                )?;
                let loads = map.evaluate(&forces)?;
                explicit_torques = Some(loads.motor_torques_nm);
                if let Some(maps) = &mut load_maps {
                    maps.push((phase, map));
                }
                (forces, loads.wrench_residual)
            } else if weights.iter().any(|v| *v > 0.0) {
                let a = constrained_point_forces(
                    &positions,
                    center,
                    std::array::from_fn(|i| required[i]),
                    self.recipe.moment_length_scale_m,
                    self.art.model.world.floor_friction,
                    &weights,
                    &self.recipe.force_allocation,
                )?;
                if !a.optimizer.as_ref().is_some_and(|o| o.converged)
                    || !a.unilateral_friction_satisfied
                {
                    return Err(
                        "support allocation did not converge within declared cone constraints"
                            .into(),
                    );
                }
                (a.forces_world_n, a.wrench_residual.to_vec())
            } else {
                (
                    vec![[0.0; 3]; points.len()],
                    (0..6).map(|i| -required[i]).collect(),
                )
            };
            for (i, r) in residual.iter().enumerate() {
                report.residuals.push(
                    scale * r
                        / if i < 3 {
                            self.recipe.force_tolerance_n
                        } else {
                            self.recipe.moment_tolerance_nm
                        },
                );
            }
            let force = residual[..3]
                .iter()
                .map(|v| v.abs())
                .fold(0.0_f64, f64::max);
            let moment = residual[3..]
                .iter()
                .map(|v| v.abs())
                .fold(0.0_f64, f64::max);
            report.maximum_force_error_n = report.maximum_force_error_n.max(force);
            report.maximum_moment_error_nm = report.maximum_moment_error_nm.max(moment);
            let torques = explicit_torques.unwrap_or_else(|| {
                (0..fit.coordinates.len())
                    .map(|j| {
                        required[6 + j]
                            - points
                                .iter()
                                .zip(&forces)
                                .map(|((_, jac), f)| {
                                    (0..3).map(|k| jac[(k, j)] * f[k]).sum::<f64>()
                                })
                                .sum::<f64>()
                    })
                    .collect::<Vec<_>>()
            });
            let margins = torques
                .iter()
                .enumerate()
                .map(|(j, t)| {
                    self.motors[j].torque_capacity(fit.reduced_velocity[6 + j], *t) - t.abs()
                })
                .collect::<Vec<_>>();
            for (j, &margin) in margins.iter().enumerate() {
                let violation = if self.recipe.extend_motoring_penalty_past_no_load {
                    self.motors[j]
                        .optimization_torque_violation(fit.reduced_velocity[6 + j], torques[j])
                } else {
                    (-margin).max(0.0)
                };
                report
                    .residuals
                    .push(scale * violation / self.recipe.torque_tolerance_nm);
                report.minimum_torque_margin_nm = report.minimum_torque_margin_nm.min(margin);
            }
            let servo_command = self.recipe.servo_command_limits.as_ref().map(|limits|
                limits.evaluate(&self.motors, &fit.coordinates, &fit.reduced_velocity[6..], &torques)
            ).transpose()?;
            if let Some(check) = &servo_command {
                report.residuals.extend(check.inequalities.iter().map(|c| scale * c.max(0.0)));
            }
            let poses = self
                .art
                .poses(&fit.motion.generalized)
                .iter()
                .zip(&self.art.links)
                .map(|((r, p), l)| LinkPose {
                    name: l.name.clone(),
                    position_m: (*p).into(),
                    rotation: std::array::from_fn(|i| std::array::from_fn(|j| r[(i, j)])),
                })
                .collect::<Vec<_>>();
            let overlap = sampled_inter_link_penetrations(self.art, &poses)?
                .iter()
                .map(|p| p.penetration_m)
                .fold(0.0_f64, f64::max);
            let floor_links = self
                .art
                .links
                .iter()
                .filter(|l| !l.contact.is_empty())
                .map(|l| l.name.clone())
                .collect::<Vec<_>>();
            let floor_clearances = sampled_floor_clearances(self.art, &poses, &floor_links)?;
            let floor = floor_clearances
                .iter()
                .map(|c| (-c.minimum_clearance_m).max(0.0))
                .fold(0.0_f64, f64::max);
            report.maximum_penetration_m = report.maximum_penetration_m.max(overlap).max(floor);
            report
                .residuals
                .push(scale * overlap / self.recipe.penetration_tolerance_m);
            report
                .residuals
                .push(scale * floor / self.recipe.penetration_tolerance_m);
            report.frames.push(ContactPlanFrame {
                time_s: time,
                clock,
                residual_weight: weight,
                planned_contacts: sample.feet.iter().map(|f| f.in_contact).collect(),
                coordinates: fit.coordinates,
                reduced_velocity: fit.reduced_velocity,
                reduced_acceleration: fit.reduced_acceleration,
                support_forces_world_n: forces,
                wrench_residual: residual,
                motor_torques_nm: torques,
                torque_capacity_margin_nm: margins,
                servo_command,
                maximum_inter_link_penetration_m: overlap,
                maximum_floor_penetration_m: floor,
                floor_clearances,
            });
        }
        report.sampled_feasible = report.maximum_force_error_n <= self.recipe.force_tolerance_n
            && report.maximum_moment_error_nm <= self.recipe.moment_tolerance_nm
            && report.minimum_torque_margin_nm >= -self.recipe.torque_tolerance_nm
            && report.maximum_penetration_m <= self.recipe.penetration_tolerance_m
            && report.frames.iter().all(|f| f.servo_command.as_ref().is_none_or(|c| c.maximum_violation_rad == 0.0));
        if report.residuals.iter().any(|v| !v.is_finite()) {
            return Err("nonfinite contact-plan residual".into());
        }
        Ok(report)
    }
    fn contact_sample_seed(
        &self,
        body: &sim_domain_control::trajectory::TrajectorySample,
    ) -> Generalized {
        let base = self.art.bases[0].state;
        let mut seed = self.seed.clone();
        for i in 0..3 {
            seed.states[base + i] += body.values[i];
        }
        let initial_rotation = sim_domain_robot::math::quat(
            seed.states[base + 3],
            seed.states[base + 4],
            seed.states[base + 5],
            seed.states[base + 6],
        );
        seed.states[base + 3..base + 7].copy_from_slice(&quat_parts(
            &(UnitQuaternion::from_scaled_axis(V::from_column_slice(&body.values[3..6]))
                * initial_rotation),
        ));
        seed
    }
    /// Refine the collocation mesh from independent dense constraint failures.
    /// Physical tolerances, search bounds and the dynamics model stay unchanged.
    /// This restores sampled feasibility; it neither certifies continuous-time
    /// feasibility nor establishes a maximum speed.
    pub fn optimize_adaptive(
        &mut self,
        motion: &ContactPhaseConfig,
        variables: &[ContactVariable],
        search: &LeastSquaresConfig,
        adaptive: &AdaptiveContactConfig,
        mut progress: impl FnMut(&ContactPlanReport),
    ) -> Result<AdaptiveContactOptimization, String> {
        if adaptive.maximum_rounds == 0
            || adaptive.maximum_rounds > 20
            || adaptive.audit_uniform_samples <= self.recipe.uniform_samples
            || adaptive.audit_uniform_samples > MAX_CONTACT_SAMPLES
            || adaptive.maximum_added_phases_per_round == 0
            || adaptive.maximum_added_phases_per_round > 64
            || !adaptive.minimum_phase_separation.is_finite()
            || !(0.0..0.1).contains(&adaptive.minimum_phase_separation)
            || adaptive.minimum_phase_separation == 0.0
        {
            return Err(
                "bounded refinement rounds, denser audit grid and positive phase spacing required"
                    .into(),
            );
        }
        let original_recipe = self.recipe.clone();
        let outcome = (|| {
            let mut current = motion.clone();
            let mut output = AdaptiveContactOptimization {
                rounds: vec![],
                dense_feasible_motion: None,
                termination: "round_limit".into(),
                scope: "Adaptive offline collocation: each solve is audited on a denser grid retaining all its fixed collocation phases. Violating phases seed the next solve without changing physical tolerances. Success means sampled feasibility only; detailed runtime tracking, continuous-time geometry and global speed optimality remain unproved.",
            };
            for _ in 0..adaptive.maximum_rounds {
                let optimization_recipe = self.recipe.clone();
                let optimization = self.optimize(&current, variables, search, &mut progress)?;
                let selected_motion = optimization
                    .best_sampled_feasible
                    .as_ref()
                    .map(|p| p.motion.clone())
                    .unwrap_or_else(|| optimization.motion.clone());
                // Retain all optimization grid locations as well as the denser
                // audit grid; changing grids must never discard an earlier check.
                self.recipe.additional_phases.extend(
                    (0..optimization_recipe.uniform_samples)
                        .map(|i| (i as f64 + 0.5) / optimization_recipe.uniform_samples as f64),
                );
                self.recipe.additional_phases.sort_by(f64::total_cmp);
                self.recipe.additional_phases.dedup();
                self.recipe.uniform_samples = adaptive.audit_uniform_samples;
                let audit_recipe = self.recipe.clone();
                let audit = self.evaluate(&selected_motion)?;
                self.recipe = optimization_recipe.clone();
                let mut occupied = self.recipe.additional_phases.clone();
                if self.recipe.include_body_knots {
                    occupied.extend(
                        selected_motion
                            .body
                            .keyframes
                            .iter()
                            .take(selected_motion.body.keyframes.len() - 1)
                            .map(|k| k.time_s / selected_motion.period_s),
                    );
                }
                let tolerances = [
                    self.recipe.force_tolerance_n,
                    self.recipe.moment_tolerance_nm,
                    self.recipe.torque_tolerance_nm,
                    self.recipe.penetration_tolerance_m,
                ];
                let added_phases = choose_refinement_phases(
                    &audit,
                    selected_motion.period_s,
                    tolerances,
                    &occupied,
                    adaptive,
                );
                let passed = audit.sampled_feasible;
                let no_new_samples = added_phases.is_empty();
                self.recipe
                    .additional_phases
                    .extend_from_slice(&added_phases);
                if self.recipe.additional_phases.len() > 10000 {
                    return Err("adaptive phase budget exceeded".into());
                }
                current = selected_motion.clone();
                output.rounds.push(AdaptiveContactRound {
                    optimization_recipe,
                    optimization,
                    selected_motion,
                    audit_recipe,
                    audit,
                    added_phases,
                });
                if passed {
                    output.dense_feasible_motion = Some(current);
                    output.termination = "dense_sampled_feasible".into();
                    break;
                }
                if no_new_samples {
                    output.termination = "no_new_violating_phases".into();
                    break;
                }
            }
            Ok(output)
        })();
        self.recipe = original_recipe;
        outcome
    }
    pub fn optimize(
        &self,
        motion: &ContactPhaseConfig,
        variables: &[ContactVariable],
        search: &LeastSquaresConfig,
        mut progress: impl FnMut(&ContactPlanReport),
    ) -> Result<ContactOptimization, String> {
        let mut scratch = motion.clone();
        let mut initial = Vec::new();
        let mut unique = std::collections::BTreeSet::new();
        if variables
            .iter()
            .any(|v| matches!(v.decision, ContactDecision::DisplacementAlongDirection))
            && variables
                .iter()
                .any(|v| matches!(v.decision, ContactDecision::Displacement { .. }))
        {
            return Err("directional and Cartesian displacement decisions overlap".into());
        }
        for v in variables {
            let key = serde_json::to_string(&v.decision).map_err(|e| e.to_string())?;
            if !unique.insert(key) {
                return Err("duplicate contact decision".into());
            }
            initial.push(
                if matches!(v.decision, ContactDecision::DisplacementAlongDirection) {
                    directional_distance(scratch.displacement_world_m, self.recipe.direction_world)?
                } else {
                    *decision(&mut scratch, &v.decision)?
                },
            );
        }
        let decode = |values: &[f64]| -> Result<ContactPhaseConfig, String> {
            let mut m = motion.clone();
            for (value, v) in values.iter().zip(variables) {
                if matches!(v.decision, ContactDecision::DisplacementAlongDirection) {
                    m.displacement_world_m = self.recipe.direction_world.map(|d| d * value);
                } else {
                    *decision(&mut m, &v.decision)? = *value;
                }
            }
            let last = m.body.keyframes.len() - 1;
            for (i, k) in m.body.keyframes.iter_mut().enumerate() {
                k.time_s = m.period_s * i as f64 / last as f64;
            }
            m.body.keyframes[last].values = m.body.keyframes[0].values.clone();
            Ok(m)
        };
        let mut best: Option<(f64, ContactPhaseConfig)> = None;
        let result = bounded_least_squares(
            &initial,
            &variables
                .iter()
                .map(|v| v.bound.clone())
                .collect::<Vec<_>>(),
            search,
            |x| {
                let candidate = decode(x)?;
                let report = self.evaluate(&candidate)?;
                if report.sampled_feasible
                    && best
                        .as_ref()
                        .is_none_or(|(speed, _)| report.speed_m_s > *speed)
                {
                    best = Some((report.speed_m_s, candidate));
                }
                progress(&report);
                Ok(report.residuals)
            },
        )?;
        let motion = decode(&result.values)?;
        let report = self.evaluate(&motion)?;
        let best_sampled_feasible = best
            .map(|(_, motion)| {
                self.evaluate(&motion)
                    .map(|report| FeasibleContactPlan { motion, report })
            })
            .transpose()?;
        Ok(ContactOptimization {
            motion,
            search: result,
            report,
            best_sampled_feasible,
            scope: "Offline weighted least-squares search over independent foot phases and 6D body references, using exact local CAD kinematics, closure curvature and whole-body inverse dynamics. Support allocation enforces unilateral/friction cones; motion feasibility is checked separately against declared residual tolerances at sampled phases. A stationary or improved least-squares result is not a feasibility, global optimality, collision-between-samples, realtime or live-controller certificate. Search intervals and actuator calibration remain explicit experimental assumptions.",
        })
    }
}
fn directional_distance(displacement: [f64; 3], direction: [f64; 3]) -> Result<f64, String> {
    let displacement = V::from(displacement);
    let direction = V::from(direction);
    let distance = displacement.dot(&direction);
    if (displacement - direction * distance).norm() > 1e-10 {
        return Err("initial displacement must align with the requested travel direction".into());
    }
    Ok(distance)
}
fn choose_refinement_phases(
    report: &ContactPlanReport,
    period: f64,
    tolerances: [f64; 4],
    occupied: &[f64],
    config: &AdaptiveContactConfig,
) -> Vec<f64> {
    rank_refinement_phases(
        report,
        period,
        [
            tolerances[0],
            tolerances[1],
            tolerances[2],
            tolerances[3],
            tolerances[3],
        ],
        occupied,
        config.maximum_added_phases_per_round,
        config.minimum_phase_separation,
    )
}
/// Select worst missed physical phases for adaptive collocation. Tolerances
/// are force, moment, torque deficit, floor penetration and inter-link overlap.
/// A zero overlap tolerance ranks any positive overlap as a strict violation.
/// This selects constraints; it neither modifies motion nor certifies feasibility.
pub fn select_contact_refinement_phases(
    report: &ContactPlanReport,
    period: f64,
    tolerances: [f64; 5],
    occupied: &[f64],
    maximum_added: usize,
    minimum_separation: f64,
) -> Result<Vec<f64>, String> {
    if !period.is_finite()
        || period <= 0.
        || maximum_added == 0
        || maximum_added > 64
        || !minimum_separation.is_finite()
        || minimum_separation <= 0.
        || minimum_separation >= 0.1
        || tolerances[..4].iter().any(|v| !v.is_finite() || *v <= 0.)
        || !tolerances[4].is_finite()
        || tolerances[4] < 0.
        || occupied
            .iter()
            .any(|p| !p.is_finite() || !(0.0..=1.0).contains(p))
        || report.frames.iter().any(|f| {
            !f.time_s.is_finite()
                || f.wrench_residual.len() != 6
                || f.wrench_residual
                    .iter()
                    .chain(&f.torque_capacity_margin_nm)
                    .any(|v| !v.is_finite())
                || !f.maximum_floor_penetration_m.is_finite()
                || f.maximum_floor_penetration_m < 0.
                || !f.maximum_inter_link_penetration_m.is_finite()
                || f.maximum_inter_link_penetration_m < 0.
        })
    {
        return Err("finite physical frames, positive period/tolerances and bounded refinement controls required".into());
    }
    Ok(rank_refinement_phases(
        report,
        period,
        tolerances,
        occupied,
        maximum_added,
        minimum_separation,
    ))
}
fn rank_refinement_phases(
    report: &ContactPlanReport,
    period: f64,
    tolerances: [f64; 5],
    occupied: &[f64],
    maximum_added: usize,
    minimum_separation: f64,
) -> Vec<f64> {
    let mut violations = report
        .frames
        .iter()
        .map(|f| {
            let ratio = f.wrench_residual[..3]
                .iter()
                .map(|v| v.abs() / tolerances[0])
                .chain(
                    f.wrench_residual[3..]
                        .iter()
                        .map(|v| v.abs() / tolerances[1]),
                )
                .chain(
                    f.torque_capacity_margin_nm
                        .iter()
                        .map(|v| -v / tolerances[2]),
                )
                .chain([
                    f.maximum_floor_penetration_m / tolerances[3],
                    if tolerances[4] == 0. {
                        if f.maximum_inter_link_penetration_m > 0. {
                            f64::INFINITY
                        } else {
                            0.
                        }
                    } else {
                        f.maximum_inter_link_penetration_m / tolerances[4]
                    },
                ])
                .fold(0.0_f64, f64::max);
            ((f.time_s / period).rem_euclid(1.0), ratio)
        })
        .filter(|(_, ratio)| *ratio > 1.0)
        .collect::<Vec<_>>();
    violations.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.total_cmp(&b.0)));
    let mut selected = Vec::new();
    for (phase, _) in violations {
        if occupied.iter().chain(&selected).all(|p| {
            let distance = (phase - p).abs();
            distance.min(1.0 - distance) >= minimum_separation
        }) {
            selected.push(phase);
            if selected.len() == maximum_added {
                break;
            }
        }
    }
    selected
}

fn decision<'a>(m: &'a mut ContactPhaseConfig, d: &ContactDecision) -> Result<&'a mut f64, String> {
    let value = match *d {
        ContactDecision::Period => Some(&mut m.period_s),
        ContactDecision::Displacement { axis } => m.displacement_world_m.get_mut(axis),
        ContactDecision::DisplacementAlongDirection => None,
        ContactDecision::FootPhase { foot } => m.feet.get_mut(foot).map(|f| &mut f.phase_offset),
        ContactDecision::FootStance { foot } => {
            m.feet.get_mut(foot).map(|f| &mut f.stance_fraction)
        }
        ContactDecision::FootCenter { foot, axis } => m
            .feet
            .get_mut(foot)
            .and_then(|f| f.center_world_m.get_mut(axis)),
        ContactDecision::FootSwing { foot, axis } => m
            .feet
            .get_mut(foot)
            .and_then(|f| f.swing_offset_world_m.get_mut(axis)),
        ContactDecision::AdditionalStepPhase { foot, step } => m.feet.get_mut(foot)
            .and_then(|f| f.additional_steps.get_mut(step)).map(|s| &mut s.phase_offset),
        ContactDecision::AdditionalStepStance { foot, step } => m.feet.get_mut(foot)
            .and_then(|f| f.additional_steps.get_mut(step)).map(|s| &mut s.stance_fraction),
        ContactDecision::AdditionalStepCenter { foot, step, axis } => m.feet.get_mut(foot)
            .and_then(|f| f.additional_steps.get_mut(step)).and_then(|s| s.center_world_m.get_mut(axis)),
        ContactDecision::AdditionalStepSwing { foot, step, axis } => m.feet.get_mut(foot)
            .and_then(|f| f.additional_steps.get_mut(step)).and_then(|s| s.swing_offset_world_m.get_mut(axis)),
        ContactDecision::BodyControl { control, channel } => {
            let last = m.body.keyframes.len().saturating_sub(1);
            if control >= last {
                None
            } else {
                m.body
                    .keyframes
                    .get_mut(control)
                    .and_then(|k| k.values.get_mut(channel))
            }
        }
    };
    value.ok_or_else(|| {
        "contact decision index out of range (repeated final body control is not independent)"
            .into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagonal_distance_preserves_speed_and_rejects_silent_heading_changes() {
        let h = std::f64::consts::FRAC_1_SQRT_2;
        assert!(
            (directional_distance([0.08 * h, 0.08 * h, 0.0], [h, h, 0.0]).unwrap() - 0.08).abs()
                < 1e-14
        );
        assert!(
            (directional_distance([-0.08 * h, 0.08 * h, 0.0], [h, -h, 0.0]).unwrap() + 0.08).abs()
                < 1e-14
        );
        assert!(directional_distance([0.08, 0.0, 0.0], [h, h, 0.0]).is_err());
    }

    #[test]
    fn operating_cases_keep_one_speed_objective_and_every_physical_gate() {
        let report =
            |speed, residuals, feasible, force, moment, margin, penetration| ContactPlanReport {
                sampling: ContactSampling::ContactIntervals,
                speed_m_s: speed,
                residuals,
                sampled_feasible: feasible,
                maximum_force_error_n: force,
                maximum_moment_error_nm: moment,
                minimum_torque_margin_nm: margin,
                maximum_penetration_m: penetration,
                frames: vec![],
            };
        let mut forward = report(0.2, vec![0.1, 0.2, 0.3], true, 0.01, 0.02, 0.1, 0.0);
        let reverse = report(-0.2, vec![900.0, 0.4, 0.5], false, 0.03, 0.01, -0.2, 0.001);
        forward.append_operating_case(reverse);
        assert_eq!(forward.speed_m_s, 0.2);
        assert_eq!(forward.residuals, vec![0.1, 0.2, 0.3, 0.4, 0.5]);
        assert!(!forward.sampled_feasible);
        assert_eq!(forward.maximum_force_error_n, 0.03);
        assert_eq!(forward.maximum_moment_error_nm, 0.02);
        assert_eq!(forward.minimum_torque_margin_nm, -0.2);
        assert_eq!(forward.maximum_penetration_m, 0.001);
    }
    #[test]
    fn refinement_ranks_physical_failures_and_respects_circular_spacing() {
        let frame = |phase| ContactPlanFrame {
            time_s: phase * 2.0,
            clock: ContactClock {
                phase_rate: 1.0,
                phase_acceleration_per_s: 0.0,
            },
            residual_weight: 1.0,
            planned_contacts: vec![true],
            coordinates: vec![],
            reduced_velocity: vec![],
            reduced_acceleration: vec![],
            support_forces_world_n: vec![],
            wrench_residual: vec![0.0; 6],
            motor_torques_nm: vec![0.0],
            torque_capacity_margin_nm: vec![1.0],
            servo_command: None,
            maximum_inter_link_penetration_m: 0.0,
            maximum_floor_penetration_m: 0.0,
            floor_clearances: vec![],
        };
        let mut frames = [0.999, 0.001, 0.25, 0.5, 0.75, 0.33].map(frame);
        frames[0].wrench_residual[0] = -2.0;
        frames[1].wrench_residual[2] = 1.5;
        frames[2].wrench_residual[4] = -3.0;
        frames[3].torque_capacity_margin_nm[0] = -4.0;
        frames[4].maximum_floor_penetration_m = 5.0;
        frames[5].maximum_inter_link_penetration_m = 6.0;
        let report = ContactPlanReport {
            sampling: ContactSampling::ContactIntervals,
            speed_m_s: 0.0,
            residuals: vec![],
            sampled_feasible: false,
            maximum_force_error_n: 2.0,
            maximum_moment_error_nm: 3.0,
            minimum_torque_margin_nm: -4.0,
            maximum_penetration_m: 6.0,
            frames: frames.into(),
        };
        let mut config = AdaptiveContactConfig {
            maximum_rounds: 3,
            audit_uniform_samples: 1000,
            maximum_added_phases_per_round: 3,
            minimum_phase_separation: 0.01,
        };
        assert_eq!(
            choose_refinement_phases(&report, 2.0, [1.0; 4], &[0.33], &config),
            vec![0.75, 0.5, 0.25]
        );
        let strict = select_contact_refinement_phases(&report,2.,[1.,1.,1.,1.,0.],&[],3,0.01).unwrap();
        assert_eq!(strict,vec![0.33,0.75,0.5]);
        let mut bad=report.clone();bad.frames[0].wrench_residual[0]=f64::NAN;
        assert!(select_contact_refinement_phases(&bad,2.,[1.;5],&[],3,0.01).is_err());
        assert!(select_contact_refinement_phases(&report,2.,[1.;5],&[],0,0.01).is_err());
        config.maximum_added_phases_per_round = 10;
        assert_eq!(
            choose_refinement_phases(&report, 2.0, [1.0; 4], &[0.33], &config),
            vec![0.75, 0.5, 0.25, 0.999]
        );
    }
}

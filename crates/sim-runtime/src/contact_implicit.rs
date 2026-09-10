//! Offline position-only contact-implicit optimization through shared CAD dynamics.
//! There are no stance flags, swing curves, foot phases or allocated support forces.
//! Smooth point-plane contacts are explicit planning approximations, not runtime physics.
use crate::tracking::{CaptureConfig, validate_markers};
use nalgebra::{UnitQuaternion, Vector3 as V};
use serde::{Deserialize, Serialize};
use sim_domain_control::{
    contact_slip::{ContactSlip, ContactSlipConfig},
    periodic_drift::{PeriodicDriftConfig, PeriodicDriftTrajectory},
    trajectory::{Interpolation, Keyframe, Trajectory, TrajectoryConfig, TrajectorySample},
};
use sim_domain_multibody::smooth_contact::{SmoothContact, SmoothContactConfig};
use sim_domain_robot::{
    Articulated, Generalized,
    articulated::embedding::{EmbeddedPoint, EmbeddingConfig, RigidEmbedding},
    effective_servo::EffectiveServo,
    math::{quat, quat_parts, rotation_vector_motion},
};
use sim_solve::equality_dogleg::{
    EqualityDoglegConfig, EqualityDoglegResult, EqualityResiduals, bounded_equality_dogleg,
};
use sim_solve::inequality_augmented_lagrangian::{
    AugmentedLagrangianConfig, AugmentedLagrangianResult, AugmentedWarmStart, InequalityResiduals,
    bounded_inequality_augmented_lagrangian_warm_started,
};
use sim_solve::least_squares::{
    DerivativeRefinement, LeastSquaresConfig, LeastSquaresResult, VariableBound,
    bounded_least_squares_scaled_refining,
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContactImplicitConfig {
    pub expected_cad_sha256: String,
    pub independent_coordinates: Vec<String>,
    pub embedding: EmbeddingConfig,
    pub actuators: BTreeMap<String, BTreeMap<String, f64>>,
    pub contact: SmoothContactConfig,
    pub step_s: f64,
    /// World linear/angular velocity followed by independent joint rates.
    pub initial_velocity: Vec<f64>,
    /// A translating periodic orbit on the flat world: the final pose equals
    /// the first except for horizontal displacement, and initial velocity is
    /// the final backward-difference velocity. Require initial_velocity empty
    /// to avoid silently ignoring an authored initial condition. The first pose
    /// is optimized too; bounds must explicitly fix any desired spatial gauge.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub periodic_horizontal_translation: bool,
    /// Optional C2 control representation for periodic orbits. Input positions
    /// become uniformly timed cubic B-spline controls with horizontal drift.
    /// Check this many analytic-motion collocation samples per control interval.
    /// This numerical basis/resolution is not a physical actuator bandwidth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub periodic_cubic_subdivisions: Option<usize>,
    /// Optional sorted phases in (0,1], ending exactly at 1, replacing the
    /// uniform cubic collocation grid. Each endpoint has its preceding interval
    /// as quadrature weight. Only analytic periodic cubic motion supports this.
    /// Fixed during a solve; no contact mode or physical property is prescribed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub periodic_collocation_phases: Option<Vec<f64>>,
    /// Positions: absolute base COM XYZ, relative rotation vector from CAD seed,
    /// independent joint coordinates. Startup mode fixes the first knot.
    pub position_reference: Vec<Vec<f64>>,
    pub position_scales: Vec<f64>,
    pub velocity_reference: Vec<f64>,
    pub velocity_scales: Vec<f64>,
    pub force_tolerance_n: f64,
    pub moment_tolerance_nm: f64,
    pub torque_tolerance_nm: f64,
    pub torque_effort_scale_nm: f64,
    pub maximum_point_penetration_m: f64,
    pub penetration_scale_m: f64,
    /// Optional energy cost: dissipated tangential contact work divided by this
    /// explicit scale. Unloaded foot motion is unpenalized; no stance flags.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_sliding_work_scale_j: Option<f64>,
}

/// Explicit policy grouping and shaping parameters; not authored contact physics.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContactSlipObjective {
    pub point_groups: Vec<String>,
    pub load_threshold_n: f64,
    pub target_ratio: f64,
    pub ratio_scale: f64,
    /// In the inequality optimizer, also constrain actual loaded slip,
    /// independently of the RMS objective. Weighted restoration rejects this
    /// option. Finite sampled constraints do not replace dense audits.
    #[serde(default, skip_serializing_if = "is_false")]
    pub constrain_loaded_slip: bool,
    /// Optional strict interior barrier on sampled actual slip, or its mean
    /// upper bound when `use_continuous_mean_bound` is enabled. Accepted
    /// inequality-optimizer states remain below the sampled target, provided
    /// the initial state is strictly inside it. This does not guard dense slip.
    /// Weight is numerical, dimensionless, finite and positive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loaded_slip_barrier_weight: Option<f64>,
    /// Use the continuous mean upper bound for shaping and, when enabled,
    /// the strict slip barrier. Actual-slip reporting/inequalities stay actual.
    /// The default preserves the original RMS shaping and actual-slip barrier.
    #[serde(default, skip_serializing_if = "is_false")]
    pub use_continuous_mean_bound: bool,
}
fn is_false(value: &bool) -> bool {
    !value
}
#[derive(Clone, Debug, Serialize)]
pub struct ContactSlipGroupReport {
    pub group: String,
    pub sampled_loaded_slip_ratio: f64,
    pub rms_slip_upper_bound: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuous_mean_slip_upper_bound: Option<f64>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ContactSlipReport {
    pub groups: Vec<ContactSlipGroupReport>,
    pub shaping_residuals: Vec<f64>,
    pub within_sampled_slip_limit: bool,
    pub body_path_m: f64,
    pub displacement_m: f64,
    pub duration_s: f64,
    pub scope: &'static str,
}
#[derive(Clone, Debug, Serialize)]
pub struct ContactImplicitFrame {
    pub time_s: f64,
    pub position: Vec<f64>,
    pub velocity: Vec<f64>,
    pub acceleration: Vec<f64>,
    pub gaps_m: Vec<f64>,
    pub contact_velocities_world_m_s: Vec<[f64; 3]>,
    pub contact_forces_world_n: Vec<[f64; 3]>,
    pub unactuated_wrench: Vec<f64>,
    pub motor_torques_nm: Vec<f64>,
    pub minimum_torque_margin_nm: Option<f64>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ContactImplicitPeriodicBoundary {
    pub translation_m: [f64; 3],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_position: Option<Vec<f64>>,
    pub initial_velocity: Vec<f64>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ContactImplicitReport {
    pub residuals: Vec<f64>,
    pub frames: Vec<ContactImplicitFrame>,
    pub maximum_force_error_n: f64,
    pub maximum_moment_error_nm: f64,
    pub minimum_torque_margin_nm: Option<f64>,
    pub maximum_point_penetration_m: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_sliding_work_j: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub periodic_boundary: Option<ContactImplicitPeriodicBoundary>,
    pub within_planning_tolerances: bool,
    pub scope: &'static str,
}
#[derive(Debug, Serialize)]
pub struct ContactImplicitOptimization {
    pub positions: Vec<Vec<f64>>,
    pub search: LeastSquaresResult,
    pub report: ContactImplicitReport,
}
#[derive(Debug, Serialize)]
pub struct ContactImplicitEqualityOptimization {
    pub positions: Vec<Vec<f64>>,
    pub search: EqualityDoglegResult,
    pub report: ContactImplicitReport,
}
/// One parameter mapping for optimization and numerical derivative audits.
pub struct ContactImplicitParameters {
    pub values: Vec<f64>,
    pub bounds: Vec<VariableBound>,
    dimension: usize,
    intervals: usize,
    fixed_initial: Option<Vec<f64>>,
}
impl ContactImplicitParameters {
    pub fn decode(&self, values: &[f64]) -> Result<Vec<Vec<f64>>, String> {
        if values.len() != self.values.len() || values.iter().any(|v| !v.is_finite()) {
            return Err("finite matched contact-planning parameters required".into());
        }
        if let Some(first) = &self.fixed_initial {
            Ok(std::iter::once(first.clone())
                .chain(values.chunks_exact(self.dimension).map(|q| q.to_vec()))
                .collect())
        } else {
            let count = self.intervals * self.dimension;
            let mut q = values[..count]
                .chunks_exact(self.dimension)
                .map(|q| q.to_vec())
                .collect::<Vec<_>>();
            let mut last = q[0].clone();
            last[0] += values[count];
            last[1] += values[count + 1];
            q.push(last);
            Ok(q)
        }
    }
}
#[derive(Debug, Serialize)]
pub struct ContactImplicitGeometryFrame {
    pub time_s: f64,
    pub maximum_inter_link_penetration_m: f64,
    pub floor_clearances: Vec<crate::contact_audit::FloorClearance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inter_link_penetrations: Option<Vec<sim_domain_robot::articulated::InterLinkPenetration>>,
    /// Authoritative embedded link poses used by this geometry audit, for
    /// read-only CAD inspection. These are planned poses, not a runtime rollout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poses: Option<Vec<crate::session::LinkPose>>,
}
#[derive(Debug, Serialize)]
pub struct ContactImplicitInequalityOptimization {
    pub positions: Vec<Vec<f64>>,
    pub search: AugmentedLagrangianResult,
    pub report: ContactImplicitReport,
}
pub struct ContactImplicitPlanner<'a> {
    art: &'a Articulated,
    seed: Generalized,
    map: RigidEmbedding<'a>,
    points: Vec<EmbeddedPoint>,
    motors: Vec<EffectiveServo>,
    contact: SmoothContact,
    config: ContactImplicitConfig,
    collocation: Vec<(f64, f64)>,
}
/// Cache lifetime is tied to one immutable planner, so contact continuation stages
/// and references cannot accidentally share entries. One entry per time step.
pub struct ContactImplicitEvaluator<'p, 'a> {
    planner: &'p ContactImplicitPlanner<'a>,
    entries: Vec<Option<CachedFrame>>,
    pub computed_frames: usize,
    pub reused_frames: usize,
}
struct CachedFrame {
    key: Vec<u64>,
    report: ContactImplicitReport,
    // Retain per-point additions to preserve the uncached summation order.
    work_terms: Vec<f64>,
}
impl ContactImplicitEvaluator<'_, '_> {
    pub fn evaluate(&mut self, positions: &[Vec<f64>]) -> Result<ContactImplicitReport, String> {
        self.planner.evaluate_internal(positions, Some(self))
    }
}
impl<'a> ContactImplicitPlanner<'a> {
    pub fn new(
        art: &'a Articulated,
        seed: &Generalized,
        markers: &CaptureConfig,
        config: ContactImplicitConfig,
    ) -> Result<Self, String> {
        validate_markers(&markers.markers)?;
        let n = 6 + config.independent_coordinates.len();
        if config.periodic_cubic_subdivisions.is_some_and(|s| {
            !config.periodic_horizontal_translation
                || !(1..=32).contains(&s)
                || config.position_reference.len() < 5
        }) || art.contact_on
            || art.bases.len() != 1
            || art.bases[0].grounded
            || art.terrain.is_some()
            || config.expected_cad_sha256.is_empty()
            || art.model.source["cad_sha256"].as_str() != Some(config.expected_cad_sha256.as_str())
            || markers.expected_cad_sha256.as_deref() != Some(config.expected_cad_sha256.as_str())
            || markers.coordinate_frame.is_empty()
            || config.position_reference.len() < 3
            || config.position_reference.len() > 1000
            || config
                .position_reference
                .iter()
                .any(|q| q.len() != n || q.iter().any(|v| !v.is_finite()))
            || if config.periodic_horizontal_translation {
                !config.initial_velocity.is_empty()
            } else {
                config.initial_velocity.len() != n
                    || config.initial_velocity.iter().any(|v| !v.is_finite())
            }
            || [
                &config.position_scales,
                &config.velocity_reference,
                &config.velocity_scales,
            ]
            .iter()
            .any(|v| v.len() != n || v.iter().any(|x| !x.is_finite()))
            || config
                .position_scales
                .iter()
                .chain(&config.velocity_scales)
                .any(|v| *v <= 0.)
            || [
                config.step_s,
                config.force_tolerance_n,
                config.moment_tolerance_nm,
                config.torque_tolerance_nm,
                config.torque_effort_scale_nm,
                config.penetration_scale_m,
            ]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.)
            || !config.maximum_point_penetration_m.is_finite()
            || config.maximum_point_penetration_m < 0.
            || config
                .contact_sliding_work_scale_j
                .is_some_and(|v| !v.is_finite() || v <= 0.)
        {
            return Err("explicit CAD-matched floating rigid model with runtime contact disabled, flat floor, finite matched position/rate data and positive scales required".into());
        }
        let intervals = config.position_reference.len() - 1;
        let period = intervals as f64 * config.step_s;
        let subdivisions = config.periodic_cubic_subdivisions.unwrap_or(1);
        let collocation = if let Some(phases) = &config.periodic_collocation_phases {
            if config.periodic_cubic_subdivisions.is_none()
                || phases.len() < 2
                || phases.len() > 8192
                || phases.last() != Some(&1.)
                || phases.iter().any(|p| !p.is_finite() || *p <= 0. || *p > 1.)
                || phases.windows(2).any(|p| p[1] <= p[0])
            {
                return Err("analytic periodic cubic collocation requires 2..8192 strictly increasing phases in (0,1], ending at 1".into());
            }
            let mut previous = 0.;
            phases
                .iter()
                .map(|phase| {
                    let time = phase * period;
                    let weight = time - previous;
                    previous = time;
                    (time, weight)
                })
                .collect::<Vec<_>>()
        } else {
            let dt = config.step_s / subdivisions as f64;
            let count = intervals * subdivisions;
            (1..=count)
                .map(|k| {
                    (
                        if k == count && config.periodic_cubic_subdivisions.is_some() {
                            period
                        } else {
                            k as f64 * dt
                        },
                        dt,
                    )
                })
                .collect()
        };
        if collocation
            .iter()
            .any(|(t, w)| !t.is_finite() || !w.is_finite() || *w <= 0.)
        {
            return Err("finite collocation times and positive quadrature weights required".into());
        }
        let map = RigidEmbedding::new(
            art,
            &config.independent_coordinates,
            config.embedding.clone(),
        )?;
        if map.reduced_dimension() != n {
            return Err("unexpected reduced coordinate layout".into());
        }
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
                    return Err(format!("unique marker link required: {}", m.link));
                }
                Ok(EmbeddedPoint {
                    link: found[0],
                    local_point_m: m.local_point_m,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let motors = config
            .independent_coordinates
            .iter()
            .map(|name| {
                EffectiveServo::new(
                    config
                        .actuators
                        .get(name)
                        .ok_or_else(|| format!("missing explicit actuator {name}"))?,
                )
                .map_err(|e| e.to_string())
            })
            .collect::<Result<Vec<_>, String>>()?;
        let contact = SmoothContact::new(config.contact.clone())?;
        Ok(Self {
            art,
            seed: seed.clone(),
            map,
            points,
            motors,
            contact,
            config,
            collocation,
        })
    }
    fn configuration_state(&self, q: &[f64]) -> Result<Generalized, String> {
        if q.len() != self.map.reduced_dimension() || q.iter().any(|v| !v.is_finite()) {
            return Err("finite reduced configuration required".into());
        }
        let base = self.art.bases[0].state;
        let initial_rotation = quat(
            self.seed.states[base + 3],
            self.seed.states[base + 4],
            self.seed.states[base + 5],
            self.seed.states[base + 6],
        );
        let mut state = self.seed.clone();
        state.states[base..base + 3].copy_from_slice(&q[..3]);
        state.states[base + 3..base + 7].copy_from_slice(&quat_parts(
            &(UnitQuaternion::from_scaled_axis(V::from_column_slice(&q[3..6])) * initial_rotation),
        ));
        Ok(state)
    }
    /// Double the periodic controls while preserving the analytic motion.
    /// Caller must halve step_s and explicitly update bounds and references.
    pub fn refine_periodic_controls(&self, controls: &[Vec<f64>]) -> Result<Vec<Vec<f64>>, String> {
        let curve = self
            .smooth_curve(controls)?
            .ok_or("analytic periodic cubic mode required")?;
        let refined = curve.refined_config()?;
        let period = curve.period_s();
        let mut positions = refined
            .periodic
            .keyframes
            .iter()
            .map(|k| {
                k.values
                    .iter()
                    .enumerate()
                    .map(|(j, v)| v + refined.displacement_per_cycle[j] * k.time_s / period)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let last = positions.len() - 1;
        positions[last] = positions[0]
            .iter()
            .enumerate()
            .map(|(j, v)| v + refined.displacement_per_cycle[j])
            .collect();
        Ok(positions)
    }
    fn smooth_curve(
        &self,
        controls: &[Vec<f64>],
    ) -> Result<Option<PeriodicDriftTrajectory>, String> {
        if self.config.periodic_cubic_subdivisions.is_none() {
            return Ok(None);
        }
        let n = self.map.reduced_dimension();
        let count = controls.len().saturating_sub(1);
        if count < 4
            || controls
                .iter()
                .any(|q| q.len() != n || q.iter().any(|v| !v.is_finite()))
            || controls[0][2..] != controls[count][2..]
        {
            return Err("finite periodic controls with exact pose closure required".into());
        }
        let mut displacement = vec![0.; n];
        for j in 0..2 {
            displacement[j] = controls[count][j] - controls[0][j];
        }
        let mut keyframes = controls[..count]
            .iter()
            .enumerate()
            .map(|(k, q)| Keyframe {
                time_s: k as f64 * self.config.step_s,
                values: q
                    .iter()
                    .enumerate()
                    .map(|(j, v)| v - displacement[j] * (k as f64 / count as f64))
                    .collect(),
            })
            .collect::<Vec<_>>();
        keyframes.push(Keyframe {
            time_s: count as f64 * self.config.step_s,
            values: keyframes[0].values.clone(),
        });
        Ok(Some(PeriodicDriftTrajectory::new(PeriodicDriftConfig {
            periodic: TrajectoryConfig {
                interpolation: Interpolation::PeriodicCubicBSpline,
                keyframes,
            },
            displacement_per_cycle: displacement,
        })?))
    }
    /// Sample full compiled CAD surfaces on the configured analytic cubic or
    /// linear curve. This check is independent of planning point contacts, but
    /// is not a continuous-time or integrated-motion proof.
    pub fn audit_geometry(
        &self,
        positions: &[Vec<f64>],
        subdivisions: usize,
    ) -> Result<Vec<ContactImplicitGeometryFrame>, String> {
        self.audit_geometry_impl(positions, subdivisions, false)
    }
    /// Include each shared CAD surface/SDF overlap, with link indices, world
    /// contact point, normal and the exact embedded poses used by the audit.
    /// The compact and detailed paths use one audit.
    pub fn audit_geometry_detailed(
        &self,
        positions: &[Vec<f64>],
        subdivisions: usize,
    ) -> Result<Vec<ContactImplicitGeometryFrame>, String> {
        self.audit_geometry_impl(positions, subdivisions, true)
    }
    fn audit_geometry_impl(
        &self,
        positions: &[Vec<f64>],
        subdivisions: usize,
        include_pairs: bool,
    ) -> Result<Vec<ContactImplicitGeometryFrame>, String> {
        if positions.len() < 2 || subdivisions == 0 || subdivisions > 100 {
            return Err("position path and 1..100 geometry subdivisions required".into());
        }
        let links = self
            .art
            .links
            .iter()
            .filter(|l| !l.contact.is_empty())
            .map(|l| l.name.clone())
            .collect::<Vec<_>>();
        let mut frames = vec![];
        let curve = self.smooth_curve(positions)?;
        for i in 0..=(positions.len() - 1) * subdivisions {
            let k = (i / subdivisions).min(positions.len() - 2);
            let t = (i - k * subdivisions) as f64 / subdivisions as f64;
            if positions[k].len() != self.map.reduced_dimension()
                || positions[k + 1].len() != self.map.reduced_dimension()
            {
                return Err("matched geometry configuration sizes required".into());
            }
            let q = if let Some(curve) = &curve {
                curve
                    .sample(i as f64 * self.config.step_s / subdivisions as f64)?
                    .values
            } else {
                positions[k]
                    .iter()
                    .zip(&positions[k + 1])
                    .map(|(a, b)| a + (b - a) * t)
                    .collect::<Vec<_>>()
            };
            let state = self.configuration_state(&q)?;
            let motion =
                self.map
                    .solve(&state, &q[6..], &vec![0.; self.map.reduced_dimension()])?;
            let poses = self
                .art
                .poses(&motion.generalized)
                .iter()
                .zip(&self.art.links)
                .map(|((r, p), l)| crate::session::LinkPose {
                    name: l.name.clone(),
                    position_m: (*p).into(),
                    rotation: std::array::from_fn(|i| std::array::from_fn(|j| r[(i, j)])),
                })
                .collect::<Vec<_>>();
            let pairs = crate::contact_audit::sampled_inter_link_penetrations(self.art, &poses)?;
            let overlap = pairs.iter().map(|p| p.penetration_m).fold(0., f64::max);
            let floor_clearances = if links.is_empty() {
                vec![]
            } else {
                crate::contact_audit::sampled_floor_clearances(self.art, &poses, &links)?
            };
            frames.push(ContactImplicitGeometryFrame {
                time_s: i as f64 * self.config.step_s / subdivisions as f64,
                maximum_inter_link_penetration_m: overlap,
                floor_clearances,
                inter_link_penetrations: include_pairs.then_some(pairs),
                poses: include_pairs.then_some(poses),
            });
        }
        Ok(frames)
    }
    pub fn evaluator(&self) -> ContactImplicitEvaluator<'_, 'a> {
        ContactImplicitEvaluator {
            planner: self,
            entries: (0..=self.collocation.len()).map(|_| None).collect(),
            computed_frames: 0,
            reused_frames: 0,
        }
    }
    /// Independent evaluation without reuse, also used for final optimization audit.
    pub fn evaluate(&self, positions: &[Vec<f64>]) -> Result<ContactImplicitReport, String> {
        self.evaluate_internal(positions, None)
    }
    fn evaluate_internal(
        &self,
        positions: &[Vec<f64>],
        mut cache: Option<&mut ContactImplicitEvaluator<'_, 'a>>,
    ) -> Result<ContactImplicitReport, String> {
        let c = &self.config;
        let n = self.map.reduced_dimension();
        let subdivisions = c.periodic_cubic_subdivisions.unwrap_or(1);
        let dt = c.step_s / subdivisions as f64;
        if positions.len() != c.position_reference.len()
            || (!c.periodic_horizontal_translation
                && positions.first() != c.position_reference.first())
            || (c.periodic_horizontal_translation
                && positions
                    .last()
                    .zip(positions.first())
                    .is_some_and(|(last, first)| last.get(2..) != first.get(2..)))
            || positions
                .iter()
                .any(|q| q.len() != n || q.iter().any(|v| !v.is_finite()))
        {
            return Err(
                "finite matched knots with fixed initial state, or exact periodic closure except horizontal translation, required"
                    .into(),
            );
        }
        let curve = self.smooth_curve(positions)?;
        let reference = if curve.is_some() {
            Some(Trajectory::new(TrajectoryConfig {
                interpolation: Interpolation::Linear,
                keyframes: c
                    .position_reference
                    .iter()
                    .enumerate()
                    .map(|(k, q)| Keyframe {
                        time_s: k as f64 * c.step_s,
                        values: q.clone(),
                    })
                    .collect(),
            })?)
        } else {
            None
        };
        let initial_sample = curve
            .as_ref()
            .map(|curve| curve.sample(0.))
            .transpose()?
            .map(spatial_sample)
            .transpose()?;
        let boundary_velocity = if let Some((_, v, _)) = &initial_sample {
            v.clone()
        } else if c.periodic_horizontal_translation {
            endpoint_velocity(
                positions.last().unwrap(),
                &positions[positions.len() - 2],
                dt,
            )?
        } else {
            c.initial_velocity.clone()
        };
        let mut previous_velocity = boundary_velocity.clone();
        let empty_report = || ContactImplicitReport {
            residuals: vec![],
            frames: vec![],
            maximum_force_error_n: 0.,
            maximum_moment_error_nm: 0.,
            minimum_torque_margin_nm: None,
            maximum_point_penetration_m: 0.,
            contact_sliding_work_j: c.contact_sliding_work_scale_j.map(|_| 0.),
            periodic_boundary: None,
            within_planning_tolerances: false,
            scope: "Position-only backward/forward finite-difference inverse dynamics at each step endpoint. Contact follows smooth point-plane distance and material velocity; no support sequence is prescribed. Knot force/torque balance is an approximate planning test, not detailed contact, interlink collision, timestep, runtime tracking or global optimality validation.",
        };
        let mut total = empty_report();
        for (index, &(time_s, dt)) in self.collocation.iter().enumerate() {
            let k = index + 1;
            let smooth = curve
                .as_ref()
                .map(|curve| curve.sample(time_s))
                .transpose()?
                .map(spatial_sample)
                .transpose()?;
            let q = if let Some((q, _, _)) = &smooth {
                q
            } else {
                &positions[k]
            };
            let smooth_reference = reference.as_ref().map(|r| r.sample(time_s)).transpose()?;
            let position_reference = if let Some(sample) = &smooth_reference {
                &sample.values
            } else {
                &c.position_reference[k]
            };
            // Velocity at k-1 contains the rotation-vector mapping at that knot.
            // Bitwise keys also distinguish signed zero; invalid data was rejected above.
            let key = if let Some((q, v, a)) = &smooth {
                q.iter()
                    .chain(v)
                    .chain(a)
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>()
            } else {
                q.iter()
                    .chain(&positions[k - 1])
                    .chain(&previous_velocity)
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>()
            };
            if let Some(cached) = cache.as_deref_mut() {
                if let Some(entry) = &cached.entries[k] {
                    if entry.key == key {
                        previous_velocity = entry.report.frames[0].velocity.clone();
                        merge_frame(&mut total, &entry.report, &entry.work_terms);
                        cached.reused_frames += 1;
                        continue;
                    }
                }
            }
            let mut report = empty_report();
            let mut work_terms = vec![];
            let velocity = if let Some((_, v, _)) = &smooth {
                v.clone()
            } else {
                endpoint_velocity(q, &positions[k - 1], dt)?
            };
            let angular = V::from_column_slice(&velocity[3..6]);
            let acceleration = if let Some((_, _, a)) = &smooth {
                a.clone()
            } else {
                velocity
                    .iter()
                    .zip(&previous_velocity)
                    .map(|(a, b)| (a - b) / dt)
                    .collect::<Vec<_>>()
            };
            let state = self.configuration_state(q)?;
            let motion = self.map.solve(&state, &q[6..], &velocity)?;
            let mut required = self
                .map
                .prepare_dynamics(&motion)?
                .required_reduced_forces(&acceleration)?;
            let (_, points) =
                self.map
                    .point_jacobians(&motion.generalized, &q[6..], &self.points)?;
            let mut gaps = vec![];
            let mut contact_velocities = vec![];
            let mut forces = vec![];
            for (position, jac) in points {
                let arm = position - V::from_column_slice(&q[..3]);
                let v = V::from_column_slice(&velocity[..3])
                    + angular.cross(&arm)
                    + &jac * nalgebra::DVector::from_column_slice(&velocity[6..]);
                let gap = position.z - self.art.floor_z;
                let f = self.contact.sample(gap, [v.z, v.x, v.y])?.force_n;
                let f = V::new(f[1], f[2], f[0]);
                if let Some(scale) = c.contact_sliding_work_scale_j {
                    // 1/2 ||r||² = dt * (-f_t dot v_t) / scale. This is
                    // physical frictional work, not a prescribed contact mode.
                    let coefficient = c.contact.friction_coefficient * f.z
                        / c.contact.stiction_velocity_m_s.hypot(v.x).hypot(v.y);
                    let factor = (2. * dt * coefficient / scale).sqrt();
                    report.residuals.extend([factor * v.x, factor * v.y]);
                    work_terms.push(dt * coefficient * (v.x * v.x + v.y * v.y));
                }
                let moment = arm.cross(&f);
                for i in 0..3 {
                    required[i] -= f[i];
                    required[3 + i] -= moment[i];
                }
                let joint_load = jac.transpose() * f;
                for j in 0..self.motors.len() {
                    required[6 + j] -= joint_load[j];
                }
                report.maximum_point_penetration_m = report.maximum_point_penetration_m.max(-gap);
                report.residuals.push(
                    dt.sqrt() * (-gap - c.maximum_point_penetration_m).max(0.)
                        / c.penetration_scale_m,
                );
                gaps.push(gap);
                contact_velocities.push(v.into());
                forces.push(f.into());
            }
            for i in 0..6 {
                let tolerance = if i < 3 {
                    c.force_tolerance_n
                } else {
                    c.moment_tolerance_nm
                };
                report.residuals.push(dt.sqrt() * required[i] / tolerance);
                if i < 3 {
                    report.maximum_force_error_n =
                        report.maximum_force_error_n.max(required[i].abs());
                } else {
                    report.maximum_moment_error_nm =
                        report.maximum_moment_error_nm.max(required[i].abs());
                }
            }
            let mut minimum = None::<f64>;
            for (j, motor) in self.motors.iter().enumerate() {
                let torque = required[6 + j];
                let margin = motor.torque_capacity(velocity[6 + j], torque) - torque.abs();
                minimum = Some(minimum.map_or(margin, |m| m.min(margin)));
                report.residuals.push(
                    dt.sqrt() * motor.optimization_torque_violation(velocity[6 + j], torque)
                        / c.torque_tolerance_nm,
                );
                report
                    .residuals
                    .push(dt.sqrt() * torque / c.torque_effort_scale_nm);
            }
            if let Some(m) = minimum {
                report.minimum_torque_margin_nm =
                    Some(report.minimum_torque_margin_nm.map_or(m, |old| old.min(m)));
            }
            for i in 0..n {
                report
                    .residuals
                    .push(dt.sqrt() * (q[i] - position_reference[i]) / c.position_scales[i]);
                report.residuals.push(
                    dt.sqrt() * (velocity[i] - c.velocity_reference[i]) / c.velocity_scales[i],
                );
            }
            report.frames.push(ContactImplicitFrame {
                time_s,
                position: q.clone(),
                velocity: velocity.clone(),
                acceleration,
                gaps_m: gaps,
                contact_velocities_world_m_s: contact_velocities,
                contact_forces_world_n: forces,
                unactuated_wrench: required.as_slice()[..6].to_vec(),
                motor_torques_nm: required.as_slice()[6..].to_vec(),
                minimum_torque_margin_nm: minimum,
            });
            previous_velocity = velocity;
            merge_frame(&mut total, &report, &work_terms);
            if let Some(cached) = cache.as_deref_mut() {
                cached.entries[k] = Some(CachedFrame {
                    key,
                    report,
                    work_terms,
                });
                cached.computed_frames += 1;
            }
        }
        let mut report = total;
        if c.periodic_horizontal_translation {
            let first = &positions[0];
            let last = positions.last().unwrap();
            report.periodic_boundary = Some(ContactImplicitPeriodicBoundary {
                translation_m: [last[0] - first[0], last[1] - first[1], 0.],
                initial_position: initial_sample.as_ref().map(|(q, _, _)| q.clone()),
                initial_velocity: boundary_velocity,
            });
        }
        if curve.is_some() {
            report.scope = "Analytic C2 periodic cubic B-spline position/rate/acceleration with horizontal drift, checked at declared collocation samples through shared inverse dynamics and smooth contact. No support sequence prescribed. Sample feasibility is not continuous-time, detailed runtime, stable orbit, entry-controller or global speed validation.";
        }
        report.within_planning_tolerances = report.maximum_force_error_n <= c.force_tolerance_n
            && report.maximum_moment_error_nm <= c.moment_tolerance_nm
            && report
                .minimum_torque_margin_nm
                .is_none_or(|m| m >= -c.torque_tolerance_nm)
            && report.maximum_point_penetration_m <= c.maximum_point_penetration_m;
        Ok(report)
    }
    pub fn optimize(
        &self,
        initial: &[Vec<f64>],
        bounds: &[Vec<VariableBound>],
        search: &LeastSquaresConfig,
        progress: impl FnMut(&ContactImplicitReport),
    ) -> Result<ContactImplicitOptimization, String> {
        self.optimize_scaled(initial, bounds, search, 0.0, progress)
    }
    pub fn optimize_scaled(
        &self,
        initial: &[Vec<f64>],
        bounds: &[Vec<VariableBound>],
        search: &LeastSquaresConfig,
        exponent: f64,
        progress: impl FnMut(&ContactImplicitReport),
    ) -> Result<ContactImplicitOptimization, String> {
        self.optimize_scaled_refining(initial, bounds, search, exponent, None, progress)
    }
    pub fn optimize_scaled_refining(
        &self,
        initial: &[Vec<f64>],
        bounds: &[Vec<VariableBound>],
        search: &LeastSquaresConfig,
        exponent: f64,
        refinement: Option<&DerivativeRefinement>,
        mut progress: impl FnMut(&ContactImplicitReport),
    ) -> Result<ContactImplicitOptimization, String> {
        let parameters = self.parameterization(initial, bounds)?;
        let mut evaluator = self.evaluator();
        let search = bounded_least_squares_scaled_refining(
            &parameters.values,
            &parameters.bounds,
            search,
            exponent,
            refinement,
            |x| {
                let report = evaluator.evaluate(&parameters.decode(x)?)?;
                progress(&report);
                Ok(report.residuals)
            },
        )?;
        let positions = parameters.decode(&search.values)?;
        let report = self.evaluate(&positions)?;
        Ok(ContactImplicitOptimization {
            positions,
            search,
            report,
        })
    }
    pub fn parameterization(
        &self,
        initial: &[Vec<f64>],
        bounds: &[Vec<VariableBound>],
    ) -> Result<ContactImplicitParameters, String> {
        self.evaluate(initial)?;
        let n = self.map.reduced_dimension();
        let intervals = initial.len() - 1;
        let periodic = self.config.periodic_horizontal_translation;
        let values = if periodic {
            if bounds.len() != initial.len()
                || bounds[..intervals].iter().any(|b| b.len() != n)
                || bounds[intervals].len() != 2
            {
                return Err("periodic bounds require one full row per unique knot and a final two-entry horizontal displacement row".into());
            }
            let mut values = initial[..intervals]
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>();
            values.extend([
                initial[intervals][0] - initial[0][0],
                initial[intervals][1] - initial[0][1],
            ]);
            values
        } else {
            if bounds.len() != intervals || bounds.iter().any(|b| b.len() != n) {
                return Err("one bound per free knot/coordinate required".into());
            }
            initial[1..].iter().flatten().copied().collect()
        };
        Ok(ContactImplicitParameters {
            values,
            bounds: bounds.iter().flatten().cloned().collect(),
            dimension: n,
            intervals,
            fixed_initial: (!periodic).then(|| initial[0].clone()),
        })
    }
    /// Restore sampled physical feasibility using the shared damped least-squares
    /// solver. Task tracking, effort and sliding-work costs are excluded. Caller
    /// bounds must explicitly fix any displacement that must be preserved.
    /// Stationarity of this residual objective is not proof of feasibility.
    pub fn restore_feasibility(
        &self,
        initial: &[Vec<f64>],
        bounds: &[Vec<VariableBound>],
        search: &LeastSquaresConfig,
        exponent: f64,
        refinement: Option<&DerivativeRefinement>,
        progress: impl FnMut(&ContactImplicitReport),
    ) -> Result<ContactImplicitOptimization, String> {
        self.restore_feasibility_with_slip(
            initial, bounds, search, exponent, refinement, None, progress,
        )
    }
    pub fn restore_feasibility_with_slip(
        &self,
        initial: &[Vec<f64>],
        bounds: &[Vec<VariableBound>],
        search: &LeastSquaresConfig,
        exponent: f64,
        refinement: Option<&DerivativeRefinement>,
        slip: Option<&ContactSlipObjective>,
        mut progress: impl FnMut(&ContactImplicitReport),
    ) -> Result<ContactImplicitOptimization, String> {
        if slip.is_some_and(|s| s.constrain_loaded_slip || s.loaded_slip_barrier_weight.is_some()) {
            return Err("actual loaded-slip constraints require the inequality optimizer".into());
        }
        let parameters = self.parameterization(initial, bounds)?;
        let mut evaluator = self.evaluator();
        let search = bounded_least_squares_scaled_refining(
            &parameters.values,
            &parameters.bounds,
            search,
            exponent,
            refinement,
            |x| {
                let report = evaluator.evaluate(&parameters.decode(x)?)?;
                progress(&report);
                let mut residuals = self.feasibility_residuals(&report)?;
                if let Some(objective) = slip {
                    residuals.extend(self.slip_report(&report, objective)?.shaping_residuals);
                }
                Ok(residuals)
            },
        )?;
        let positions = parameters.decode(&search.values)?;
        let report = self.evaluate(&positions)?;
        Ok(ContactImplicitOptimization {
            positions,
            search,
            report,
        })
    }
    /// Signed inequalities matching the existing sampled physical gates. Unlike
    /// zero-residual restoration, balance is allowed inside its stated tolerance.
    /// Contact/capacity calculations use the same report and actuator components.
    pub fn physical_inequalities(
        &self,
        report: &ContactImplicitReport,
    ) -> Result<Vec<f64>, String> {
        self.residual_layout(report)?;
        let mut constraints = Vec::new();
        for f in &report.frames {
            if f.gaps_m.len() != self.points.len()
                || f.unactuated_wrench.len() != 6
                || f.velocity.len() != self.map.reduced_dimension()
                || f.motor_torques_nm.len() != self.motors.len()
                || !f
                    .gaps_m
                    .iter()
                    .chain(&f.unactuated_wrench)
                    .chain(&f.velocity)
                    .chain(&f.motor_torques_nm)
                    .all(|v| v.is_finite())
            {
                return Err("matched finite physical frames required for inequalities".into());
            }
            constraints.extend(f.gaps_m.iter().map(|g| {
                (-g - self.config.maximum_point_penetration_m) / self.config.penetration_scale_m
            }));
            for (j, wrench) in f.unactuated_wrench.iter().enumerate() {
                let tolerance = if j < 3 {
                    self.config.force_tolerance_n
                } else {
                    self.config.moment_tolerance_nm
                };
                constraints.extend([wrench / tolerance - 1., -wrench / tolerance - 1.]);
            }
            for (j, motor) in self.motors.iter().enumerate() {
                let torque = f.motor_torques_nm[j];
                let margin = motor.torque_capacity(f.velocity[6 + j], torque) - torque.abs();
                constraints.push(-margin / self.config.torque_tolerance_nm - 1.);
            }
        }
        Ok(constraints)
    }
    /// Compose the shared physical inequalities and optional actual loaded-slip
    /// inequalities. Group order is the deterministic order in the slip report.
    /// The load threshold makes this actual metric piecewise smooth; it remains
    /// distinct from the conservative RMS objective and requires dense checking.
    pub fn inequality_residuals(
        &self,
        report: &ContactImplicitReport,
        objective: &ContactSlipObjective,
    ) -> Result<InequalityResiduals, String> {
        let slip = self.slip_report(report, objective)?;
        let mut inequalities = self.physical_inequalities(report)?;
        let slip_rows = slip.groups.iter().map(|g| {
            (g.sampled_loaded_slip_ratio - objective.target_ratio) / objective.ratio_scale
        }).collect::<Vec<_>>();
        let mut residuals = slip.shaping_residuals;
        if let Some(weight) = objective.loaded_slip_barrier_weight {
            let barrier_rows = if objective.use_continuous_mean_bound {
                slip.groups.iter().map(|g| {
                    (g.continuous_mean_slip_upper_bound.expect("enabled mean bound") - objective.target_ratio)
                        / objective.ratio_scale
                }).collect::<Vec<_>>()
            } else { slip_rows.clone() };
            residuals.extend(sim_solve::inequality_barrier::reciprocal_inequality_barrier(
                &barrier_rows, weight,
            )?);
        }
        if objective.constrain_loaded_slip {
            inequalities.extend(slip_rows);
        }
        Ok(InequalityResiduals {
            objective: residuals,
            inequalities,
        })
    }
    pub fn optimize_slip_with_inequalities(
        &self,
        initial: &[Vec<f64>],
        bounds: &[Vec<VariableBound>],
        search: &AugmentedLagrangianConfig,
        slip: &ContactSlipObjective,
        progress: impl FnMut(&ContactImplicitReport),
    ) -> Result<ContactImplicitInequalityOptimization, String> {
        self.optimize_slip_with_inequalities_warm_started(
            initial, bounds, search, slip, None, progress,
        )
    }
    pub fn optimize_slip_with_inequalities_warm_started(
        &self,
        initial: &[Vec<f64>],
        bounds: &[Vec<VariableBound>],
        search: &AugmentedLagrangianConfig,
        slip: &ContactSlipObjective,
        warm_start: Option<&AugmentedWarmStart>,
        mut progress: impl FnMut(&ContactImplicitReport),
    ) -> Result<ContactImplicitInequalityOptimization, String> {
        let parameters = self.parameterization(initial, bounds)?;
        let values = if let Some(warm) = warm_start {
            if parameters.decode(&warm.values)? != initial {
                return Err("AL continuation must decode to the supplied initial motion".into());
            }
            &warm.values
        } else {
            &parameters.values
        };
        let mut evaluator = self.evaluator();
        let search = bounded_inequality_augmented_lagrangian_warm_started(
            values,
            &parameters.bounds,
            search,
            warm_start,
            |x| {
                let report = evaluator.evaluate(&parameters.decode(x)?)?;
                progress(&report);
                self.inequality_residuals(&report, slip)
            },
        )?;
        let positions = parameters.decode(&search.values)?;
        let report = self.evaluate(&positions)?;
        Ok(ContactImplicitInequalityOptimization {
            positions,
            search,
            report,
        })
    }
    /// Evaluate actual sampled slip and its conservative shaping surrogate from
    /// shared contact frames. Analytic periodic motion supplies the physical
    /// initial position and nonzero displacement needed by the bound.
    pub fn slip_report(
        &self,
        report: &ContactImplicitReport,
        objective: &ContactSlipObjective,
    ) -> Result<ContactSlipReport, String> {
        self.residual_layout(report)?;
        if objective.point_groups.len() != self.points.len()
            || objective.point_groups.iter().any(|g| g.trim().is_empty())
            || !objective.target_ratio.is_finite()
            || objective.target_ratio < 0.
            || !objective.ratio_scale.is_finite()
            || objective.ratio_scale <= 0.
            || objective.loaded_slip_barrier_weight.is_some_and(|w| !w.is_finite() || w <= 0.)
        {
            return Err("slip objective requires an explicit group per contact point and finite nonnegative target / positive scale".into());
        }
        let boundary = report
            .periodic_boundary
            .as_ref()
            .ok_or("slip objective requires periodic motion")?;
        let initial = boundary
            .initial_position
            .as_ref()
            .ok_or("slip objective requires analytic periodic motion")?;
        let displacement = boundary.translation_m[0].hypot(boundary.translation_m[1]);
        let duration = self
            .collocation
            .last()
            .ok_or("slip objective requires samples")?
            .0;
        let component = ContactSlip::new(ContactSlipConfig {
            duration_s: duration,
            displacement_m: displacement,
            load_threshold_n: objective.load_threshold_n,
        })?;
        let mut previous = initial;
        let mut body_path = 0.;
        for f in &report.frames {
            body_path += (f.position[0] - previous[0]).hypot(f.position[1] - previous[1]);
            previous = &f.position;
            if f.contact_forces_world_n.len() != self.points.len()
                || f.contact_velocities_world_m_s.len() != self.points.len()
            {
                return Err("matched contact point observations required for slip".into());
            }
        }
        if !body_path.is_finite() || body_path <= 0. || body_path + 1e-12 < displacement {
            return Err("positive periodic body path must cover its displacement".into());
        }
        let mut indices = BTreeMap::<String, Vec<usize>>::new();
        for (i, group) in objective.point_groups.iter().enumerate() {
            indices.entry(group.clone()).or_default().push(i);
        }
        let mut groups = Vec::new();
        let mut residuals = Vec::new();
        for (group, points) in indices {
            let mut squared = 0.;
            let mut loaded_path = 0.;
            let mut mean_path = 0.;
            for (f, &(_, dt)) in report.frames.iter().zip(&self.collocation) {
                let load = points
                    .iter()
                    .map(|&i| f.contact_forces_world_n[i][2])
                    .sum::<f64>();
                let mut weighted_speed = 0.;
                for &i in &points {
                    let force = f.contact_forces_world_n[i][2];
                    let v = f.contact_velocities_world_m_s[i];
                    let r = component.sample(dt, force, load, [v[0], v[1]])?;
                    squared += r[0] * r[0] + r[1] * r[1];
                    if objective.use_continuous_mean_bound {
                        mean_path += component.mean_path_sample(dt, force, load, [v[0], v[1]])?;
                    }
                    weighted_speed += force * v[0].hypot(v[1]);
                }
                if load >= objective.load_threshold_n {
                    loaded_path += dt * weighted_speed / load;
                }
            }
            let bound = squared.sqrt();
            let actual = loaded_path / body_path;
            let mean = objective.use_continuous_mean_bound.then_some(mean_path / body_path);
            let residual = (mean.unwrap_or(bound) - objective.target_ratio).max(0.) / objective.ratio_scale;
            if !bound.is_finite()
                || !actual.is_finite()
                || !residual.is_finite()
                || actual > bound + 1e-10
                || mean.is_some_and(|m| !m.is_finite() || actual > m + 1e-10 || m > bound + 1e-10)
            {
                return Err("nonfinite slip metric or violated sampled bound".into());
            }
            groups.push(ContactSlipGroupReport {
                group,
                sampled_loaded_slip_ratio: actual,
                rms_slip_upper_bound: bound,
                continuous_mean_slip_upper_bound: mean,
            });
            residuals.push(residual);
        }
        Ok(ContactSlipReport {
            within_sampled_slip_limit: groups
                .iter()
                .all(|g| g.sampled_loaded_slip_ratio <= objective.target_ratio),
            groups,
            shaping_residuals: residuals,
            body_path_m: body_path,
            displacement_m: displacement,
            duration_s: duration,
            scope: if objective.use_continuous_mean_bound {
                "Actual loaded-slip measure with continuous mean and RMS upper bounds at declared samples. Mean bound supplies shaping and the optional barrier; independent dense/runtime slip and physical gates remain required."
            } else {
                "Actual loaded-slip measure and conservative RMS shaping bound at declared samples. Surrogate target is sufficient but not necessary; it does not replace dense/runtime slip, force, collision or speed-optimality checks."
            },
        })
    }
    fn residual_layout(
        &self,
        report: &ContactImplicitReport,
    ) -> Result<(usize, usize, usize), String> {
        let stride = if self.config.contact_sliding_work_scale_j.is_some() {
            3
        } else {
            1
        };
        let point_rows = self.points.len() * stride;
        let block = point_rows + 6 + 2 * self.motors.len() + 2 * self.map.reduced_dimension();
        if report.frames.len() != self.collocation.len()
            || report.residuals.len() != block * report.frames.len()
            || report
                .frames
                .iter()
                .zip(&self.collocation)
                .any(|(f, &(time, _))| f.time_s != time)
        {
            return Err("matched planner report required for residual partition".into());
        }
        Ok((stride, point_rows, block))
    }
    /// Unweighted normalized balance, motor violation and point-penetration
    /// residuals, selected from the shared physical report without recalculating
    /// contact or actuator physics. Every collocation sample has equal weight.
    pub fn feasibility_residuals(
        &self,
        report: &ContactImplicitReport,
    ) -> Result<Vec<f64>, String> {
        let (stride, point_rows, block) = self.residual_layout(report)?;
        let mut residuals = Vec::new();
        for (row, &(_, dt)) in report.residuals.chunks_exact(block).zip(&self.collocation) {
            let weight = dt.sqrt();
            for j in 0..self.points.len() {
                residuals.push(row[(j + 1) * stride - 1] / weight);
            }
            residuals.extend(row[point_rows..point_rows + 6].iter().map(|v| v / weight));
            for j in 0..self.motors.len() {
                residuals.push(row[point_rows + 6 + 2 * j] / weight);
            }
        }
        Ok(residuals)
    }
    /// Retain every task/effort/slip/limit residual, moving only the six base
    /// balance rows from weighted penalties into unweighted normalized equalities.
    pub fn equality_residuals(
        &self,
        report: &ContactImplicitReport,
    ) -> Result<EqualityResiduals, String> {
        let (_, point_rows, block) = self.residual_layout(report)?;
        let mut objective = Vec::new();
        for row in report.residuals.chunks_exact(block) {
            objective.extend_from_slice(&row[..point_rows]);
            objective.extend_from_slice(&row[point_rows + 6..]);
        }
        let equalities = report
            .frames
            .iter()
            .flat_map(|f| {
                f.unactuated_wrench.iter().enumerate().map(|(j, v)| {
                    v / if j < 3 {
                        self.config.force_tolerance_n
                    } else {
                        self.config.moment_tolerance_nm
                    }
                })
            })
            .collect();
        Ok(EqualityResiduals {
            objective,
            equalities,
        })
    }
    pub fn optimize_equalities(
        &self,
        initial: &[Vec<f64>],
        bounds: &[Vec<VariableBound>],
        config: &EqualityDoglegConfig,
        mut progress: impl FnMut(&ContactImplicitReport),
    ) -> Result<ContactImplicitEqualityOptimization, String> {
        let parameters = self.parameterization(initial, bounds)?;
        let mut evaluator = self.evaluator();
        let search =
            bounded_equality_dogleg(&parameters.values, &parameters.bounds, config, |x| {
                let report = evaluator.evaluate(&parameters.decode(x)?)?;
                progress(&report);
                self.equality_residuals(&report)
            })?;
        let positions = parameters.decode(&search.values)?;
        let report = self.evaluate(&positions)?;
        Ok(ContactImplicitEqualityOptimization {
            positions,
            search,
            report,
        })
    }
}

fn merge_frame(total: &mut ContactImplicitReport, frame: &ContactImplicitReport, work: &[f64]) {
    total.residuals.extend_from_slice(&frame.residuals);
    total.frames.extend_from_slice(&frame.frames);
    total.maximum_force_error_n = total.maximum_force_error_n.max(frame.maximum_force_error_n);
    total.maximum_moment_error_nm = total
        .maximum_moment_error_nm
        .max(frame.maximum_moment_error_nm);
    total.maximum_point_penetration_m = total
        .maximum_point_penetration_m
        .max(frame.maximum_point_penetration_m);
    if let Some(m) = frame.minimum_torque_margin_nm {
        total.minimum_torque_margin_nm =
            Some(total.minimum_torque_margin_nm.map_or(m, |old| old.min(m)));
    }
    if let Some(sum) = total.contact_sliding_work_j.as_mut() {
        for term in work {
            *sum += term;
        }
    }
}

fn endpoint_velocity(q: &[f64], previous: &[f64], dt: f64) -> Result<Vec<f64>, String> {
    let mut velocity = q
        .iter()
        .zip(previous)
        .map(|(a, b)| (a - b) / dt)
        .collect::<Vec<_>>();
    let angular = rotation_vector_motion(
        V::from_column_slice(&q[3..6]),
        V::from_column_slice(&velocity[3..6]),
        V::zeros(),
    )?
    .0;
    velocity[3..6].copy_from_slice(angular.as_slice());
    Ok(velocity)
}

fn spatial_sample(sample: TrajectorySample) -> Result<(Vec<f64>, Vec<f64>, Vec<f64>), String> {
    let (omega, alpha) = rotation_vector_motion(
        V::from_column_slice(&sample.values[3..6]),
        V::from_column_slice(&sample.rates[3..6]),
        V::from_column_slice(&sample.accelerations[3..6]),
    )?;
    let mut velocity = sample.rates;
    let mut acceleration = sample.accelerations;
    velocity[3..6].copy_from_slice(omega.as_slice());
    acceleration[3..6].copy_from_slice(alpha.as_slice());
    Ok((sample.values, velocity, acceleration))
}

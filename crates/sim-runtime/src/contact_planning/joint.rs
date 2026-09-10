//! Joint phase/motion/load optimization. Uses the existing CAD evaluation and
//! shared inequality AL solver; no contact forces are injected into simulation.
use super::*;
use sim_domain_control::trajectory::{Interpolation, Trajectory, TrajectoryConfig};
#[path = "joint_steps.rs"]
mod steps;
use steps::{ForceNode, ForceSlot, force_slots};
use sim_domain_robot::motion_capability::point_force_cone_inequalities;
use sim_solve::inequality_augmented_lagrangian::{
    AugmentedLagrangianConfig, AugmentedLagrangianResult, InequalityResiduals,
    bounded_inequality_augmented_lagrangian_with_jacobian,
};
#[cfg(feature = "conic")]
#[path = "joint_conic.rs"]
mod conic;
#[cfg(feature = "conic")]
pub use conic::JointForceConicOptimization;
#[cfg(feature = "conic")]
#[path = "joint_orders.rs"]
mod orders;
#[cfg(feature = "conic")]
pub use orders::ContactOrderNeighbor;
#[path = "joint_restore.rs"]
mod restore;
pub use restore::JointDomainRestoration;
#[path = "joint_timing.rs"]
mod timing;
pub use timing::{ContactEvent, ForceKnotBinding, JointForceTiming};
#[cfg(all(feature = "native-ipopt", not(target_arch = "wasm32")))]
#[path = "joint_ipopt.rs"]
mod native_ipopt;
#[cfg(all(feature = "native-ipopt", not(target_arch = "wasm32")))]
pub use native_ipopt::{JointIpoptConfig, JointIpoptOptimization};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JointContactMotion {
    pub motion: ContactPhaseConfig,
    /// One set per operating clock (primary first), one xyz world-force template
    /// per stance in foot-major order: primary step, then additional_steps.
    /// Values are N. Template time 0..1 s is stretched to that stance's
    /// stance duration; endpoints are zero. Linear or quintic interpolation
    /// preserves the convex friction cone when all nodes are inside it.
    pub force_templates: Vec<Vec<TrajectoryConfig>>,
    /// Optional contact-event-relative knot layout. Absent preserves legacy
    /// fixed normalized stance times. Present resolves times on every probe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_timing: Option<JointForceTiming>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum JointContactDecision {
    Motion {
        decision: ContactDecision,
    },
    /// Endpoints cannot be varied, so lift-off/touchdown force remains zero.
    Force {
        clock: usize,
        foot: usize,
        node: usize,
        axis: usize,
    },
    /// Step is zero-based within the physical foot's additional_steps.
    AdditionalForce { clock: usize, foot: usize, step: usize, node: usize, axis: usize },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JointContactVariable {
    pub decision: JointContactDecision,
    pub bound: VariableBound,
}
#[derive(Clone, Debug, Serialize)]
pub struct JointContactReport {
    pub motion_report: ContactPlanReport,
    /// Signed constraints use physical tolerances for scaling, not mesh weights.
    pub constraints: InequalityResiduals,
    pub maximum_cone_violation_n: f64,
    pub sampled_feasible: bool,
}
#[derive(Debug, Serialize)]
pub struct JointContactOptimization {
    pub candidate: JointContactMotion,
    pub report: JointContactReport,
    pub search: AugmentedLagrangianResult,
    pub best_sampled_feasible: Option<JointContactMotion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_derivative_mode: Option<&'static str>,
    pub scope: &'static str,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct JointCacheStatistics {
    pub hits: usize,
    pub misses: usize,
}
/// Unactuated affine load system at one instant, before any force-curve,
/// friction or actuator restriction. Columns are xyz forces for every foot.
#[derive(Debug)]
pub struct JointForceFrameSystem {
    pub phase: f64,
    pub clock_index: usize,
    pub clock: ContactClock,
    pub planned_contacts: Vec<bool>,
    pub force_to_wrench: nalgebra::DMatrix<f64>,
    pub zero_force_residual: Vec<f64>,
}
#[derive(Debug, Serialize)]
pub struct ContactSurfacePoint {
    pub marker_id: String,
    pub foot: usize,
    pub position_world_m: [f64; 3],
    pub floor_gap_m: f64,
}
#[derive(Debug, Serialize)]
pub struct ContactSurfaceFrame {
    pub phase: f64,
    pub clock: ContactClock,
    pub planned_contacts: Vec<bool>,
    pub reference_world_m: [f64; 3],
    pub required_wrench: Vec<f64>,
    pub original_marker_geometry_error_m: f64,
    pub points: Vec<ContactSurfacePoint>,
}
#[derive(Default)]
pub(super) struct JointCache {
    entry: Option<JointCacheEntry>,
    statistics: JointCacheStatistics,
}
struct JointCacheEntry {
    key: Vec<u8>,
    reports: Vec<ContactPlanReport>,
    maps: Vec<Vec<(f64, PointForceLoadMap)>>,
}

pub(super) struct PreparedForces {
    trajectories: Vec<Trajectory>,
    slots: Vec<ForceSlot>,
    pub knot_phases: Vec<f64>,
}
impl PreparedForces {
    fn new(motion: &ContactPhaseConfig, templates: &[TrajectoryConfig]) -> Result<Self, String> {
        let slots = force_slots(motion);
        if slots.len() != motion.feet.len() { ContactPhaseMotion::new(motion.clone())?; }
        if templates.len() != slots.len() {
            return Err("one force template per stance in foot-major step order required".into());
        }
        let mut trajectories = Vec::new();
        let mut knot_phases = Vec::new();
        for (slot, template) in slots.iter().zip(templates) {
            let foot = &slot.step;
            let ks = &template.keyframes;
            if ks.len() < 3
                || ks.len() > 128
                || ks[0].time_s != 0.0
                || ks.last().unwrap().time_s != 1.0
                || ks[0].values != [0.0; 3]
                || ks.last().unwrap().values != [0.0; 3]
                || ks.iter().any(|k| k.values.len() != 3)
                || !matches!(
                    template.interpolation,
                    Interpolation::Linear | Interpolation::QuinticRestToRest
                )
            {
                return Err("3..128 xyz force nodes over unit duration, zero endpoints and convex interpolation required".into());
            }
            trajectories.push(Trajectory::new(template.clone())?);
            knot_phases
                .extend(ks.iter().map(|k| {
                    (foot.phase_offset + k.time_s * foot.stance_fraction).rem_euclid(1.0)
                }));
        }
        Ok(Self {
            trajectories,
            slots,
            knot_phases,
        })
    }
    pub fn sample(&self, motion: &ContactPhaseConfig, phase: f64) -> Result<Vec<[f64; 3]>, String> {
        if !phase.is_finite() { return Err("finite force phase required".into()); }
        if self.slots.len() != motion.feet.len() {
            let mut forces = vec![[0.0; 3]; motion.feet.len()];
            for (slot, trajectory) in self.slots.iter().zip(&self.trajectories) {
                let local = (phase - slot.step.phase_offset).rem_euclid(1.0);
                if local < slot.step.stance_fraction {
                    let sample = trajectory.sample(local / slot.step.stance_fraction)?;
                    forces[slot.foot] = std::array::from_fn(|axis| sample.values[axis]);
                }
            }
            return Ok(forces);
        }
        motion
            .feet
            .iter()
            .zip(&self.trajectories)
            .map(|(foot, trajectory)| {
                let local = (phase - foot.phase_offset).rem_euclid(1.0);
                if local >= foot.stance_fraction {
                    return Ok([0.0; 3]);
                }
                let s = trajectory.sample(local / foot.stance_fraction)?;
                Ok(std::array::from_fn(|i| s.values[i]))
            })
            .collect()
    }
}

impl JointContactMotion {
    /// Place linear force knots at the current contact events, stance midpoints
    /// and body knots shared across feet. This is an explicit initialization /
    /// refinement operation, not a change to knots during an optimizer call.
    /// Existing linear knots are retained, preserving their entire curves.
    /// Converting a quintic template samples (and changes) its force curve;
    /// body/foot motion remains unchanged and requires a fresh force audit.
    pub fn with_event_aligned_linear_forces(&self) -> Result<Self, String> {
        if self.force_timing.is_some() {
            return Err(
                "materialize and explicitly clear force timing before static refinement".into(),
            );
        }
        let motion = ContactPhaseMotion::new(self.motion.clone())?;
        if self.force_templates.is_empty() {
            return Err("at least one force operating clock required".into());
        }
        let mut phases = self
            .motion
            .body
            .keyframes
            .iter()
            .map(|k| (k.time_s / self.motion.period_s).rem_euclid(1.0))
            .collect::<Vec<_>>();
        for interval in motion.contact_intervals() {
            phases.extend([interval.start_phase, interval.end_phase.rem_euclid(1.0)]);
        }
        let slots = force_slots(&self.motion);
        for slot in &slots {
            let foot = &slot.step;
            phases.push((foot.phase_offset + 0.5 * foot.stance_fraction).rem_euclid(1.0));
        }
        phases.sort_by(f64::total_cmp);
        phases.dedup_by(|a, b| (*a - *b).abs() <= 1e-12);
        let mut result = self.clone();
        for (input, output) in self.force_templates.iter().zip(&mut result.force_templates) {
            let prepared = PreparedForces::new(&self.motion, input)?;
            for (index, (slot, template)) in
                slots.iter().zip(output.iter_mut()).enumerate()
            {
                let foot = &slot.step;
                let mut times = if matches!(input[index].interpolation, Interpolation::Linear) {
                    input[index]
                        .keyframes
                        .iter()
                        .map(|k| k.time_s)
                        .collect::<Vec<_>>()
                } else {
                    vec![0.0, 1.0]
                };
                for &phase in &phases {
                    let local = (phase - foot.phase_offset).rem_euclid(1.0);
                    if local > 1e-12 && local < foot.stance_fraction - 1e-12 {
                        let time = local / foot.stance_fraction;
                        if times.iter().all(|old| (*old - time).abs() > 1e-12) {
                            times.push(time);
                        }
                    }
                }
                times.sort_by(f64::total_cmp);
                times.dedup_by(|a, b| *a == *b);
                let last = times.len() - 1;
                let keyframes = times
                    .into_iter()
                    .enumerate()
                    .map(|(i, time_s)| {
                        let values = if i == 0 || i == last {
                            vec![0.0; 3]
                        } else {
                            prepared.trajectories[index].sample(time_s)?.values
                        };
                        Ok(sim_domain_control::trajectory::Keyframe { time_s, values })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                *template = TrajectoryConfig {
                    interpolation: Interpolation::Linear,
                    keyframes,
                };
            }
            PreparedForces::new(&self.motion, output)?;
        }
        Ok(result)
    }
}

impl ContactPlanner<'_> {
    /// Exact fixed-motion map from force-node values to normalized unactuated
    /// wrench residuals. Rows are six balance components per reported frame;
    /// columns follow the supplied force decisions. Motor and cone derivatives
    /// are deliberately absent, so their nonsmooth points cannot invalidate
    /// this affine balance map. No finite subtraction of loaded reports.
    pub fn joint_balance_jacobian(
        &self,
        candidate: &JointContactMotion,
        variables: &[JointContactVariable],
    ) -> Result<(JointContactReport, nalgebra::DMatrix<f64>), String> {
        self.joint_affine_force_jacobian(candidate, variables, false)
    }

    /// Exact force-node map to normalized upper/lower nominal servo-command
    /// inequalities, in reported frame/motor order. Fixed motion only; no
    /// finite subtraction, torque-capacity switches or cone derivatives.
    pub fn joint_servo_command_jacobian(
        &self,
        candidate: &JointContactMotion,
        variables: &[JointContactVariable],
    ) -> Result<(JointContactReport, nalgebra::DMatrix<f64>), String> {
        if self.recipe.servo_command_limits.is_none() {
            return Err("explicit servo-command limits required".into());
        }
        self.joint_affine_force_jacobian(candidate, variables, true)
    }

    fn joint_affine_force_jacobian(
        &self,
        candidate: &JointContactMotion,
        variables: &[JointContactVariable],
        servo_commands: bool,
    ) -> Result<(JointContactReport, nalgebra::DMatrix<f64>), String> {
        if variables.is_empty() {
            return Err("nonempty force decisions required".into());
        }
        let resolved = candidate.resolved_force_timing()?;
        let candidate = resolved.as_ref();
        let report = self.evaluate_joint(candidate)?;
        let cache = self.joint_cache.borrow();
        let entry = cache
            .entry
            .as_ref()
            .ok_or("missing joint balance geometry")?;
        let rows_per_frame = if servo_commands {
            2 * self.motors.len()
        } else {
            6
        };
        let motor_maps = servo_commands.then(|| {
            entry
                .maps
                .iter()
                .map(|maps| {
                    maps.iter()
                        .map(|(_, map)| map.motor_jacobian())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        });
        let mut a = nalgebra::DMatrix::zeros(
            rows_per_frame * report.motion_report.frames.len(),
            variables.len(),
        );
        let mut seen = std::collections::BTreeSet::new();
        for (column, variable) in variables.iter().enumerate() {
            let Some(ForceNode { clock, slot, foot, node, axis }) = variable.decision.force_node(&candidate.motion)?
            else {
                return Err("balance Jacobian accepts force decisions only".into());
            };
            if !seen.insert((clock, slot, node, axis)) {
                return Err("duplicate force decision in balance Jacobian".into());
            }
            let mut scratch = candidate.clone();
            joint_decision(&mut scratch, &variable.decision)?;
            let mut templates = candidate.force_templates[clock].clone();
            for template in &mut templates {
                for key in &mut template.keyframes {
                    key.values.fill(0.);
                }
            }
            templates[slot].keyframes[node].values[axis] = 1.;
            let basis = PreparedForces::new(&candidate.motion, &templates)?;
            let mut row = 0;
            for (ci, maps) in entry.maps.iter().enumerate() {
                for (fi, (phase, map)) in maps.iter().enumerate() {
                    if ci == clock {
                        let weight = basis.sample(&candidate.motion, *phase)?[foot][axis];
                        if let Some(motor_maps) = &motor_maps {
                            let limits = self.recipe.servo_command_limits.as_ref().unwrap();
                            for (j, motor) in self.motors.iter().enumerate() {
                                let torque = weight * motor_maps[ci][fi][(j, 3 * foot + axis)];
                                let derivative = motor.reference_target(0., 0., torque)?
                                    / limits.residual_scale_rad;
                                a[(row + 2 * j, column)] = derivative;
                                a[(row + 2 * j + 1, column)] = -derivative;
                            }
                        } else {
                            for i in 0..6 {
                                let tolerance = if i < 3 {
                                    self.recipe.force_tolerance_n
                                } else {
                                    self.recipe.moment_tolerance_nm
                                };
                                a[(row + i, column)] = weight
                                    * map.wrench_jacobian()[(i, 3 * foot + axis)]
                                    / tolerance;
                            }
                        }
                    }
                    row += rows_per_frame;
                }
            }
            if row != a.nrows() {
                return Err("joint balance Jacobian row mismatch".into());
            }
        }
        Ok((report, a))
    }

    /// Exact force columns at fixed CAD motion and fixed force-knot layout.
    /// Motion columns request numerical differentiation. Circular-cone apexes
    /// use the zero tangential subgradient. A zero motor torque at nonzero
    /// speed may switch the signed capacity discontinuously; affected columns
    /// request numerical differentiation instead of claiming a derivative.
    pub fn linearize_joint_forces(
        &self,
        candidate: &JointContactMotion,
        variables: &[JointContactVariable],
    ) -> Result<
        (
            JointContactReport,
            sim_solve::least_squares::PartialJacobian,
        ),
        String,
    > {
        let resolved = candidate.resolved_force_timing()?;
        let candidate = resolved.as_ref();
        let report = self.evaluate_joint(candidate)?;
        let cache = self.joint_cache.borrow();
        let entry = cache
            .entry
            .as_ref()
            .ok_or("missing force linearization geometry")?;
        let motor_maps = entry
            .maps
            .iter()
            .map(|maps| {
                maps.iter()
                    .map(|(_, map)| map.motor_jacobian())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let mut columns = Vec::with_capacity(variables.len());
        for variable in variables {
            let Some(ForceNode { clock, slot, foot, node, axis }) = variable.decision.force_node(&candidate.motion)?
            else {
                columns.push(None);
                continue;
            };
            let mut scratch = candidate.clone();
            joint_decision(&mut scratch, &variable.decision)?;
            let mut templates = candidate.force_templates[clock].clone();
            for template in &mut templates {
                for key in &mut template.keyframes {
                    key.values.fill(0.0);
                }
            }
            templates[slot].keyframes[node].values[axis] = 1.0;
            let basis = PreparedForces::new(&candidate.motion, &templates)?;
            let mut column = vec![
                0.0;
                report.constraints.objective.len()
                    + report.constraints.inequalities.len()
            ];
            let mut row = report.constraints.objective.len();
            let mut frame_index = 0;
            let mut nonsmooth_capacity = false;
            for (ci, maps) in entry.maps.iter().enumerate() {
                for (fi, (phase, map)) in maps.iter().enumerate() {
                    let frame = &report.motion_report.frames[frame_index];
                    if ci == clock {
                        let weight = basis.sample(&candidate.motion, *phase)?[foot][axis];
                        for i in 0..6 {
                            let tolerance = if i < 3 {
                                self.recipe.force_tolerance_n
                            } else {
                                self.recipe.moment_tolerance_nm
                            };
                            let derivative =
                                weight * map.wrench_jacobian()[(i, 3 * foot + axis)] / tolerance;
                            column[row + 2 * i] = derivative;
                            column[row + 2 * i + 1] = -derivative;
                        }
                        for (j, torque) in frame.motor_torques_nm.iter().enumerate() {
                            let derivative = weight * motor_maps[ci][fi][(j, 3 * foot + axis)];
                            if *torque == 0.0
                                && frame.reduced_velocity[6 + j] != 0.0
                                && derivative != 0.0
                            {
                                nonsmooth_capacity = true;
                            }
                            let sign = if *torque == 0.0 { 0.0 } else { torque.signum() };
                            column[row + 12 + j] =
                                sign * derivative / self.recipe.torque_tolerance_nm;
                            if let Some(limits) = &self.recipe.servo_command_limits {
                                let d = self.motors[j].reference_target(0.0, 0.0, derivative)? / limits.residual_scale_rad;
                                let start = row + 12 + frame.motor_torques_nm.len() + 2 * j;
                                column[start] = d;
                                column[start + 1] = -d;
                            }
                        }
                    }
                    row += self.joint_frame_load_rows(frame) + 2;
                    frame_index += 1;
                }
                for (f, template) in candidate.force_templates[ci].iter().enumerate() {
                    for (n, key) in template.keyframes.iter().enumerate() {
                        if ci == clock && f == slot && n == node {
                            let tangent = key.values[0].hypot(key.values[1]);
                            column[row] = if axis == 2 {
                                -1.0 / self.recipe.force_tolerance_n
                            } else {
                                0.0
                            };
                            column[row + 1] = if axis == 2 {
                                -self.art.model.world.floor_friction
                            } else if tangent > 0.0 {
                                key.values[axis] / tangent
                            } else {
                                0.0
                            } / self.recipe.force_tolerance_n;
                        }
                        row += 2;
                    }
                }
            }
            if row != column.len() || frame_index != report.motion_report.frames.len() {
                return Err("joint force Jacobian row mismatch".into());
            }
            columns.push((!nonsmooth_capacity).then_some(column));
        }
        Ok((report, columns))
    }

    /// Inspect explicit CAD surface markers at the same whole-body poses used
    /// by the force audit. No new forces, geometry or contact eligibility are
    /// assigned here; the caller must declare an eligibility rule.
    pub fn joint_contact_surface_frames(
        &self,
        candidate: &JointContactMotion,
        surfaces: &CaptureConfig,
    ) -> Result<Vec<ContactSurfaceFrame>, String> {
        if surfaces.expected_cad_sha256.as_deref() != Some(self.recipe.expected_cad_sha256.as_str())
            || surfaces.coordinate_frame != self.coordinate_frame
            || surfaces.markers.is_empty()
        {
            return Err("matched CAD/frame and explicit nonempty surface markers required".into());
        }
        crate::tracking::validate_markers(&surfaces.markers)?;
        let mut points = self.points.clone();
        let mut owners = Vec::new();
        for marker in &surfaces.markers {
            let link = self
                .art
                .links
                .iter()
                .position(|l| l.name == marker.link)
                .ok_or("unknown contact surface link")?;
            let matching = self
                .points
                .iter()
                .enumerate()
                .filter(|(_, p)| p.link == link)
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            if matching.len() != 1 {
                return Err("each surface link must match one motion foot marker".into());
            }
            if !self.art.links[link]
                .contact
                .iter()
                .any(|p| (*p - V::from(marker.local_point_m)).norm() <= 1e-12)
            {
                return Err("support marker is not a compiled CAD contact sample".into());
            }
            owners.push(matching[0]);
            points.push(EmbeddedPoint {
                link,
                local_point_m: marker.local_point_m,
            });
        }
        self.evaluate_joint(candidate)?;
        let reference = ContactPhaseMotion::new(candidate.motion.clone())?;
        let cache = self.joint_cache.borrow();
        let entry = cache.entry.as_ref().ok_or("missing joint geometry")?;
        let mut output = Vec::new();
        for (report, maps) in entry.reports.iter().zip(&entry.maps) {
            for (frame, (phase, map)) in report.frames.iter().zip(maps) {
                let sample = reference.sample_retimed(
                    *phase * candidate.motion.period_s,
                    frame.clock.phase_rate,
                    frame.clock.phase_acceleration_per_s,
                )?;
                let seed = self.contact_sample_seed(&sample.body);
                let (_, located) = self
                    .map
                    .point_jacobians(&seed, &frame.coordinates, &points)?;
                let base = self.art.bases[0].state;
                let center = std::array::from_fn(|i| seed.states[base + i]);
                let motion_points = located[..self.points.len()]
                    .iter()
                    .map(|(p, _)| (*p).into())
                    .collect::<Vec<[f64; 3]>>();
                let a = sim_domain_robot::motion_capability::point_force_wrench_matrix(
                    &motion_points,
                    center,
                )?;
                let error = (a - map.wrench_jacobian()).amax();
                let zero = map.evaluate(&vec![[0.; 3]; self.points.len()])?;
                let surfaces = located[self.points.len()..]
                    .iter()
                    .zip(&surfaces.markers)
                    .zip(&owners)
                    .map(|(((p, _), marker), &foot)| ContactSurfacePoint {
                        marker_id: marker.id.clone(),
                        foot,
                        position_world_m: (*p).into(),
                        floor_gap_m: p[2] - self.art.floor_z,
                    })
                    .collect();
                output.push(ContactSurfaceFrame {
                    phase: *phase,
                    clock: frame.clock,
                    planned_contacts: frame.planned_contacts.clone(),
                    reference_world_m: center,
                    required_wrench: zero.wrench_residual.iter().map(|v| -v).collect(),
                    original_marker_geometry_error_m: error,
                    points: surfaces,
                });
            }
        }
        Ok(output)
    }

    /// Expose the same CAD affine maps for independent instantaneous support
    /// relaxations. The caller must enforce zero swing force and explicitly
    /// declare any force bounds; this method does not certify feasibility.
    pub fn joint_force_frame_systems(
        &self,
        candidate: &JointContactMotion,
    ) -> Result<Vec<JointForceFrameSystem>, String> {
        self.evaluate_joint(candidate)?;
        let cache = self.joint_cache.borrow();
        let entry = cache
            .entry
            .as_ref()
            .ok_or("missing current joint force maps")?;
        let mut output = Vec::new();
        for (clock_index, (report, maps)) in entry.reports.iter().zip(&entry.maps).enumerate() {
            for (frame, (phase, map)) in report.frames.iter().zip(maps) {
                let zero = map.evaluate(&vec![[0.0; 3]; frame.planned_contacts.len()])?;
                output.push(JointForceFrameSystem {
                    phase: *phase,
                    clock_index,
                    clock: frame.clock,
                    planned_contacts: frame.planned_contacts.clone(),
                    force_to_wrench: map.wrench_jacobian().clone(),
                    zero_force_residual: zero.wrench_residual,
                });
            }
        }
        Ok(output)
    }

    /// Initialize nodes from the wrench allocator at their exact phases. This
    /// is only a warm start; the joint solve subsequently varies loads freely.
    pub fn seed_joint_forces(
        &self,
        candidate: &JointContactMotion,
    ) -> Result<JointContactMotion, String> {
        let resolved = candidate.resolved_force_timing()?;
        let candidate = resolved.as_ref();
        if candidate.force_templates.len() != 1 + self.recipe.additional_clocks.len() {
            return Err("one independent force-template set per operating clock required".into());
        }
        ContactPhaseMotion::new(candidate.motion.clone())?;
        let mut output = candidate.clone();
        let mut clocks = vec![ContactClock {
            phase_rate: self.recipe.phase_rate,
            phase_acceleration_per_s: self.recipe.phase_acceleration_per_s,
        }];
        clocks.extend_from_slice(&self.recipe.additional_clocks);
        for (clock, templates) in clocks.into_iter().zip(&mut output.force_templates) {
            let prepared = PreparedForces::new(&candidate.motion, templates)?;
            let report = self.evaluate_clock_with_forces(
                &candidate.motion,
                clock,
                None,
                &prepared.knot_phases,
                None,
            )?;
            for (slot, template) in prepared.slots.iter().zip(templates) {
                let foot = &slot.step;
                let last = template.keyframes.len() - 1;
                for node in template.keyframes.iter_mut().take(last).skip(1) {
                    let phase =
                        (foot.phase_offset + node.time_s * foot.stance_fraction).rem_euclid(1.0);
                    let frame = report
                        .frames
                        .iter()
                        .find(|frame| {
                            (frame.time_s / candidate.motion.period_s - phase).abs() < 1e-12
                        })
                        .ok_or("missing force-initialization collocation node")?;
                    node.values = frame.support_forces_world_n[slot.foot].to_vec();
                }
            }
        }
        Ok(output)
    }

    pub fn evaluate_joint(
        &self,
        candidate: &JointContactMotion,
    ) -> Result<JointContactReport, String> {
        self.evaluate_joint_impl(candidate, true)
    }
    /// Independent full CAD evaluation for cache/fidelity audits.
    pub fn evaluate_joint_uncached(
        &self,
        candidate: &JointContactMotion,
    ) -> Result<JointContactReport, String> {
        self.evaluate_joint_impl(candidate, false)
    }
    pub fn joint_cache_statistics(&self) -> JointCacheStatistics {
        self.joint_cache.borrow().statistics
    }
    fn evaluate_joint_impl(
        &self,
        candidate: &JointContactMotion,
        cached: bool,
    ) -> Result<JointContactReport, String> {
        let resolved = candidate.resolved_force_timing()?;
        let candidate = resolved.as_ref();
        if candidate.force_templates.len() != 1 + self.recipe.additional_clocks.len() {
            return Err("one independent force-template set per operating clock required".into());
        }
        // Validate motion before constructing phase-relative force samples.
        ContactPhaseMotion::new(candidate.motion.clone())?;
        let mut clocks = vec![ContactClock {
            phase_rate: self.recipe.phase_rate,
            phase_acceleration_per_s: self.recipe.phase_acceleration_per_s,
        }];
        clocks.extend_from_slice(&self.recipe.additional_clocks);
        let prepared = candidate
            .force_templates
            .iter()
            .map(|templates| PreparedForces::new(&candidate.motion, templates))
            .collect::<Result<Vec<_>, _>>()?;
        let reports = self.joint_clock_reports(candidate, &clocks, &prepared, cached)?;
        let mut combined: Option<ContactPlanReport> = None;
        let mut inequalities = Vec::new();
        let mut maximum_cone_violation_n = 0.0_f64;
        for (report, templates) in reports.into_iter().zip(&candidate.force_templates) {
            for frame in &report.frames {
                for (i, r) in frame.wrench_residual.iter().enumerate() {
                    let tol = if i < 3 {
                        self.recipe.force_tolerance_n
                    } else {
                        self.recipe.moment_tolerance_nm
                    };
                    // Both signs retain a smooth residual through zero.
                    inequalities.extend([r / tol - 1.0, -r / tol - 1.0]);
                }
                inequalities.extend(
                    frame
                        .torque_capacity_margin_nm
                        .iter()
                        .map(|m| -m / self.recipe.torque_tolerance_nm - 1.0),
                );
                if let Some(check) = &frame.servo_command {
                    inequalities.extend_from_slice(&check.inequalities);
                }
                // Inter-link overlap must be zero, even though historical
                // diagnostic phase reports share one penetration tolerance.
                inequalities.push(
                    frame.maximum_inter_link_penetration_m / self.recipe.penetration_tolerance_m,
                );
                inequalities.push(
                    frame.maximum_floor_penetration_m / self.recipe.penetration_tolerance_m - 1.0,
                );
            }
            // Convex interpolation makes these cone checks cover the entire
            // force template, not only the collocation mesh. Other constraints
            // (dynamics, motor limits, geometry) are still sampled.
            for template in templates {
                for node in &template.keyframes {
                    let f = std::array::from_fn(|i| node.values[i]);
                    for c in point_force_cone_inequalities(f, self.art.model.world.floor_friction)?
                    {
                        maximum_cone_violation_n = maximum_cone_violation_n.max(c);
                        inequalities.push(c / self.recipe.force_tolerance_n);
                    }
                }
            }
            if let Some(r) = &mut combined {
                r.append_operating_case(report);
            } else {
                combined = Some(report);
            }
        }
        let motion_report = combined.unwrap();
        let sampled_feasible = inequalities.iter().all(|c| *c <= 0.0);
        let constraints = InequalityResiduals {
            objective: vec![motion_report.residuals[0]],
            inequalities,
        };
        Ok(JointContactReport {
            motion_report,
            constraints,
            maximum_cone_violation_n,
            sampled_feasible,
        })
    }

    fn joint_clock_reports(
        &self,
        candidate: &JointContactMotion,
        clocks: &[ContactClock],
        prepared: &[PreparedForces],
        cached: bool,
    ) -> Result<Vec<ContactPlanReport>, String> {
        if !cached {
            return clocks
                .iter()
                .zip(prepared)
                .map(|(&clock, forces)| {
                    self.evaluate_clock_with_forces(
                        &candidate.motion,
                        clock,
                        Some(forces),
                        &[],
                        None,
                    )
                })
                .collect();
        }
        // Include the complete recipe and motion, plus every force sampling
        // knot and interpolation type. Values alone cannot change kinematics.
        // The cache belongs to this planner, so its CAD model and seed cannot
        // be confused with another planner's model or initial state.
        let layouts = candidate
            .force_templates
            .iter()
            .map(|templates| {
                templates
                    .iter()
                    .map(|t| {
                        (
                            t.interpolation,
                            t.keyframes.iter().map(|k| k.time_s).collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let key = serde_json::to_vec(&(&self.recipe, &candidate.motion, layouts))
            .map_err(|e| e.to_string())?;
        let mut cache = self.joint_cache.borrow_mut();
        if let Some(entry) = cache.entry.as_ref().filter(|e| e.key == key) {
            let reports = entry
                .reports
                .iter()
                .zip(&entry.maps)
                .zip(prepared)
                .map(|((report, maps), forces)| {
                    self.reload_joint_clock(&candidate.motion, report, maps, forces)
                })
                .collect::<Result<Vec<_>, _>>()?;
            cache.statistics.hits += 1;
            return Ok(reports);
        }
        let mut reports = Vec::new();
        let mut maps = Vec::new();
        for (&clock, forces) in clocks.iter().zip(prepared) {
            let mut loads = Vec::new();
            let report = self.evaluate_clock_with_forces(
                &candidate.motion,
                clock,
                Some(forces),
                &[],
                Some(&mut loads),
            )?;
            if loads.len() != report.frames.len() {
                return Err("joint force cache frame mismatch".into());
            }
            reports.push(report);
            maps.push(loads);
        }
        cache.entry = Some(JointCacheEntry {
            key,
            reports: reports.clone(),
            maps,
        });
        cache.statistics.misses += 1;
        Ok(reports)
    }

    fn joint_frame_load_rows(&self, frame: &ContactPlanFrame) -> usize {
        12 + frame.motor_torques_nm.len()
            + if self.recipe.servo_command_limits.is_some() { 2 * frame.motor_torques_nm.len() } else { 0 }
    }

    fn reload_joint_clock(
        &self,
        motion: &ContactPhaseConfig,
        original: &ContactPlanReport,
        maps: &[(f64, PointForceLoadMap)],
        forces: &PreparedForces,
    ) -> Result<ContactPlanReport, String> {
        let mut report = original.clone();
        report.residuals.truncate(1);
        report.maximum_force_error_n = 0.0;
        report.maximum_moment_error_nm = 0.0;
        report.minimum_torque_margin_nm = f64::INFINITY;
        for (frame, (phase, map)) in report.frames.iter_mut().zip(maps) {
            let scale = frame.residual_weight.sqrt();
            frame.support_forces_world_n = forces.sample(motion, *phase)?;
            let load = map.evaluate(&frame.support_forces_world_n)?;
            frame.wrench_residual = load.wrench_residual;
            frame.motor_torques_nm = load.motor_torques_nm;
            for (i, r) in frame.wrench_residual.iter().enumerate() {
                report.residuals.push(
                    scale * r
                        / if i < 3 {
                            self.recipe.force_tolerance_n
                        } else {
                            self.recipe.moment_tolerance_nm
                        },
                );
                if i < 3 {
                    report.maximum_force_error_n = report.maximum_force_error_n.max(r.abs());
                } else {
                    report.maximum_moment_error_nm = report.maximum_moment_error_nm.max(r.abs());
                }
            }
            frame.torque_capacity_margin_nm = frame
                .motor_torques_nm
                .iter()
                .enumerate()
                .map(|(j, t)| {
                    self.motors[j].torque_capacity(frame.reduced_velocity[6 + j], *t) - t.abs()
                })
                .collect();
            for (j, margin) in frame.torque_capacity_margin_nm.iter().enumerate() {
                let violation = if self.recipe.extend_motoring_penalty_past_no_load {
                    self.motors[j].optimization_torque_violation(
                        frame.reduced_velocity[6 + j],
                        frame.motor_torques_nm[j],
                    )
                } else {
                    (-margin).max(0.0)
                };
                report
                    .residuals
                    .push(scale * violation / self.recipe.torque_tolerance_nm);
                report.minimum_torque_margin_nm = report.minimum_torque_margin_nm.min(*margin);
            }
            frame.servo_command = self.recipe.servo_command_limits.as_ref().map(|limits|
                limits.evaluate(&self.motors, &frame.coordinates, &frame.reduced_velocity[6..], &frame.motor_torques_nm)
            ).transpose()?;
            if let Some(check) = &frame.servo_command {
                report.residuals.extend(check.inequalities.iter().map(|c| scale * c.max(0.0)));
            }
            report.residuals.push(
                scale * frame.maximum_inter_link_penetration_m
                    / self.recipe.penetration_tolerance_m,
            );
            report.residuals.push(
                scale * frame.maximum_floor_penetration_m / self.recipe.penetration_tolerance_m,
            );
        }
        report.sampled_feasible = report.maximum_force_error_n <= self.recipe.force_tolerance_n
            && report.maximum_moment_error_nm <= self.recipe.moment_tolerance_nm
            && report.minimum_torque_margin_nm >= -self.recipe.torque_tolerance_nm
            && report.maximum_penetration_m <= self.recipe.penetration_tolerance_m
            && report.frames.iter().all(|f| f.servo_command.as_ref().is_none_or(|c| c.maximum_violation_rad == 0.0));
        if report.residuals.iter().any(|v| !v.is_finite()) {
            return Err("nonfinite cached contact-plan residual".into());
        }
        Ok(report)
    }

    pub fn optimize_joint(
        &self,
        initial: &JointContactMotion,
        variables: &[JointContactVariable],
        search: &AugmentedLagrangianConfig,
        progress: impl FnMut(&JointContactReport),
    ) -> Result<JointContactOptimization, String> {
        self.optimize_joint_derivatives(initial, variables, search, false, progress)
    }

    /// Opt-in hybrid derivative path; the original numerical solver remains
    /// available for independent comparisons with identical physical gates.
    pub fn optimize_joint_with_force_jacobian(
        &self,
        initial: &JointContactMotion,
        variables: &[JointContactVariable],
        search: &AugmentedLagrangianConfig,
        progress: impl FnMut(&JointContactReport),
    ) -> Result<JointContactOptimization, String> {
        self.optimize_joint_derivatives(initial, variables, search, true, progress)
    }

    fn optimize_joint_derivatives(
        &self,
        initial: &JointContactMotion,
        variables: &[JointContactVariable],
        search: &AugmentedLagrangianConfig,
        analytic_forces: bool,
        mut progress: impl FnMut(&JointContactReport),
    ) -> Result<JointContactOptimization, String> {
        self.evaluate_joint(initial)?;
        let values = joint_values(initial, variables, self.recipe.direction_world)?;
        let decode = |values: &[f64]| {
            decode_joint_values(initial, variables, self.recipe.direction_world, values)
        };
        let best: std::cell::RefCell<Option<(f64, JointContactMotion)>> = Default::default();
        let observe = std::cell::RefCell::new(
            |candidate: JointContactMotion, report: &JointContactReport| {
                let mut best = best.borrow_mut();
                if report.sampled_feasible
                    && best
                        .as_ref()
                        .is_none_or(|(speed, _)| report.motion_report.speed_m_s > *speed)
                {
                    *best = Some((report.motion_report.speed_m_s, candidate));
                }
                progress(report);
            },
        );
        let mut jacobian = |x: &[f64]| {
            let candidate = decode(x)?;
            let (report, columns) = self.linearize_joint_forces(&candidate, variables)?;
            observe.borrow_mut()(candidate, &report);
            Ok(columns)
        };
        let result = bounded_inequality_augmented_lagrangian_with_jacobian(
            &values,
            &variables
                .iter()
                .map(|v| v.bound.clone())
                .collect::<Vec<_>>(),
            search,
            None,
            |x| {
                let candidate = decode(x)?;
                let report = self.evaluate_joint(&candidate)?;
                observe.borrow_mut()(candidate, &report);
                Ok(report.constraints)
            },
            if analytic_forces {
                Some(&mut jacobian)
            } else {
                None
            },
        )?;
        let candidate = decode(&result.values)?;
        let report = self.evaluate_joint(&candidate)?;
        Ok(JointContactOptimization {
            candidate,
            report,
            search: result,
            best_sampled_feasible: best.into_inner().map(|(_, c)| c),
            force_derivative_mode: analytic_forces.then_some("CAD affine force columns; cone subgradients; numerical fallback at signed-capacity switches; motion derivatives numerical"),
            scope: "Joint body/foot/timing/per-stance-force search with shared CAD inverse dynamics and inequality augmented Lagrangian. Target-speed objective; force cones hold throughout convex templates, while balance, actuator and geometry constraints remain sampled. Multiple independently timed stances per foot; finite-difference trajectory derivatives. No global speed, runtime gait, continuous collision or browser certificate.",
        })
    }
}

fn joint_values(
    initial: &JointContactMotion,
    variables: &[JointContactVariable],
    direction: [f64; 3],
) -> Result<Vec<f64>, String> {
    let mut unique = std::collections::BTreeSet::new();
    let mut has_direction = false;
    let mut has_cartesian = false;
    let mut values = Vec::new();
    let mut scratch = initial.clone();
    for variable in variables {
        if !unique.insert(serde_json::to_string(&variable.decision).map_err(|e| e.to_string())?) {
            return Err("duplicate joint contact decision".into());
        }
        match &variable.decision {
            JointContactDecision::Motion {
                decision: ContactDecision::DisplacementAlongDirection,
            } => {
                has_direction = true;
                values.push(directional_distance(
                    scratch.motion.displacement_world_m,
                    direction,
                )?);
            }
            d => {
                has_cartesian |= matches!(
                    d,
                    JointContactDecision::Motion {
                        decision: ContactDecision::Displacement { .. }
                    }
                );
                values.push(*joint_decision(&mut scratch, d)?);
            }
        }
    }
    if has_direction && has_cartesian {
        return Err("overlapping displacement decisions".into());
    }

    Ok(values)
}
impl JointContactMotion {
    /// Apply explicit bounded initializer edits through the same decoder used by
    /// the joint optimizer, including body timestamps and contact-force timing.
    pub fn with_bounded_overrides(
        &self,
        variables: &[JointContactVariable],
        direction: [f64; 3],
        overrides: &[(usize, f64)],
    ) -> Result<Self, String> {
        let mut values = joint_values(self, variables, direction)?;
        let mut seen = std::collections::BTreeSet::new();
        for &(index, value) in overrides {
            if !seen.insert(index) {
                return Err("duplicate initializer override".into());
            }
            *values.get_mut(index).ok_or("initializer override index out of range")? = value;
        }
        for (value, variable) in values.iter().zip(variables) {
            let b = &variable.bound;
            if !value.is_finite() || !b.lower.is_finite() || !b.upper.is_finite()
                || b.lower > b.upper || *value < b.lower || *value > b.upper {
                return Err("initializer outside explicit variable bounds".into());
            }
        }
        let candidate = decode_joint_values(self, variables, direction, &values)?;
        ContactPhaseMotion::new(candidate.motion.clone())?;
        for template in &candidate.force_templates {
            PreparedForces::new(&candidate.motion, template)?;
        }
        Ok(candidate)
    }
}

fn decode_joint_values(
    initial: &JointContactMotion,
    variables: &[JointContactVariable],
    direction: [f64; 3],
    values: &[f64],
) -> Result<JointContactMotion, String> {
    if values.len() != variables.len() {
        return Err("joint variable/value dimension mismatch".into());
    }

    let mut candidate = initial.clone();
    for (value, variable) in values.iter().zip(variables) {
        if matches!(
            variable.decision,
            JointContactDecision::Motion {
                decision: ContactDecision::DisplacementAlongDirection
            }
        ) {
            candidate.motion.displacement_world_m = direction.map(|d| d * value);
        } else {
            *joint_decision(&mut candidate, &variable.decision)? = *value;
        }
    }
    let motion = &mut candidate.motion;
    let last = motion.body.keyframes.len() - 1;
    for (i, k) in motion.body.keyframes.iter_mut().enumerate() {
        k.time_s = motion.period_s * i as f64 / last as f64;
    }
    motion.body.keyframes[last].values = motion.body.keyframes[0].values.clone();
    candidate.materialized_force_timing()
}

fn joint_decision<'a>(
    candidate: &'a mut JointContactMotion,
    d: &JointContactDecision,
) -> Result<&'a mut f64, String> {
    if let JointContactDecision::Motion { decision: d } = d {
        return decision(&mut candidate.motion, d);
    }
    let ForceNode { clock, slot, node, axis, .. } = d.force_node(&candidate.motion)?
        .ok_or("force decision required")?;
    candidate.force_templates.get_mut(clock)
        .and_then(|v| v.get_mut(slot))
        .filter(|t| node > 0 && node < t.keyframes.len().saturating_sub(1))
        .and_then(|t| t.keyframes.get_mut(node))
        .and_then(|k| k.values.get_mut(axis))
        .ok_or_else(|| "force decision out of range or attempts to vary a zero endpoint".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_domain_control::{contact_phase::FootPhase, trajectory::Keyframe};
    #[test]
    fn event_aligned_linear_forces_can_partition_a_constant_load_and_refine_exactly() {
        let motion = ContactPhaseConfig {
            period_s: 1.0,
            displacement_world_m: [0.; 3],
            body: TrajectoryConfig {
                interpolation: Interpolation::PeriodicCubicBSpline,
                keyframes: (0..=4)
                    .map(|i| Keyframe {
                        time_s: i as f64 / 4.,
                        values: vec![0.; 6],
                    })
                    .collect(),
            },
            feet: [0., 0.5]
                .into_iter()
                .map(|phase_offset| FootPhase {
                    phase_offset,
                    stance_fraction: 0.65,
                    center_world_m: [0.; 3],
                    swing_offset_world_m: [0., 0., 0.1],
                    return_ramp_fraction: None,
                    additional_steps: vec![],
                })
                .collect(),
        };
        let template = TrajectoryConfig {
            interpolation: Interpolation::Linear,
            keyframes: [0., 0.05, 0.25, 0.5, 0.75, 0.95, 1.]
                .into_iter()
                .map(|time_s| Keyframe {
                    time_s,
                    values: vec![0.; 3],
                })
                .collect(),
        };
        let original = JointContactMotion {
            motion,
            force_templates: vec![vec![template.clone(), template]],
            force_timing: None,
        };
        // A valid expanded motion must never silently reuse a force curve for
        // just its first stance while evaluating the additional contacts.
        let mut expanded = original.clone();
        expanded.motion = expanded.motion.repeated_cycle(2).unwrap();
        assert!(PreparedForces::new(&expanded.motion, &expanded.force_templates[0]).is_err());
        assert!(expanded.with_event_aligned_linear_forces().is_err());
        assert!(expanded.with_contact_timed_forces().is_err());
        // A generous box even permits negative loads: failure here isolates
        // the force basis, without invoking friction or actuator restrictions.
        let samples = 512;
        let mut matrix = nalgebra::DMatrix::zeros(samples, 10);
        for foot in 0..2 {
            for node in 1..6 {
                let mut unit = original.force_templates[0].clone();
                unit[foot].keyframes[node].values[2] = 1.;
                let prepared = PreparedForces::new(&original.motion, &unit).unwrap();
                for row in 0..samples {
                    matrix[(row, foot * 5 + node - 1)] = prepared
                        .sample(&original.motion, (row as f64 + 0.5) / samples as f64)
                        .unwrap()
                        .iter()
                        .map(|f| f[2])
                        .sum();
                }
            }
        }
        let bound = sim_solve::affine_feasibility::affine_residual_lower_bound(
            &matrix,
            &nalgebra::DVector::from_element(samples, -1.),
            &vec![
                sim_solve::least_squares::VariableBound {
                    lower: -2.,
                    upper: 2.
                };
                10
            ],
        )
        .unwrap();
        assert!(bound.maximum_residual_lower_bound > 1e-3);
        let mut aligned = original.with_event_aligned_linear_forces().unwrap();
        let feet = aligned.motion.feet.clone();
        for (i, template) in aligned.force_templates[0].iter_mut().enumerate() {
            let last = template.keyframes.len() - 1;
            for node in template.keyframes.iter_mut().take(last).skip(1) {
                // Complementary linear ramps across the overlap, with unit
                // load on the sole support. Retained extra knots must sample
                // these ramps, not independently split load at each node.
                let duration = feet[i].stance_fraction;
                let local = node.time_s * duration;
                let overlap = duration - 0.5;
                node.values[2] = (local / overlap).min((duration - local) / overlap).min(1.0);
            }
        }
        let before = PreparedForces::new(&aligned.motion, &aligned.force_templates[0]).unwrap();
        let timed_binding = aligned.with_contact_timed_forces().unwrap();
        let mut shifted = timed_binding.clone();
        shifted.motion.feet[1].phase_offset = 0.47;
        shifted.motion.feet[0].stance_fraction = 0.64;
        shifted.motion.feet[1].stance_fraction = 0.69;
        let shifted = shifted.materialized_force_timing().unwrap();
        let mut wrapped = shifted.clone();
        for foot in &mut wrapped.motion.feet {
            foot.phase_offset = (foot.phase_offset + 0.9).rem_euclid(1.);
        }
        wrapped.motion.period_s *= 2.;
        for key in &mut wrapped.motion.body.keyframes {
            key.time_s *= 2.;
        }
        let wrapped = wrapped.materialized_force_timing().unwrap();
        for (a, b) in shifted
            .force_templates
            .iter()
            .flatten()
            .flat_map(|t| &t.keyframes)
            .zip(
                wrapped
                    .force_templates
                    .iter()
                    .flatten()
                    .flat_map(|t| &t.keyframes),
            )
        {
            assert!((a.time_s - b.time_s).abs() < 1e-12);
            assert_eq!(a.values, b.values);
        }
        let timed = PreparedForces::new(&shifted.motion, &shifted.force_templates[0]).unwrap();
        let fixed = PreparedForces::new(&shifted.motion, &aligned.force_templates[0]).unwrap();
        let mut timed_error = 0.0_f64;
        let mut fixed_error = 0.0_f64;
        for i in 0..=2000 {
            let phase = i as f64 / 2000.;
            timed_error = timed_error.max(
                (timed
                    .sample(&shifted.motion, phase)
                    .unwrap()
                    .iter()
                    .map(|f| f[2])
                    .sum::<f64>()
                    - 1.)
                    .abs(),
            );
            fixed_error = fixed_error.max(
                (fixed
                    .sample(&shifted.motion, phase)
                    .unwrap()
                    .iter()
                    .map(|f| f[2])
                    .sum::<f64>()
                    - 1.)
                    .abs(),
            );
            let a = before.sample(&aligned.motion, phase).unwrap();
            let b = PreparedForces::new(&timed_binding.motion, &timed_binding.force_templates[0])
                .unwrap()
                .sample(&timed_binding.motion, phase)
                .unwrap();
            for (x, y) in a.iter().flatten().zip(b.iter().flatten()) {
                assert!((x - y).abs() < 1e-12);
            }
        }
        assert!(timed_error < 1e-12);
        assert!(fixed_error > 0.01);
        for (old, new) in aligned
            .force_templates
            .iter()
            .flatten()
            .flat_map(|t| &t.keyframes)
            .zip(
                shifted
                    .force_templates
                    .iter()
                    .flatten()
                    .flat_map(|t| &t.keyframes),
            )
        {
            assert_eq!(old.values, new.values);
        }
        let mut crossed = timed_binding.clone();
        crossed.motion.feet[1].phase_offset = 0.7;
        assert!(crossed.materialized_force_timing().is_err());
        let mut coincident = timed_binding.clone();
        coincident.motion.feet[1].phase_offset = 0.;
        assert!(coincident.materialized_force_timing().is_err());
        let mut invalid = timed_binding.clone();
        invalid.force_timing.as_mut().unwrap().knots[0][0][0].fraction = 0.1;
        assert!(invalid.materialized_force_timing().is_err());
        assert!(timed_binding.with_event_aligned_linear_forces().is_err());
        assert!(original.with_contact_timed_forces().is_err());
        let encoded = serde_json::to_string(&aligned).unwrap();
        assert!(!encoded.contains("force_timing"));
        assert_eq!(
            encoded,
            serde_json::to_string(&aligned.materialized_force_timing().unwrap()).unwrap()
        );
        let replay: JointContactMotion =
            serde_json::from_str(&serde_json::to_string(&shifted).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_string(&shifted).unwrap(),
            serde_json::to_string(&replay.materialized_force_timing().unwrap()).unwrap()
        );
        eprintln!(
            "contact timing changed with fixed force coefficients: fixed-knot load error {fixed_error}, event-bound load error {timed_error}"
        );
        aligned.motion.body = Trajectory::new(aligned.motion.body.clone())
            .unwrap()
            .refined_periodic_config()
            .unwrap();
        let refined = aligned.with_event_aligned_linear_forces().unwrap();
        let after = PreparedForces::new(&refined.motion, &refined.force_templates[0]).unwrap();
        for (old, new) in aligned.force_templates[0]
            .iter()
            .zip(&refined.force_templates[0])
        {
            assert!(new.keyframes.len() > old.keyframes.len());
            for node in &old.keyframes {
                assert!(new.keyframes.iter().any(|k| k.time_s == node.time_s));
            }
        }
        let mut maximum_error = 0.0_f64;
        for i in 0..=2000 {
            let phase = i as f64 / 2000.;
            let a = before.sample(&aligned.motion, phase).unwrap();
            let b = after.sample(&refined.motion, phase).unwrap();
            maximum_error = maximum_error.max((a.iter().map(|f| f[2]).sum::<f64>() - 1.).abs());
            for (x, y) in a.iter().flatten().zip(b.iter().flatten()) {
                assert!((x - y).abs() < 1e-12);
            }
        }
        assert!(maximum_error < 1e-12);
        eprintln!(
            "constant-load force basis: original seven-node linear normalized residual lower bound {}, event-aligned linear maximum error {}",
            bound.maximum_residual_lower_bound, maximum_error
        );
    }
    #[test]
    fn forces_follow_changed_stance_duration_and_remain_zero_in_swing() {
        let mut motion = ContactPhaseConfig {
            period_s: 0.8,
            displacement_world_m: [0.0; 3],
            body: TrajectoryConfig {
                interpolation: Interpolation::Linear,
                keyframes: vec![],
            },
            feet: vec![FootPhase {
                phase_offset: 0.8,
                stance_fraction: 0.4,
                center_world_m: [0.0; 3],
                swing_offset_world_m: [0.0; 3],
                return_ramp_fraction: None,
                additional_steps: vec![],
            }],
        };
        let mut templates = vec![TrajectoryConfig {
            interpolation: Interpolation::Linear,
            keyframes: vec![
                Keyframe {
                    time_s: 0.0,
                    values: vec![0.0; 3],
                },
                Keyframe {
                    time_s: 0.5,
                    values: vec![1.0, 0.0, 10.0],
                },
                Keyframe {
                    time_s: 1.0,
                    values: vec![0.0; 3],
                },
            ],
        }];
        let a = PreparedForces::new(&motion, &templates).unwrap();
        assert!((a.sample(&motion, 0.0).unwrap()[0][2] - 10.0).abs() < 1e-12);
        assert_eq!(a.sample(&motion, 0.5).unwrap(), vec![[0.0; 3]]);
        motion.feet[0].stance_fraction = 0.6;
        let a = PreparedForces::new(&motion, &templates).unwrap();
        assert!((a.sample(&motion, 0.1).unwrap()[0][2] - 10.0).abs() < 1e-12);
        assert_eq!(a.sample(&motion, 0.5).unwrap(), vec![[0.0; 3]]);
        templates[0].keyframes[0].values[2] = 1.0;
        assert!(PreparedForces::new(&motion, &templates).is_err());
        let mut candidate = JointContactMotion {
            motion,
            force_templates: vec![templates],
            force_timing: None,
        };
        assert!(
            joint_decision(
                &mut candidate,
                &JointContactDecision::Force {
                    clock: 0,
                    foot: 0,
                    node: 0,
                    axis: 2
                }
            )
            .is_err()
        );
    }
}

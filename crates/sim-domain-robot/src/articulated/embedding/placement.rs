//! Local, bounded kinematic placement. This is an initializer/planning tool,
//! not a dynamic transition or a certificate of collision-free reachability.
use super::{EmbeddedMotion, Generalized, RigidEmbedding};
use crate::math::V;
use nalgebra::{DMatrix, DVector};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddedPoint {
    pub link: usize,
    /// Relative to the link COM, in its local frame, metres.
    pub local_point_m: [f64; 3],
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointPlaneTarget {
    pub point: EmbeddedPoint,
    /// Unit normal in world coordinates; signed distance is n.dot(p)-offset.
    pub normal_world: [f64; 3],
    pub offset_m: f64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointTarget {
    pub point: EmbeddedPoint,
    pub position_world_m: [f64; 3],
}

#[derive(Debug)]
pub struct PointPlacement {
    pub motion: EmbeddedMotion,
    pub coordinates: Vec<f64>,
    pub iterations: usize,
    pub maximum_position_error_m: f64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinateInterval {
    /// Independent-coordinate units: radians or metres, in map order.
    pub lower: f64,
    pub upper: f64,
    /// Positive trial step scale in the same units. Equal bounds fix a DOF.
    pub max_step: f64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlanePlacementConfig {
    pub tolerance_m: f64,
    /// Damping of the step-scaled task Jacobian, in metres.
    pub damping_m: f64,
    pub max_iterations: usize,
}
impl Default for PlanePlacementConfig {
    fn default() -> Self {
        Self {
            tolerance_m: 1e-7,
            damping_m: 1e-6,
            max_iterations: 40,
        }
    }
}

#[derive(Debug)]
pub struct PlanePlacement {
    pub motion: EmbeddedMotion,
    pub coordinates: Vec<f64>,
    pub iterations: usize,
    pub maximum_plane_error_m: f64,
}

impl RigidEmbedding<'_> {
    /// Fit full world-space point positions through the same bounded closure
    /// solver as plane placement. Base pose stays fixed. The configured metre
    /// tolerance bounds each point's Euclidean error, not just each axis.
    /// This is local inverse kinematics, not collision or dynamic feasibility.
    pub fn place_points(
        &self,
        seed: &Generalized,
        targets: &[PointTarget],
        bounds: &[CoordinateInterval],
        config: &PlanePlacementConfig,
    ) -> Result<PointPlacement, String> {
        let planes: Vec<_> = targets
            .iter()
            .flat_map(|t| {
                (0..3).map(move |axis| PointPlaneTarget {
                    point: t.point.clone(),
                    normal_world: std::array::from_fn(|k| if k == axis { 1.0 } else { 0.0 }),
                    offset_m: t.position_world_m[axis],
                })
            })
            .collect();
        let mut axis_config = config.clone();
        axis_config.tolerance_m /= 3.0_f64.sqrt();
        let fit = self.place_points_on_planes(seed, &planes, bounds, &axis_config)?;
        let (kin, _, _) = self.art.kinematics(&fit.motion.generalized);
        let error = targets
            .iter()
            .map(|t| {
                let k = &kin[t.point.link];
                (k.p + k.r * V::from(t.point.local_point_m) - V::from(t.position_world_m)).norm()
            })
            .fold(0.0_f64, f64::max);
        if !error.is_finite() || error > config.tolerance_m {
            return Err("point placement exceeded Euclidean tolerance".into());
        }
        Ok(PointPlacement {
            motion: fit.motion,
            coordinates: fit.coordinates,
            iterations: fit.iterations,
            maximum_position_error_m: error,
        })
    }

    /// World point positions and exact local Jacobians with respect to the
    /// independent joint coordinates. Base pose is held fixed. Uses rigid
    /// unit-velocity kinematics composed with the verified closure tangent;
    /// no finite-difference step or dynamics/contact evaluation is involved.
    pub fn point_jacobians(
        &self,
        seed: &Generalized,
        positions: &[f64],
        points: &[EmbeddedPoint],
    ) -> Result<(EmbeddedMotion, Vec<(V, DMatrix<f64>)>), String> {
        self.validate_points(points)?;
        let motion = self.solve(seed, positions, &vec![0.0; self.reduced_dimension()])?;
        let values = self.point_derivatives(&motion, points);
        Ok((motion, values))
    }

    fn validate_points(&self, points: &[EmbeddedPoint]) -> Result<(), String> {
        if points.iter().any(|p| {
            p.link >= self.art.links.len() || p.local_point_m.iter().any(|x| !x.is_finite())
        }) {
            return Err("invalid embedded point link or local coordinates".into());
        }
        Ok(())
    }

    fn point_derivatives(
        &self,
        motion: &EmbeddedMotion,
        points: &[EmbeddedPoint],
    ) -> Vec<(V, DMatrix<f64>)> {
        let n = self.independent.len();
        let (kin, _, _) = self.art.kinematics(&motion.generalized);
        let mut out: Vec<_> = points
            .iter()
            .map(|p| {
                let k = &kin[p.link];
                (k.p + k.r * V::from(p.local_point_m), DMatrix::zeros(3, n))
            })
            .collect();
        let mut probe = motion.generalized.clone();
        let zero = vec![0.0; self.full_dimension()];
        for j in 0..n {
            let velocity = motion.tangent.column(self.base_columns + j);
            self.set_motion(&mut probe, velocity.as_slice(), &zero);
            let (kin, _, _) = self.art.kinematics(&probe);
            for (point, (_, jac)) in points.iter().zip(&mut out) {
                let k = &kin[point.link];
                jac.set_column(
                    j,
                    &(k.vel + k.w.cross(&(k.r * V::from(point.local_point_m)))),
                );
            }
        }
        out
    }

    /// Fit point-to-plane distances on a nearby assembly branch, preserving
    /// the base pose and all original closure equations. Caller bounds limit
    /// independent coordinates; authored joint bounds also reject trial poses.
    /// Input state is never modified. Success is geometric only: no support
    /// loads, collision clearance, stopping distance or actuator feasibility
    /// is inferred. A rank-deficient task or a bound may prevent a local fit.
    pub fn place_points_on_planes(
        &self,
        seed: &Generalized,
        targets: &[PointPlaneTarget],
        bounds: &[CoordinateInterval],
        config: &PlanePlacementConfig,
    ) -> Result<PlanePlacement, String> {
        self.validate_seed(seed)?;
        let points: Vec<_> = targets.iter().map(|t| t.point.clone()).collect();
        self.validate_points(&points)?;
        if targets.is_empty()
            || targets.iter().any(|t| {
                !t.offset_m.is_finite()
                    || t.normal_world.iter().any(|x| !x.is_finite())
                    || (V::from(t.normal_world).norm() - 1.0).abs() > 1e-10
            })
            || bounds.len() != self.independent.len()
            || bounds.iter().any(|b| {
                !b.lower.is_finite()
                    || !b.upper.is_finite()
                    || b.lower > b.upper
                    || !b.max_step.is_finite()
                    || b.max_step <= 0.0
            })
            || !config.tolerance_m.is_finite()
            || config.tolerance_m <= 0.0
            || !config.damping_m.is_finite()
            || config.damping_m <= 0.0
            || config.max_iterations == 0
            || config.max_iterations > 10000
        {
            return Err(
                "invalid plane targets, coordinate intervals or placement configuration".into(),
            );
        }
        let within_authored_limits = |g: &Generalized| {
            self.art.dofs().enumerate().all(|(i, (_, d))| {
                d.lower.is_none_or(|v| g.q[i] >= v) && d.upper.is_none_or(|v| g.q[i] <= v)
            })
        };
        let mut q: Vec<_> = self.independent.iter().map(|i| seed.q[*i]).collect();
        if q.iter()
            .zip(bounds)
            .any(|(x, b)| *x < b.lower || *x > b.upper)
        {
            return Err("initial coordinates are outside placement intervals".into());
        }
        let mut motion = self.solve(seed, &q, &vec![0.0; self.reduced_dimension()])?;
        if !within_authored_limits(&motion.generalized) {
            return Err("initial closed pose violates authored joint limits".into());
        }
        let residual = |motion: &EmbeddedMotion| {
            let (kin, _, _) = self.art.kinematics(&motion.generalized);
            DVector::from_iterator(
                targets.len(),
                targets.iter().map(|t| {
                    let k = &kin[t.point.link];
                    V::from(t.normal_world).dot(&(k.p + k.r * V::from(t.point.local_point_m)))
                        - t.offset_m
                }),
            )
        };
        for iteration in 0..=config.max_iterations {
            let error = residual(&motion);
            if error.iter().any(|x| !x.is_finite()) {
                return Err("nonfinite point-plane residual".into());
            }
            if error.amax() <= config.tolerance_m {
                return Ok(PlanePlacement {
                    motion,
                    coordinates: q,
                    iterations: iteration,
                    maximum_plane_error_m: error.amax(),
                });
            }
            if iteration == config.max_iterations {
                return Err(format!(
                    "point-plane placement iteration limit; maximum error {} m",
                    error.amax()
                ));
            }
            if self.independent.is_empty() {
                return Err(
                    "point-plane target requires motion but no coordinates are independent".into(),
                );
            }
            let derivatives = self.point_derivatives(&motion, &points);
            let j = DMatrix::from_fn(targets.len(), q.len(), |r, c| {
                if bounds[c].lower == bounds[c].upper {
                    0.0
                } else {
                    V::from(targets[r].normal_world).dot(&derivatives[r].1.column(c))
                        * bounds[c].max_step
                }
            });
            let gram = &j * j.transpose()
                + DMatrix::identity(targets.len(), targets.len()) * config.damping_m.powi(2);
            let factor = gram.cholesky().ok_or("point-plane damped solve failed")?;
            let mut delta = -j.transpose() * factor.solve(&error);
            if delta.iter().any(|x| !x.is_finite()) {
                return Err("nonfinite placement correction".into());
            }
            delta /= delta.amax().max(1.0);
            let mut accepted = None;
            for backtrack in 0..16 {
                let scale = 0.5_f64.powi(backtrack);
                let next: Vec<_> = q
                    .iter()
                    .zip(bounds)
                    .enumerate()
                    .map(|(i, (x, b))| (x + scale * delta[i] * b.max_step).clamp(b.lower, b.upper))
                    .collect();
                if let Ok(candidate) = self.solve(
                    &motion.generalized,
                    &next,
                    &vec![0.0; self.reduced_dimension()],
                ) {
                    if within_authored_limits(&candidate.generalized)
                        && residual(&candidate).norm_squared() < error.norm_squared()
                    {
                        accepted = Some((candidate, next));
                        break;
                    }
                }
            }
            match accepted {
                Some((next, positions)) => {
                    motion = next;
                    q = positions;
                }
                None => {
                    return Err(format!(
                        "no improving bounded point-plane step; maximum error {} m; independent coordinates {:?}; active bounds {:?}",
                        error.amax(),
                        q,
                        q.iter()
                            .zip(bounds)
                            .enumerate()
                            .filter_map(|(i, (x, b))| {
                                (*x == b.lower || *x == b.upper).then_some(i)
                            })
                            .collect::<Vec<_>>()
                    ));
                }
            }
        }
        unreachable!()
    }
}

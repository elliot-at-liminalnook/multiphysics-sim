//! Conditional kinematic bounds. Rate budgets are supplied assumptions, not
//! inferred hard actuator limits; loads and acceleration feasibility are separate.
use nalgebra::{DMatrix, DVector};

/// Affine inverse-load map at one fixed CAD configuration, velocity and
/// acceleration. Reuse is valid only while those quantities and point locations
/// remain unchanged. Force decisions do not require another kinematic solve.
#[derive(Clone, Debug)]
pub struct PointForceLoadMap {
    wrench: DMatrix<f64>,
    point_jacobians: Vec<DMatrix<f64>>,
    required: Vec<f64>,
}
#[derive(Clone, Debug)]
pub struct PointForceLoads {
    pub wrench_residual: Vec<f64>,
    pub motor_torques_nm: Vec<f64>,
}
impl PointForceLoadMap {
    /// Jacobians are world xyz point derivatives per independent coordinate.
    /// Required loads are six floating-base wrench components then joint loads.
    pub fn new(
        points: &[[f64; 3]],
        reference: [f64; 3],
        point_jacobians: Vec<DMatrix<f64>>,
        required: Vec<f64>,
    ) -> Result<Self, String> {
        if required.len() < 6
            || required.iter().any(|v| !v.is_finite())
            || point_jacobians.len() != points.len()
            || point_jacobians.iter().any(|j| {
                j.nrows() != 3
                    || j.ncols() != required.len() - 6
                    || j.iter().any(|v| !v.is_finite())
            })
        {
            return Err("matched finite point Jacobians and reduced inverse loads required".into());
        }
        Ok(Self {
            wrench: point_force_wrench_matrix(points, reference)?,
            point_jacobians,
            required,
        })
    }
    pub fn wrench_jacobian(&self) -> &DMatrix<f64> {
        &self.wrench
    }
    pub fn motor_jacobian(&self) -> DMatrix<f64> {
        DMatrix::from_fn(
            self.required.len() - 6,
            self.point_jacobians.len() * 3,
            |j, k| -self.point_jacobians[k / 3][(k % 3, j)],
        )
    }
    pub fn evaluate(&self, forces: &[[f64; 3]]) -> Result<PointForceLoads, String> {
        if forces.len() != self.point_jacobians.len()
            || forces.iter().flatten().any(|v| !v.is_finite())
        {
            return Err("matched finite point forces required".into());
        }
        let actual = &self.wrench
            * DVector::from_iterator(3 * forces.len(), forces.iter().flatten().copied());
        let wrench_residual = (0..6)
            .map(|i| actual[i] - self.required[i])
            .collect::<Vec<_>>();
        let motor_torques_nm = (0..self.required.len() - 6)
            .map(|j| {
                self.required[6 + j]
                    - self
                        .point_jacobians
                        .iter()
                        .zip(forces)
                        .map(|(jac, f)| (0..3).map(|k| jac[(k, j)] * f[k]).sum::<f64>())
                        .sum::<f64>()
            })
            .collect::<Vec<_>>();
        if wrench_residual
            .iter()
            .chain(&motor_torques_nm)
            .any(|v| !v.is_finite())
        {
            return Err("nonfinite point-force inverse load".into());
        }
        Ok(PointForceLoads {
            wrench_residual,
            motor_torques_nm,
        })
    }
}

/// World point-force to world wrench map, about the supplied reference (metres).
/// Columns are xyz forces in N; rows are xyz force in N then moment in N m.
/// This is also the exact wrench Jacobian with respect to the force variables.
pub fn point_force_wrench_matrix(
    points: &[[f64; 3]],
    reference: [f64; 3],
) -> Result<DMatrix<f64>, String> {
    if points.is_empty()
        || points
            .iter()
            .flatten()
            .chain(reference.iter())
            .any(|v| !v.is_finite())
    {
        return Err("finite nonempty points and reference required".into());
    }
    let mut a = DMatrix::zeros(6, 3 * points.len());
    for (i, p) in points.iter().enumerate() {
        let r = nalgebra::Vector3::from(*p) - nalgebra::Vector3::from(reference);
        for j in 0..3 {
            a[(j, 3 * i + j)] = 1.0;
            let mut axis = nalgebra::Vector3::zeros();
            axis[j] = 1.0;
            let moment = r.cross(&axis);
            for k in 0..3 {
                a[(3 + k, 3 * i + j)] = moment[k];
            }
        }
    }
    if a.iter().any(|v| !v.is_finite()) {
        return Err("nonfinite wrench map".into());
    }
    Ok(a)
}

/// Signed unilateral and circular Coulomb-cone inequalities in N, feasible <=0.
/// Flat surface normal is explicitly world +Z. No projection hides violations.
pub fn point_force_cone_inequalities(force: [f64; 3], friction: f64) -> Result<[f64; 2], String> {
    if force.iter().any(|v| !v.is_finite()) || !friction.is_finite() || friction < 0.0 {
        return Err("finite force and nonnegative friction required".into());
    }
    let result = [-force[2], force[0].hypot(force[1]) - friction * force[2]];
    if result.iter().any(|v| !v.is_finite()) {
        return Err("nonfinite force cone residual".into());
    }
    Ok(result)
}

#[derive(Debug, serde::Serialize)]
pub struct PointForceAllocation {
    pub forces_world_n: Vec<[f64; 3]>,
    /// Actual minus required: first force (N), then moment (N m).
    pub wrench_residual: [f64; 6],
    pub scaled_wrench_residual_norm_n: f64,
    pub unilateral_friction_satisfied: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optimizer: Option<PointForceOptimizerReport>,
}
#[derive(Debug, serde::Serialize)]
pub struct PointForceOptimizerReport {
    pub iterations: usize,
    pub converged: bool,
    pub gradient_mapping_norm_n: f64,
    pub objective_n2: f64,
}
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConstrainedForceConfig {
    pub regularization: f64,
    pub maximum_iterations: usize,
    pub gradient_tolerance_n: f64,
}

/// Minimum-norm point forces for a required world wrench about `reference`.
/// This is one candidate allocation, not an inequality-constrained optimum.
/// A failed friction check does not prove that no other allocation exists.
/// Normals are explicitly world +Z; positions and the COM reference are metres.
pub fn minimum_norm_point_forces(
    points: &[[f64; 3]],
    reference: [f64; 3],
    required: [f64; 6],
    length_scale_m: f64,
    friction_coefficient: f64,
) -> Result<PointForceAllocation, String> {
    weighted_minimum_norm_point_forces(
        points,
        reference,
        required,
        length_scale_m,
        friction_coefficient,
        &vec![1.; points.len()],
    )
}

/// Weighted minimum-norm allocation: minimize sum ||f_i||²/weight_i after
/// minimizing the scaled wrench residual. Zero weight disables that point.
/// Relative weights encode prescribed support availability, not measured force
/// capacity. Unilateral/friction inequalities are checked, not optimized.
pub fn weighted_minimum_norm_point_forces(
    points: &[[f64; 3]],
    reference: [f64; 3],
    required: [f64; 6],
    length_scale_m: f64,
    friction_coefficient: f64,
    weights: &[f64],
) -> Result<PointForceAllocation, String> {
    let system = point_force_system(
        points,
        reference,
        required,
        length_scale_m,
        friction_coefficient,
        weights,
    )?;
    let svd = system.weighted.clone().svd(true, true);
    let cutoff = svd.singular_values.amax() * 1e-10;
    let mut f = svd.solve(&system.b, cutoff).map_err(str::to_owned)?;
    system.to_forces(&mut f);
    system.report(f, None)
}
struct PointForceSystem {
    a: DMatrix<f64>,
    scaled: DMatrix<f64>,
    weighted: DMatrix<f64>,
    b: DVector<f64>,
    roots: Vec<f64>,
    required: [f64; 6],
    friction: f64,
}
fn point_force_system(
    points: &[[f64; 3]],
    reference: [f64; 3],
    required: [f64; 6],
    length_scale_m: f64,
    friction_coefficient: f64,
    weights: &[f64],
) -> Result<PointForceSystem, String> {
    if points.is_empty()
        || weights.len() != points.len()
        || weights.iter().any(|w| !w.is_finite() || *w < 0.)
        || !weights.iter().any(|w| *w > 0.)
        || points
            .iter()
            .flatten()
            .chain(reference.iter())
            .chain(required.iter())
            .any(|x| !x.is_finite())
        || !length_scale_m.is_finite()
        || length_scale_m <= 0.0
        || !friction_coefficient.is_finite()
        || friction_coefficient < 0.0
    {
        return Err(
            "finite points/wrench, positive length scale, nonnegative friction and matched nonnegative weights with at least one active point required".into(),
        );
    }
    let a = point_force_wrench_matrix(points, reference)?;
    let mut scaled = a.clone();
    let mut b = DVector::from_column_slice(&required);
    for i in 3..6 {
        scaled.row_mut(i).scale_mut(1.0 / length_scale_m);
        b[i] /= length_scale_m;
    }
    let maximum_weight = weights.iter().copied().fold(0.0_f64, f64::max);
    let roots: Vec<_> = weights
        .iter()
        .map(|w| (w / maximum_weight).sqrt())
        .collect();
    let mut weighted = scaled.clone();
    for (i, root) in roots.iter().enumerate() {
        for axis in 0..3 {
            weighted.column_mut(3 * i + axis).scale_mut(*root);
        }
    }
    if weighted
        .iter()
        .chain(scaled.iter())
        .chain(b.iter())
        .any(|v| !v.is_finite())
    {
        return Err("nonfinite scaled support wrench system".into());
    }
    Ok(PointForceSystem {
        a,
        scaled,
        weighted,
        b,
        roots,
        required,
        friction: friction_coefficient,
    })
}
impl PointForceSystem {
    fn to_forces(&self, f: &mut DVector<f64>) {
        for (i, root) in self.roots.iter().enumerate() {
            for axis in 0..3 {
                f[3 * i + axis] *= root;
            }
        }
    }
    fn report(
        &self,
        f: DVector<f64>,
        optimizer: Option<PointForceOptimizerReport>,
    ) -> Result<PointForceAllocation, String> {
        let residual = &self.a * &f - DVector::from_column_slice(&self.required);
        let scaled_residual = (&self.scaled * &f - &self.b).norm();
        if f.iter().chain(residual.iter()).any(|x| !x.is_finite()) || !scaled_residual.is_finite() {
            return Err("nonfinite force allocation".into());
        }
        let forces: Vec<[f64; 3]> = f
            .as_slice()
            .chunks_exact(3)
            .map(|f| [f[0], f[1], f[2]])
            .collect();
        let friction_ok = forces
            .iter()
            .all(|f| f[2] >= -1e-10 && f[0].hypot(f[1]) <= self.friction * f[2].max(0.0) + 1e-10);
        Ok(PointForceAllocation {
            forces_world_n: forces,
            wrench_residual: std::array::from_fn(|i| residual[i]),
            scaled_wrench_residual_norm_n: scaled_residual,
            unilateral_friction_satisfied: friction_ok,
            optimizer,
        })
    }
}

/// Minimize 1/2 ||scaled wrench residual||² + regularization/2 * sum
/// ||f_i||²/(weight_i/max_weight), subject to +Z unilateral circular friction
/// cones and zero force on disabled supports. Accelerated projected gradient
/// uses a spectral step and reports a fixed-point optimality residual. A
/// converged allocation need not balance the requested wrench: residuals remain
/// explicit, and force/torque capacity limits beyond these cones are omitted.
pub fn constrained_point_forces(
    points: &[[f64; 3]],
    reference: [f64; 3],
    required: [f64; 6],
    length_scale_m: f64,
    friction_coefficient: f64,
    weights: &[f64],
    config: &ConstrainedForceConfig,
) -> Result<PointForceAllocation, String> {
    if !config.regularization.is_finite()
        || config.regularization <= 0.
        || config.maximum_iterations == 0
        || config.maximum_iterations > 100_000
        || !config.gradient_tolerance_n.is_finite()
        || config.gradient_tolerance_n <= 0.
    {
        return Err(
            "positive finite regularization/tolerance and 1..100000 force iterations required"
                .into(),
        );
    }
    let system = point_force_system(
        points,
        reference,
        required,
        length_scale_m,
        friction_coefficient,
        weights,
    )?;
    let largest = system
        .weighted
        .clone()
        .svd(false, false)
        .singular_values
        .amax();
    let lipschitz = largest * largest + config.regularization;
    if !lipschitz.is_finite() || lipschitz <= 0. {
        return Err("invalid support gradient scale".into());
    }
    let transpose = system.weighted.transpose();
    let gradient = |x: &DVector<f64>| {
        &transpose * (&system.weighted * x - &system.b) + x * config.regularization
    };
    let objective = |x: &DVector<f64>| {
        0.5 * (&system.weighted * x - &system.b).norm_squared()
            + 0.5 * config.regularization * x.norm_squared()
    };
    let project = |mut x: DVector<f64>| {
        let norm = 1.0_f64.hypot(friction_coefficient);
        let (radial, vertical) = (friction_coefficient / norm, 1. / norm);
        for i in 0..points.len() {
            let j = 3 * i;
            let radius = x[j].hypot(x[j + 1]);
            let z = x[j + 2];
            if system.roots[i] == 0. {
                x[j] = 0.;
                x[j + 1] = 0.;
                x[j + 2] = 0.;
                continue;
            }
            if z >= 0. && radius <= friction_coefficient * z {
                continue;
            }
            let along = (radial * radius + vertical * z).max(0.);
            let next_radius = radial * along;
            if radius > 0. {
                x[j] = x[j] / radius * next_radius;
                x[j + 1] = x[j + 1] / radius * next_radius;
            } else {
                x[j] = 0.;
                x[j + 1] = 0.;
            }
            x[j + 2] = vertical * along;
        }
        x
    };
    let mut x = DVector::zeros(3 * points.len());
    let mut y = x.clone();
    let mut momentum = 1.0_f64;
    let mut report = PointForceOptimizerReport {
        iterations: 0,
        converged: false,
        gradient_mapping_norm_n: f64::INFINITY,
        objective_n2: objective(&x),
    };
    for iteration in 1..=config.maximum_iterations {
        let next = project(&y - gradient(&y) / lipschitz);
        let next_objective = objective(&next);
        let mapped = project(&next - gradient(&next) / lipschitz);
        let stationarity = (&next - mapped).norm() * lipschitz;
        if !next_objective.is_finite() || !stationarity.is_finite() {
            return Err("nonfinite constrained force iteration".into());
        }
        report = PointForceOptimizerReport {
            iterations: iteration,
            converged: stationarity <= config.gradient_tolerance_n,
            gradient_mapping_norm_n: stationarity,
            objective_n2: next_objective,
        };
        if report.converged {
            x = next;
            break;
        }
        let next_momentum = (1. + (1. + 4. * momentum * momentum).sqrt()) / 2.;
        // Restart extrapolation when its objective rises; all accepted x remain
        // cone-feasible, and convergence is checked at x rather than y.
        if next_objective > objective(&x) {
            y = next.clone();
            momentum = 1.;
        } else {
            y = &next + (&next - &x) * ((momentum - 1.) / next_momentum);
            momentum = next_momentum;
        }
        x = next;
    }
    system.to_forces(&mut x);
    system.report(x, Some(report))
}

#[derive(Debug, serde::Serialize)]
pub struct DirectionalRateBound {
    /// Coordinate rates for one unit of the requested task velocity.
    pub coordinate_rates_per_unit: Vec<f64>,
    pub maximum_task_speed: f64,
    pub limiting_coordinate: usize,
}

/// Solve J q_dot = direction * speed for a unique coordinate-rate vector,
/// then maximize speed subject to |q_dot_i| <= rate_budget_i.
/// Rows and columns must have consistent physical units. The direction is not
/// normalized: its magnitude defines one unit of task speed. Redundant robots
/// need an allocation optimizer and are deliberately rejected by this API.
pub fn directional_rate_bound(
    jacobian: &DMatrix<f64>,
    direction: &DVector<f64>,
    rate_budgets: &[f64],
) -> Result<DirectionalRateBound, String> {
    let n = jacobian.ncols();
    if n == 0
        || jacobian.nrows() < n
        || direction.len() != jacobian.nrows()
        || rate_budgets.len() != n
        || jacobian
            .iter()
            .chain(direction.iter())
            .any(|v| !v.is_finite())
        || rate_budgets.iter().any(|v| !v.is_finite() || *v <= 0.0)
        || direction.norm() == 0.0
    {
        return Err("finite dimension-matched full-column-rank Jacobian, nonzero direction and positive rate budgets required".into());
    }
    let svd = jacobian.clone().svd(true, true);
    let cutoff = svd.singular_values.amax() * 1e-10;
    if svd.singular_values.amin() <= cutoff {
        return Err("singular task Jacobian: unique directional bound unavailable".into());
    }
    let rates = svd.solve(direction, cutoff).map_err(str::to_owned)?;
    if (jacobian * &rates - direction).norm() > 1e-9 * direction.norm() {
        return Err("requested direction is outside the Jacobian image".into());
    }
    let (limiting_coordinate, maximum_task_speed) = rate_budgets
        .iter()
        .enumerate()
        .map(|(i, b)| (i, b / rates[i].abs()))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    if !maximum_task_speed.is_finite() || rates.iter().any(|v| !v.is_finite()) {
        return Err("nonfinite directional bound".into());
    }
    Ok(DirectionalRateBound {
        coordinate_rates_per_unit: rates.as_slice().to_vec(),
        maximum_task_speed,
        limiting_coordinate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affine_force_load_map_matches_moments_joint_work_and_force_derivatives() {
        let points = [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]];
        let mut j0 = DMatrix::zeros(3, 2);
        j0[(2, 0)] = 0.2;
        let mut j1 = DMatrix::zeros(3, 2);
        j1[(2, 1)] = 0.4;
        let map = PointForceLoadMap::new(
            &points,
            [0.; 3],
            vec![j0, j1],
            vec![0., 0., 10., 0., 0., 0., 2., 3.],
        )
        .unwrap();
        let forces = [[0., 0., 4.], [0., 0., 6.]];
        let load = map.evaluate(&forces).unwrap();
        assert_eq!(load.wrench_residual, vec![0., 0., 0., 0., 2., 0.]);
        assert!((load.motor_torques_nm[0] - 1.2).abs() < 1e-14);
        assert!((load.motor_torques_nm[1] - 0.6).abs() < 1e-14);
        let motor_jac = map.motor_jacobian();
        for c in 0..6 {
            let mut lo = forces;
            let mut hi = forces;
            lo[c / 3][c % 3] -= 1e-5;
            hi[c / 3][c % 3] += 1e-5;
            let a = map.evaluate(&lo).unwrap();
            let b = map.evaluate(&hi).unwrap();
            for r in 0..6 {
                assert!(
                    ((b.wrench_residual[r] - a.wrench_residual[r]) / 2e-5
                        - map.wrench_jacobian()[(r, c)])
                        .abs()
                        < 1e-9
                );
            }
            for r in 0..2 {
                assert!(
                    ((b.motor_torques_nm[r] - a.motor_torques_nm[r]) / 2e-5 - motor_jac[(r, c)])
                        .abs()
                        < 1e-9
                );
            }
        }
        assert!(map.evaluate(&[[0.; 3]]).is_err());
        assert!(
            PointForceLoadMap::new(&points, [0.; 3], vec![DMatrix::zeros(2, 2); 2], vec![0.; 8])
                .is_err()
        );
    }

    #[test]
    fn force_jacobian_and_cone_residuals_are_physical() {
        let points = [[0.2, -0.3, 0.1], [-0.4, 0.5, 0.2]];
        let a = point_force_wrench_matrix(&points, [0.1, 0.1, 0.8]).unwrap();
        let f = DVector::from_vec(vec![1.0, 2.0, 10.0, -2.0, 1.0, 12.0]);
        for j in 0..f.len() {
            let mut lo = f.clone();
            let mut hi = f.clone();
            lo[j] -= 1e-5;
            hi[j] += 1e-5;
            let derivative = (&a * hi - &a * lo) / 2e-5;
            assert!((derivative - a.column(j)).amax() < 1e-9);
        }
        let moment =
            nalgebra::Vector3::from(points[0]).cross(&nalgebra::Vector3::new(0.0, 0.0, 10.0));
        let origin = point_force_wrench_matrix(&points, [0.0; 3]).unwrap()
            * DVector::from_vec(vec![0.0, 0.0, 10.0, 0.0, 0.0, 0.0]);
        assert!((origin.rows(3, 3) - moment).amax() < 1e-14);
        assert_eq!(
            point_force_cone_inequalities([3.0, 4.0, 10.0], 0.5).unwrap(),
            [-10.0, 0.0]
        );
        assert!(point_force_cone_inequalities([3.0, 4.0, 9.0], 0.5).unwrap()[1] > 0.0);
        assert!(point_force_cone_inequalities([0.0, 0.0, -1.0], 0.5).unwrap()[0] > 0.0);
        assert!(point_force_cone_inequalities([0.0; 3], -0.1).is_err());
        assert!(point_force_wrench_matrix(&[], [0.0; 3]).is_err());
    }

    #[test]
    fn joint_load_and_speed_search_reaches_analytic_limit_missed_by_equal_allocation() {
        use sim_solve::{
            inequality_augmented_lagrangian::{
                AugmentedLagrangianConfig, InequalityResiduals,
                bounded_inequality_augmented_lagrangian,
            },
            least_squares::{LeastSquaresConfig, VariableBound},
        };
        // Four unit moment-arm actuators support 10 N on a square. Opposite
        // weak legs have capacity 2-v/2, strong legs 4-v/2 (Nm). Summing capacity
        // proves v<=1. At v=1, opposite forces 1.5/3.5 attain that bound and
        // cancel moments. This is an analytic test model, not a robot speed bound.
        let points = [[-1., -1., 0.], [1., -1., 0.], [1., 1., 0.], [-1., 1., 0.]];
        let required = [0., 0., 10., 0., 0., 0.];
        let old = constrained_point_forces(
            &points,
            [0.; 3],
            required,
            1.,
            0.3,
            &[1.; 4],
            &ConstrainedForceConfig {
                regularization: 1e-12,
                maximum_iterations: 1000,
                gradient_tolerance_n: 1e-9,
            },
        )
        .unwrap();
        assert!(old.wrench_residual.iter().all(|r| r.abs() < 1e-8));
        assert!(old.forces_world_n[0][2] > 2.0); // Fails weak actuator even at v=0.
        let a = point_force_wrench_matrix(&points, [0.; 3]).unwrap();
        let result = bounded_inequality_augmented_lagrangian(
            &[0., 2.5, 2.5, 2.5, 2.5],
            &[
                VariableBound {
                    lower: 0.,
                    upper: 2.,
                },
                VariableBound {
                    lower: 0.,
                    upper: 10.,
                },
                VariableBound {
                    lower: 0.,
                    upper: 10.,
                },
                VariableBound {
                    lower: 0.,
                    upper: 10.,
                },
                VariableBound {
                    lower: 0.,
                    upper: 10.,
                },
            ],
            &AugmentedLagrangianConfig {
                maximum_outer_iterations: 30,
                maximum_evaluations: 10000,
                initial_penalty: 1.,
                maximum_penalty: 1e12,
                penalty_growth: 10.,
                required_reduction: 0.25,
                constraint_tolerance: 1e-7,
                complementarity_tolerance: 1e-6,
                scaling_exponent: 0.,
                inner: LeastSquaresConfig {
                    maximum_iterations: 40,
                    maximum_evaluations: 1000,
                    difference_step: 1e-5,
                    initial_damping: 1e-3,
                    gradient_tolerance: 1e-9,
                },
            },
            |x| {
                let forces = DVector::from_iterator(12, x[1..].iter().flat_map(|f| [0., 0., *f]));
                let wrench = &a * forces;
                let mut inequalities = Vec::new();
                for i in 0..6 {
                    let r = wrench[i] - required[i];
                    inequalities.extend([r - 1e-8, -r - 1e-8]);
                }
                for i in 0..4 {
                    let capacity = if i % 2 == 0 { 2.0 } else { 4.0 } - 0.5 * x[0];
                    inequalities.push(x[1 + i] - capacity);
                }
                Ok(InequalityResiduals {
                    objective: vec![2. - x[0]],
                    inequalities,
                })
            },
        )
        .unwrap();
        assert!(result.maximum_violation < 1e-6, "{result:?}");
        assert!((result.values[0] - 1.0).abs() < 1e-5, "{result:?}");
        for i in 0..4 {
            let expected = if i % 2 == 0 { 1.5 } else { 3.5 };
            assert!((result.values[1 + i] - expected).abs() < 1e-5);
        }
    }

    #[test]
    fn point_support_matches_static_balance_and_identifies_unsupported_moment() {
        let square = [[-1., -1., 0.], [1., -1., 0.], [1., 1., 0.], [-1., 1., 0.]];
        let r =
            minimum_norm_point_forces(&square, [0., 0., 1.], [0., 0., 40., 0., 0., 0.], 1., 0.3)
                .unwrap();
        assert!(r.scaled_wrench_residual_norm_n < 1e-12);
        assert!(r.unilateral_friction_satisfied);
        for f in r.forces_world_n {
            assert!((f[2] - 10.).abs() < 1e-12);
        }
        let line = [[-1., 0., 0.], [1., 0., 0.]];
        let r = minimum_norm_point_forces(&line, [0., 0., 0.], [0., 0., 40., 1., 0., 0.], 1., 0.3)
            .unwrap();
        assert!((r.wrench_residual[3] + 1.).abs() < 1e-12);
        let r =
            minimum_norm_point_forces(&square, [0., 0., 0.], [40., 0., 40., 0., 0., 0.], 1., 0.3)
                .unwrap();
        assert!(!r.unilateral_friction_satisfied);
    }

    #[test]
    fn weighted_support_uses_availability_and_disables_zero_weight_points() {
        let points = [[0., 0., 0.]; 3];
        let required = [0., 0., 40., 0., 0., 0.];
        let a = weighted_minimum_norm_point_forces(
            &points,
            [0.; 3],
            required,
            1.,
            0.3,
            &[1., 0.25, 0.],
        )
        .unwrap();
        assert!((a.forces_world_n[0][2] - 32.).abs() < 1e-12);
        assert!((a.forces_world_n[1][2] - 8.).abs() < 1e-12);
        assert_eq!(a.forces_world_n[2], [0.; 3]);
        assert!(a.scaled_wrench_residual_norm_n < 1e-12);
        assert!(a.unilateral_friction_satisfied);
        let b =
            weighted_minimum_norm_point_forces(&points, [0.; 3], required, 1., 0.3, &[4., 1., 0.])
                .unwrap();
        assert_eq!(a.forces_world_n, b.forces_world_n);
        for weights in [
            vec![],
            vec![0.; 3],
            vec![1., -1., 0.],
            vec![1., f64::NAN, 0.],
        ] {
            assert!(
                weighted_minimum_norm_point_forces(&points, [0.; 3], required, 1., 0.3, &weights)
                    .is_err()
            );
        }
        assert!(
            weighted_minimum_norm_point_forces(
                &points,
                [0.; 3],
                [0., 0., 40., 1., 0., 0.],
                1e-320,
                0.3,
                &[1.; 3]
            )
            .is_err()
        );
    }

    #[test]
    fn constrained_support_matches_analytic_friction_cone_projection() {
        let config = ConstrainedForceConfig {
            regularization: 0.001,
            maximum_iterations: 1000,
            gradient_tolerance_n: 1e-9,
        };
        let r = constrained_point_forces(
            &[[0.; 3]],
            [0.; 3],
            [10., 0., 1., 0., 0., 0.],
            1.,
            0.3,
            &[1.],
            &config,
        )
        .unwrap();
        let z = (0.3 * 10. + 1.) / (1. + 0.3 * 0.3) / (1. + config.regularization);
        assert!((r.forces_world_n[0][2] - z).abs() < 1e-12);
        assert!((r.forces_world_n[0][0] - 0.3 * z).abs() < 1e-12);
        assert!(r.unilateral_friction_satisfied);
        assert!(r.optimizer.unwrap().converged);
        let down = constrained_point_forces(
            &[[0.; 3]],
            [0.; 3],
            [0., 0., -10., 0., 0., 0.],
            1.,
            0.3,
            &[1.],
            &config,
        )
        .unwrap();
        assert_eq!(down.forces_world_n, vec![[0.; 3]]);
        assert!(down.optimizer.unwrap().converged);
        let zero_mu = constrained_point_forces(
            &[[0.; 3]],
            [0.; 3],
            [10., 0., 1., 0., 0., 0.],
            1.,
            0.,
            &[1.],
            &config,
        )
        .unwrap();
        assert!((zero_mu.forces_world_n[0][2] - 1. / 1.001).abs() < 1e-12);
        assert_eq!(zero_mu.forces_world_n[0][0], 0.);
    }

    #[test]
    fn constrained_support_retains_balance_regularization_and_disabled_points() {
        let config = ConstrainedForceConfig {
            regularization: 0.001,
            maximum_iterations: 5000,
            gradient_tolerance_n: 1e-9,
        };
        let points = [[-1., -1., 0.], [1., -1., 0.], [1., 1., 0.], [-1., 1., 0.]];
        let r = constrained_point_forces(
            &points,
            [0.; 3],
            [0., 0., 40., 0., 0., 0.],
            1.,
            0.3,
            &[1.; 4],
            &config,
        )
        .unwrap();
        for f in r.forces_world_n {
            assert!((f[2] - 40. / 4.001).abs() < 1e-8);
        }
        assert!(r.optimizer.unwrap().converged);
        let r = constrained_point_forces(
            &[[0.; 3]; 3],
            [0.; 3],
            [0., 0., 40., 0., 0., 0.],
            1.,
            0.3,
            &[1., 0.25, 0.],
            &config,
        )
        .unwrap();
        assert_eq!(r.forces_world_n[2], [0.; 3]);
        assert!((r.forces_world_n[0][2] - 40. / 1.251).abs() < 1e-8);
        assert!((r.forces_world_n[1][2] - 10. / 1.251).abs() < 1e-8);
        let short = ConstrainedForceConfig {
            maximum_iterations: 1,
            ..config.clone()
        };
        let r = constrained_point_forces(
            &points,
            [0.; 3],
            [2., 0., 40., 1., 2., 0.],
            1.,
            0.3,
            &[1.; 4],
            &short,
        )
        .unwrap();
        assert!(!r.optimizer.unwrap().converged);
        assert!(r.unilateral_friction_satisfied);
        let invalid = ConstrainedForceConfig {
            regularization: 0.,
            ..config
        };
        assert!(
            constrained_point_forces(&points, [0.; 3], [0.; 6], 1., 0.3, &[1.; 4], &invalid)
                .is_err()
        );
    }

    #[test]
    fn constrained_pair_transfers_load_for_moment_and_rejects_tensile_support() {
        let points = [[-1., 0., 0.], [1., 0., 0.]];
        let config = ConstrainedForceConfig {
            regularization: 0.001,
            maximum_iterations: 5000,
            gradient_tolerance_n: 1e-9,
        };
        let r = constrained_point_forces(
            &points,
            [0.; 3],
            [0., 0., 40., 0., 20., 0.],
            1.,
            0.3,
            &[1.; 2],
            &config,
        )
        .unwrap();
        assert!(r.optimizer.unwrap().converged);
        assert!((r.forces_world_n[0][2] - 60. / 2.001).abs() < 1e-8);
        assert!((r.forces_world_n[1][2] - 20. / 2.001).abs() < 1e-8);
        // Excess moment would require a negative normal force at the right
        // support. The constrained optimum unloads it and reports both errors.
        let r = constrained_point_forces(
            &points,
            [0.; 3],
            [0., 0., 40., 0., 80., 0.],
            1.,
            0.3,
            &[1.; 2],
            &config,
        )
        .unwrap();
        assert!(r.optimizer.unwrap().converged);
        assert!((r.forces_world_n[0][2] - 120. / 2.001).abs() < 1e-8);
        assert_eq!(r.forces_world_n[1], [0.; 3]);
        assert!(r.wrench_residual[2] > 19.9 && r.wrench_residual[4] < -20.);
        assert!(r.unilateral_friction_satisfied);
    }

    #[test]
    fn coupled_transmission_bound_satisfies_velocity_and_active_rate_constraint() {
        // x = 2*q0 + q1, y = q0 - q1. Unit x travel requires q0=q1=1/3.
        let j = DMatrix::from_row_slice(2, 2, &[2.0, 1.0, 1.0, -1.0]);
        let d = DVector::from_vec(vec![1.0, 0.0]);
        let result = directional_rate_bound(&j, &d, &[2.0, 1.0]).unwrap();
        assert!((result.maximum_task_speed - 3.0).abs() < 1e-12);
        assert_eq!(result.limiting_coordinate, 1);
        let qd = DVector::from_vec(result.coordinate_rates_per_unit) * result.maximum_task_speed;
        assert!((&j * qd - d * 3.0).norm() < 1e-12);
    }

    #[test]
    fn rejects_unreachable_singular_and_redundant_cases() {
        assert!(
            directional_rate_bound(
                &DMatrix::zeros(2, 2),
                &DVector::from_vec(vec![1.0, 0.0]),
                &[1.0; 2]
            )
            .is_err()
        );
        assert!(
            directional_rate_bound(
                &DMatrix::from_row_slice(2, 1, &[1.0, 0.0]),
                &DVector::from_vec(vec![0.0, 1.0]),
                &[1.0]
            )
            .is_err()
        );
        assert!(
            directional_rate_bound(
                &DMatrix::from_row_slice(1, 2, &[1.0, 1.0]),
                &DVector::from_vec(vec![1.0]),
                &[1.0; 2]
            )
            .is_err()
        );
    }
}

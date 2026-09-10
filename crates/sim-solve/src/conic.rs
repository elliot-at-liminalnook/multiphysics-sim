//! Optional convex linear optimization over affine zero, positive and Lorentz cones.
//! The returned native status is retained; infeasibility rays are not solutions.
use clarabel::{algebra::CscMatrix, solver::*};
use nalgebra::{DMatrix, DVector};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Cone {
    Zero(usize),
    Nonnegative(usize),
    SecondOrder(usize),
}
impl Cone {
    fn dimension(&self) -> usize {
        match *self {
            Self::Zero(n) | Self::Nonnegative(n) | Self::SecondOrder(n) => n,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConicConfig {
    pub maximum_iterations: u32,
    pub tolerance: f64,
}
impl Default for ConicConfig {
    fn default() -> Self {
        Self {
            maximum_iterations: 200,
            tolerance: 1e-9,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct ConicResult {
    pub status: String,
    /// Only exact native `Solved`, not `AlmostSolved` or a budget stop.
    pub solved: bool,
    /// Finite primal iterate rather than an infeasibility ray; still requires an independent audit.
    pub has_primal_candidate: bool,
    /// On infeasibility termination these may be certificate rays, not a primal solution.
    pub values: Vec<f64>,
    pub dual_values: Vec<f64>,
    pub iterations: u32,
    pub reported_objective: Option<f64>,
    pub reported_dual_objective: Option<f64>,
    pub reported_primal_residual: Option<f64>,
    pub reported_dual_residual: Option<f64>,
    pub independently_computed_objective: f64,
    pub maximum_primal_cone_violation: f64,
    pub maximum_dual_cone_violation: f64,
    pub maximum_stationarity_residual: f64,
    pub primal_dual_objective_gap: f64,
}
fn cone_violation(values: &[f64], cones: &[Cone], dual: bool) -> f64 {
    let mut offset = 0;
    let mut maximum = 0.0_f64;
    for cone in cones {
        let slice = &values[offset..offset + cone.dimension()];
        let error = match cone {
            Cone::Zero(_) if dual => 0., // dual of {0} is the full space
            Cone::Zero(_) => slice.iter().map(|x| x.abs()).fold(0., f64::max),
            Cone::Nonnegative(_) => slice.iter().map(|x| -x).fold(0., f64::max),
            Cone::SecondOrder(_) => slice[1..].iter().fold(0.0_f64, |a, x| a.hypot(*x)) - slice[0],
        };
        maximum = maximum.max(error);
        offset += cone.dimension();
    }
    maximum
}
/// Minimize q'x subject to A*x+s=b, s in the supplied product of cones.
/// Data and returned diagnostics remain in original coordinates. Internal
/// equilibration is the pinned solver default and cannot change physical gates.
pub fn solve_linear_conic(
    q: &DVector<f64>,
    a: &DMatrix<f64>,
    b: &DVector<f64>,
    cones: &[Cone],
    config: &ConicConfig,
) -> Result<ConicResult, String> {
    let rows = cones
        .iter()
        .try_fold(0usize, |n, c| n.checked_add(c.dimension()));
    if q.is_empty()
        || a.nrows() == 0
        || a.ncols() != q.len()
        || a.nrows() != b.len()
        || rows != Some(b.len())
        || cones
            .iter()
            .any(|c| c.dimension() == 0 || matches!(c,Cone::SecondOrder(n) if *n<2))
        || q.iter()
            .chain(a.iter())
            .chain(b.iter())
            .any(|x| !x.is_finite())
        || config.maximum_iterations == 0
        || !config.tolerance.is_finite()
        || config.tolerance <= 0.
    {
        return Err(
            "finite matched conic data, valid cone dimensions and positive solver limits required"
                .into(),
        );
    }
    let mut pointers = vec![0];
    let mut indices = Vec::new();
    let mut entries = Vec::new();
    for j in 0..a.ncols() {
        for i in 0..a.nrows() {
            if a[(i, j)] != 0. {
                indices.push(i);
                entries.push(a[(i, j)]);
            }
        }
        pointers.push(entries.len());
    }
    let sparse = CscMatrix::new(a.nrows(), a.ncols(), pointers, indices, entries);
    sparse
        .check_format()
        .map_err(|e| format!("invalid sparse conic matrix: {e:?}"))?;
    let native_cones = cones
        .iter()
        .map(|c| match *c {
            Cone::Zero(n) => ZeroConeT(n),
            Cone::Nonnegative(n) => NonnegativeConeT(n),
            Cone::SecondOrder(n) => SecondOrderConeT(n),
        })
        .collect::<Vec<_>>();
    let settings = DefaultSettings {
        verbose: false,
        max_iter: config.maximum_iterations,
        tol_gap_abs: config.tolerance,
        tol_gap_rel: config.tolerance,
        tol_feas: config.tolerance,
        tol_infeas_abs: config.tolerance,
        tol_infeas_rel: config.tolerance,
        ..DefaultSettings::default()
    };
    let p = CscMatrix::zeros((q.len(), q.len()));
    let mut solver = DefaultSolver::new(
        &p,
        q.as_slice(),
        &sparse,
        b.as_slice(),
        &native_cones,
        settings,
    )
    .map_err(|e| format!("conic solver setup: {e}"))?;
    solver.solve();
    let s = &solver.solution;
    if s.x.iter().chain(&s.z).any(|x| !x.is_finite()) {
        return Err(format!("nonfinite conic vectors after {:?}", s.status));
    }
    let x = DVector::from_column_slice(&s.x);
    let z = DVector::from_column_slice(&s.z);
    let slack = b - a * &x;
    let stationarity = q + a.transpose() * &z;
    if slack
        .iter()
        .chain(stationarity.iter())
        .any(|v| !v.is_finite())
    {
        return Err("nonfinite independent conic residuals".into());
    }
    if !q.dot(&x).is_finite() || !b.dot(&z).is_finite() || !(q.dot(&x) + b.dot(&z)).is_finite() {
        return Err("nonfinite independent conic objectives".into());
    }
    let finite = |v: f64| v.is_finite().then_some(v);
    Ok(ConicResult {
        status: format!("{:?}", s.status),
        solved: s.status == SolverStatus::Solved,
        has_primal_candidate: !matches!(
            s.status,
            SolverStatus::Unsolved
                | SolverStatus::PrimalInfeasible
                | SolverStatus::DualInfeasible
                | SolverStatus::AlmostPrimalInfeasible
                | SolverStatus::AlmostDualInfeasible
        ),
        values: s.x.clone(),
        dual_values: s.z.clone(),
        iterations: s.iterations,
        reported_objective: finite(s.obj_val),
        reported_dual_objective: finite(s.obj_val_dual),
        reported_primal_residual: finite(s.r_prim),
        reported_dual_residual: finite(s.r_dual),
        independently_computed_objective: q.dot(&x),
        maximum_primal_cone_violation: cone_violation(slack.as_slice(), cones, false),
        maximum_dual_cone_violation: cone_violation(z.as_slice(), cones, true),
        maximum_stationarity_residual: stationarity.amax(),
        primal_dual_objective_gap: q.dot(&x) + b.dot(&z),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn circular_support_limit_and_equality_are_solved_with_independent_dual_checks() {
        // max x on x^2+y^2<=1 with y=.6 has analytic x=.8.
        let a = DMatrix::from_row_slice(4, 2, &[0., 1., 0., 0., -1., 0., 0., -1.]);
        let r = solve_linear_conic(
            &DVector::from_vec(vec![-1., 0.]),
            &a,
            &DVector::from_vec(vec![0.6, 1., 0., 0.]),
            &[Cone::Zero(1), Cone::SecondOrder(3)],
            &ConicConfig::default(),
        )
        .unwrap();
        assert!(r.solved);
        assert!((r.values[0] - 0.8).abs() < 1e-7);
        assert!((r.values[1] - 0.6).abs() < 1e-8);
        assert!(r.maximum_primal_cone_violation < 1e-8);
        assert!(r.maximum_dual_cone_violation < 1e-8);
        assert!(r.maximum_stationarity_residual < 1e-8);
        assert!(r.primal_dual_objective_gap.abs() < 1e-8);
    }
    #[test]
    fn minimax_balance_retains_bounds_and_infeasibility_status() {
        // min t with |x-2|<=t and |x+2|<=t, -10<=x<=10: t=2, x=0.
        let a = DMatrix::from_row_slice(
            6,
            2,
            &[1., -1., -1., -1., 1., -1., -1., -1., 1., 0., -1., 0.],
        );
        let q = DVector::from_vec(vec![0., 1.]);
        let r = solve_linear_conic(
            &q,
            &a,
            &DVector::from_vec(vec![2., -2., -2., 2., 10., 10.]),
            &[Cone::Nonnegative(6)],
            &ConicConfig::default(),
        )
        .unwrap();
        assert!(r.solved);
        assert!(r.values[0].abs() < 1e-8);
        assert!((r.values[1] - 2.).abs() < 1e-8);
        let impossible = solve_linear_conic(
            &DVector::from_vec(vec![0.]),
            &DMatrix::from_row_slice(2, 1, &[1., -1.]),
            &DVector::from_vec(vec![0., -1.]),
            &[Cone::Nonnegative(2)],
            &ConicConfig::default(),
        )
        .unwrap();
        assert!(!impossible.solved);
        assert!(!impossible.has_primal_candidate);
        assert_eq!(impossible.status, "PrimalInfeasible");
        assert_eq!(impossible.reported_objective, None);
        let limited = solve_linear_conic(
            &q,
            &a,
            &DVector::from_vec(vec![2., -2., -2., 2., 10., 10.]),
            &[Cone::Nonnegative(6)],
            &ConicConfig {
                maximum_iterations: 1,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!limited.solved);
        assert!(limited.has_primal_candidate);
        assert_eq!(limited.status, "MaxIterations");
    }
    #[test]
    fn malformed_and_nonfinite_problems_reject_before_native_code() {
        let a = DMatrix::identity(1, 1);
        let b = DVector::zeros(1);
        let q = DVector::zeros(1);
        for cones in [
            vec![],
            vec![Cone::Zero(0)],
            vec![Cone::SecondOrder(1)],
            vec![Cone::Nonnegative(2)],
        ] {
            assert!(solve_linear_conic(&q, &a, &b, &cones, &Default::default()).is_err());
        }
        assert!(
            solve_linear_conic(
                &DVector::from_vec(vec![f64::NAN]),
                &a,
                &b,
                &[Cone::Nonnegative(1)],
                &Default::default()
            )
            .is_err()
        );
    }
}

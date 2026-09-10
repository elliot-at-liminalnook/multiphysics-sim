//! Bounded least-squares objectives subject to caller-scaled inequalities c<=0.
//! Powell–Hestenes–Rockafellar inequality AL, with bounded LM subproblems.
//! Finite local solves can remain infeasible; termination never proves global optimality.
use crate::least_squares::{
    LeastSquaresConfig, LeastSquaresResult, PartialJacobian, Termination, VariableBound,
    bounded_least_squares_scaled_refining_with_jacobian,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct InequalityResiduals {
    pub objective: Vec<f64>,
    /// Signed dimensionless inequalities; nonpositive is feasible.
    pub inequalities: Vec<f64>,
}
/// Resume only at a completed outer-iteration boundary. The caller must retain
/// the same objective, constraint ordering/model and variable interpretation.
/// Exact initial residual re-evaluation catches changed states/models locally;
/// it does not replace caller-owned model provenance.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AugmentedWarmStart {
    pub values: Vec<f64>,
    pub residuals: InequalityResiduals,
    pub multipliers: Vec<f64>,
    pub next_penalty: f64,
    pub previous_shifted_norm: f64,
    pub completed_outer_iterations: usize,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AugmentedLagrangianConfig {
    pub maximum_outer_iterations: usize,
    /// Includes initial/final re-evaluations and all rejected inner calls.
    pub maximum_evaluations: usize,
    pub initial_penalty: f64,
    pub maximum_penalty: f64,
    pub penalty_growth: f64,
    pub required_reduction: f64,
    pub constraint_tolerance: f64,
    pub complementarity_tolerance: f64,
    pub scaling_exponent: f64,
    pub inner: LeastSquaresConfig,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AugmentedTermination {
    StationaryWithinTolerance,
    OuterIterationLimit,
    EvaluationLimit,
    PenaltyLimit,
}
#[derive(Clone, Debug, Serialize)]
pub struct AugmentedIteration {
    pub iteration: usize,
    pub penalty: f64,
    pub objective_cost: f64,
    pub maximum_violation: f64,
    pub complementarity: f64,
    pub shifted_constraint_norm: f64,
    pub inner: LeastSquaresResult,
}
#[derive(Clone, Debug, Serialize)]
pub struct AugmentedLagrangianResult {
    pub values: Vec<f64>,
    pub residuals: InequalityResiduals,
    pub multipliers: Vec<f64>,
    pub evaluations: usize,
    pub maximum_violation: f64,
    pub within_constraint_tolerance: bool,
    pub termination: AugmentedTermination,
    pub history: Vec<AugmentedIteration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuation: Option<AugmentedWarmStart>,
}
fn valid(r: &InequalityResiduals, dimensions: (usize, usize)) -> bool {
    (r.objective.len(), r.inequalities.len()) == dimensions
        && dimensions.1 > 0
        && r.objective
            .iter()
            .chain(&r.inequalities)
            .all(|v| v.is_finite())
        && r.objective.iter().map(|v| v * v).sum::<f64>().is_finite()
}
fn violation(r: &InequalityResiduals) -> f64 {
    r.inequalities.iter().copied().fold(0., f64::max)
}
/// Minimize 0.5*||objective||² with c(x)<=0 and explicit variable bounds.
/// Each inner residual is sqrt(rho)*max(c+lambda/rho,0); omitting the
/// x-independent -||lambda||²/(2*rho) does not change its minimizer.
/// Multipliers update as max(lambda+rho*c,0). Caller tolerances are diagnostic
/// convergence tests, not permission to relax external physical acceptance.
pub fn bounded_inequality_augmented_lagrangian(
    initial: &[f64],
    bounds: &[VariableBound],
    config: &AugmentedLagrangianConfig,
    evaluate: impl FnMut(&[f64]) -> Result<InequalityResiduals, String>,
) -> Result<AugmentedLagrangianResult, String> {
    bounded_inequality_augmented_lagrangian_warm_started(initial, bounds, config, None, evaluate)
}
/// Continue the outer method with a fresh per-call evaluation budget. Inner LM
/// starts with its configured damping exactly as at every ordinary outer step.
pub fn bounded_inequality_augmented_lagrangian_warm_started(
    initial: &[f64],
    bounds: &[VariableBound],
    config: &AugmentedLagrangianConfig,
    warm_start: Option<&AugmentedWarmStart>,
    evaluate: impl FnMut(&[f64]) -> Result<InequalityResiduals, String>,
) -> Result<AugmentedLagrangianResult, String> {
    bounded_inequality_augmented_lagrangian_with_jacobian(
        initial, bounds, config, warm_start, evaluate, None,
    )
}

/// Original-unit derivatives of objective rows followed by inequality rows.
/// None columns retain numerical derivatives. Callback calls consume the same
/// evaluation budget as residual calls. At a zero shifted hinge, choose its
/// zero subgradient; the physical inequalities themselves remain unchanged.
pub type InequalityJacobianCallback<'a> = dyn FnMut(&[f64]) -> Result<PartialJacobian, String> + 'a;
pub fn bounded_inequality_augmented_lagrangian_with_jacobian(
    initial: &[f64],
    bounds: &[VariableBound],
    config: &AugmentedLagrangianConfig,
    warm_start: Option<&AugmentedWarmStart>,
    mut evaluate: impl FnMut(&[f64]) -> Result<InequalityResiduals, String>,
    mut jacobian: Option<&mut InequalityJacobianCallback<'_>>,
) -> Result<AugmentedLagrangianResult, String> {
    let c = config;
    if c.maximum_outer_iterations == 0
        || c.maximum_outer_iterations > 10000
        || c.maximum_evaluations < 3
        || c.maximum_evaluations > 10_000_000
        || !c.initial_penalty.is_finite()
        || c.initial_penalty <= 0.
        || !c.maximum_penalty.is_finite()
        || c.maximum_penalty < c.initial_penalty
        || !c.penalty_growth.is_finite()
        || c.penalty_growth <= 1.
        || !c.required_reduction.is_finite()
        || !(0.0..1.0).contains(&c.required_reduction)
        || c.required_reduction == 0.
        || !c.constraint_tolerance.is_finite()
        || c.constraint_tolerance < 0.
        || !c.complementarity_tolerance.is_finite()
        || c.complementarity_tolerance < 0.
    {
        return Err("finite positive bounded AL controls required".into());
    }
    let first = evaluate(initial)?;
    let dimensions = (first.objective.len(), first.inequalities.len());
    if !valid(&first, dimensions) {
        return Err("finite objective and nonempty signed inequalities required".into());
    }
    if warm_start.is_some_and(|w| {
        w.values != initial
            || w.residuals != first
            || w.multipliers.len() != dimensions.1
            || w.multipliers.iter().any(|v| !v.is_finite() || *v < 0.)
            || !w.next_penalty.is_finite()
            || w.next_penalty <= 0.
            || w.next_penalty > c.maximum_penalty
            || !w.previous_shifted_norm.is_finite()
            || w.previous_shifted_norm < 0.
            || w.completed_outer_iterations > 10_000_000 - c.maximum_outer_iterations
    }) {
        return Err(
            "matched initial values/residuals and finite valid AL continuation required".into(),
        );
    }
    let mut result = AugmentedLagrangianResult {
        values: initial.to_vec(),
        multipliers: warm_start.map_or_else(|| vec![0.; dimensions.1], |w| w.multipliers.clone()),
        evaluations: 1,
        maximum_violation: violation(&first),
        within_constraint_tolerance: violation(&first) <= c.constraint_tolerance,
        residuals: first,
        termination: AugmentedTermination::OuterIterationLimit,
        history: vec![],
        continuation: None,
    };
    let mut penalty = warm_start.map_or(c.initial_penalty, |w| w.next_penalty);
    let mut previous_measure =
        warm_start.map_or(result.maximum_violation, |w| w.previous_shifted_norm);
    let offset = warm_start.map_or(0, |w| w.completed_outer_iterations);
    for local_iteration in 0..c.maximum_outer_iterations {
        let iteration = offset + local_iteration;
        let remaining = c.maximum_evaluations - result.evaluations;
        if remaining < 2 {
            result.termination = AugmentedTermination::EvaluationLimit;
            break;
        }
        let mut inner_config = c.inner.clone();
        inner_config.maximum_evaluations = inner_config.maximum_evaluations.min(remaining - 1);
        let calls = std::cell::Cell::new(0);
        let use_jacobian = jacobian.is_some();
        let mut inner_jacobian = |x: &[f64], residuals: &[f64]| {
            calls.set(calls.get() + 1);
            let mut columns = jacobian.as_deref_mut().unwrap()(x)?;
            for column in columns.iter_mut().flatten() {
                if column.len() != dimensions.0 + dimensions.1 {
                    return Err("changed AL Jacobian dimensions".into());
                }
                for i in dimensions.0..column.len() {
                    column[i] *= if residuals[i] > 0.0 {
                        penalty.sqrt()
                    } else {
                        0.0
                    };
                }
            }
            Ok(columns)
        };
        let inner = bounded_least_squares_scaled_refining_with_jacobian(
            &result.values,
            bounds,
            &inner_config,
            c.scaling_exponent,
            None,
            |x| {
                calls.set(calls.get() + 1);
                let r = evaluate(x)?;
                if !valid(&r, dimensions) {
                    return Err("changed or nonfinite AL residual dimensions".into());
                }
                let mut residuals = r.objective;
                residuals.extend(
                    r.inequalities
                        .iter()
                        .zip(&result.multipliers)
                        .map(|(g, l)| penalty.sqrt() * (g + l / penalty).max(0.)),
                );
                Ok(residuals)
            },
            if use_jacobian {
                Some(&mut inner_jacobian)
            } else {
                None
            },
        )?;
        result.evaluations += calls.get();
        result.values = inner.values.clone();
        result.evaluations += 1;
        let r = evaluate(&result.values)?;
        if !valid(&r, dimensions) {
            return Err("invalid final AL re-evaluation".into());
        }
        let shifted = r
            .inequalities
            .iter()
            .zip(&result.multipliers)
            .map(|(g, l)| g.max(-l / penalty).abs())
            .fold(0., f64::max);
        for (lambda, g) in result.multipliers.iter_mut().zip(&r.inequalities) {
            *lambda = (*lambda + penalty * g).max(0.);
            if !lambda.is_finite() {
                return Err("AL multiplier overflow".into());
            }
        }
        let complementarity = result
            .multipliers
            .iter()
            .zip(&r.inequalities)
            .map(|(l, g)| (l * g).abs())
            .fold(0., f64::max);
        result.maximum_violation = violation(&r);
        result.within_constraint_tolerance = result.maximum_violation <= c.constraint_tolerance;
        let stationary = inner.termination == Termination::Stationary;
        result.history.push(AugmentedIteration {
            iteration,
            penalty,
            objective_cost: 0.5 * r.objective.iter().map(|v| v * v).sum::<f64>(),
            maximum_violation: result.maximum_violation,
            complementarity,
            shifted_constraint_norm: shifted,
            inner,
        });
        result.residuals = r;
        if stationary
            && result.within_constraint_tolerance
            && complementarity <= c.complementarity_tolerance
        {
            result.termination = AugmentedTermination::StationaryWithinTolerance;
            break;
        }
        if result.evaluations >= c.maximum_evaluations {
            result.termination = AugmentedTermination::EvaluationLimit;
            break;
        }
        if shifted > c.required_reduction * previous_measure {
            if penalty >= c.maximum_penalty {
                result.termination = AugmentedTermination::PenaltyLimit;
                break;
            }
            penalty = (penalty * c.penalty_growth).min(c.maximum_penalty);
        }
        previous_measure = shifted;
    }
    if result.termination == AugmentedTermination::OuterIterationLimit && !result.history.is_empty()
    {
        result.continuation = Some(AugmentedWarmStart {
            values: result.values.clone(),
            residuals: result.residuals.clone(),
            multipliers: result.multipliers.clone(),
            next_penalty: penalty,
            previous_shifted_norm: previous_measure,
            completed_outer_iterations: result.history.last().unwrap().iteration + 1,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn supplied_inequality_derivatives_enforce_constraint_and_count_both_callbacks() {
        let calls = std::cell::Cell::new(0);
        let mut derivative = |_: &[f64]| {
            calls.set(calls.get() + 1);
            Ok(vec![Some(vec![1., 1.])])
        };
        let result = bounded_inequality_augmented_lagrangian_with_jacobian(
            &[0.],
            &bounds(),
            &config(),
            None,
            |x| {
                calls.set(calls.get() + 1);
                Ok(InequalityResiduals {
                    objective: vec![x[0] - 2.],
                    inequalities: vec![x[0] - 1.],
                })
            },
            Some(&mut derivative),
        )
        .unwrap();
        assert!(result.within_constraint_tolerance);
        assert!((result.values[0] - 1.).abs() < 1e-6);
        assert_eq!(result.evaluations, calls.get());
    }
    fn config() -> AugmentedLagrangianConfig {
        AugmentedLagrangianConfig {
            maximum_outer_iterations: 30,
            maximum_evaluations: 20000,
            initial_penalty: 1.,
            maximum_penalty: 1e10,
            penalty_growth: 10.,
            required_reduction: 0.5,
            constraint_tolerance: 1e-7,
            complementarity_tolerance: 1e-6,
            scaling_exponent: 0.25,
            inner: LeastSquaresConfig {
                maximum_iterations: 100,
                maximum_evaluations: 1000,
                difference_step: 1e-6,
                initial_damping: 1e-3,
                gradient_tolerance: 1e-6,
            },
        }
    }
    fn bounds() -> Vec<VariableBound> {
        vec![VariableBound {
            lower: -5.,
            upper: 5.,
        }]
    }
    #[test]
    fn split_outer_solve_reproduces_uninterrupted_values_multipliers_and_steps() {
        let evaluate = |x: &[f64]| {
            Ok(InequalityResiduals {
                objective: vec![x[0] - 2.],
                inequalities: vec![x[0] - 1.],
            })
        };
        let mut c = config();
        c.inner.maximum_iterations = 1;
        c.maximum_outer_iterations = 4;
        let full = bounded_inequality_augmented_lagrangian(&[0.], &bounds(), &c, evaluate).unwrap();
        c.maximum_outer_iterations = 1;
        let first =
            bounded_inequality_augmented_lagrangian(&[0.], &bounds(), &c, evaluate).unwrap();
        let checkpoint = first.continuation.unwrap();
        let serialized = serde_json::to_string(&checkpoint).unwrap();
        let checkpoint: AugmentedWarmStart = serde_json::from_str(&serialized).unwrap();
        c.maximum_outer_iterations = 3;
        let resumed = bounded_inequality_augmented_lagrangian_warm_started(
            &checkpoint.values,
            &bounds(),
            &c,
            Some(&checkpoint),
            evaluate,
        )
        .unwrap();
        assert_eq!(full.values, resumed.values);
        assert_eq!(full.multipliers, resumed.multipliers);
        assert_eq!(
            serde_json::to_value(&full.history[1..]).unwrap(),
            serde_json::to_value(&resumed.history).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&full.continuation).unwrap(),
            serde_json::to_value(&resumed.continuation).unwrap()
        );
        assert_eq!(
            full.evaluations + 1,
            first.evaluations + resumed.evaluations
        );
        let mut bad = checkpoint.clone();
        bad.multipliers[0] = -1.;
        assert!(
            bounded_inequality_augmented_lagrangian_warm_started(
                &bad.values,
                &bounds(),
                &c,
                Some(&bad),
                evaluate
            )
            .is_err()
        );
        bad = checkpoint.clone();
        bad.residuals.inequalities[0] += 0.01;
        assert!(
            bounded_inequality_augmented_lagrangian_warm_started(
                &bad.values,
                &bounds(),
                &c,
                Some(&bad),
                evaluate
            )
            .is_err()
        );
        bad = checkpoint.clone();
        bad.values[0] += 0.01;
        assert!(
            bounded_inequality_augmented_lagrangian_warm_started(
                &checkpoint.values,
                &bounds(),
                &c,
                Some(&bad),
                evaluate
            )
            .is_err()
        );
    }
    #[test]
    fn active_inequality_recovers_constrained_minimum_and_multiplier() {
        let r = bounded_inequality_augmented_lagrangian(&[0.], &bounds(), &config(), |x| {
            Ok(InequalityResiduals {
                objective: vec![x[0] - 2.],
                inequalities: vec![x[0] - 1.],
            })
        })
        .unwrap();
        assert!((r.values[0] - 1.).abs() < 1e-6, "{r:?}");
        assert!((r.multipliers[0] - 1.).abs() < 1e-5);
        assert!(r.within_constraint_tolerance);
        assert_eq!(
            r.termination,
            AugmentedTermination::StationaryWithinTolerance
        );
    }
    #[test]
    fn inactive_constraints_leave_objective_minimum_free() {
        let r = bounded_inequality_augmented_lagrangian(&[0.], &bounds(), &config(), |x| {
            Ok(InequalityResiduals {
                objective: vec![x[0] - 2.],
                inequalities: vec![x[0] - 3., -x[0] - 3.],
            })
        })
        .unwrap();
        assert!((r.values[0] - 2.).abs() < 1e-6);
        assert_eq!(r.multipliers, vec![0., 0.]);
        assert!(r.within_constraint_tolerance);
    }
    #[test]
    fn nonlinear_disk_boundary_is_satisfied() {
        let r = bounded_inequality_augmented_lagrangian(&[0.5], &bounds(), &config(), |x| {
            Ok(InequalityResiduals {
                objective: vec![x[0] - 2.],
                inequalities: vec![x[0] * x[0] - 1.],
            })
        })
        .unwrap();
        assert!((r.values[0] - 1.).abs() < 1e-6);
        assert!(r.within_constraint_tolerance);
    }
    #[test]
    fn stationary_fixed_infeasible_point_is_never_success() {
        let mut c = config();
        c.maximum_outer_iterations = 3;
        let r = bounded_inequality_augmented_lagrangian(
            &[0.],
            &[VariableBound {
                lower: 0.,
                upper: 0.,
            }],
            &c,
            |_| {
                Ok(InequalityResiduals {
                    objective: vec![0.],
                    inequalities: vec![1.],
                })
            },
        )
        .unwrap();
        assert!(!r.within_constraint_tolerance);
        assert_ne!(
            r.termination,
            AugmentedTermination::StationaryWithinTolerance
        );
        assert_eq!(r.maximum_violation, 1.);
        assert!(r.history[1].penalty > r.history[0].penalty);
    }
    #[test]
    fn hard_evaluation_budget_counts_every_call() {
        let mut c = config();
        c.maximum_evaluations = 5;
        let mut calls = 0;
        let r = bounded_inequality_augmented_lagrangian(&[0.], &bounds(), &c, |x| {
            calls += 1;
            Ok(InequalityResiduals {
                objective: vec![x[0] - 2.],
                inequalities: vec![x[0] - 1.],
            })
        })
        .unwrap();
        assert_eq!(r.evaluations, calls);
        assert!(calls <= 5);
        assert_eq!(r.termination, AugmentedTermination::EvaluationLimit);
    }
    #[test]
    fn rejects_invalid_controls_and_nonfinite_inequalities() {
        let mut c = config();
        c.initial_penalty = 0.;
        assert!(
            bounded_inequality_augmented_lagrangian(&[0.], &bounds(), &c, |_| panic!(
                "must validate first"
            ))
            .is_err()
        );
        assert!(
            bounded_inequality_augmented_lagrangian(&[0.], &bounds(), &config(), |_| Ok(
                InequalityResiduals {
                    objective: vec![0.],
                    inequalities: vec![f64::NAN]
                }
            ))
            .is_err()
        );
    }
}

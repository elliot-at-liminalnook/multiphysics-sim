//! Bounded nonlinear least squares with scaled variables and damped Gauss–Newton.
//! Residual units/weights belong to the caller. A small gradient is a local
//! stationarity test, not proof that residual constraints are feasible.
use nalgebra::{DMatrix, DVector};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VariableBound {
    pub lower: f64,
    pub upper: f64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LeastSquaresConfig {
    pub maximum_iterations: usize,
    pub maximum_evaluations: usize,
    /// Probe in normalized [0,1] coordinates; one-sided at bounds.
    pub difference_step: f64,
    pub initial_damping: f64,
    /// Infinity norm of the projected gradient in normalized coordinates.
    pub gradient_tolerance: f64,
}
/// Retry a stalled solve with more local derivatives, retaining the same
/// objective, bounds and iteration/evaluation budgets. This is not a claim that
/// every model is differentiable; exhausting refinement still reports failure.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DerivativeRefinement {
    pub minimum_step: f64,
    pub reduction_factor: f64,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Termination {
    Stationary,
    IterationLimit,
    EvaluationLimit,
    InvalidDerivative,
    DampingLimit,
}
#[derive(Clone, Debug, Serialize)]
pub struct Iteration {
    pub iteration: usize,
    pub evaluations: usize,
    pub cost: f64,
    pub projected_gradient: f64,
    pub damping: f64,
    pub accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub difference_step: Option<f64>,
}
#[derive(Clone, Debug, Serialize)]
pub struct LeastSquaresResult {
    pub values: Vec<f64>,
    pub residuals: Vec<f64>,
    pub initial_cost: f64,
    pub cost: f64,
    pub evaluations: usize,
    pub rejected_evaluations: usize,
    pub termination: Termination,
    pub history: Vec<Iteration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derivative_refinements: Option<usize>,
}

/// Minimize 0.5*sum(residual²) inside explicit finite bounds. Equal bounds fix a
/// variable. Invalid model evaluations reject a trial or shorten a derivative
/// probe; they never substitute a fabricated residual. The initial point must
/// evaluate successfully. Trial acceptance uses actual cost reduction and a
/// predicted reduction computed from the *bounded* displacement.
pub fn bounded_least_squares(
    initial: &[f64],
    bounds: &[VariableBound],
    config: &LeastSquaresConfig,
    evaluate: impl FnMut(&[f64]) -> Result<Vec<f64>, String>,
) -> Result<LeastSquaresResult, String> {
    bounded_least_squares_scaled(initial, bounds, config, 0.0, evaluate)
}

/// Optional diagonal Hessian scaling, D_ii=H_ii^(-exponent). The normalized
/// bounded variables, residuals and acceptance checks are unchanged. Exponent
/// 0 preserves the original arithmetic; 1/4 follows IDTO's scaling choice and
/// 1/2 is Jacobi scaling. This remains damped Gauss-Newton, not constrained dogleg.
pub fn bounded_least_squares_scaled(
    initial: &[f64],
    bounds: &[VariableBound],
    config: &LeastSquaresConfig,
    exponent: f64,
    evaluate: impl FnMut(&[f64]) -> Result<Vec<f64>, String>,
) -> Result<LeastSquaresResult, String> {
    bounded_least_squares_scaled_refining(initial, bounds, config, exponent, None, evaluate)
}

pub fn bounded_least_squares_scaled_refining(
    initial: &[f64],
    bounds: &[VariableBound],
    config: &LeastSquaresConfig,
    exponent: f64,
    refinement: Option<&DerivativeRefinement>,
    evaluate: impl FnMut(&[f64]) -> Result<Vec<f64>, String>,
) -> Result<LeastSquaresResult, String> {
    bounded_least_squares_scaled_refining_with_jacobian(
        initial, bounds, config, exponent, refinement, evaluate, None,
    )
}

/// Columns in original variable units; None requests the ordinary bounded
/// finite difference for that variable. Rows follow the residual ordering.
pub type PartialJacobian = Vec<Option<Vec<f64>>>;
pub type JacobianCallback<'a> = dyn FnMut(&[f64], &[f64]) -> Result<PartialJacobian, String> + 'a;

/// Mixed supplied/numerical derivatives. Each callback invocation consumes one
/// evaluation from the same budget, including failed derivatives. Supplied
/// columns are scaled to normalized coordinates here. Trials still require
/// ordinary residual evaluation and actual cost reduction.
pub fn bounded_least_squares_scaled_refining_with_jacobian(
    initial: &[f64],
    bounds: &[VariableBound],
    config: &LeastSquaresConfig,
    exponent: f64,
    refinement: Option<&DerivativeRefinement>,
    mut evaluate: impl FnMut(&[f64]) -> Result<Vec<f64>, String>,
    mut jacobian: Option<&mut JacobianCallback<'_>>,
) -> Result<LeastSquaresResult, String> {
    if refinement.is_some_and(|r| {
        !r.minimum_step.is_finite()
            || r.minimum_step <= 0.
            || r.minimum_step > config.difference_step
            || !r.reduction_factor.is_finite()
            || r.reduction_factor <= 0.
            || r.reduction_factor >= 1.
    }) {
        return Err(
            "finite positive derivative floor and reduction factor in (0,1) required".into(),
        );
    }
    if !exponent.is_finite()
        || !(0.0..=0.5).contains(&exponent)
        || initial.is_empty()
        || bounds.len() != initial.len()
        || initial.iter().zip(bounds).any(|(x, b)| {
            !x.is_finite()
                || !b.lower.is_finite()
                || !b.upper.is_finite()
                || b.upper < b.lower
                || !(b.upper - b.lower).is_finite()
                || *x < b.lower
                || *x > b.upper
        })
        || config.maximum_iterations == 0
        || config.maximum_iterations > 100_000
        || config.maximum_evaluations == 0
        || config.maximum_evaluations > 10_000_000
        || !config.difference_step.is_finite()
        || !(0.0..=0.1).contains(&config.difference_step)
        || config.difference_step == 0.0
        || !config.initial_damping.is_finite()
        || config.initial_damping <= 0.0
        || !config.gradient_tolerance.is_finite()
        || config.gradient_tolerance <= 0.0
    {
        return Err(
            "finite matched bounds/initial values and positive bounded search controls required"
                .into(),
        );
    }
    let active: Vec<_> = bounds
        .iter()
        .enumerate()
        .filter(|(_, b)| b.upper > b.lower)
        .map(|(i, _)| i)
        .collect();
    let decode = |z: &DVector<f64>| {
        let mut x = initial.to_vec();
        for (&i, &v) in active.iter().zip(z.iter()) {
            x[i] = (bounds[i].lower + v * (bounds[i].upper - bounds[i].lower))
                .clamp(bounds[i].lower, bounds[i].upper);
        }
        x
    };
    let mut x = DVector::from_iterator(
        active.len(),
        active
            .iter()
            .map(|&i| (initial[i] - bounds[i].lower) / (bounds[i].upper - bounds[i].lower)),
    );
    let initial_r = evaluate(initial)?;
    let m = initial_r.len();
    let valid = |r: &[f64]| {
        r.len() == m
            && m > 0
            && r.iter().all(|v| v.is_finite())
            && r.iter().map(|v| v * v).sum::<f64>().is_finite()
    };
    if !valid(&initial_r) {
        return Err("nonempty finite residual vector and finite squared norm required".into());
    }
    let mut r = DVector::from_vec(initial_r);
    let mut cost = 0.5 * r.norm_squared();
    let mut result = LeastSquaresResult {
        values: initial.to_vec(),
        residuals: r.as_slice().to_vec(),
        initial_cost: cost,
        cost,
        evaluations: 1,
        rejected_evaluations: 0,
        termination: Termination::IterationLimit,
        history: vec![],
        derivative_refinements: refinement.map(|_| 0),
    };
    let mut damping = config.initial_damping;
    let mut difference_step = config.difference_step;
    'iterations: for iteration in 0..config.maximum_iterations {
        let mut jac = DMatrix::zeros(m, active.len());
        let supplied = if let Some(callback) = jacobian.as_deref_mut() {
            if result.evaluations >= config.maximum_evaluations {
                result.termination = Termination::EvaluationLimit;
                break;
            }
            result.evaluations += 1;
            match callback(&result.values, r.as_slice()) {
                Ok(columns)
                    if columns.len() == initial.len()
                        && columns
                            .iter()
                            .flatten()
                            .all(|v| v.len() == m && v.iter().all(|x| x.is_finite())) =>
                {
                    Some(columns)
                }
                _ => {
                    result.rejected_evaluations += 1;
                    result.termination = Termination::InvalidDerivative;
                    break;
                }
            }
        } else {
            None
        };
        for j in 0..active.len() {
            if let Some(column) = supplied.as_ref().and_then(|c| c[active[j]].as_ref()) {
                let width = bounds[active[j]].upper - bounds[active[j]].lower;
                let column = DVector::from_iterator(m, column.iter().map(|v| v * width));
                if column.iter().any(|v| !v.is_finite()) {
                    result.termination = Termination::InvalidDerivative;
                    break 'iterations;
                }
                jac.set_column(j, &column);
                continue;
            }
            let mut sides = [None, None];
            for (slot, sign) in [-1.0, 1.0].into_iter().enumerate() {
                let mut h = difference_step;
                for _ in 0..12 {
                    let mut probe = x.clone();
                    probe[j] = (x[j] + sign * h).clamp(0.0, 1.0);
                    let actual = probe[j] - x[j];
                    if actual.abs() < 1e-14 {
                        break;
                    }
                    if result.evaluations >= config.maximum_evaluations {
                        result.termination = Termination::EvaluationLimit;
                        break 'iterations;
                    }
                    result.evaluations += 1;
                    match evaluate(&decode(&probe)) {
                        Ok(v) if valid(&v) => {
                            sides[slot] = Some((actual, DVector::from_vec(v)));
                            break;
                        }
                        _ => {
                            result.rejected_evaluations += 1;
                            h *= 0.5;
                        }
                    }
                }
            }
            let column = match (&sides[0], &sides[1]) {
                (Some((a, ra)), Some((b, rb))) => (rb - ra) / (b - a),
                (Some((h, v)), None) | (None, Some((h, v))) => (v - &r) / *h,
                (None, None) => {
                    result.termination = Termination::InvalidDerivative;
                    break 'iterations;
                }
            };
            if column.iter().any(|v| !v.is_finite()) {
                result.termination = Termination::InvalidDerivative;
                break 'iterations;
            }
            jac.set_column(j, &column);
        }
        let gradient = jac.transpose() * &r;
        let projected = gradient
            .iter()
            .enumerate()
            .map(|(j, g)| {
                if (x[j] <= 1e-14 && *g > 0.0) || (x[j] >= 1.0 - 1e-14 && *g < 0.0) {
                    0.0
                } else {
                    g.abs()
                }
            })
            .fold(0.0_f64, f64::max);
        if !projected.is_finite() {
            result.termination = Termination::InvalidDerivative;
            break;
        }
        if projected <= config.gradient_tolerance {
            result.history.push(Iteration {
                iteration,
                evaluations: result.evaluations,
                cost,
                projected_gradient: projected,
                damping,
                accepted: false,
                difference_step: refinement.map(|_| difference_step),
            });
            result.termination = Termination::Stationary;
            break;
        }
        let normal = jac.transpose() * &jac;
        let scaling = (exponent != 0.0).then(|| {
            DVector::from_iterator(
                active.len(),
                (0..active.len()).map(|j| normal[(j, j)].max(1e-24).powf(-exponent)),
            )
        });
        let mut accepted = false;
        for _ in 0..24 {
            if result.evaluations >= config.maximum_evaluations {
                result.termination = Termination::EvaluationLimit;
                break 'iterations;
            }
            let mut matrix = normal.clone();
            if let Some(d) = &scaling {
                for i in 0..active.len() {
                    for j in 0..active.len() {
                        matrix[(i, j)] *= d[i] * d[j];
                    }
                }
            }
            for j in 0..active.len() {
                matrix[(j, j)] += damping;
            }
            let Some(factor) = matrix.cholesky() else {
                damping *= 10.0;
                continue;
            };
            let delta = if let Some(d) = &scaling {
                factor.solve(&(-gradient.component_mul(d))).component_mul(d)
            } else {
                factor.solve(&(-&gradient))
            };
            let trial = (&x + delta).map(|v| v.clamp(0.0, 1.0));
            let displacement = &trial - &x;
            let prediction =
                -gradient.dot(&displacement) - 0.5 * displacement.dot(&(&normal * &displacement));
            if !prediction.is_finite() || prediction <= 0.0 {
                damping *= 10.0;
                continue;
            }
            result.evaluations += 1;
            let trial_values = decode(&trial);
            match evaluate(&trial_values) {
                Ok(values) if valid(&values) => {
                    let next = DVector::from_vec(values);
                    let next_cost = 0.5 * next.norm_squared();
                    let ratio = (cost - next_cost) / prediction;
                    if next_cost < cost && ratio > 1e-4 {
                        x = trial;
                        result.values = trial_values;
                        r = next;
                        cost = next_cost;
                        accepted = true;
                        if ratio > 0.75 {
                            damping = (damping / 3.0).max(1e-14);
                        } else if ratio < 0.25 {
                            damping *= 3.0;
                        }
                        break;
                    }
                }
                _ => result.rejected_evaluations += 1,
            }
            damping *= 10.0;
            if !damping.is_finite() || damping > 1e30 {
                break;
            }
        }
        result.history.push(Iteration {
            iteration,
            evaluations: result.evaluations,
            cost,
            projected_gradient: projected,
            damping,
            accepted,
            difference_step: refinement.map(|_| difference_step),
        });
        if !accepted {
            if let Some(policy) = refinement {
                if difference_step > policy.minimum_step
                    && iteration + 1 < config.maximum_iterations
                {
                    difference_step =
                        (difference_step * policy.reduction_factor).max(policy.minimum_step);
                    damping = config.initial_damping;
                    *result.derivative_refinements.as_mut().unwrap() += 1;
                    continue;
                }
            }
            result.termination = Termination::DampingLimit;
            break;
        }
    }
    // Preserve the exact evaluated values, including an untouched initial point.
    // Normalization followed by decoding can change its last bits even when no
    // trial was accepted, making the returned values disagree with the residuals.
    result.residuals = r.as_slice().to_vec();
    result.cost = cost;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mixed_jacobian_scales_columns_preserves_fixed_variables_and_counts_calls() {
        let bounds = [
            VariableBound {
                lower: -100.,
                upper: 100.,
            },
            VariableBound {
                lower: 0.,
                upper: 1.,
            },
            VariableBound {
                lower: 7.,
                upper: 7.,
            },
        ];
        let evaluations = std::cell::Cell::new(0);
        let derivatives = std::cell::Cell::new(0);
        let mut jacobian = |x: &[f64], _: &[f64]| {
            assert_eq!(x[2], 7.);
            derivatives.set(derivatives.get() + 1);
            Ok(vec![Some(vec![1., 0.]), None, None])
        };
        let result = bounded_least_squares_scaled_refining_with_jacobian(
            &[0., 0.8, 7.],
            &bounds,
            &config(),
            0.25,
            None,
            |x| {
                evaluations.set(evaluations.get() + 1);
                Ok(vec![x[0] - 2., x[1] - 0.25])
            },
            Some(&mut jacobian),
        )
        .unwrap();
        assert!((result.values[0] - 2.).abs() < 1e-6);
        assert!((result.values[1] - 0.25).abs() < 1e-6);
        assert_eq!(result.values[2], 7.);
        assert_eq!(result.evaluations, evaluations.get() + derivatives.get());
        let mut controls = config();
        controls.maximum_evaluations = 2;
        let exhausted = bounded_least_squares_scaled_refining_with_jacobian(
            &[0., 0.8, 7.],
            &bounds,
            &controls,
            0.,
            None,
            |_| Ok(vec![1., 2.]),
            Some(&mut jacobian),
        )
        .unwrap();
        assert_eq!(exhausted.termination, Termination::EvaluationLimit);
        assert_eq!(exhausted.evaluations, 2);
        let mut bad = |_: &[f64], _: &[f64]| Ok(vec![Some(vec![f64::NAN]); 3]);
        let invalid = bounded_least_squares_scaled_refining_with_jacobian(
            &[0., 0.8, 7.],
            &bounds,
            &config(),
            0.,
            None,
            |_| Ok(vec![1., 2.]),
            Some(&mut bad),
        )
        .unwrap();
        assert_eq!(invalid.termination, Termination::InvalidDerivative);
        assert_eq!(invalid.values, vec![0., 0.8, 7.]);
    }
    fn config() -> LeastSquaresConfig {
        LeastSquaresConfig {
            maximum_iterations: 100,
            maximum_evaluations: 2000,
            difference_step: 1e-5,
            initial_damping: 1e-3,
            gradient_tolerance: 1e-7,
        }
    }
    #[test]
    fn derivative_refinement_recovers_from_an_aliased_cost_gradient() {
        // The residual vector rotates rapidly; its exact cost is .5*(1+x)^2.
        // A wide central difference aliases that rotation and reverses g's sign.
        let objective = |x: &[f64]| {
            Ok(vec![
                (1. + x[0]) * (2000. * x[0]).cos(),
                (1. + x[0]) * (2000. * x[0]).sin(),
            ])
        };
        let bounds = [VariableBound {
            lower: -0.5,
            upper: 0.5,
        }];
        let mut search = config();
        search.difference_step = 1e-3;
        search.maximum_iterations = 8;
        let stalled =
            bounded_least_squares_scaled(&[0.], &bounds, &search, 0.25, objective).unwrap();
        assert_eq!(stalled.termination, Termination::DampingLimit);
        assert_eq!(stalled.values, vec![0.]);
        let policy = DerivativeRefinement {
            minimum_step: 1e-6,
            reduction_factor: 0.1,
        };
        let result = bounded_least_squares_scaled_refining(
            &[0.],
            &bounds,
            &search,
            0.25,
            Some(&policy),
            objective,
        )
        .unwrap();
        assert!(result.derivative_refinements.unwrap() > 0);
        assert!(result.values[0] < -1e-7 && result.cost < result.initial_cost - 1e-7);
        assert!(result.values[0] >= bounds[0].lower && result.values[0] <= bounds[0].upper);
        assert!(result.evaluations <= search.maximum_evaluations);
        assert!(
            result
                .history
                .iter()
                .any(|i| i.difference_step.unwrap() < 1e-3)
        );
        assert!(result.history.windows(2).all(|w| w[1].cost <= w[0].cost));
    }
    #[test]
    fn hessian_scaling_resolves_independent_stiff_and_soft_directions() {
        let bounds = vec![
            VariableBound {
                lower: 0.0,
                upper: 1.0
            };
            2
        ];
        let mut search = config();
        search.gradient_tolerance = 1e-14;
        for exponent in [0.25, 0.5] {
            let result =
                bounded_least_squares_scaled(&[0.5, 0.5], &bounds, &search, exponent, |x| {
                    Ok(vec![1e8 * (x[0] - 0.25), 1e-4 * (x[1] - 0.75)])
                })
                .unwrap();
            assert!((result.values[0] - 0.25).abs() < 1e-10);
            assert!(
                (result.values[1] - 0.75).abs() < 1e-5,
                "{exponent}: {:?}",
                result.values
            );
            assert!(result.history.windows(2).all(|w| w[1].cost <= w[0].cost));
        }
        for exponent in [-0.1, 0.6, f64::NAN] {
            assert!(
                bounded_least_squares_scaled(&[0.5, 0.5], &bounds, &search, exponent, |_| panic!(
                    "invalid scaling must be rejected before evaluation"
                ))
                .is_err()
            );
        }
    }
    #[test]
    fn coupled_nonlinear_fit_and_unit_scaling() {
        for scale in [1.0, 1e6] {
            let b = [
                VariableBound {
                    lower: -2.0 * scale,
                    upper: 2.0 * scale,
                },
                VariableBound {
                    lower: -1.0,
                    upper: 3.0,
                },
            ];
            let r = bounded_least_squares(&[-1.2 * scale, 1.0], &b, &config(), |x| {
                Ok(vec![
                    10.0 * (x[1] - (x[0] / scale).powi(2)),
                    1.0 - x[0] / scale,
                ])
            })
            .unwrap();
            assert_eq!(r.termination, Termination::Stationary);
            assert!(r.cost < 1e-12);
            assert!((r.values[0] / scale - 1.0).abs() < 1e-6);
            assert!(r.history.windows(2).all(|w| w[1].cost <= w[0].cost));
        }
    }
    #[test]
    fn active_and_fixed_bounds_do_not_imply_residual_feasibility() {
        let b = [
            VariableBound {
                lower: 0.0,
                upper: 1.0,
            },
            VariableBound {
                lower: 7.0,
                upper: 7.0,
            },
        ];
        let r = bounded_least_squares(&[0.2, 7.0], &b, &config(), |x| {
            assert_eq!(x[1], 7.0);
            Ok(vec![x[0] - 2.0])
        })
        .unwrap();
        assert_eq!(r.termination, Termination::Stationary);
        assert_eq!(r.values, vec![1.0, 7.0]);
        assert_eq!(r.cost, 0.5);
    }
    #[test]
    fn rejects_invalid_trials_and_honors_evaluation_budget() {
        let b = [VariableBound {
            lower: 0.0,
            upper: 2.0,
        }];
        let mut c = config();
        c.maximum_evaluations = 4;
        let r = bounded_least_squares(&[0.5], &b, &c, |x| {
            if x[0] > 0.6 {
                Err("domain".into())
            } else {
                Ok(vec![x[0] - 1.0])
            }
        })
        .unwrap();
        assert_eq!(r.termination, Termination::EvaluationLimit);
        assert_eq!(r.evaluations, 4);
        assert_eq!(r.rejected_evaluations, 1);
        assert_eq!(r.values, vec![0.5]);
        c.maximum_evaluations = 100;
        assert!(bounded_least_squares(&[0.5], &b, &c, |_| Ok(vec![f64::NAN])).is_err());
    }
    #[test]
    fn unmodified_initial_value_keeps_exact_residual_pair() {
        let mut c = config();
        c.maximum_evaluations = 1;
        let initial = 0.1_f64;
        let r = bounded_least_squares(
            &[initial],
            &[VariableBound {
                lower: -2.0,
                upper: 2.0,
            }],
            &c,
            |x| Ok(vec![x[0]]),
        )
        .unwrap();
        assert_eq!(r.values[0].to_bits(), initial.to_bits());
        assert_eq!(r.residuals[0].to_bits(), r.values[0].to_bits());
        assert_eq!(r.evaluations, 1);
    }
}

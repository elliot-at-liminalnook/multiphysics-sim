//! Dense equality-constrained Gauss–Newton dogleg, following IDTO equations
//! 16–20. Bounds shorten a trial; they do not silently relax equalities.
//! This is a local solver, with explicit rank/linear-solve and budget failures.
use crate::least_squares::VariableBound;
use nalgebra::{DMatrix, DVector};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EqualityGlobalization {
    #[default]
    LagrangianDogleg,
    /// Radially shorten the constrained Newton direction; choose an exact L1
    /// merit weight exceeding the multiplier norm. Linear feasibility progress
    /// is retained when the full direction is shortened for trust or bounds.
    ExactPenaltyNewton,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EqualityDoglegConfig {
    #[serde(default)]
    pub globalization: EqualityGlobalization,
    pub maximum_iterations: usize,
    pub maximum_evaluations: usize,
    pub difference_step: f64,
    pub initial_radius: f64,
    pub maximum_radius: f64,
    pub minimum_radius: f64,
    pub hessian_regularization: f64,
    pub scaling_exponent: f64,
    pub stationarity_tolerance: f64,
    pub equality_tolerance: f64,
    pub linear_tolerance: f64,
}
#[derive(Clone, Debug)]
pub struct EqualityResiduals {
    pub objective: Vec<f64>,
    pub equalities: Vec<f64>,
}
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EqualityTermination {
    Converged,
    IterationLimit,
    EvaluationLimit,
    InvalidDerivative,
    LinearConstraintFailure,
    RadiusLimit,
    BoundStall,
}
#[derive(Debug, Serialize)]
pub struct EqualityIteration {
    pub iteration: usize,
    pub evaluations: usize,
    pub objective_cost: f64,
    pub equality_inf: f64,
    pub merit_gradient_inf: f64,
    pub radius: f64,
    pub accepted: bool,
    pub trust_ratio: Option<f64>,
    pub full_step_linear_error: f64,
    pub full_step_norm: f64,
    pub linear_diagnostics: EqualityLinearDiagnostics,
    pub schur_rank: usize,
    pub bound_fraction: f64,
    pub roundoff_acceptance: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_penalty_weight: Option<f64>,
}
#[derive(Debug, Serialize)]
pub struct EqualityDoglegResult {
    pub values: Vec<f64>,
    pub objective_cost: f64,
    pub equality_inf: f64,
    pub initial_objective_cost: f64,
    pub initial_equality_inf: f64,
    pub evaluations: usize,
    pub rejected_evaluations: usize,
    pub termination: EqualityTermination,
    pub history: Vec<EqualityIteration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linear_failure: Option<EqualityLinearFailure>,
}
#[derive(Debug, Serialize)]
pub struct EqualityLinearFailure {
    pub reason: &'static str,
    pub schur_rank: Option<usize>,
    pub relative_kkt_error: Option<f64>,
    pub diagnostics: Option<EqualityLinearDiagnostics>,
}
#[derive(Clone, Debug, Serialize)]
pub struct EqualityLinearDiagnostics {
    pub initial_relative_kkt_error: f64,
    pub relative_constraint_error: f64,
    pub relative_stationarity_error: f64,
    pub refinement_steps: usize,
}
impl EqualityLinearDiagnostics {
    fn error(&self) -> f64 {
        if self.relative_constraint_error.is_finite()
            && self.relative_stationarity_error.is_finite()
        {
            self.relative_constraint_error
                .max(self.relative_stationarity_error)
        } else {
            f64::INFINITY
        }
    }
}
fn inf(x: &DVector<f64>) -> f64 {
    x.iter()
        .map(|v| {
            if v.is_finite() {
                v.abs()
            } else {
                f64::INFINITY
            }
        })
        .fold(0., f64::max)
}

/// Solve the linear KKT equations by eliminating an SPD Hessian. A rank-
/// deficient Schur system is accepted only when its original equations close.
fn newton_step(
    h: &DMatrix<f64>,
    g: &DVector<f64>,
    a: &DMatrix<f64>,
    c: &DVector<f64>,
    tolerance: f64,
) -> Result<(DVector<f64>, DVector<f64>, usize, EqualityLinearDiagnostics), EqualityLinearFailure> {
    let failure = |reason, rank| EqualityLinearFailure {
        reason,
        schur_rank: rank,
        relative_kkt_error: None,
        diagnostics: None,
    };
    let factor = h
        .clone()
        .cholesky()
        .ok_or_else(|| failure("Hessian factorization failed", None))?;
    let hi_g = factor.solve(g);
    let hi_at = factor.solve(&a.transpose());
    let schur = a * &hi_at;
    let rhs = c - a * &hi_g;
    let check = |p: &DVector<f64>, lambda: &DVector<f64>| {
        let relative_constraint_error = inf(&(a * p + c)) / (1. + inf(c));
        let relative_stationarity_error = inf(&(h * p + a.transpose() * lambda + g))
            / (1. + inf(g) + inf(&(a.transpose() * lambda)));
        EqualityLinearDiagnostics {
            initial_relative_kkt_error: relative_constraint_error.max(relative_stationarity_error),
            relative_constraint_error,
            relative_stationarity_error,
            refinement_steps: 0,
        }
    };
    if let Some(f) = schur.clone().cholesky() {
        let lambda = f.solve(&rhs);
        let p = -&hi_g - &hi_at * &lambda;
        let diagnostic = check(&p, &lambda);
        if diagnostic.error().is_finite() && diagnostic.error() <= tolerance {
            return Ok((p, lambda, a.nrows(), diagnostic));
        }
    }
    // B=A L^{-T} avoids squaring the constraint matrix condition number.
    let lower = factor.l();
    let upper = lower.transpose();
    let b = lower
        .solve_lower_triangular(&a.transpose())
        .ok_or_else(|| failure("Whitened constraint solve failed", None))?
        .transpose();
    let svd = b.svd(true, true);
    let largest = svd.singular_values.iter().copied().fold(0., f64::max);
    let cutoff = (largest * 1e-12).max(1e-24);
    let rank = svd.singular_values.iter().filter(|&&v| v > cutoff).count();
    // Reuse the same factors for the initial solve and residual corrections.
    let solve = |gradient: &DVector<f64>, constraint: &DVector<f64>| {
        let hi_gradient = factor.solve(gradient);
        let rhs = constraint - a * &hi_gradient;
        let y = svd
            .solve(&rhs, cutoff)
            .map_err(|_| failure("Whitened constraint SVD solve failed", Some(rank)))?;
        let u = svd.u.as_ref().unwrap();
        let mut coefficients = u.transpose() * &rhs;
        for (i, singular) in svd.singular_values.iter().enumerate() {
            coefficients[i] = if *singular > cutoff {
                coefficients[i] / singular / singular
            } else {
                0.
            };
        }
        let lambda = u * coefficients;
        let correction = upper
            .solve_upper_triangular(&y)
            .ok_or_else(|| failure("Whitened correction solve failed", Some(rank)))?;
        Ok((-hi_gradient - correction, lambda))
    };
    let (mut p, mut lambda) = solve(g, c)?;
    let mut diagnostic = check(&p, &lambda);
    let initial_error = diagnostic.error();
    for attempt in 1..=3 {
        if diagnostic.error().is_finite() && diagnostic.error() <= tolerance {
            break;
        }
        let primal = a * &p + c;
        let dual = h * &p + a.transpose() * &lambda + g;
        if !inf(&primal).is_finite() || !inf(&dual).is_finite() {
            break;
        }
        // K delta = -residual, then verify against the ORIGINAL equations.
        let (dp, dlambda) = solve(&dual, &primal)?;
        let candidate_p = &p + dp;
        let candidate_lambda = &lambda + dlambda;
        let mut candidate = check(&candidate_p, &candidate_lambda);
        if !candidate.error().is_finite() || candidate.error() >= diagnostic.error() {
            break;
        }
        candidate.initial_relative_kkt_error = initial_error;
        candidate.refinement_steps = attempt;
        p = candidate_p;
        lambda = candidate_lambda;
        diagnostic = candidate;
    }
    if diagnostic.error().is_finite() && diagnostic.error() <= tolerance {
        Ok((p, lambda, rank, diagnostic))
    } else {
        Err(EqualityLinearFailure {
            reason: "Linearized equalities or stationarity failed residual check",
            schur_rank: Some(rank),
            relative_kkt_error: diagnostic.error().is_finite().then_some(diagnostic.error()),
            diagnostics: diagnostic.error().is_finite().then_some(diagnostic),
        })
    }
}
fn dogleg(
    h: &DMatrix<f64>,
    gradient: &DVector<f64>,
    full: &DVector<f64>,
    radius: f64,
) -> DVector<f64> {
    if full.norm() <= radius {
        return full.clone();
    }
    let norm = gradient.norm();
    if norm == 0. {
        return full * (radius / full.norm());
    }
    let curvature = gradient.dot(&(h * gradient));
    let cauchy = -gradient * (norm * norm / curvature);
    if cauchy.norm() >= radius {
        return -gradient * (radius / norm);
    }
    let direction = full - &cauchy;
    let aa = direction.norm_squared();
    let bb = cauchy.dot(&direction);
    let cc = cauchy.norm_squared() - radius * radius;
    let root = (bb * bb - aa * cc).max(0.).sqrt();
    let t = if bb >= 0. {
        -cc / (bb + root)
    } else {
        (-bb + root) / aa
    };
    cauchy + direction * t.clamp(0., 1.)
}

pub fn bounded_equality_dogleg(
    initial: &[f64],
    bounds: &[VariableBound],
    config: &EqualityDoglegConfig,
    mut evaluate: impl FnMut(&[f64]) -> Result<EqualityResiduals, String>,
) -> Result<EqualityDoglegResult, String> {
    if initial.is_empty()
        || initial.len() != bounds.len()
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
        || config.maximum_iterations > 100000
        || config.maximum_evaluations == 0
        || config.maximum_evaluations > 10000000
        || !config.scaling_exponent.is_finite()
        || !(0. ..=0.5).contains(&config.scaling_exponent)
        || [
            config.difference_step,
            config.initial_radius,
            config.maximum_radius,
            config.minimum_radius,
            config.hessian_regularization,
            config.stationarity_tolerance,
            config.equality_tolerance,
            config.linear_tolerance,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.)
        || config.difference_step > 0.1
        || config.minimum_radius > config.initial_radius
        || config.initial_radius > config.maximum_radius
    {
        return Err("finite matched bounded initial values and positive consistent equality-dogleg controls required".into());
    }
    let active = bounds
        .iter()
        .enumerate()
        .filter(|(_, b)| b.upper > b.lower)
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    let decode = |z: &DVector<f64>| {
        let mut x = initial.to_vec();
        for (&i, &v) in active.iter().zip(z.iter()) {
            x[i] = (bounds[i].lower + v * (bounds[i].upper - bounds[i].lower))
                .clamp(bounds[i].lower, bounds[i].upper);
        }
        x
    };
    let mut z = DVector::from_iterator(
        active.len(),
        active
            .iter()
            .map(|&i| (initial[i] - bounds[i].lower) / (bounds[i].upper - bounds[i].lower)),
    );
    let first = evaluate(initial)?;
    let m = first.objective.len();
    let p = first.equalities.len();
    let valid = |v: &EqualityResiduals| {
        v.objective.len() == m
            && v.equalities.len() == p
            && m > 0
            && p > 0
            && v.objective
                .iter()
                .chain(&v.equalities)
                .all(|x| x.is_finite())
            && v.objective
                .iter()
                .chain(&v.equalities)
                .map(|x| x * x)
                .sum::<f64>()
                .is_finite()
    };
    if !valid(&first) {
        return Err("nonempty finite objective and equality vectors required".into());
    }
    let mut r = DVector::from_vec(first.objective);
    let mut c = DVector::from_vec(first.equalities);
    let mut result = EqualityDoglegResult {
        values: initial.to_vec(),
        objective_cost: 0.5 * r.norm_squared(),
        equality_inf: inf(&c),
        initial_objective_cost: 0.5 * r.norm_squared(),
        initial_equality_inf: inf(&c),
        evaluations: 1,
        rejected_evaluations: 0,
        termination: EqualityTermination::IterationLimit,
        history: vec![],
        linear_failure: None,
    };
    let mut radius = config.initial_radius;
    let mut penalty_weight = 1.0_f64;
    'outer: for iteration in 0..config.maximum_iterations {
        if active.is_empty() {
            result.termination = if inf(&c) <= config.equality_tolerance {
                EqualityTermination::Converged
            } else {
                EqualityTermination::BoundStall
            };
            break;
        }
        let mut jr = DMatrix::zeros(m, active.len());
        let mut a = DMatrix::zeros(p, active.len());
        for j in 0..active.len() {
            let mut sides = [None, None];
            for (slot, sign) in [-1., 1.].into_iter().enumerate() {
                let mut h = config.difference_step;
                for _ in 0..12 {
                    let mut probe = z.clone();
                    probe[j] = (z[j] + sign * h).clamp(0., 1.);
                    let displacement = probe[j] - z[j];
                    if displacement.abs() < 1e-14 {
                        break;
                    }
                    if result.evaluations >= config.maximum_evaluations {
                        result.termination = EqualityTermination::EvaluationLimit;
                        break 'outer;
                    }
                    result.evaluations += 1;
                    match evaluate(&decode(&probe)) {
                        Ok(v) if valid(&v) => {
                            sides[slot] = Some((
                                displacement,
                                DVector::from_vec(v.objective),
                                DVector::from_vec(v.equalities),
                            ));
                            break;
                        }
                        _ => {
                            result.rejected_evaluations += 1;
                            h *= 0.5;
                        }
                    }
                }
            }
            let (dr, dc) = match (&sides[0], &sides[1]) {
                (Some((u, ru, cu)), Some((v, rv, cv))) => {
                    ((rv - ru) / (v - u), (cv - cu) / (v - u))
                }
                (Some((h, rr, cc)), None) | (None, Some((h, rr, cc))) => {
                    ((rr - &r) / *h, (cc - &c) / *h)
                }
                _ => {
                    result.termination = EqualityTermination::InvalidDerivative;
                    break 'outer;
                }
            };
            if dr.iter().chain(dc.iter()).any(|v| !v.is_finite()) {
                result.termination = EqualityTermination::InvalidDerivative;
                break 'outer;
            }
            jr.set_column(j, &dr);
            a.set_column(j, &dc);
        }
        let gradient = jr.transpose() * &r;
        let normal = jr.transpose() * &jr;
        let scaling = DVector::from_iterator(
            active.len(),
            (0..active.len()).map(|j| normal[(j, j)].max(1e-24).powf(-config.scaling_exponent)),
        );
        let g = gradient.component_mul(&scaling);
        let mut h = normal.clone();
        for i in 0..active.len() {
            for j in 0..active.len() {
                h[(i, j)] *= scaling[i] * scaling[j];
            }
        }
        let diagonal = h.diagonal().iter().copied().fold(0., f64::max).max(1.);
        for j in 0..active.len() {
            h[(j, j)] += config.hessian_regularization * diagonal;
            a.column_mut(j).scale_mut(scaling[j]);
        }
        let (full, lambda, rank, linear_diagnostics) =
            match newton_step(&h, &g, &a, &c, config.linear_tolerance) {
                Ok(step) => step,
                Err(error) => {
                    result.termination = EqualityTermination::LinearConstraintFailure;
                    result.linear_failure = Some(error);
                    break;
                }
            };
        let merit_gradient = &g + a.transpose() * &lambda;
        let stationarity = inf(&merit_gradient);
        let exact_penalty = config.globalization == EqualityGlobalization::ExactPenaltyNewton;
        if exact_penalty {
            penalty_weight = penalty_weight.max(1. + 1.1 * inf(&lambda));
        }
        if stationarity <= config.stationarity_tolerance && inf(&c) <= config.equality_tolerance {
            result.termination = EqualityTermination::Converged;
            break;
        }
        let mut accepted = false;
        let mut ratio = None;
        let mut bound_fraction = 1.;
        let mut roundoff_acceptance = false;
        for _ in 0..24 {
            let step = if exact_penalty {
                &full * (radius / full.norm().max(radius)).min(1.)
            } else {
                dogleg(&h, &merit_gradient, &full, radius)
            };
            let delta = step.component_mul(&scaling);
            bound_fraction = delta
                .iter()
                .enumerate()
                .map(|(j, d)| {
                    if *d > 0. {
                        (1. - z[j]) / d
                    } else if *d < 0. {
                        -z[j] / d
                    } else {
                        1.
                    }
                })
                .fold(1., f64::min)
                .clamp(0., 1.);
            if bound_fraction <= 1e-14 {
                result.termination = EqualityTermination::BoundStall;
                break;
            }
            let trial = (&z + delta * bound_fraction).map(|v| v.clamp(0., 1.));
            let actual_step = (&trial - &z).component_div(&scaling);
            let predicted = if exact_penalty {
                -g.dot(&actual_step) - 0.5 * actual_step.dot(&(&h * &actual_step))
                    + penalty_weight * (c.abs().sum() - (&c + &a * &actual_step).abs().sum())
            } else {
                -merit_gradient.dot(&actual_step) - 0.5 * actual_step.dot(&(&h * &actual_step))
            };
            if predicted.is_finite() && predicted > 0. {
                if result.evaluations >= config.maximum_evaluations {
                    result.termination = EqualityTermination::EvaluationLimit;
                    break 'outer;
                }
                result.evaluations += 1;
                let values = decode(&trial);
                match evaluate(&values) {
                    Ok(v) if valid(&v) => {
                        let rr = DVector::from_vec(v.objective);
                        let cc = DVector::from_vec(v.equalities);
                        let reduction = 0.5 * (&r - &rr).dot(&(&r + &rr))
                            + if exact_penalty {
                                penalty_weight * (c.abs().sum() - cc.abs().sum())
                            } else {
                                (&c - &cc).dot(&lambda)
                            };
                        let noise = 32.
                            * f64::EPSILON
                            * (1.
                                + r.norm_squared()
                                + rr.norm_squared()
                                + if exact_penalty {
                                    penalty_weight * (c.abs().sum() + cc.abs().sum())
                                } else {
                                    c.dot(&lambda).abs() + cc.dot(&lambda).abs()
                                });
                        // Near a constrained solution, opposite objective and
                        // multiplier terms can cancel below f64 resolution.
                        // Permit that case only with clear feasibility progress;
                        // convergence still checks the original tolerances.
                        roundoff_acceptance =
                            predicted <= noise && reduction >= -noise && inf(&cc) < 0.9 * inf(&c);
                        let rho = if roundoff_acceptance {
                            0.5
                        } else {
                            reduction / predicted
                        };
                        ratio = rho.is_finite().then_some(rho);
                        if rho.is_finite() && rho > 1e-4 {
                            if rho > 0.75 && step.norm() >= 0.9 * radius {
                                radius = (2. * radius).min(config.maximum_radius);
                            } else if rho < 0.25 {
                                radius *= 0.25;
                            }
                            z = trial;
                            r = rr;
                            c = cc;
                            result.values = values;
                            accepted = true;
                            break;
                        }
                    }
                    _ => result.rejected_evaluations += 1,
                }
            }
            radius *= 0.25;
            if radius < config.minimum_radius {
                result.termination = EqualityTermination::RadiusLimit;
                break;
            }
        }
        result.history.push(EqualityIteration {
            iteration,
            evaluations: result.evaluations,
            objective_cost: 0.5 * r.norm_squared(),
            equality_inf: inf(&c),
            merit_gradient_inf: stationarity,
            radius,
            accepted,
            trust_ratio: ratio,
            full_step_linear_error: linear_diagnostics.error(),
            full_step_norm: full.norm(),
            linear_diagnostics,
            schur_rank: rank,
            bound_fraction,
            roundoff_acceptance,
            exact_penalty_weight: exact_penalty.then_some(penalty_weight),
        });
        if !accepted {
            if result.termination == EqualityTermination::IterationLimit {
                result.termination = EqualityTermination::RadiusLimit;
            }
            break;
        }
    }
    result.objective_cost = 0.5 * r.norm_squared();
    result.equality_inf = inf(&c);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> EqualityDoglegConfig {
        EqualityDoglegConfig {
            globalization: EqualityGlobalization::LagrangianDogleg,
            maximum_iterations: 60,
            maximum_evaluations: 10000,
            difference_step: 1e-6,
            initial_radius: 1.,
            maximum_radius: 10.,
            minimum_radius: 1e-12,
            hessian_regularization: 1e-12,
            scaling_exponent: 0.25,
            stationarity_tolerance: 1e-7,
            equality_tolerance: 1e-8,
            linear_tolerance: 1e-7,
        }
    }
    fn bounds() -> Vec<VariableBound> {
        vec![
            VariableBound {
                lower: -4.,
                upper: 4.
            };
            2
        ]
    }
    #[test]
    fn equality_prevents_trading_feasibility_for_tracking() {
        let r = bounded_equality_dogleg(&[0., 0.], &bounds(), &config(), |x| {
            Ok(EqualityResiduals {
                objective: vec![x[0] - 2., x[1] - 2.],
                equalities: vec![x[0] + x[1] - 1.],
            })
        })
        .unwrap();
        assert_eq!(r.termination, EqualityTermination::Converged);
        assert!((r.values[0] - 0.5).abs() < 1e-7 && (r.values[1] - 0.5).abs() < 1e-7);
        assert!(r.history.iter().all(|h| h.full_step_linear_error < 1e-7));
    }
    #[test]
    fn nonlinear_manifold_requires_both_stationarity_and_feasibility() {
        let r = bounded_equality_dogleg(&[0.7, 0.6], &bounds(), &config(), |x| {
            Ok(EqualityResiduals {
                objective: vec![x[0] - 2., x[1]],
                equalities: vec![x[0] * x[0] + x[1] * x[1] - 1.],
            })
        })
        .unwrap();
        assert_eq!(
            r.termination,
            EqualityTermination::Converged,
            "values {:?}, equality {}, last {:?}",
            r.values,
            r.equality_inf,
            r.history.last()
        );
        assert!((r.values[0] - 1.).abs() < 1e-7 && r.values[1].abs() < 1e-7);
    }
    #[test]
    fn redundant_consistent_constraints_and_inconsistent_constraints_are_distinct() {
        for inconsistent in [false, true] {
            let r = bounded_equality_dogleg(&[0., 0.], &bounds(), &config(), |x| {
                Ok(EqualityResiduals {
                    objective: vec![x[0] - 2., x[1] - 2.],
                    equalities: vec![
                        x[0] + x[1] - 1.,
                        2. * (x[0] + x[1]) - if inconsistent { 3. } else { 2. },
                    ],
                })
            })
            .unwrap();
            assert_eq!(
                r.termination,
                if inconsistent {
                    EqualityTermination::LinearConstraintFailure
                } else {
                    EqualityTermination::Converged
                }
            );
        }
    }
    #[test]
    fn infeasible_bounds_and_evaluation_budget_never_report_convergence() {
        let mut c = config();
        c.maximum_evaluations = 1;
        let f = |x: &[f64]| {
            Ok(EqualityResiduals {
                objective: vec![x[0]],
                equalities: vec![x[0] - 2.],
            })
        };
        assert_eq!(
            bounded_equality_dogleg(
                &[0.],
                &[VariableBound {
                    lower: 0.,
                    upper: 1.
                }],
                &c,
                f
            )
            .unwrap()
            .termination,
            EqualityTermination::EvaluationLimit
        );
        c = config();
        let r = bounded_equality_dogleg(
            &[0.],
            &[VariableBound {
                lower: 0.,
                upper: 1.,
            }],
            &c,
            f,
        )
        .unwrap();
        assert_eq!(r.termination, EqualityTermination::BoundStall);
        assert!(r.equality_inf >= 1.);
    }
    #[test]
    fn whitened_constraints_recover_a_direction_lost_by_normal_equations() {
        let h = DMatrix::identity(2, 2);
        let g = DVector::zeros(2);
        let a = DMatrix::from_row_slice(2, 2, &[1., 0., 1., 1e-9]);
        let c = DVector::from_vec(vec![1., 1.0001]);
        let (p, _, rank, error) = newton_step(&h, &g, &a, &c, 1e-6).unwrap();
        assert_eq!(rank, 2);
        assert!(error.error() < 1e-6);
        assert!((p[0] + 1.).abs() < 1e-8 && (p[1] + 1e5).abs() < 1e-3);
    }
    #[test]
    fn refinement_closes_ill_conditioned_original_equations() {
        let h = DMatrix::identity(2, 2);
        let g = DVector::from_vec(vec![3., -2.]);
        let expected = DVector::from_vec(vec![0.123, -0.789]);
        let mut refined = 0;
        let mut rejected = 0;
        for epsilon in [1e-4, 1e-5, 1e-6, 1e-7, 1e-8] {
            let a = DMatrix::from_row_slice(2, 2, &[1., 1., 1., 1. + epsilon]);
            let c = -&a * &expected;
            let (p, _, rank, diagnostic) = match newton_step(&h, &g, &a, &c, 1e-12) {
                Ok(step) => step,
                Err(failure) => {
                    let diagnostic = failure.diagnostics.unwrap();
                    assert!(diagnostic.error() > 1e-12);
                    assert!(diagnostic.error() <= diagnostic.initial_relative_kkt_error);
                    rejected += 1;
                    continue;
                }
            };
            assert_eq!(rank, 2);
            assert!(diagnostic.error() <= 1e-12);
            assert!(inf(&(&a * &p + &c)) <= 2e-12);
            assert!(inf(&(&p - &expected)) < 1e-6);
            if diagnostic.refinement_steps > 0 {
                assert!(diagnostic.error() < diagnostic.initial_relative_kkt_error);
                refined += 1;
            }
        }
        assert!(
            refined > 0,
            "the regression must exercise residual correction"
        );
        assert!(rejected > 0, "unattainable accuracy must remain a failure");
        let invalid = EqualityLinearDiagnostics {
            initial_relative_kkt_error: 0.,
            relative_constraint_error: 0.,
            relative_stationarity_error: f64::NAN,
            refinement_steps: 0,
        };
        assert!(!invalid.error().is_finite());
    }
    #[test]
    fn exact_penalty_globalization_solves_nonlinear_and_linear_constraints() {
        let mut c = config();
        c.globalization = EqualityGlobalization::ExactPenaltyNewton;
        for nonlinear in [false, true] {
            let r = bounded_equality_dogleg(&[0.7, 0.6], &bounds(), &c, |x| {
                Ok(EqualityResiduals {
                    objective: vec![x[0] - 2., x[1]],
                    equalities: vec![if nonlinear {
                        x[0] * x[0] + x[1] * x[1] - 1.
                    } else {
                        x[0] + x[1] - 1.
                    }],
                })
            })
            .unwrap();
            assert_eq!(
                r.termination,
                EqualityTermination::Converged,
                "values {:?} eq {} last {:?}",
                r.values,
                r.equality_inf,
                r.history.last()
            );
            assert!(r.equality_inf <= c.equality_tolerance);
            assert!(
                r.history
                    .iter()
                    .all(|h| h.exact_penalty_weight.is_some_and(|w| w >= 1.))
            );
        }
        assert!(inf(&DVector::from_vec(vec![f64::NAN])).is_infinite());
    }
}

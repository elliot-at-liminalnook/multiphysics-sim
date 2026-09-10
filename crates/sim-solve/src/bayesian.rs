//! Constrained Bayesian experiment selection using pinned EGObox EGO.
//!
//! This is an ask/tell adapter for expensive deterministic evaluations. It is
//! not SCBO: the backend service does not retain TREGO state between calls.
//! Surrogate predictions choose experiments; only observed constraints certify
//! membership in the caller's sampled feasible set.
use egobox_doe::{Lhs, LhsKind, SamplingMethod};
use egobox_ego::{CorrelationSpec, EgorServiceBuilder, InfillStrategy, RegressionSpec};
use ndarray::{Array1, Array2};
use rand::SeedableRng;
use rand_xoshiro::Xoshiro256Plus;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Seeded Latin hypercube for initial experiments or an independent search
/// control. Each parameter is stratified; no feasibility or performance assumed.
pub fn initial_design(problem: &Problem, count: usize, seed: u64) -> Result<Vec<Vec<f64>>, String> {
    problem.validate()?;
    if count == 0 || count > 4096 {
        return Err("Latin hypercube needs 1..4096 points".into());
    }
    let limits = Array2::from_shape_fn((problem.parameters.len(), 2), |(i, j)| {
        problem.parameters[i].bounds[j]
    });
    let values = Lhs::new_with_rng(&limits, Xoshiro256Plus::seed_from_u64(seed))
        .kind(LhsKind::Classic)
        .sample(count)
        .rows()
        .into_iter()
        .map(|r| r.to_vec())
        .collect::<Vec<_>>();
    for v in &values {
        problem.normalized(v)?;
    }
    Ok(values)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    pub name: String,
    pub unit: String,
    pub bounds: [f64; 2],
}

/// An explicitly conditioned parameter in physical units, not a new bound.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixedParameter {
    pub name: String,
    pub value: f64,
}

/// Stratify only free coordinates and reinsert the conditioned coordinates.
/// An empty condition preserves the original seeded design exactly.
pub fn conditioned_design(
    problem: &Problem, count: usize, seed: u64, fixed: &[FixedParameter],
) -> Result<Vec<Vec<f64>>, String> {
    let coordinates = problem.fixed_coordinates(fixed)?;
    if fixed.is_empty() { return initial_design(problem, count, seed); }
    let mut free = problem.clone();
    free.parameters = problem.parameters.iter().enumerate()
        .filter(|(i, _)| !coordinates.iter().any(|(j, _)| i == j))
        .map(|(_, p)| p.clone()).collect();
    let rows = if free.parameters.is_empty() {
        if count != 1 { return Err("fully conditioned design requires exactly one point".into()); }
        vec![vec![]]
    } else { initial_design(&free, count, seed)? };
    Ok(rows.into_iter().map(|row| {
        let mut values = row.into_iter();
        (0..problem.parameters.len()).map(|i| {
            coordinates.iter().find(|(j, _)| *j == i)
                .map(|(_, x)| *x).unwrap_or_else(|| values.next().unwrap())
        }).collect()
    }).collect())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Constraint {
    pub name: String,
    pub unit: String,
    /// Positive numerical conditioning scale; feasibility remains residual <= 0.
    pub scale: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Problem {
    /// Caller-owned identity of model, policy, fidelity, parameter mappings,
    /// environment seed and measurement definitions. Different contexts cannot mix.
    pub context_id: String,
    pub parameters: Vec<Parameter>,
    pub objective_name: String,
    pub objective_unit: String,
    pub constraints: Vec<Constraint>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    Complete {
        /// Minimized: a speed maximization supplies negative measured speed.
        objective: f64,
        residuals: Vec<f64>,
    },
    Failed {
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub context_id: String,
    pub values: Vec<f64>,
    pub outcome: Outcome,
    /// Durable result reference, not interpreted or fetched by the solver.
    pub evidence: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub seed: u64,
    pub acquisition_starts: usize,
    /// Reject oversized input rather than silently dropping training history.
    pub maximum_training_rows: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Proposal {
    pub values: Vec<f64>,
    pub seed: u64,
    pub training_indices: Vec<usize>,
    pub failed_indices: Vec<usize>,
    pub observed_best_feasible_index: Option<usize>,
    pub method: String,
    pub scope: String,
}

impl Problem {
    pub(crate) fn fixed_coordinates(&self, fixed: &[FixedParameter]) -> Result<Vec<(usize, f64)>, String> {
        self.validate()?;
        let mut seen = HashSet::new();
        fixed.iter().map(|condition| {
            let i = self.parameters.iter().position(|p| p.name == condition.name)
                .ok_or("unknown conditioned parameter")?;
            let p = &self.parameters[i];
            if !seen.insert(i) || !condition.value.is_finite()
                || condition.value < p.bounds[0] || condition.value > p.bounds[1] {
                return Err("conditioned parameters require unique names and finite in-domain values".into());
            }
            Ok((i, condition.value))
        }).collect()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.context_id.trim().is_empty()
            || self.objective_name.trim().is_empty()
            || self.objective_unit.trim().is_empty()
            || self.parameters.is_empty()
            || self.parameters.len() > 64
            || self.constraints.len() > 64
        {
            return Err("Bayesian problem needs an explicit context, objective and 1..64 parameters (at most 64 constraints)".into());
        }
        let mut names = HashSet::new();
        for p in &self.parameters {
            let [lo, hi] = p.bounds;
            if p.name.trim().is_empty()
                || p.unit.trim().is_empty()
                || !names.insert(&p.name)
                || !lo.is_finite()
                || !hi.is_finite()
                || lo >= hi
                || !(hi - lo).is_finite()
            {
                return Err(
                    "Bayesian parameter names/units and finite ordered bounds required".into(),
                );
            }
        }
        names.clear();
        for c in &self.constraints {
            if c.name.trim().is_empty()
                || c.unit.trim().is_empty()
                || !names.insert(&c.name)
                || !c.scale.is_finite()
                || c.scale <= 0.0
            {
                return Err(
                    "Bayesian constraints require unique names, units and positive finite scales"
                        .into(),
                );
            }
        }
        Ok(())
    }

    pub(crate) fn normalized(&self, values: &[f64]) -> Result<Vec<f64>, String> {
        if values.len() != self.parameters.len() {
            return Err("Bayesian observation parameter dimension mismatch".into());
        }
        values
            .iter()
            .zip(&self.parameters)
            .map(|(&x, p)| {
                if !x.is_finite() || x < p.bounds[0] || x > p.bounds[1] {
                    Err(format!(
                        "Bayesian point outside declared bounds for {}",
                        p.name
                    ))
                } else {
                    Ok((x - p.bounds[0]) / (p.bounds[1] - p.bounds[0]))
                }
            })
            .collect()
    }
}

/// Propose one unevaluated point. All complete rows train independent objective
/// and constraint GPs; failed evaluations retain their identities without a
/// fabricated speed, violation or imputed observation. They are not yet modeled
/// with a viability classifier. Pending evaluations must be managed by the caller.
pub fn suggest(
    problem: &Problem,
    observations: &[Observation],
    config: &Config,
) -> Result<Proposal, String> {
    suggest_in_region(problem, observations, config, None)
}

/// Limit acquisition optimization in normalized parameter coordinates, while
/// retaining every completed observation for GP training. This is an optimizer
/// region, not a changed physical domain or an additional feasibility condition.
pub fn suggest_in_region(
    problem: &Problem,
    observations: &[Observation],
    config: &Config,
    region: Option<&[[f64; 2]]>,
) -> Result<Proposal, String> {
    problem.validate()?;
    if let Some(region) = region {
        if region.len() != problem.parameters.len() || region.iter().any(|[lo, hi]|
            !lo.is_finite() || !hi.is_finite() || *lo < 0. || *hi > 1. || lo >= hi)
        {
            return Err("acquisition region requires dimension-matched ordered bounds within normalized0..1".into());
        }
    }
    if config.acquisition_starts == 0
        || config.acquisition_starts > 128
        || config.maximum_training_rows < problem.parameters.len() + 1
        || config.maximum_training_rows > 4096
    {
        return Err("Bayesian selector needs 1..128 acquisition starts and dimension+1..4096 maximum training rows".into());
    }
    let mut training_indices = Vec::new();
    let mut failed_indices = Vec::new();
    let mut x = Vec::new();
    let mut y = Vec::new();
    let mut seen = HashSet::new();
    let mut best: Option<(usize, f64)> = None;
    let mut normalized = Vec::new();
    for (i, observation) in observations.iter().enumerate() {
        if observation.context_id != problem.context_id || observation.evidence.trim().is_empty() {
            return Err(
                "Bayesian observations must match the declared context and retain evidence".into(),
            );
        }
        let point = problem.normalized(&observation.values)?;
        normalized.push(point.clone());
        match &observation.outcome {
            Outcome::Failed { reason } => {
                if reason.trim().is_empty() {
                    return Err("failed evaluation needs a reason".into());
                }
                failed_indices.push(i);
            }
            Outcome::Complete {
                objective,
                residuals,
            } => {
                if !objective.is_finite()
                    || residuals.len() != problem.constraints.len()
                    || residuals.iter().any(|r| !r.is_finite())
                {
                    return Err("completed Bayesian evaluation needs finite objective and matching residuals".into());
                }
                let bits = point
                    .iter()
                    .map(|x| if *x == 0.0 { 0 } else { x.to_bits() })
                    .collect::<Vec<_>>();
                if !seen.insert(bits) {
                    return Err("duplicate completed deterministic point; reconcile repeated evidence first".into());
                }
                if residuals.iter().all(|r| *r <= 0.0) && best.is_none_or(|(_, b)| *objective < b) {
                    best = Some((i, *objective));
                }
                training_indices.push(i);
                x.extend(point);
                y.push(*objective);
                for (r, c) in residuals.iter().zip(&problem.constraints) {
                    let scaled = r / c.scale;
                    if !scaled.is_finite() || (*r != 0.0 && scaled == 0.0) {
                        return Err("constraint conditioning overflow/underflow would lose the observed boundary".into());
                    }
                    y.push(scaled);
                }
            }
        }
    }
    let n = training_indices.len();
    let dim = problem.parameters.len();
    if n < dim + 1 || n > config.maximum_training_rows {
        return Err("provide dimension+1..maximum_training_rows complete observations; failed trials are not training values".into());
    }
    let xs = Array2::from_shape_vec((n, dim), x).map_err(|e| e.to_string())?;
    let ys =
        Array2::from_shape_vec((n, 1 + problem.constraints.len()), y).map_err(|e| e.to_string())?;
    let limits = Array2::from_shape_fn((dim, 2), |(i, j)| region.map_or(j as f64, |r| r[i][j]));
    let proposal = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let service = EgorServiceBuilder::optimize()
            .configure(|c| {
                let c = c.seed(config.seed)
                    .n_start(config.acquisition_starts)
                    .n_cstr(problem.constraints.len())
                    .cstr_tol(Array1::zeros(problem.constraints.len()))
                    .infill_strategy(InfillStrategy::LogEI)
                    .cstr_infill(true)
                    .configure_gp(|gp| {
                        gp.regression_spec(RegressionSpec::CONSTANT)
                            .correlation_spec(CorrelationSpec::MATERN52)
                    });
                // Data-based midpoint starts can lie outside a local region.
                // Keep global training rows, but seed local acquisition inside
                // its own bounds instead of using those outside midpoints.
                if region.is_some() {
                    c.configure_runtime_flags(|f| f.disable_middlepicker_multistarter(true))
                } else {
                    c
                }
            })
            .min_within(&limits)
            .map_err(|e| e.to_string())?;
        Ok::<_, String>(service.suggest(&xs, &ys))
    }))
    .map_err(|_| {
        "EGObox proposal failed internally; no experiment or feasible result fabricated".to_string()
    })??;
    if proposal.nrows() != 1 || proposal.ncols() != dim {
        return Err("EGObox returned an unexpected proposal shape".into());
    }
    let p = proposal.row(0).to_vec();
    if p.iter().any(|x| !x.is_finite() || !(0.0..=1.0).contains(x)) {
        return Err("EGObox returned an invalid or out-of-box proposal".into());
    }
    if region.is_some_and(|r| p.iter().zip(r).any(|(x, [lo, hi])| x < lo || x > hi)) {
        return Err(format!("EGObox returned point {p:?} outside acquisition region {region:?}"));
    }
    if normalized
        .iter()
        .any(|old| old.iter().zip(&p).all(|(a, b)| (a - b).abs() <= 1e-10))
    {
        return Err(
            "EGObox repeated a completed or failed point; explicit exploration/restart needed"
                .into(),
        );
    }
    let values = p
        .iter()
        .zip(&problem.parameters)
        .map(|(x, p)| (1.0 - x) * p.bounds[0] + x * p.bounds[1])
        .collect::<Vec<_>>();
    problem.normalized(&values)?;
    Ok(Proposal { values, seed: config.seed, training_indices, failed_indices,
        observed_best_feasible_index: best.map(|b| b.0),
        method: "egobox-ego-0.38.1 / constrained LogEI / constant-mean Matern-5/2 GP / normalized inputs / zero constraint tolerance".into(),
        scope: "Unevaluated Bayesian proposal from explicit same-context completed measurements. Failures are recorded but not imputed or modeled. No SCBO/TREGO state, physical feasibility, runtime qualification or global optimum is claimed.".into() })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conditioned_design_stratifies_free_coordinates_and_checks_bindings() {
        let mut p = problem();
        let free = p.clone();
        p.parameters.push(Parameter { name: "step".into(), unit: "s".into(), bounds: [0.1, 0.2] });
        let fixed = vec![FixedParameter { name: "step".into(), value: 0.1 }];
        let rows = conditioned_design(&p, 12, 19, &fixed).unwrap();
        let expected = initial_design(&free, 12, 19).unwrap();
        for (r, e) in rows.iter().zip(expected) { assert_eq!(r, &vec![e[0], 0.1]); }
        assert_eq!(conditioned_design(&p, 12, 19, &[]).unwrap(), initial_design(&p, 12, 19).unwrap());
        for bad in [vec![fixed[0].clone(), fixed[0].clone()],
            vec![FixedParameter { name: "absent".into(), value: 0.1 }],
            vec![FixedParameter { name: "step".into(), value: f64::NAN }],
            vec![FixedParameter { name: "step".into(), value: 0.3 }]] {
            assert!(conditioned_design(&p, 12, 19, &bad).is_err());
        }
        let all = vec![fixed[0].clone(), FixedParameter {name:p.parameters[0].name.clone(), value:p.parameters[0].bounds[0]}];
        assert_eq!(conditioned_design(&p, 1, 19, &all).unwrap(), vec![vec![p.parameters[0].bounds[0], 0.1]]);
        assert!(conditioned_design(&p, 2, 19, &all).is_err());
    }
    #[test]
    fn design_is_stratified_and_seed_reproducible() {
        let mut p = problem();
        p.parameters.push(Parameter {
            name: "second".into(),
            unit: "rad".into(),
            bounds: [-2., 4.],
        });
        let a = initial_design(&p, 12, 19).unwrap();
        assert_eq!(a, initial_design(&p, 12, 19).unwrap());
        for j in 0..2 {
            let bins = a
                .iter()
                .map(|v| (12. * p.normalized(v).unwrap()[j]).floor() as usize)
                .collect::<HashSet<_>>();
            assert_eq!(bins, (0..12).collect());
        }
    }
    #[test]
    fn constant_boundary_gate_and_failed_trial_do_not_become_fake_measurements() {
        let mut p = problem();
        p.constraints.push(Constraint {
            name: "overlap".into(),
            unit: "m".into(),
            scale: 0.0001,
        });
        let mut data = vec![
            observation(0.),
            observation(0.2),
            observation(0.4),
            observation(0.9),
        ];
        for o in &mut data {
            if let Outcome::Complete { residuals, .. } = &mut o.outcome {
                residuals.push(0.);
            }
        }
        data.push(Observation {
            context_id: p.context_id.clone(),
            values: vec![0.75],
            outcome: Outcome::Failed {
                reason: "runtime incomplete".into(),
            },
            evidence: "failed.json".into(),
        });
        let r = suggest(
            &p,
            &data,
            &Config {
                seed: 42,
                acquisition_starts: 4,
                maximum_training_rows: 32,
            },
        )
        .unwrap();
        assert_eq!(r.training_indices, vec![0, 1, 2, 3]);
        assert_eq!(r.failed_indices, vec![4]);
        assert_eq!(r.observed_best_feasible_index, Some(2));
    }
    fn problem() -> Problem {
        Problem {
            context_id: "analytic-boundary-v1".into(),
            parameters: vec![Parameter {
                name: "x".into(),
                unit: "1".into(),
                bounds: [0., 1.],
            }],
            objective_name: "quadratic".into(),
            objective_unit: "1".into(),
            constraints: vec![Constraint {
                name: "upper".into(),
                unit: "1".into(),
                scale: 1.,
            }],
        }
    }
    fn observation(x: f64) -> Observation {
        Observation {
            context_id: problem().context_id,
            values: vec![x],
            outcome: Outcome::Complete {
                objective: (x - 0.8).powi(2),
                residuals: vec![x - 0.6],
            },
            evidence: format!("analytic:{x}"),
        }
    }
    #[test]
    fn rejects_mixed_context_nonfinite_and_missing_values_without_imputation() {
        let p = problem();
        let c = Config {
            seed: 42,
            acquisition_starts: 4,
            maximum_training_rows: 32,
        };
        let mut data = vec![observation(0.), observation(0.4), observation(0.9)];
        data[1].context_id = "different-physics".into();
        assert!(suggest(&p, &data, &c).is_err());
        data[1] = observation(0.4);
        data[0].values[0] = f64::NAN;
        assert!(suggest(&p, &data, &c).is_err());
        let failed = Observation {
            context_id: p.context_id.clone(),
            values: vec![0.5],
            outcome: Outcome::Failed {
                reason: "IK outside domain".into(),
            },
            evidence: "failure.json".into(),
        };
        assert!(suggest(&p, &[observation(0.), failed], &c).is_err());
        assert!(suggest(&p, &[observation(0.), observation(-0.)], &c).is_err());
    }
    #[test]
    fn seeded_proposals_improve_an_observed_constrained_optimum() {
        let p = problem();
        let c = Config {
            seed: 42,
            acquisition_starts: 4,
            maximum_training_rows: 32,
        };
        let mut data = vec![
            observation(0.),
            observation(0.2),
            observation(0.4),
            observation(0.9),
        ];
        let a = suggest(&p, &data, &c).unwrap();
        let b = suggest(&p, &data, &c).unwrap();
        assert_eq!(a.values, b.values);
        assert_eq!(a.observed_best_feasible_index, Some(2));
        for i in 0..8 {
            let proposal = suggest(
                &p,
                &data,
                &Config {
                    seed: 42 + i,
                    ..c.clone()
                },
            )
            .unwrap_or_else(|e| {
                panic!(
                    "step {i}: {e}; evaluated {:?}",
                    data.iter().map(|o| &o.values).collect::<Vec<_>>()
                )
            });
            data.push(observation(proposal.values[0]));
        }
        let best = data
            .iter()
            .filter_map(|o| match &o.outcome {
                Outcome::Complete {
                    objective,
                    residuals,
                } if residuals.iter().all(|r| *r <= 0.) => Some(*objective),
                _ => None,
            })
            .fold(f64::INFINITY, f64::min);
        // Independently known optimum: x=.6, objective=.04. Do not count
        // infeasible x=.8 (objective zero) as a solution.
        assert!(
            best >= 0.04 - 1e-12 && best < 0.09,
            "observed constrained best {best}"
        );
    }
}

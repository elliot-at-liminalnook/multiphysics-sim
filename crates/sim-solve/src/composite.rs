//! Candidate-set expected improvement for a cheap function of expensive responses.
//!
//! Independent response GPs represent a diagonal multi-output posterior. Common
//! seeded Gaussian samples propagate that posterior through the caller's cheap
//! function; this is not EI applied to the function of posterior means. No
//! cross-response covariance, acquisition-gradient optimization, calibrated
//! uncertainty, failure classifier or convergence guarantee is claimed.
use crate::bayesian::{Problem, FixedParameter};
use egobox_gp::{GpParams, correlation_models::Matern52Corr, mean_models::ConstantMean};
use linfa::prelude::*;
use ndarray::{Array1, Array2};
use rand::{Rng, SeedableRng};
use rand_xoshiro::Xoshiro256Plus;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub name: String,
    pub unit: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    Complete { responses: Vec<f64> },
    Failed { reason: String },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub context_id: String,
    pub values: Vec<f64>,
    pub outcome: Outcome,
    pub evidence: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub seed: u64,
    pub posterior_samples: usize,
    pub maximum_training_rows: usize,
    /// Optional dimensionless post-hoc scales. Empty preserves the raw GP posterior.
    /// Estimation and out-of-sample validation belong to the caller's experiment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub response_std_scales: Vec<f64>,
    /// All completed rows train the model; only matching rows define improvement.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incumbent_conditions: Vec<FixedParameter>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Candidate {
    pub index: usize,
    pub values: Vec<f64>,
    pub response_mean: Vec<f64>,
    pub response_std: Vec<f64>,
    pub objective_at_mean: f64,
    pub posterior_mean_objective: f64,
    pub expected_improvement: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub selected: Candidate,
    pub candidates: Vec<Candidate>,
    pub training_indices: Vec<usize>,
    pub failed_indices: Vec<usize>,
    pub observed_best_index: usize,
    pub observed_best_objective: f64,
    pub seed: u64,
    pub posterior_samples: usize,
    pub method: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub response_std_scales: Vec<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub incumbent_conditions: Vec<FixedParameter>,
}

fn gaussian_samples(count: usize, dimensions: usize, seed: u64) -> Vec<Vec<f64>> {
    let mut rng = Xoshiro256Plus::seed_from_u64(seed);
    (0..count)
        .map(|_| {
            (0..dimensions)
                .map(|_| {
                    let u = 1. - rng.r#gen::<f64>();
                    let angle = std::f64::consts::TAU * rng.r#gen::<f64>();
                    (-2. * u.ln()).sqrt() * angle.cos()
                })
                .collect()
        })
        .collect()
}

fn expected_improvement(
    mean: &[f64],
    std: &[f64],
    normals: &[Vec<f64>],
    best: f64,
    compose: &impl Fn(&[f64]) -> Result<f64, String>,
) -> Result<(f64, f64), String> {
    let mut ei = 0.;
    let mut average = 0.;
    for z in normals {
        let response: Vec<_> = mean
            .iter()
            .zip(std)
            .zip(z)
            .map(|((&m, &s), &z)| m + s * z)
            .collect();
        if response.iter().any(|v| !v.is_finite()) {
            return Err("nonfinite posterior response".into());
        }
        let f = compose(&response)?;
        if !f.is_finite() {
            return Err("nonfinite composed objective".into());
        }
        // Divide each contribution before summing to avoid a large sum overflow.
        ei += (best - f).max(0.) / normals.len() as f64;
        average += f / normals.len() as f64;
    }
    if !ei.is_finite() || !average.is_finite() {
        return Err("nonfinite composite acquisition".into());
    }
    Ok((ei, average))
}

/// Rank a caller-owned finite candidate set for a minimized cheap objective.
/// Failed experiments retain identities and exclude repeated proposals, but
/// are not assigned fabricated responses. Physical acceptance stays external.
/// This first interface rejects constraints instead of silently ignoring them.
pub fn rank_candidates(
    problem: &Problem,
    responses: &[Response],
    observations: &[Observation],
    candidates: &[Vec<f64>],
    config: &Config,
    compose: impl Fn(&[f64]) -> Result<f64, String>,
) -> Result<Report, String> {
    problem.validate()?;
    let incumbent_coordinates = problem.fixed_coordinates(&config.incumbent_conditions)?;
    if !problem.constraints.is_empty() {
        return Err("composite ranking does not model constraints; use separately qualified observations and validate proposed experiments".into());
    }
    let mut names = HashSet::new();
    if responses.is_empty()
        || responses.len() > 64
        || responses
            .iter()
            .any(|r| r.name.trim().is_empty() || r.unit.trim().is_empty() || !names.insert(&r.name))
    {
        return Err("1..64 uniquely named responses with units required".into());
    }
    if !(2..=65536).contains(&config.posterior_samples)
        || config.maximum_training_rows < problem.parameters.len() + 1
        || config.maximum_training_rows > 4096
        || candidates.is_empty()
        || candidates.len() > 16384
    {
        return Err("invalid composite sampling, training or candidate count".into());
    }
    let mut training_indices = vec![];
    if !config.response_std_scales.is_empty()
        && (config.response_std_scales.len() != responses.len()
            || config.response_std_scales.iter().any(|s| !s.is_finite() || *s <= 0.))
    {
        return Err("positive dimension-matched response standard-deviation scales required".into());
    }
    let mut failed_indices = vec![];
    let mut old_points = vec![];
    let mut x = vec![];
    let mut y = vec![];
    let mut seen = HashSet::new();
    let mut best: Option<(usize, f64)> = None;
    for (i, o) in observations.iter().enumerate() {
        if o.context_id != problem.context_id || o.evidence.trim().is_empty() {
            return Err("response observations require matching context and evidence".into());
        }
        let point = problem.normalized(&o.values)?;
        old_points.push(point.clone());
        match &o.outcome {
            Outcome::Failed { reason } => {
                if reason.trim().is_empty() {
                    return Err("failure needs a reason".into());
                }
                failed_indices.push(i);
            }
            Outcome::Complete { responses: values } => {
                if values.len() != responses.len() || values.iter().any(|v| !v.is_finite()) {
                    return Err("finite dimension-matched responses required".into());
                }
                let key: Vec<_> = point
                    .iter()
                    .map(|x| if *x == 0. { 0 } else { x.to_bits() })
                    .collect();
                if !seen.insert(key) {
                    return Err("duplicate completed response observation".into());
                }
                let objective = compose(values)?;
                if !objective.is_finite() {
                    return Err("nonfinite observed composite objective".into());
                }
                if incumbent_coordinates.iter().all(|(j, v)| o.values[*j] == *v)
                    && best.is_none_or(|(_, b)| objective < b) {
                    best = Some((i, objective));
                }
                training_indices.push(i);
                x.extend(point);
                y.push(values.clone());
            }
        }
    }
    if training_indices.len() < problem.parameters.len() + 1
        || training_indices.len() > config.maximum_training_rows
    {
        return Err(
            "dimension+1..maximum_training_rows complete response observations required".into(),
        );
    }
    let (observed_best_index, observed_best_objective) = best
        .ok_or("no completed observation matches incumbent conditions")?;
    let mut query = vec![];
    seen.clear();
    for c in candidates {
        let point = problem.normalized(c)?;
        if old_points
            .iter()
            .any(|p| p.iter().zip(&point).all(|(a, b)| (a - b).abs() <= 1e-10))
        {
            return Err("candidate repeats a completed or failed observation".into());
        }
        let key: Vec<_> = point
            .iter()
            .map(|x| if *x == 0. { 0 } else { x.to_bits() })
            .collect();
        if !seen.insert(key) {
            return Err("duplicate candidate".into());
        }
        query.extend(point);
    }
    let xs = Array2::from_shape_vec((y.len(), problem.parameters.len()), x)
        .map_err(|e| e.to_string())?;
    let qs = Array2::from_shape_vec((candidates.len(), problem.parameters.len()), query)
        .map_err(|e| e.to_string())?;
    let mut means = vec![vec![0.; responses.len()]; candidates.len()];
    let mut stds = means.clone();
    // n_start(0) uses exactly the declared initial hyperparameters and avoids
    // the backend's entropy-seeded single-restart path. Its deterministic
    // likelihood optimizer still tunes those hyperparameters.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
        for j in 0..responses.len() {
            let data = Dataset::new(xs.clone(), Array1::from_iter(y.iter().map(|r| r[j])));
            let gp = GpParams::new(ConstantMean::default(), Matern52Corr::default())
                .n_start(0)
                .fit(&data)
                .map_err(|e| e.to_string())?;
            let (mean, var) = gp.predict_valvar(&qs).map_err(|e| e.to_string())?;
            for i in 0..candidates.len() {
                if !mean[i].is_finite() || !var[i].is_finite() || var[i] < 0. {
                    return Err("invalid response posterior".into());
                }
                means[i][j] = mean[i];
                stds[i][j] = var[i].sqrt() * config.response_std_scales.get(j).copied().unwrap_or(1.);
                if !stds[i][j].is_finite() {
                    return Err("nonfinite scaled response posterior".into());
                }
            }
        }
        Ok(())
    }))
    .map_err(|_| "response GP failed internally; no proposal fabricated".to_string())??;
    let normals = gaussian_samples(config.posterior_samples, responses.len(), config.seed);
    let mut ranked = vec![];
    for i in 0..candidates.len() {
        let objective_at_mean = compose(&means[i])?;
        if !objective_at_mean.is_finite() {
            return Err("nonfinite objective at posterior mean".into());
        }
        let (expected_improvement, posterior_mean_objective) = expected_improvement(
            &means[i],
            &stds[i],
            &normals,
            observed_best_objective,
            &compose,
        )?;
        ranked.push(Candidate {
            index: i,
            values: candidates[i].clone(),
            response_mean: means[i].clone(),
            response_std: stds[i].clone(),
            objective_at_mean,
            posterior_mean_objective,
            expected_improvement,
        });
    }
    let selected = ranked
        .iter()
        .max_by(|a, b| {
            a.expected_improvement
                .total_cmp(&b.expected_improvement)
                .then_with(|| b.objective_at_mean.total_cmp(&a.objective_at_mean))
                .then_with(|| b.index.cmp(&a.index))
        })
        .unwrap()
        .clone();
    Ok(Report { selected, candidates:ranked, training_indices, failed_indices, observed_best_index,
        observed_best_objective, seed:config.seed, posterior_samples:config.posterior_samples,
        method:"Independent constant-mean Matern5/2 response GPs; seeded common-random-number Monte Carlo composite EI on a finite candidate set; single deterministic hyperparameter start".into(),
        response_std_scales:config.response_std_scales.clone(),
        incumbent_conditions:config.incumbent_conditions.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bayesian::Parameter;

    #[test]
    fn conditioned_incumbent_keeps_auxiliary_training_rows() {
        let p = Problem { context_id:"conditioned".into(), parameters:vec![
            Parameter{name:"x".into(),unit:"1".into(),bounds:[0.,1.]},
            Parameter{name:"step".into(),unit:"s".into(),bounds:[0.1,0.2]}],
            objective_name:"response".into(),objective_unit:"m/s".into(),constraints:vec![] };
        let observations:Vec<_> = [(0.,0.1,2.),(0.5,0.1,1.),(1.,0.1,3.),(0.25,0.2,-5.)]
            .into_iter().map(|(x,t,r)| Observation {context_id:p.context_id.clone(),values:vec![x,t],
                outcome:Outcome::Complete{responses:vec![r]},evidence:format!("analytic:{x}:{t}")}).collect();
        let responses = vec![Response{name:"speed".into(),unit:"m/s".into()}];
        let candidates = vec![vec![0.3,0.1],vec![0.7,0.1]];
        let mut cfg = Config{seed:23,posterior_samples:128,maximum_training_rows:16,
            response_std_scales:vec![],incumbent_conditions:vec![]};
        let raw = rank_candidates(&p,&responses,&observations,&candidates,&cfg,|r|Ok(r[0])).unwrap();
        assert_eq!(raw.observed_best_index,3);
        cfg.incumbent_conditions = vec![FixedParameter{name:"step".into(),value:0.1}];
        let conditioned = rank_candidates(&p,&responses,&observations,&candidates,&cfg,|r|Ok(r[0])).unwrap();
        assert_eq!(conditioned.observed_best_index,1);
        assert_eq!(conditioned.observed_best_objective,1.);
        assert_eq!(conditioned.training_indices,vec![0,1,2,3]);
        for (a,b) in raw.candidates.iter().zip(&conditioned.candidates) {
            assert_eq!(a.response_mean,b.response_mean); assert_eq!(a.response_std,b.response_std);
            assert!(b.expected_improvement >= a.expected_improvement);
        }
        cfg.incumbent_conditions[0].value = 0.15;
        assert!(rank_candidates(&p,&responses,&observations,&candidates,&cfg,|r|Ok(r[0]))
            .unwrap_err().contains("no completed observation"));
        cfg.incumbent_conditions[0].name = "absent".into();
        assert!(rank_candidates(&p,&responses,&observations,&candidates,&cfg,|r|Ok(r[0])).is_err());
    }

    #[test]
    fn smooth_response_reveals_unsampled_minima_of_an_oscillatory_objective() {
        let problem = Problem {
            context_id: "oscillatory-composition".into(),
            parameters: vec![Parameter {
                name: "x".into(),
                unit: "1".into(),
                bounds: [0., 1.],
            }],
            objective_name: "sin(8*pi*h)".into(),
            objective_unit: "1".into(),
            constraints: vec![],
        };
        // All observed scalar objectives are zero, but their intermediate
        // responses identify the monotonic map h(x)=x.
        let observations: Vec<_> = [0., 0.5, 1.]
            .into_iter()
            .map(|x| Observation {
                context_id: problem.context_id.clone(),
                values: vec![x],
                outcome: Outcome::Complete { responses: vec![x] },
                evidence: format!("analytic:{x}"),
            })
            .collect();
        let candidates: Vec<_> = (1..100)
            .filter(|i| *i != 50)
            .map(|i| vec![i as f64 / 100.])
            .collect();
        let result = rank_candidates(
            &problem,
            &[Response {
                name: "h".into(),
                unit: "1".into(),
            }],
            &observations,
            &candidates,
            &Config {
                seed: 32,
                posterior_samples: 1024,
                maximum_training_rows: 16,
                response_std_scales: vec![],
                incumbent_conditions: vec![],
            },
            |r| Ok((8. * std::f64::consts::PI * r[0]).sin()),
        )
        .unwrap();
        let measured = (8. * std::f64::consts::PI * result.selected.values[0]).sin();
        assert!(measured < -0.95, "selected true objective {measured}");
    }

    #[test]
    fn nonlinear_composition_integrates_uncertainty_and_linear_ei_matches_analytic_case() {
        let normals = gaussian_samples(65536, 1, 49);
        let (ei, mean) = expected_improvement(&[0.], &[1.], &normals, 0., &|r| Ok(r[0])).unwrap();
        assert!((ei - 1. / std::f64::consts::TAU.sqrt()).abs() < 0.008);
        assert!(mean.abs() < 0.015);
        let (ei, mean) =
            expected_improvement(&[0.], &[1.], &normals, 0., &|r| Ok(-r[0] * r[0])).unwrap();
        assert!((ei - 1.).abs() < 0.025);
        assert!((mean + 1.).abs() < 0.025);
        let exact = expected_improvement(&[3.], &[0.], &normals, 5., &|r| Ok(r[0])).unwrap();
        assert_eq!(exact, (2., 3.));
    }

    #[test]
    fn response_gp_is_reproducible_and_preserves_physical_units() {
        let p = Problem {
            context_id: "unit-test".into(),
            parameters: vec![Parameter {
                name: "x".into(),
                unit: "m".into(),
                bounds: [0., 1.],
            }],
            objective_name: "quadratic".into(),
            objective_unit: "m2".into(),
            constraints: vec![],
        };
        let responses = vec![Response {
            name: "position".into(),
            unit: "m".into(),
        }];
        let observations: Vec<_> = [0., 0.25, 0.75, 1.]
            .into_iter()
            .map(|x| Observation {
                context_id: p.context_id.clone(),
                values: vec![x],
                outcome: Outcome::Complete {
                    responses: vec![2. * x + 1.],
                },
                evidence: format!("analytic:{x}"),
            })
            .collect();
        let candidates = vec![vec![0.45], vec![0.55]];
        let cfg = Config {
            seed: 9,
            posterior_samples: 128,
            maximum_training_rows: 32,
            response_std_scales: vec![],
            incumbent_conditions: vec![],
        };
        let objective = |r: &[f64]| Ok((r[0] - 2.).powi(2));
        let a =
            rank_candidates(&p, &responses, &observations, &candidates, &cfg, objective).unwrap();
        let b =
            rank_candidates(&p, &responses, &observations, &candidates, &cfg, objective).unwrap();
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            serde_json::to_value(&b).unwrap()
        );
        assert_eq!(a.training_indices, vec![0, 1, 2, 3]);
        let scaled_config = Config { response_std_scales: vec![2.], ..cfg.clone() };
        let scaled = rank_candidates(&p, &responses, &observations, &candidates, &scaled_config, objective).unwrap();
        for (raw, adjusted) in a.candidates.iter().zip(&scaled.candidates) {
            assert_eq!(raw.response_mean, adjusted.response_mean);
            assert_eq!(raw.objective_at_mean, adjusted.objective_at_mean);
            assert_eq!(adjusted.response_std[0], raw.response_std[0] * 2.);
        }
        assert_eq!(scaled.response_std_scales, vec![2.]);
        for scales in [vec![0.], vec![-1.], vec![f64::NAN], vec![1., 2.]] {
            let bad = Config { response_std_scales: scales, ..cfg.clone() };
            assert!(rank_candidates(&p, &responses, &observations, &candidates, &bad, objective).unwrap_err().contains("scales"));
        }
        for c in &a.candidates {
            assert!((c.response_mean[0] - (2. * c.values[0] + 1.)).abs() < 0.05);
        }
        let mut bad = observations.clone();
        bad[0].context_id = "other".into();
        assert!(
            rank_candidates(&p, &responses, &bad, &candidates, &cfg, objective)
                .unwrap_err()
                .contains("context")
        );
        let mut bad = observations.clone();
        bad[0].outcome = Outcome::Complete { responses: vec![] };
        assert!(
            rank_candidates(&p, &responses, &bad, &candidates, &cfg, objective)
                .unwrap_err()
                .contains("dimension")
        );
        let mut failed = observations;
        failed.push(Observation {
            context_id: p.context_id.clone(),
            values: vec![0.5],
            outcome: Outcome::Failed {
                reason: "simulator stopped".into(),
            },
            evidence: "failure-log".into(),
        });
        assert!(
            rank_candidates(&p, &responses, &failed, &[vec![0.5]], &cfg, objective)
                .unwrap_err()
                .contains("repeats")
        );
    }
}

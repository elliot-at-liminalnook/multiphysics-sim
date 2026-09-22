//! Interchangeable proposal policies over one immutable experiment history.
//! Hosts own scheduling/timing; neither optimizer can change physical gates.
use serde::{Deserialize, Serialize};
use sim_solve::bayesian::{self, Observation, Problem};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Algorithm {
    Bayesian,
    CmaEs,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub seed: u64,
    pub initial_design: usize,
    pub acquisition_starts: usize,
    pub maximum_training_rows: usize,
    pub cma_population: usize,
    pub cma_sigma: f64,
}
// Keep experiment identity, purpose and attempt in separate fields. Adding an
// attempt to the master seed makes neighboring experiments share random streams.
fn attempt_seed(master: u64, attempt: usize, purpose: &str) -> u64 {
    let mut hash = blake3::Hasher::new();
    hash.update(b"sim-search-attempt-seed-v1\0");
    hash.update(&master.to_le_bytes());
    hash.update(&(attempt as u64).to_le_bytes());
    hash.update(&(purpose.len() as u64).to_le_bytes());
    hash.update(purpose.as_bytes());
    u64::from_le_bytes(hash.finalize().as_bytes()[..8].try_into().unwrap())
}
pub fn proposal_method(
    problem: &Problem,
    history: &[Observation],
    algorithm: Algorithm,
    settings: &Settings,
) -> &'static str {
    if history.is_empty() {
        return "baseline";
    }
    if algorithm == Algorithm::CmaEs {
        return "cma_es_deterministic_restarts";
    }
    if history.len() <= settings.initial_design {
        return "latin_hypercube";
    }
    let complete = history
        .iter()
        .filter(|o| matches!(o.outcome, bayesian::Outcome::Complete { .. }))
        .count();
    if complete < problem.parameters.len() + 1 {
        "local_feasibility_bootstrap"
    } else {
        "bayesian_log_ei"
    }
}
/// Attempt zero is the identical authored baseline for each algorithm. Later
/// failed/screened attempts remain in history and consume the caller's budget.
pub fn suggest(
    problem: &Problem,
    baseline: &[f64],
    history: &[Observation],
    algorithm: Algorithm,
    settings: &Settings,
) -> Result<Vec<f64>, String> {
    problem.validate()?;
    problem.normalized(baseline)?;
    if settings.initial_design < problem.parameters.len() + 1 {
        return Err("initial design needs at least dimension plus one".into());
    }
    if history.is_empty() {
        return Ok(baseline.to_vec());
    }
    if history[0].values != baseline
        || history
            .iter()
            .any(|o| o.context_id != problem.context_id || o.evidence.trim().is_empty())
    {
        return Err("search baseline/context/evidence mismatch".into());
    }
    match algorithm {
        Algorithm::CmaEs => sim_solve::evolution::suggest(
            problem,
            &history[1..],
            &sim_solve::evolution::Config {
                seed: settings.seed,
                population: settings.cma_population,
                sigma: settings.cma_sigma,
                initial_mean: baseline.to_vec(),
            },
        ),
        Algorithm::Bayesian => {
            if history.len() <= settings.initial_design {
                Ok(
                    bayesian::initial_design(problem, settings.initial_design, settings.seed)?
                        [history.len() - 1]
                        .clone(),
                )
            } else if proposal_method(problem, history, algorithm, settings)
                == "local_feasibility_bootstrap"
            {
                // Failures cannot be fabricated into objective observations.
                // Gather enough valid rows locally before fitting the GP.
                let center = history
                    .iter()
                    .filter_map(|o| match o.outcome {
                        bayesian::Outcome::Complete { objective, .. } => {
                            Some((objective, &o.values))
                        }
                        _ => None,
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map_or(baseline, |(_, v)| v.as_slice());
                let mut local = problem.clone();
                for (p, x) in local.parameters.iter_mut().zip(center) {
                    let radius = (p.bounds[1] - p.bounds[0]) * 0.05;
                    p.bounds = [(x - radius).max(p.bounds[0]), (x + radius).min(p.bounds[1])];
                }
                Ok(bayesian::initial_design(
                    &local,
                    1,
                    attempt_seed(settings.seed, history.len(), "feasibility-bootstrap"),
                )?
                .remove(0))
            } else {
                Ok(bayesian::suggest(
                    problem,
                    history,
                    &bayesian::Config {
                        seed: attempt_seed(settings.seed, history.len(), "bayesian-acquisition"),
                        acquisition_starts: settings.acquisition_starts,
                        maximum_training_rows: settings.maximum_training_rows,
                    },
                )?
                .values)
            }
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trial {
    pub algorithm: Algorithm,
    pub seed: u64,
    pub attempt: usize,
    pub proposal_method: String,
    pub observation: Observation,
    pub proposal_wall_s: f64,
    pub preparation_wall_s: f64,
    pub simulation_wall_s: f64,
    /// Includes proposal, preparation, simulation and capture persistence.
    pub total_wall_s: f64,
    pub charged_simulation_s: f64,
    pub actual_simulation_s: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Progress {
    pub attempts: usize,
    pub failed_attempts: usize,
    pub wall_hours: f64,
    pub charged_simulation_s: f64,
    pub actual_simulation_s: f64,
    pub best_speed_m_s: Option<f64>,
    pub improvement_m_s: Option<f64>,
    pub improvement_per_wall_hour: Option<f64>,
}
pub fn progress(trials: &[Trial]) -> Result<Progress, String> {
    let mut p = Progress {
        attempts: 0,
        failed_attempts: 0,
        wall_hours: 0.,
        charged_simulation_s: 0.,
        actual_simulation_s: 0.,
        best_speed_m_s: None,
        improvement_m_s: None,
        improvement_per_wall_hour: None,
    };
    let mut baseline = None;
    for (i, t) in trials.iter().enumerate() {
        if t.attempt != i
            || trials.first().is_some_and(|first| {
                first.algorithm != t.algorithm
                    || first.seed != t.seed
                    || first.observation.context_id != t.observation.context_id
            })
            || [
                t.total_wall_s,
                t.proposal_wall_s,
                t.preparation_wall_s,
                t.simulation_wall_s,
                t.charged_simulation_s,
                t.actual_simulation_s,
            ]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.)
            || t.actual_simulation_s > t.charged_simulation_s + 1e-8
        {
            return Err("invalid matched trial order, timing or simulation budget".into());
        }
        p.attempts += 1;
        p.wall_hours += t.total_wall_s / 3600.;
        p.charged_simulation_s += t.charged_simulation_s;
        p.actual_simulation_s += t.actual_simulation_s;
        match &t.observation.outcome {
            bayesian::Outcome::Complete {
                objective,
                residuals,
            } if objective.is_finite() && residuals.is_empty() => {
                let speed = -*objective;
                if i == 0 {
                    baseline = Some(speed);
                }
                p.best_speed_m_s = Some(p.best_speed_m_s.map_or(speed, |x| x.max(speed)));
            }
            bayesian::Outcome::Failed { .. } => p.failed_attempts += 1,
            _ => return Err("invalid comparison outcome".into()),
        }
    }
    p.improvement_m_s = p.best_speed_m_s.zip(baseline).map(|(b, a)| b - a);
    p.improvement_per_wall_hour = p
        .improvement_m_s
        .filter(|_| p.wall_hours > 0.)
        .map(|v| v / p.wall_hours);
    Ok(p)
}

#[cfg(test)]
mod seed_tests {
    use super::attempt_seed;
    #[test]
    fn adjacent_experiments_and_proposal_purposes_do_not_share_attempt_seeds() {
        let mut seen = std::collections::BTreeSet::new();
        for master in [2301, 2302, 1002301] {
            for attempt in 0..66 {
                for purpose in ["feasibility-bootstrap", "bayesian-acquisition"] {
                    let seed = attempt_seed(master, attempt, purpose);
                    assert_eq!(seed, attempt_seed(master, attempt, purpose));
                    assert!(
                        seen.insert(seed),
                        "reused seed at {master}/{attempt}/{purpose}"
                    );
                }
            }
        }
        assert_ne!(
            attempt_seed(2301, 26, "bayesian-acquisition"),
            attempt_seed(2302, 25, "bayesian-acquisition")
        );
    }
}

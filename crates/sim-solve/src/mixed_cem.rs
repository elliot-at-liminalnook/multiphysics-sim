//! Persistent mixed categorical/continuous cross-entropy experiment selection.
//! Uses the existing experiment schema and seeded RNG, not a Bayesian surrogate.
//! Physical feasibility is lexicographic: objective never buys a violation.
use crate::bayesian::{Outcome, Problem as ContinuousProblem};
use rand::{Rng, SeedableRng};
use rand_xoshiro::Xoshiro256Plus;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Category {
    pub name: String,
    pub labels: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Problem {
    pub continuous: ContinuousProblem,
    pub categories: Vec<Category>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub seed: u64,
    pub population: usize,
    pub elites: usize,
    pub learning_rate: f64,
    /// Probability of a uniform draw of the entire mixed point.
    pub exploration_probability: f64,
    /// Standard deviation floor in normalized continuous coordinates.
    pub minimum_std: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub context_id: String,
    pub generation: u64,
    pub means: Vec<f64>,
    pub stds: Vec<f64>,
    pub probabilities: Vec<Vec<f64>>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub continuous: Vec<f64>,
    pub categories: Vec<usize>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    pub state: State,
    pub points: Vec<Point>,
    pub uniform_draws: usize,
    pub truncated_normal_fallbacks: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub point: Point,
    pub outcome: Outcome,
    pub evidence: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Update {
    pub state: State,
    pub elite_indices: Vec<usize>,
    pub failed_indices: Vec<usize>,
    pub best_feasible_index: Option<usize>,
    pub scope: String,
}

impl Problem {
    fn validate(&self, config: &Config) -> Result<(), String> {
        self.continuous.validate()?;
        if self.categories.is_empty() || self.categories.len() > 64 {
            return Err("mixed CEM needs 1..64 categorical parameters".into());
        }
        let mut names: HashSet<&str> = self
            .continuous
            .parameters
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        for c in &self.categories {
            let unique: HashSet<_> = c.labels.iter().collect();
            if c.name.trim().is_empty()
                || !names.insert(&c.name)
                || c.labels.is_empty()
                || c.labels.len() > 4096
                || unique.len() != c.labels.len()
                || c.labels.iter().any(|s| s.trim().is_empty())
            {
                return Err(
                    "categorical names and 1..4096 distinct nonempty labels required".into(),
                );
            }
        }
        if !(2..=4096).contains(&config.population)
            || config.elites < 2
            || config.elites > config.population
            || !config.learning_rate.is_finite()
            || !(0.0 < config.learning_rate && config.learning_rate <= 1.0)
            || !config.exploration_probability.is_finite()
            || !(0.0 < config.exploration_probability && config.exploration_probability <= 1.0)
            || !config.minimum_std.is_finite()
            || !(0.0 < config.minimum_std && config.minimum_std <= 0.5)
        {
            return Err("invalid CEM population, elite count, update rate, exploration probability or spread floor".into());
        }
        Ok(())
    }
    fn validate_state(&self, config: &Config, state: &State) -> Result<(), String> {
        self.validate(config)?;
        let n = self.continuous.parameters.len();
        if state.context_id != self.continuous.context_id
            || state.means.len() != n
            || state.stds.len() != n
            || state.probabilities.len() != self.categories.len()
            || state
                .means
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || state
                .stds
                .iter()
                .any(|v| !v.is_finite() || *v < config.minimum_std || *v > 0.5)
        {
            return Err("invalid or mismatched CEM state".into());
        }
        for (p, c) in state.probabilities.iter().zip(&self.categories) {
            if p.len() != c.labels.len()
                || p.iter().any(|v| !v.is_finite() || *v < 0.0)
                || (p.iter().sum::<f64>() - 1.0).abs() > 1e-12
            {
                return Err(
                    "categorical probabilities must be finite, nonnegative and sum to one".into(),
                );
            }
        }
        Ok(())
    }
}

pub fn initialize(problem: &Problem, config: &Config) -> Result<State, String> {
    problem.validate(config)?;
    Ok(State {
        context_id: problem.continuous.context_id.clone(),
        generation: 0,
        means: vec![0.5; problem.continuous.parameters.len()],
        stds: vec![0.5; problem.continuous.parameters.len()],
        probabilities: problem
            .categories
            .iter()
            .map(|c| vec![1.0 / c.labels.len() as f64; c.labels.len()])
            .collect(),
    })
}

pub fn ask(problem: &Problem, config: &Config, state: &State) -> Result<Batch, String> {
    problem.validate_state(config, state)?;
    let seed = config
        .seed
        .wrapping_add(state.generation.wrapping_mul(0x9e3779b97f4a7c15));
    let mut rng = Xoshiro256Plus::seed_from_u64(seed);
    let mut batch = Batch {
        state: state.clone(),
        points: Vec::new(),
        uniform_draws: 0,
        truncated_normal_fallbacks: 0,
    };
    let mut seen = HashSet::new();
    // Uniform mixture preserves exploration even when elites assign zero mass
    // to a category. Bounded Gaussian rejection never clips samples to a limit.
    for _ in 0..config.population * 100 {
        if batch.points.len() == config.population {
            return Ok(batch);
        }
        let uniform = rng.r#gen::<f64>() < config.exploration_probability;
        if uniform {
            batch.uniform_draws += 1;
        }
        let mut continuous = Vec::new();
        for (j, p) in problem.continuous.parameters.iter().enumerate() {
            let mut x = None;
            if !uniform {
                for _ in 0..128 {
                    let u = 1.0 - rng.r#gen::<f64>();
                    let z =
                        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * rng.r#gen::<f64>()).cos();
                    let y = state.means[j] + state.stds[j] * z;
                    if (0.0..=1.0).contains(&y) {
                        x = Some(y);
                        break;
                    }
                }
                if x.is_none() {
                    batch.truncated_normal_fallbacks += 1;
                }
            }
            let x = x.unwrap_or_else(|| rng.r#gen::<f64>());
            continuous.push(p.bounds[0] + x * (p.bounds[1] - p.bounds[0]));
        }
        problem.continuous.normalized(&continuous)?;
        let categories = problem
            .categories
            .iter()
            .enumerate()
            .map(|(j, c)| {
                if uniform {
                    rng.gen_range(0..c.labels.len())
                } else {
                    let draw = rng.r#gen::<f64>();
                    let mut sum = 0.0;
                    state.probabilities[j]
                        .iter()
                        .position(|p| {
                            sum += p;
                            draw < sum
                        })
                        .unwrap_or(c.labels.len() - 1)
                }
            })
            .collect::<Vec<_>>();
        let key = (
            continuous
                .iter()
                .map(|v| if *v == 0.0 { 0 } else { v.to_bits() })
                .collect::<Vec<_>>(),
            categories.clone(),
        );
        if seen.insert(key) {
            batch.points.push(Point {
                continuous,
                categories,
            });
        }
    }
    Err("could not draw a distinct representable CEM population".into())
}

pub fn tell(
    problem: &Problem,
    config: &Config,
    batch: &Batch,
    observations: &[Observation],
) -> Result<Update, String> {
    // Recomputing the cheap seeded draw rejects stale, edited or mismatched
    // pending populations. Callers persist the batch before running physics.
    if ask(problem, config, &batch.state)? != *batch || observations.len() != batch.points.len() {
        return Err("CEM update must match the complete pending batch".into());
    }
    let mut ranked = Vec::new();
    let mut failed = Vec::new();
    for (i, (point, observation)) in batch.points.iter().zip(observations).enumerate() {
        if point != &observation.point || observation.evidence.trim().is_empty() {
            return Err("CEM observation must match its pending point and retain evidence".into());
        }
        match &observation.outcome {
            Outcome::Failed { reason } => {
                if reason.trim().is_empty() {
                    return Err("failed experiment needs a reason".into());
                }
                failed.push(i);
            }
            Outcome::Complete {
                objective,
                residuals,
            } => {
                if !objective.is_finite()
                    || residuals.len() != problem.continuous.constraints.len()
                    || residuals.iter().any(|r| !r.is_finite())
                {
                    return Err(
                        "complete CEM outcome needs finite objective and matching residuals".into(),
                    );
                }
                let feasible = residuals.iter().all(|v| *v <= 0.0);
                let mut maximum = 0.0_f64;
                for (r, c) in residuals.iter().zip(&problem.continuous.constraints) {
                    let scaled = r / c.scale;
                    if !scaled.is_finite() || (*r != 0.0 && scaled == 0.0) {
                        return Err("constraint scaling loses the observed boundary".into());
                    }
                    maximum = maximum.max(scaled);
                }
                ranked.push((i, feasible, maximum, *objective));
            }
        }
    }
    ranked.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.2.total_cmp(&b.2))
            .then_with(|| a.3.total_cmp(&b.3))
            .then(a.0.cmp(&b.0))
    });
    let best_feasible_index = ranked.iter().find(|r| r.1).map(|r| r.0);
    let elites = ranked
        .iter()
        .take(config.elites)
        .map(|r| r.0)
        .collect::<Vec<_>>();
    let mut next = batch.state.clone();
    next.generation = next
        .generation
        .checked_add(1)
        .ok_or("CEM generation overflow")?;
    if !elites.is_empty() {
        let normalized = elites
            .iter()
            .map(|&i| problem.continuous.normalized(&batch.points[i].continuous))
            .collect::<Result<Vec<_>, _>>()?;
        let n = normalized.len() as f64;
        let rate = config.learning_rate;
        for j in 0..next.means.len() {
            let mean = normalized.iter().map(|p| p[j]).sum::<f64>() / n;
            let variance = normalized
                .iter()
                .map(|p| (p[j] - mean).powi(2))
                .sum::<f64>()
                / n;
            let old = next.means[j];
            next.means[j] = (1.0 - rate) * old + rate * mean;
            next.stds[j] = ((1.0 - rate) * next.stds[j].powi(2)
                + rate * variance
                + rate * (1.0 - rate) * (old - mean).powi(2))
            .sqrt()
            .clamp(config.minimum_std, 0.5);
        }
        for (j, p) in next.probabilities.iter_mut().enumerate() {
            for (k, v) in p.iter_mut().enumerate() {
                let count = elites
                    .iter()
                    .filter(|&&i| batch.points[i].categories[j] == k)
                    .count();
                *v = (1.0 - rate) * (*v) + rate * (count as f64 / n);
            }
            let total = p.iter().sum::<f64>();
            for v in p {
                *v /= total;
            }
        }
    }
    problem.validate_state(config, &next)?;
    Ok(Update {state:next,elite_indices:elites,failed_indices:failed,best_feasible_index,
        scope:"Mixed CEM adaptation with diagonal bounded Gaussian and categorical distributions, uniform exploration, spread floor and lexicographic observed feasibility. Failed executions are retained but not imputed. An elite or sampled feasible point is not a physical/global certificate.".into()})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bayesian::{Constraint, Parameter};
    fn fixture() -> (Problem, Config) {
        (
            Problem {
                continuous: ContinuousProblem {
                    context_id: "analytic-v1".into(),
                    parameters: vec![Parameter {
                        name: "x".into(),
                        unit: "1".into(),
                        bounds: [0.0, 1.0],
                    }],
                    objective_name: "quadratic".into(),
                    objective_unit: "1".into(),
                    constraints: vec![Constraint {
                        name: "admissible".into(),
                        unit: "1".into(),
                        scale: 1.0,
                    }],
                },
                categories: vec![Category {
                    name: "mode".into(),
                    labels: vec!["a".into(), "b".into(), "c".into()],
                }],
            },
            Config {
                seed: 42,
                population: 32,
                elites: 8,
                learning_rate: 0.7,
                exploration_probability: 0.2,
                minimum_std: 0.02,
            },
        )
    }
    fn measure(batch: &Batch) -> Vec<Observation> {
        batch
            .points
            .iter()
            .map(|p| {
                let c = p.categories[0];
                let x = p.continuous[0];
                Observation {
                    point: p.clone(),
                    evidence: "analytic-known-solution".into(),
                    outcome: Outcome::Complete {
                        // Mode b looks best on objective but is strictly infeasible.
                        objective: if c == 1 {
                            -1000.0
                        } else {
                            (x - 0.73).powi(2) + if c == 0 { 0.2 } else { 0.0 }
                        },
                        residuals: vec![if c == 1 { 1e-12 } else { 0.6 - x }],
                    },
                }
            })
            .collect()
    }
    #[test]
    fn learns_mixed_constrained_optimum_and_replays_persisted_state() {
        let (p, c) = fixture();
        let mut state = initialize(&p, &c).unwrap();
        let mut best = f64::INFINITY;
        for _ in 0..16 {
            let batch = ask(&p, &c, &state).unwrap();
            let saved: State =
                serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
            assert_eq!(batch, ask(&p, &c, &saved).unwrap());
            let observations = measure(&batch);
            let update = tell(&p, &c, &batch, &observations).unwrap();
            if let Some(i) = update.best_feasible_index {
                assert_ne!(batch.points[i].categories[0], 1);
                if let Outcome::Complete { objective, .. } = observations[i].outcome {
                    best = best.min(objective);
                }
            }
            state = update.state;
        }
        assert!(best < 1e-5, "known feasible optimum not approached: {best}");
        assert!(state.probabilities[0][2] > 0.8);
        assert!((state.means[0] - 0.73).abs() < 0.05);
    }
    #[test]
    fn failed_batch_preserves_distribution_and_mismatched_updates_reject() {
        let (p, c) = fixture();
        let state = initialize(&p, &c).unwrap();
        let batch = ask(&p, &c, &state).unwrap();
        let observations = batch
            .points
            .iter()
            .map(|point| Observation {
                point: point.clone(),
                evidence: "failure-log".into(),
                outcome: Outcome::Failed {
                    reason: "execution unavailable".into(),
                },
            })
            .collect::<Vec<_>>();
        let u = tell(&p, &c, &batch, &observations).unwrap();
        assert!(u.elite_indices.is_empty());
        assert!(u.best_feasible_index.is_none());
        let mut expected = state;
        expected.generation += 1;
        assert_eq!(u.state, expected);
        let mut wrong = batch.clone();
        wrong.state.generation += 1;
        assert!(tell(&p, &c, &wrong, &observations).is_err());
        let mut wrong = observations.clone();
        wrong[0].point.categories[0] = 100;
        assert!(tell(&p, &c, &batch, &wrong).is_err());
        let mut wrong = measure(&batch);
        wrong[0].outcome = Outcome::Complete {
            objective: f64::NAN,
            residuals: vec![0.0],
        };
        assert!(tell(&p, &c, &batch, &wrong).is_err());
        let mut bad = c.clone();
        bad.exploration_probability = 0.0;
        assert!(initialize(&p, &bad).is_err());
    }
}

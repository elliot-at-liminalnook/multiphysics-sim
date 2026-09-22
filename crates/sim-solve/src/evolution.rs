//! Seeded, replayable CMA-ES proposals using the pinned native backend.
//! Full history reconstructs optimizer state; unavailable evaluations interrupt
//! sampling before a generation update. No synthetic observation is learned.
use crate::bayesian::{Observation, Outcome, Problem};
use cmaes::{CMAESOptions, DVector};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub seed: u64,
    pub population: usize,
    /// Initial standard deviation in normalized parameter coordinates.
    pub sigma: f64,
    pub initial_mean: Vec<f64>,
}

/// Mirror into the box rather than clamp a population onto its boundary.
fn reflect(x: f64) -> f64 {
    let y = x.rem_euclid(2.);
    if y > 1. { 2. - y } else { y }
}

pub fn suggest(
    problem: &Problem,
    history: &[Observation],
    config: &Config,
) -> Result<Vec<f64>, String> {
    problem.validate()?;
    if !problem.constraints.is_empty() {
        return Err("CMA adapter uses explicit failed outcomes for rejected candidates; numeric constraints are not implemented".into());
    }
    if !(4..=4096).contains(&config.population)
        || !config.sigma.is_finite()
        || config.sigma <= 0.
        || config.sigma > 1.
        || history.len() > 100_000
    {
        return Err("invalid CMA population, normalized spread or history size".into());
    }
    let mean = problem.normalized(&config.initial_mean)?;
    for o in history {
        problem.normalized(&o.values)?;
        if o.context_id != problem.context_id || o.evidence.trim().is_empty() {
            return Err("CMA history context/evidence mismatch".into());
        }
        match &o.outcome {
            Outcome::Complete {
                objective,
                residuals,
            } if objective.is_finite() && residuals.is_empty() => {}
            Outcome::Failed { reason } if !reason.trim().is_empty() => {}
            _ => return Err("invalid CMA observation".into()),
        }
    }
    let cursor = RefCell::new(0usize);
    let missing = RefCell::new(None);
    let mismatch = RefCell::new(false);
    let objective = |point: &DVector<f64>| {
        let values = point
            .iter()
            .zip(&problem.parameters)
            .map(|(x, p)| p.bounds[0] + reflect(*x) * (p.bounds[1] - p.bounds[0]))
            .collect::<Vec<_>>();
        let mut i = cursor.borrow_mut();
        if let Some(o) = history.get(*i) {
            if o.values != values {
                *mismatch.borrow_mut() = true;
                return f64::NAN;
            }
            *i += 1;
            match o.outcome {
                // Strict monotone transform: any finite valid objective beats failure.
                Outcome::Complete { objective, .. } => objective.atan(),
                Outcome::Failed { .. } => std::f64::consts::PI,
            }
        } else {
            *missing.borrow_mut() = Some(values);
            f64::NAN // Native backend stops before adapting an incomplete generation.
        }
    };
    for restart in 0..=history.len() / config.population {
        let mut optimizer = CMAESOptions::new(mean.clone(), config.sigma)
            .population_size(config.population)
            .seed(
                config
                    .seed
                    .wrapping_add((restart as u64).wrapping_mul(0x9e3779b97f4a7c15)),
            )
            .build(objective)
            .map_err(|e| format!("CMA initialization: {e:?}"))?;
        for _ in 0..=history.len() / config.population {
            let stop = optimizer.next();
            if *mismatch.borrow() {
                return Err("CMA history is not the deterministic proposal prefix".into());
            }
            if let Some(point) = missing.borrow_mut().take() {
                return Ok(point);
            }
            if stop.is_some() {
                break;
            }
        }
    }
    Err("CMA replay did not produce a proposal".into())
}

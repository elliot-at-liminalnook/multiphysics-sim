//! Adaptive local/global acquisition using the shared Bayesian backend.
//! Inspired by trust-region BO, but not a reproduction of TuRBO/TREGO: there is
//! one isotropic acquisition region, a global GP and periodic global proposals.
//! No convergence theorem or physical search-space exhaustion is claimed.
use crate::bayesian::{Config, Observation, Outcome, Problem, Proposal, suggest_in_region};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionConfig {
    /// Half-width as a fraction of each declared parameter interval.
    pub initial_radius: f64,
    pub minimum_radius: f64,
    pub maximum_radius: f64,
    pub successes_to_expand: usize,
    pub failures_to_contract: usize,
    pub global_every: usize,
    /// Only controls radius adaptation; any observed improvement can move the center.
    pub relative_success: f64,
}
impl RegionConfig {
    fn validate(&self) -> Result<(), String> {
        if !self.initial_radius.is_finite()
            || !self.minimum_radius.is_finite()
            || !self.maximum_radius.is_finite()
            || self.minimum_radius <= 0.
            || self.minimum_radius > self.initial_radius
            || self.initial_radius > self.maximum_radius
            || self.maximum_radius > 1.
            || self.successes_to_expand == 0
            || self.failures_to_contract == 0
            || self.global_every < 2
            || !self.relative_success.is_finite()
            || self.relative_success < 0.
        {
            return Err(
                "finite ordered normalized region radii and positive adaptation counts required"
                    .into(),
            );
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionState {
    pub context_id: String,
    pub config: RegionConfig,
    pub radius: f64,
    pub successes: usize,
    pub failures: usize,
    pub proposals_made: usize,
    /// Full prefix retained so a resumed run cannot silently change its history.
    pub history: Vec<Observation>,
    pub pending_values: Vec<f64>,
}
#[derive(Clone, Debug, Serialize)]
pub struct RegionProposal {
    pub proposal: Proposal,
    pub state: RegionState,
    pub mode: String,
    pub normalized_region: Option<Vec<[f64; 2]>>,
}
fn incumbent(problem: &Problem, observations: &[Observation]) -> Result<(usize, f64), String> {
    let mut best = None;
    for (i, o) in observations.iter().enumerate() {
        if o.context_id != problem.context_id {
            return Err("region observation context mismatch".into());
        }
        problem.normalized(&o.values)?;
        if let Outcome::Complete {
            objective,
            residuals,
        } = &o.outcome
        {
            if !objective.is_finite()
                || residuals.len() != problem.constraints.len()
                || residuals.iter().any(|v| !v.is_finite())
            {
                return Err("invalid complete region observation".into());
            }
            if residuals.iter().all(|v| *v <= 0.)
                && best.is_none_or(|(_, value)| *objective < value)
            {
                best = Some((i, *objective));
            }
        }
    }
    best.ok_or("local/global acquisition needs an observed feasible incumbent".into())
}
/// Consume exactly one newly observed pending point on continuation. The caller
/// persists the returned state and supplies the complete unchanged observation
/// prefix on the next call. Failures contract the optimizer region without
/// inventing objective values; all completed rows still train the global GP.
pub fn suggest(
    problem: &Problem,
    observations: &[Observation],
    config: &Config,
    region_config: &RegionConfig,
    previous: Option<&RegionState>,
) -> Result<RegionProposal, String> {
    problem.validate()?;
    region_config.validate()?;
    let (best_index, best_value) = incumbent(problem, observations)?;
    let mut state = match previous {
        None => RegionState {
            context_id: problem.context_id.clone(),
            config: region_config.clone(),
            radius: region_config.initial_radius,
            successes: 0,
            failures: 0,
            proposals_made: 0,
            history: vec![],
            pending_values: vec![],
        },
        Some(old) => {
            if old.context_id != problem.context_id
                || old.config != *region_config
                || !old.radius.is_finite()
                || old.radius < region_config.minimum_radius
                || old.radius > region_config.maximum_radius
                || old.proposals_made == 0
                || old.successes >= region_config.successes_to_expand
                || old.failures >= region_config.failures_to_contract
                || observations.len() != old.history.len() + 1
                || observations[..old.history.len()] != old.history
                || observations.last().unwrap().values != old.pending_values
            {
                return Err("region continuation requires unchanged context/history and exactly the pending observation".into());
            }
            let (_, before) = incumbent(problem, &old.history)?;
            let mut next = old.clone();
            let success =
                best_value < before - region_config.relative_success * before.abs().max(1e-12);
            if success {
                next.successes += 1;
                next.failures = 0;
            } else {
                next.successes = 0;
                next.failures += 1;
            }
            if next.successes >= region_config.successes_to_expand {
                next.radius = (next.radius * 2.).min(region_config.maximum_radius);
                next.successes = 0;
            }
            if next.failures >= region_config.failures_to_contract {
                next.radius *= 0.5;
                next.failures = 0;
            }
            next
        }
    };
    let reset = state.radius < region_config.minimum_radius;
    if reset {
        state.radius = region_config.initial_radius;
        state.successes = 0;
        state.failures = 0;
    }
    let next_count = state
        .proposals_made
        .checked_add(1)
        .ok_or("region proposal counter overflow")?;
    let global = reset || next_count % region_config.global_every == 0;
    let center = problem.normalized(&observations[best_index].values)?;
    let region = (!global).then(|| {
        center
            .iter()
            .map(|v| [(v - state.radius).max(0.), (v + state.radius).min(1.)])
            .collect::<Vec<_>>()
    });
    let proposal = suggest_in_region(problem, observations, config, region.as_deref())?;
    state.proposals_made = next_count;
    state.history = observations.to_vec();
    state.pending_values = proposal.values.clone();
    Ok(RegionProposal {
        proposal,
        state,
        mode: if reset {
            "global_after_radius_reset"
        } else if global {
            "periodic_global"
        } else {
            "local"
        }
        .into(),
        normalized_region: region,
    })
}

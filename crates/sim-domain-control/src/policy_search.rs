//! Deterministic paired random policy search over an episodic reward callback.
//! This initial optimizer keeps the best completed candidate; it is not PPO,
//! a gradient estimator, or a reproduction of a published ES implementation.
use crate::neural::Network;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchConfig {
    pub seed: u64,
    pub iterations: usize,
    pub perturbation: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trial {
    pub iteration: usize,
    pub sign: i8,
    pub score: Option<f64>,
    pub error: Option<String>,
    pub accepted: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchResult {
    pub policy: Network,
    pub initial_score: f64,
    pub best_score: f64,
    pub trials: Vec<Trial>,
}
pub fn search(
    initial: Network,
    config: &SearchConfig,
    mut evaluate: impl FnMut(&Network) -> Result<f64, String>,
) -> Result<SearchResult, String> {
    initial.validate()?;
    if config.iterations == 0 || config.iterations > 100_000 || !config.perturbation.is_finite() || config.perturbation <= 0.0 {
        return Err("policy search requires 1..100000 iterations and finite positive perturbation".into());
    }
    let score = evaluate(&initial)?;
    if !score.is_finite() { return Err("nonfinite baseline policy score".into()); }
    let mut result = SearchResult {policy:initial,initial_score:score,best_score:score,trials:vec![]};
    let mut state = config.seed;
    for iteration in 0..config.iterations {
        let center = result.policy.parameters();
        // SplitMix64 signs; only integer operations determine the perturbations.
        let direction: Vec<f64> = center.iter().map(|_| {
            state = state.wrapping_add(0x9e3779b97f4a7c15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
            if (z ^ (z >> 31)) & 1 == 0 { -1.0 } else { 1.0 }
        }).collect();
        for sign in [1i8, -1] {
            let values: Vec<_> = center.iter().zip(&direction).map(|(p,d)| p + f64::from(sign)*config.perturbation*d).collect();
            let candidate = result.policy.with_parameters(&values)?;
            let score = evaluate(&candidate).and_then(|score| if score.is_finite() { Ok(score) } else { Err("nonfinite candidate policy score".into()) });
            let accepted = score.as_ref().is_ok_and(|s| *s > result.best_score);
            let (value,error) = match score { Ok(v)=>(Some(v),None), Err(e)=>(None,Some(e)) };
            if accepted { result.best_score = value.unwrap(); result.policy = candidate; }
            result.trials.push(Trial {iteration,sign,score:value,error,accepted});
        }
    }
    Ok(result)
}

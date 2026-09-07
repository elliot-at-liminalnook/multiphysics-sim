//! Complete-episode policy evaluation through the production environment.
//! Partial rewards remain diagnostic; failed episodes never receive a score.
use crate::{
    embedded::Config,
    environment::{EmbeddedEnvironment, Task, Transition},
    session::Scene,
    walking_task::StepOutcome,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EpisodeEvaluation {
    pub score: Option<f64>,
    pub accrued_reward: f64,
    pub completed_actions: usize,
    pub final_transition: Option<Transition>,
    pub walking_outcomes: Vec<StepOutcome>,
    pub error: Option<String>,
}

pub fn evaluate_episode(
    scene: Scene,
    config: Config,
    task: Task,
    actions: &[Vec<f64>],
    seed: u64,
) -> EpisodeEvaluation {
    let mut report = EpisodeEvaluation {
        score: None,
        accrued_reward: 0.0,
        completed_actions: 0,
        final_transition: None,
        walking_outcomes: vec![],
        error: None,
    };
    let run = (|| -> Result<(), String> {
        let horizon = config.step_s * config.steps as f64;
        // The environment independently validates alignment of physics and
        // controller periods. Reject missing/extra actions before any stepping.
        let mut env = EmbeddedEnvironment::new(scene, config, task.clone(), seed)?;
        report.final_transition = Some(env.transition().clone());
        let expected = (horizon / task.period_s).round() as usize;
        if actions.len() != expected || expected == 0 {
            return Err("evaluation actions must cover the complete nonempty episode".into());
        }
        for action in actions {
            let t = env.step(action)?;
            report.accrued_reward += t.reward;
            report.completed_actions += 1;
            if let Some(outcome) = t.walking.as_ref().and_then(|w| w.outcome.clone()) {
                report.walking_outcomes.push(outcome);
            }
            report.final_transition = Some(t.clone());
            if t.terminated {
                return Err(format!(
                    "sampled task termination at {} s: {:?}",
                    t.time_s, t.termination_reasons
                ));
            }
        }
        if !env.transition().truncated || !report.accrued_reward.is_finite() {
            return Err("evaluation requires a finite reward and the declared horizon".into());
        }
        Ok(())
    })();
    match run {
        Ok(()) => report.score = Some(report.accrued_reward),
        Err(e) => report.error = Some(e),
    }
    report
}

/// Maximize the weakest case's reward per simulated second. Duration
/// normalization prevents longer episodes from winning through survival alone.
/// A completed score does not certify walking: retain independent task gates.
pub fn worst_reward_rate(reports: &[EpisodeEvaluation]) -> Result<f64, String> {
    if reports.is_empty() {
        return Err("policy evaluation suite is empty".into());
    }
    reports.iter().try_fold(f64::INFINITY, |worst, report| {
        if let Some(error) = &report.error {
            return Err(error.clone());
        }
        let score = report.score.ok_or("episode has no completed score")?;
        let transition = report
            .final_transition
            .as_ref()
            .ok_or("episode has no final transition")?;
        let rate = score / transition.time_s;
        if !transition.truncated
            || transition.terminated
            || transition.time_s <= 0.0
            || !rate.is_finite()
        {
            return Err("suite requires completed finite episodes".into());
        }
        Ok(worst.min(rate))
    })
}

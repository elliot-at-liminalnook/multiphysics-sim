//! Native Bayesian proposal adapter. Evaluations and journals remain portable.
use crate::experiment::{Journal, Objective, Proposal, Status};
use serde::{Deserialize, Serialize};
use sim_solve::bayesian::{self, Outcome};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub seed: u64,
    pub initial_design: usize,
    pub acquisition_starts: usize,
    pub maximum_training_rows: usize,
}

/// Recomputed from immutable journal history and explicit settings. No hidden
/// optimizer process state is required across a pause, crash or host restart.
pub fn ask(journal: &Journal, settings: &Settings) -> Result<Proposal, String> {
    journal.validate()?;
    if journal.pending().is_some() {
        return Err("finish or resume the pending trial before asking for another".into());
    }
    let space = &journal.experiment.spec.parameterization.space;
    let free = space
        .parameters
        .iter()
        .filter(|p| p.bounds[0] < p.bounds[1])
        .collect::<Vec<_>>();
    if free.is_empty() || free.iter().any(|p| p.integer) {
        return Err("this Bayesian adapter requires free continuous parameters; integer coordinates need a discrete selector, never rounding".into());
    }
    if settings.initial_design < free.len() + 1
        || settings.initial_design > 4096
        || settings.acquisition_starts == 0
        || settings.acquisition_starts > 128
        || settings.maximum_training_rows < free.len() + 1
        || settings.maximum_training_rows > 4096
    {
        return Err("invalid Bayesian motion search settings".into());
    }
    let problem = bayesian::Problem {
        context_id: journal.experiment.context_id.clone(),
        parameters: free
            .iter()
            .map(|p| bayesian::Parameter {
                name: p.name.clone(),
                unit: p.kind.unit().into(),
                bounds: p.bounds,
            })
            .collect(),
        objective_name: match journal.experiment.spec.objective {
            Objective::NetSpeed => "negative_net_speed",
            Objective::RewardRate => "negative_reward_rate",
        }
        .into(),
        objective_unit: match journal.experiment.spec.objective {
            Objective::NetSpeed => "m/s",
            Objective::RewardRate => "task_reward/s",
        }
        .into(),
        constraints: vec![],
    };
    let index = journal.trials.len();
    if index == 0 {
        return journal
            .experiment
            .propose(journal.experiment.spec.baseline.clone(), "baseline".into());
    }
    let (coordinates, method) = if index <= settings.initial_design {
        let design = bayesian::initial_design(&problem, settings.initial_design, settings.seed)?;
        (
            design[index - 1].clone(),
            format!("latin_hypercube:{}:{}", settings.seed, index - 1),
        )
    } else {
        let observations = journal
            .trials
            .iter()
            .map(|trial| {
                let outcome = match &trial.checkpoint {
                    Some(checkpoint) => {
                        match checkpoint.score(journal.experiment.spec.objective)? {
                            Some(score) => Outcome::Complete {
                                objective: -score,
                                residuals: vec![],
                            },
                            None => {
                                if matches!(
                                    checkpoint.status(),
                                    Status::Running | Status::Replaying
                                ) {
                                    return Err("unfinished trial cannot train the selector".into());
                                }
                                Outcome::Failed {
                                    reason: format!(
                                        "{:?}: {:?}",
                                        checkpoint.status(),
                                        checkpoint.final_transition.termination_reasons
                                    ),
                                }
                            }
                        }
                    }
                    None => Outcome::Failed {
                        reason: trial
                            .preparation_failure
                            .clone()
                            .ok_or("missing trial outcome")?,
                    },
                };
                Ok(bayesian::Observation {
                    context_id: problem.context_id.clone(),
                    values: free
                        .iter()
                        .map(|p| trial.proposal.values[&p.name])
                        .collect(),
                    outcome,
                    evidence: format!("journal:{}:trial:{}", problem.context_id, trial.proposal.id),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let seed = settings.seed.wrapping_add(index as u64);
        let proposal = bayesian::suggest(
            &problem,
            &observations,
            &bayesian::Config {
                seed,
                acquisition_starts: settings.acquisition_starts,
                maximum_training_rows: settings.maximum_training_rows,
            },
        )?;
        (proposal.values, format!("{}:{}", proposal.method, seed))
    };
    let mut values = journal.experiment.spec.baseline.clone();
    for (p, value) in free.iter().zip(coordinates) {
        values.insert(p.name.clone(), value);
    }
    let proposal = journal.experiment.propose(values, method)?;
    if journal.trials.iter().any(|t| t.proposal.id == proposal.id) {
        return Err("selector repeated a trial; choose an explicit exploration strategy instead of fabricating progress".into());
    }
    Ok(proposal)
}

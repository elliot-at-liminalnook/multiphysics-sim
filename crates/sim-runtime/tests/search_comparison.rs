#![cfg(all(feature = "evolution", not(target_arch = "wasm32")))]
use sim_runtime::search_comparison::*;
use sim_solve::bayesian::{Observation, Outcome, Parameter, Problem};
#[test]
fn same_baseline_reproducible_proposals_and_failures_count_against_budget() {
    let problem = Problem {
        context_id: "comparison-test".into(),
        parameters: vec![Parameter {
            name: "x".into(),
            unit: "1".into(),
            bounds: [-1., 1.],
        }],
        objective_name: "loss".into(),
        objective_unit: "1".into(),
        constraints: vec![],
    };
    let settings = Settings {
        seed: 13,
        initial_design: 3,
        acquisition_starts: 2,
        maximum_training_rows: 32,
        cma_population: 4,
        cma_sigma: 0.2,
        feasibility_candidates: 1,
        feasibility_length_scale: None,
    };
    for algorithm in [Algorithm::Bayesian, Algorithm::CmaEs] {
        assert_eq!(
            suggest(&problem, &[0.], &[], algorithm, &settings).unwrap(),
            vec![0.]
        );
        let first = Observation {
            context_id: problem.context_id.clone(),
            values: vec![0.],
            outcome: Outcome::Complete {
                objective: -0.1,
                residuals: vec![],
            },
            evidence: "baseline".into(),
        };
        let second = suggest(&problem, &[0.], &[first.clone()], algorithm, &settings).unwrap();
        assert_eq!(
            second,
            suggest(&problem, &[0.], &[first.clone()], algorithm, &settings).unwrap()
        );
        let failed = Observation {
            values: second,
            outcome: Outcome::Failed {
                reason: "screened".into(),
            },
            evidence: "screen-1".into(),
            ..first.clone()
        };
        let t = |attempt, observation| Trial {
            algorithm,
            seed: 13,
            attempt,
            proposal_method: "test".into(),
            observation,
            proposal_wall_s: 1.,
            preparation_wall_s: 2.,
            simulation_wall_s: 3.,
            total_wall_s: 3600.,
            charged_simulation_s: 5.,
            actual_simulation_s: if attempt == 0 { 5. } else { 0. },
            screened: vec![],
        };
        let mut trials = vec![t(0, first), t(1, failed)];
        let p = progress(&trials).unwrap();
        assert_eq!(p.failed_attempts, 1);
        assert_eq!(p.charged_simulation_s, 10.);
        assert_eq!(p.actual_simulation_s, 5.);
        assert_eq!(p.wall_hours, 2.);
        assert_eq!(p.improvement_per_wall_hour, Some(0.));
        trials[1].actual_simulation_s = 6.;
        assert!(progress(&trials).is_err());
    }
}

#[test]
fn adjacent_master_seeds_do_not_replay_shifted_bootstrap_samples() {
    let p = Problem {
        context_id: "seed-separation".into(),
        parameters: vec![Parameter {
            name: "x".into(),
            unit: "1".into(),
            bounds: [-1., 1.],
        }],
        objective_name: "loss".into(),
        objective_unit: "1".into(),
        constraints: vec![],
    };
    let settings = Settings {
        seed: 2301,
        initial_design: 3,
        acquisition_starts: 2,
        maximum_training_rows: 32,
        cma_population: 4,
        cma_sigma: 0.2,
        feasibility_candidates: 1,
        feasibility_length_scale: None,
    };
    let baseline = Observation {
        context_id: p.context_id.clone(),
        values: vec![0.],
        outcome: Outcome::Complete {
            objective: -0.1,
            residuals: vec![],
        },
        evidence: "baseline".into(),
    };
    let mut history = vec![baseline.clone()];
    for i in 0..4 {
        history.push(Observation {
            values: vec![-0.9 + 0.1 * i as f64],
            outcome: Outcome::Failed {
                reason: "preparation failed".into(),
            },
            evidence: format!("failed-{i}"),
            ..baseline.clone()
        });
    }
    let first = suggest(&p, &[0.], &history, Algorithm::Bayesian, &settings).unwrap();
    let mut adjacent = settings.clone();
    adjacent.seed += 1;
    history.pop();
    let second = suggest(&p, &[0.], &history, Algorithm::Bayesian, &adjacent).unwrap();
    assert_ne!(first, second);
    assert_eq!(
        second,
        suggest(&p, &[0.], &history, Algorithm::Bayesian, &adjacent).unwrap()
    );
}

#![cfg(all(feature = "bayesian", not(target_arch = "wasm32")))]
use sim_solve::{
    bayesian::{Config, Observation, Outcome, Parameter, Problem},
    local_global::{RegionConfig, RegionState, suggest},
};
fn fixture() -> (Problem, Vec<Observation>, Config, RegionConfig) {
    let problem = Problem {
        context_id: "analytic".into(),
        parameters: vec![Parameter {
            name: "x".into(),
            unit: "1".into(),
            bounds: [0., 1.],
        }],
        objective_name: "quadratic".into(),
        objective_unit: "1".into(),
        constraints: vec![],
    };
    let observations = [0_f64, 0.2, 0.7, 1.]
        .into_iter()
        .map(|x| Observation {
            context_id: problem.context_id.clone(),
            values: vec![x],
            outcome: Outcome::Complete {
                objective: (x - 0.33).powi(2),
                residuals: vec![],
            },
            evidence: format!("analytic{x}"),
        })
        .collect();
    (
        problem,
        observations,
        Config {
            seed: 921,
            acquisition_starts: 4,
            maximum_training_rows: 64,
        },
        RegionConfig {
            initial_radius: 0.05,
            minimum_radius: 0.0125,
            maximum_radius: 0.8,
            successes_to_expand: 2,
            failures_to_contract: 1,
            global_every: 3,
            relative_success: 0.001,
        },
    )
}
#[test]
fn actual_quadratic_search_expands_and_returns_to_global_domain() {
    let (p, mut observations, mut config, region) = fixture();
    let mut state = None;
    let mut modes = vec![];
    let mut radii = vec![];
    for i in 0..5 {
        config.seed = 921 + i;
        let r = suggest(&p, &observations, &config, &region, state.as_ref()).unwrap();
        modes.push(r.mode.clone());
        radii.push(r.state.radius);
        assert_eq!(r.proposal.training_indices.len(), observations.len());
        let x = r.proposal.values[0];
        observations.push(Observation {
            context_id: p.context_id.clone(),
            values: vec![x],
            outcome: Outcome::Complete {
                objective: (x - 0.33).powi(2),
                residuals: vec![],
            },
            evidence: format!("step{i}"),
        });
        state = Some(r.state);
    }
    assert_eq!(modes[0], "local");
    assert_eq!(modes[2], "periodic_global");
    assert!(radii.iter().any(|r| *r > 0.05));
    let best = observations
        .iter()
        .filter_map(|o| match o.outcome {
            Outcome::Complete { objective, .. } => Some(objective),
            _ => None,
        })
        .fold(f64::INFINITY, f64::min);
    assert!(best < 0.001, "{best}");
}
#[test]
fn failed_trials_contract_and_reset_without_imputing_scores() {
    let (p, mut observations, mut config, region) = fixture();
    let mut state = None;
    let mut modes = vec![];
    let mut radii = vec![];
    for i in 0..4 {
        config.seed = 942 + i;
        let r = suggest(&p, &observations, &config, &region, state.as_ref()).unwrap();
        modes.push(r.mode.clone());
        radii.push(r.state.radius);
        assert_eq!(r.proposal.training_indices, vec![0, 1, 2, 3]);
        assert_eq!(r.proposal.failed_indices.len(), i as usize);
        observations.push(Observation {
            context_id: p.context_id.clone(),
            values: r.proposal.values.clone(),
            outcome: Outcome::Failed {
                reason: "analytic failure".into(),
            },
            evidence: format!("failure{i}"),
        });
        state = Some(r.state);
    }
    assert_eq!(radii, vec![0.05, 0.025, 0.0125, 0.05]);
    assert_eq!(modes[3], "global_after_radius_reset");
}
#[test]
fn continuation_rejects_changed_history_or_unobserved_pending_point() {
    let (p, mut observations, config, region) = fixture();
    let r = suggest(&p, &observations, &config, &region, None).unwrap();
    let state: RegionState =
        serde_json::from_str(&serde_json::to_string(&r.state).unwrap()).unwrap();
    assert!(suggest(&p, &observations, &config, &region, Some(&state)).is_err());
    observations.push(Observation {
        context_id: p.context_id.clone(),
        values: r.proposal.values.clone(),
        outcome: Outcome::Failed {
            reason: "failure".into(),
        },
        evidence: "failure".into(),
    });
    let replay = suggest(&p, &observations, &config, &region, Some(&state)).unwrap();
    assert_eq!(
        replay.proposal.values,
        suggest(&p, &observations, &config, &region, Some(&state))
            .unwrap()
            .proposal
            .values
    );
    observations[0].evidence = "changed".into();
    assert!(suggest(&p, &observations, &config, &region, Some(&state)).is_err());
}

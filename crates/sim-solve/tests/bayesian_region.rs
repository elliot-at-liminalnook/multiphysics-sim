#![cfg(all(feature = "bayesian", not(target_arch = "wasm32")))]
use sim_solve::bayesian::{Config, Observation, Outcome, Parameter, Problem, suggest_in_region};
#[test]
fn local_acquisition_keeps_global_training_data() {
    let problem = Problem {
        context_id: "quadratic".into(),
        parameters: vec![Parameter {
            name: "x".into(),
            unit: "1".into(),
            bounds: [-2., 2.],
        }],
        objective_name: "quadratic".into(),
        objective_unit: "1".into(),
        constraints: vec![],
    };
    let observations: Vec<_> = [-2_f64, -0.3, 0.1, 2.]
        .into_iter()
        .map(|x| Observation {
            context_id: problem.context_id.clone(),
            values: vec![x],
            outcome: Outcome::Complete {
                objective: (x - 0.2).powi(2),
                residuals: vec![],
            },
            evidence: format!("analytic{x}"),
        })
        .collect();
    let config = Config {
        seed: 811,
        acquisition_starts: 4,
        maximum_training_rows: 64,
    };
    let region = [[0.45, 0.6]];
    let a = suggest_in_region(&problem, &observations, &config, Some(&region)).unwrap();
    assert!((-0.2..=0.4).contains(&a.values[0]));
    assert_eq!(a.training_indices, vec![0, 1, 2, 3]);
    assert_eq!(
        a.values,
        suggest_in_region(&problem, &observations, &config, Some(&region))
            .unwrap()
            .values
    );
    assert!(suggest_in_region(&problem, &observations, &config, Some(&[[0.7, 0.6]])).is_err());
    assert!(suggest_in_region(&problem, &observations, &config, Some(&[])).is_err());
}

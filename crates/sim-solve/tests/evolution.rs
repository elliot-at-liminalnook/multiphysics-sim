#![cfg(all(
    feature = "evolution",
    feature = "bayesian",
    not(target_arch = "wasm32")
))]
use sim_solve::{
    bayesian::{Observation, Outcome, Parameter, Problem},
    evolution::{self, Config},
};
fn setup() -> (Problem, Config) {
    (
        Problem {
            context_id: "analytic-v1".into(),
            parameters: (0..3)
                .map(|i| Parameter {
                    name: format!("x{i}"),
                    unit: "1".into(),
                    bounds: [-5., 5.],
                })
                .collect(),
            objective_name: "rotated_quadratic".into(),
            objective_unit: "1".into(),
            constraints: vec![],
        },
        Config {
            seed: 47,
            population: 8,
            sigma: 0.2,
            initial_mean: vec![3.; 3],
        },
    )
}
fn loss(x: &[f64]) -> f64 {
    (x[0] + x[1] - 1.).powi(2) + 40. * (x[0] - x[1] - 0.2).powi(2) + (x[2] + 0.8).powi(2)
}
#[test]
fn converges_with_replayed_covariance_and_roundtrip_history() {
    let (p, c) = setup();
    let mut h = vec![];
    let mut best = f64::INFINITY;
    for i in 0..320 {
        let x = evolution::suggest(&p, &h, &c).unwrap();
        assert!(x.iter().all(|x| (-5. ..=5.).contains(x)));
        let f = loss(&x);
        best = best.min(f);
        h.push(Observation {
            context_id: p.context_id.clone(),
            values: x,
            outcome: Outcome::Complete {
                objective: f,
                residuals: vec![],
            },
            evidence: format!("trial-{i}"),
        });
    }
    assert!(best < 0.002, "best {best}");
    let copy: Vec<Observation> = serde_json::from_str(&serde_json::to_string(&h).unwrap()).unwrap();
    assert_eq!(
        evolution::suggest(&p, &h, &c).unwrap(),
        evolution::suggest(&p, &copy, &c).unwrap()
    );
}
#[test]
fn failures_count_and_tampering_is_rejected() {
    let (p, c) = setup();
    let first = evolution::suggest(&p, &[], &c).unwrap();
    let mut h = vec![Observation {
        context_id: p.context_id.clone(),
        values: first.clone(),
        outcome: Outcome::Failed {
            reason: "reachability".into(),
        },
        evidence: "screen-0".into(),
    }];
    assert_ne!(evolution::suggest(&p, &h, &c).unwrap(), first);
    h[0].values[0] += 0.01;
    assert!(evolution::suggest(&p, &h, &c).is_err());
    h[0].context_id = "different".into();
    assert!(evolution::suggest(&p, &h, &c).is_err());
}

#[test]
fn all_failed_generations_do_not_fabricate_success_or_exhaust_proposals() {
    let (p, c) = setup();
    let mut h = vec![];
    for i in 0..128 {
        let x = evolution::suggest(&p, &h, &c).unwrap();
        assert!(x.iter().all(|x| x.is_finite() && (-5. ..=5.).contains(x)));
        h.push(Observation {
            context_id: p.context_id.clone(),
            values: x,
            outcome: Outcome::Failed {
                reason: "infeasible".into(),
            },
            evidence: format!("rejection-{i}"),
        });
    }
}

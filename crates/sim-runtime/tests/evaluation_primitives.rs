//! Synthetic saved metrics only. No Environment or optimizer is started.
use sim_core::{QuantityKind as Q, definitions::DefinitionId};
use sim_runtime::evaluation_primitives::*;
use std::collections::BTreeMap;
fn policy() -> Policy {
    Policy {
        objective: "speed".into(),
        objective_quantity: Q::LinearVelocity,
        maximize: true,
        required: BTreeMap::from([(
            "tracking".into(),
            Bound {
                quantity: Q::Angle,
                minimum: None,
                maximum: Some(0.03),
            },
        )]),
    }
}
fn outcome(id: &str, speed: f64, error: f64) -> Outcome {
    Outcome {
        context_id: "fixture-context".into(),
        candidate_id: id.into(),
        completed: true,
        failures: vec![],
        metrics: BTreeMap::from([
            (
                "speed".into(),
                Metric {
                    quantity: Q::LinearVelocity,
                    value: speed,
                },
            ),
            (
                "tracking".into(),
                Metric {
                    quantity: Q::Angle,
                    value: error,
                },
            ),
        ]),
    }
}
#[test]
fn failed_fast_candidate_is_retained_but_cannot_win() {
    let scores = rank(RankRequest {
        policy: policy(),
        outcomes: vec![
            outcome("bad-fast", 9., 0.2),
            outcome("slow", 0.1, 0.02),
            outcome("fast", 0.2, 0.02),
        ],
    })
    .unwrap();
    assert_eq!(
        scores
            .iter()
            .map(|s| s.candidate_id.as_str())
            .collect::<Vec<_>>(),
        ["fast", "slow", "bad-fast"]
    );
    assert_eq!(scores[2].eligible_score, None);
    assert_eq!(scores[2].rejection_reasons, vec!["gate failed: tracking"]);
}
#[test]
fn missing_incomplete_and_mismatched_metrics_fail_closed() {
    let mut o = outcome("a", 1., 0.01);
    o.completed = false;
    assert!(
        score(ScoreRequest {
            policy: policy(),
            outcome: o
        })
        .unwrap()
        .eligible_score
        .is_none()
    );
    let mut o = outcome("a", 1., 0.01);
    o.metrics.remove("tracking");
    assert!(
        score(ScoreRequest {
            policy: policy(),
            outcome: o
        })
        .unwrap()
        .eligible_score
        .is_none()
    );
    let mut o = outcome("a", 1., 0.01);
    o.metrics.get_mut("tracking").unwrap().quantity = Q::Length;
    assert!(
        score(ScoreRequest {
            policy: policy(),
            outcome: o
        })
        .is_err()
    );
    let mut o = outcome("b", 1., 0.01);
    o.context_id = "different-world".into();
    assert!(
        rank(RankRequest {
            policy: policy(),
            outcomes: vec![outcome("a", 1., 0.01), o]
        })
        .is_err()
    );
}
#[test]
fn irregular_sampling_retains_exact_quadratic_command_acceleration() {
    let r = TrackingRequest {
        names: vec!["axis".into()],
        quantities: vec![Q::Angle],
        time_s: vec![0., 1., 3.],
        targets: vec![vec![0.], vec![1.], vec![9.]],
        observations: vec![vec![0.5], vec![1.5], vec![9.5]],
        after_startup_s: 0.,
    };
    let stats = tracking(r.clone()).unwrap();
    assert_eq!(stats[0].rms_error, 0.5);
    assert_eq!(stats[0].maximum_command_acceleration, 2.);
    assert_eq!(stats[0].maximum_command_rate, 4.);
    let mut bad = r;
    bad.time_s[1] = 0.;
    assert!(tracking(bad).is_err());
}
#[test]
fn shared_registry_has_all_five_families_and_calls_native_scoring() {
    let r = sim_runtime::registry();
    let descriptors = r.primitive_descriptors().collect::<Vec<_>>();
    for name in [
        "mechanics.local_reduction",
        "actuation.dc_envelope",
        "actuation.prepare_reduction",
        "contact.force_feasibility",
        "mechanics.centroidal_wrench",
        "motion.contact_template",
        "motion.contact_sample",
        "motion.govern_angles",
        "experiment.score",
        "experiment.rank",
        "experiment.tracking_statistics",
        "experiment.compare_fidelity",
    ] {
        assert!(descriptors.iter().any(|d| d.id.name == name));
    }
    let request = ScoreRequest {
        policy: policy(),
        outcome: outcome("candidate", 0.1, 0.01),
    };
    let via_registry = r
        .call_primitive(
            &DefinitionId::new("experiment.score", 1),
            serde_json::to_value(&request).unwrap(),
        )
        .unwrap();
    assert_eq!(
        via_registry,
        serde_json::to_value(score(request).unwrap()).unwrap()
    );
    // Existing composable delay, nonlinear gearbox and power elements remain.
    for id in [
        "control.sampled_fixed_pd",
        "robot.motor_unit",
        "rotational.backlash_mesh",
        "robot.h_bridge",
        "robot.battery",
    ] {
        assert!(r.get(&id.into()).is_ok());
    }
}

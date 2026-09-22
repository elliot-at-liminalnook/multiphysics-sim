use sim_runtime::experiment_comparison::{Limits, Observation, Trace, compare, hx_archive};
fn trace(values: &[f64]) -> Trace {
    Trace {
        quantity: sim_core::QuantityKind::Angle.definition_id(),
        unit: "rad".into(),
        samples: values
            .iter()
            .enumerate()
            .map(|(i, &value)| Observation {
                time_s: i as f64,
                value,
                request_s: i as f64,
                completion_s: i as f64,
            })
            .collect(),
    }
}
#[test]
fn metrics_and_explicit_comparison_contract() {
    let measured = trace(&[1., 2., 3.]);
    let predicted = trace(&[2., 0., 5.]);
    let limits = Limits {
        rmse: 2.,
        final_abs_error: 1.,
    };
    let c = compare(&measured, &predicted, &limits).unwrap();
    assert_eq!(c.residuals, [1., -2., 2.]);
    assert!((c.rmse - 3f64.sqrt()).abs() < 1e-12);
    assert_eq!(c.final_error, 2.);
    assert!(!c.passes);
    let mut wrong = predicted.clone();
    wrong.unit = "deg".into();
    assert!(compare(&measured, &wrong, &limits).is_err());
    let mut shifted = predicted.clone();
    shifted.samples[1].time_s += 0.01;
    shifted.samples[1].completion_s += 0.01;
    assert!(compare(&measured, &shifted, &limits).is_err());
    let mut missing = predicted.clone();
    missing.samples.pop();
    assert!(compare(&measured, &missing, &limits).is_err());
    let mut invalid = predicted.clone();
    invalid.samples[0].value = f64::NAN;
    assert!(compare(&measured, &invalid, &limits).is_err());
}
#[test]
fn retained_archives_reproduce_declared_held_out_results() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (name, total, held_out, passing, inputs) in [
        ("pwm-identification", 63, 36, 33, 16),
        ("pwm-full-range-identification", 216, 162, 81, 5),
    ] {
        let archive =
            hx_archive::load(&repo.join("examples/actuators/hx30hm").join(name), &repo).unwrap();
        assert_eq!(archive.trials.len(), total);
        assert!(
            archive.integrity_issues.is_empty(),
            "{:?}",
            archive.integrity_issues
        );
        assert_eq!(archive.verified_inputs, inputs);
        let validation: Vec<_> = archive
            .trials
            .iter()
            .filter(|t| t.split != "train")
            .collect();
        assert_eq!(validation.len(), held_out);
        assert_eq!(
            validation.iter().filter(|t| t.comparison.passes).count(),
            passing
        );
        // Check every reproduced metric, not just the aggregate pass count.
        let results: serde_json::Value = serde_json::from_slice(
            &std::fs::read(
                repo.join("examples/actuators/hx30hm")
                    .join(name)
                    .join("results.json"),
            )
            .unwrap(),
        )
        .unwrap();
        for m in results["models"].as_array().unwrap() {
            for e in m["evaluations"].as_array().unwrap() {
                let t = archive
                    .trials
                    .iter()
                    .find(|t| {
                        t.device as u64 == m["id"].as_u64().unwrap()
                            && t.run == e["run"].as_str().unwrap()
                            && t.stage as u64 == e["stage"].as_u64().unwrap()
                    })
                    .unwrap();
                assert!(
                    (t.comparison.rmse / hx_archive::ENCODER_QUANTUM_RAD
                        - e["metrics"]["rmse_encoder_counts"].as_f64().unwrap())
                    .abs()
                        < 1e-9
                );
            }
        }
    }
}

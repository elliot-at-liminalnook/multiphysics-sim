use sim_domain_control::trajectory::{Interpolation, Keyframe, Trajectory, TrajectoryConfig};

#[test]
fn periodic_channel_shifts_preserve_curves_derivatives_and_exact_identity() {
    let config = TrajectoryConfig {
        interpolation: Interpolation::PeriodicCubicBSpline,
        keyframes: [
            [0.1, 0.7],
            [0.5, -0.3],
            [-0.2, 0.2],
            [0.8, -0.1],
            [0.1, 0.7],
        ]
        .into_iter()
        .enumerate()
        .map(|(i, values)| Keyframe {
            time_s: i as f64 * 0.25,
            values: values.to_vec(),
        })
        .collect(),
    };
    let base = Trajectory::new(config.clone()).unwrap();
    for shifts in [[0, 0], [4, -8], [i64::MIN, i64::MIN]] {
        assert_eq!(
            serde_json::to_value(base.shifted_periodic_controls(&shifts).unwrap()).unwrap(),
            serde_json::to_value(&config).unwrap()
        );
    }
    let shifted = Trajectory::new(base.shifted_periodic_controls(&[1, -1]).unwrap()).unwrap();
    for t in [0., 0.02, 0.1, 0.249, 0.25, 0.77, 1.05, 4.41] {
        let actual = shifted.sample(t).unwrap();
        for (j, offset) in [0.25, -0.25].into_iter().enumerate() {
            let expected = base.sample((t + offset).rem_euclid(1.)).unwrap();
            assert!((actual.values[j] - expected.values[j]).abs() < 1e-13);
            assert!((actual.rates[j] - expected.rates[j]).abs() < 1e-12);
            assert!((actual.accelerations[j] - expected.accelerations[j]).abs() < 1e-11);
        }
    }
    let restored = shifted.shifted_periodic_controls(&[-1, 1]).unwrap();
    assert_eq!(
        serde_json::to_value(restored).unwrap(),
        serde_json::to_value(&config).unwrap()
    );
    let large = base
        .shifted_periodic_controls(&[i64::MAX, i64::MIN + 1])
        .unwrap();
    assert_eq!(
        serde_json::to_value(large).unwrap(),
        serde_json::to_value(base.shifted_periodic_controls(&[3, 1]).unwrap()).unwrap()
    );
    assert!(base.shifted_periodic_controls(&[0]).is_err());
    let mut nonperiodic = config;
    nonperiodic.interpolation = Interpolation::Linear;
    assert!(
        Trajectory::new(nonperiodic)
            .unwrap()
            .shifted_periodic_controls(&[0, 0])
            .is_err()
    );
}

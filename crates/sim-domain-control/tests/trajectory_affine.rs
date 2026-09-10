use sim_domain_control::trajectory::{Interpolation, Keyframe, Trajectory, TrajectoryConfig};

#[test]
fn affine_reference_preserves_identity_and_transforms_motion_derivatives() {
    for interpolation in [
        Interpolation::Linear,
        Interpolation::QuinticRestToRest,
        Interpolation::PeriodicCubicBSpline,
    ] {
        let config = TrajectoryConfig {
            interpolation,
            keyframes: [
                [0.1, -0.6, 0.9],
                [0.5, -0.3, 0.8],
                [-0.2, 0.7, 1.2],
                [0.8, 0.4, 1.4],
                [0.1, -0.6, 0.9],
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
        let identity = base.affine_values(&[1.; 3], &[f64::MAX; 3]).unwrap();
        assert_eq!(
            serde_json::to_value(&identity).unwrap(),
            serde_json::to_value(&config).unwrap()
        );
        let scales = [2.3, -0.7, 0.];
        let centers = [0.3, -0.2, 0.8];
        let mapped = Trajectory::new(base.affine_values(&scales, &centers).unwrap()).unwrap();
        for t in [0., 0.13, 0.39, 0.82, 1., 1.2] {
            let a = base.sample(t).unwrap();
            let b = mapped.sample(t).unwrap();
            for j in 0..3 {
                assert!(
                    (b.values[j] - (centers[j] + scales[j] * (a.values[j] - centers[j]))).abs()
                        < 1e-12
                );
                assert!((b.rates[j] - scales[j] * a.rates[j]).abs() < 1e-12);
                assert!((b.accelerations[j] - scales[j] * a.accelerations[j]).abs() < 1e-11);
            }
        }
        assert!(base.affine_values(&[1.; 2], &centers).is_err());
        assert!(base.affine_values(&[f64::NAN, 1., 1.], &centers).is_err());
        assert!(
            base.affine_values(&[2., 1., 1.], &[f64::MAX, 0., 0.])
                .is_err()
        );
    }
}

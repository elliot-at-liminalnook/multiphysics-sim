use sim_domain_control::{
    planar_prediction::{
        PlanarPrediction, PlanarResponseWindow, PlanarTrendPrediction, TimedPlanarPose,
    },
    stepping::advance_planar,
};

#[test]
fn response_windows_preserve_shared_anchor_translation_and_validate_bindings() {
    let windows = [
        PlanarResponseWindow {
            pose: [0, 1, 2],
            twist: [3, 4, 5],
        },
        PlanarResponseWindow {
            pose: [0, 1, 6],
            twist: [7, 8, 9],
        },
    ];
    let responses = [
        2.,
        3.,
        0.,
        1.,
        0.,
        0.,
        std::f64::consts::FRAC_PI_2,
        1.,
        0.,
        0.,
    ];
    let mut shifted = responses;
    shifted[0] += 5.;
    for (window, expected) in windows.iter().zip([[4., 3.], [2., 5.]]) {
        let pose = window.predict(&responses, 2.).unwrap();
        let moved = window.predict(&shifted, 2.).unwrap();
        assert!((pose[0] - expected[0]).abs() < 1e-12);
        assert!((pose[1] - expected[1]).abs() < 1e-12);
        assert!((moved[0] - pose[0] - 5.).abs() < 1e-12);
        assert_eq!(moved[1], pose[1]);
    }
    assert!(
        windows[1]
            .predict(&responses[..7], 2.)
            .unwrap_err()
            .contains("index")
    );
    shifted[0] = f64::NAN;
    assert!(
        windows[0]
            .predict(&shifted, 2.)
            .unwrap_err()
            .contains("nonfinite")
    );
    assert!(windows[0].predict(&responses, -1.).is_err());
}

#[test]
fn recovers_straight_and_turning_motion_with_wrapped_headings() {
    for twist in [
        [0.5, -0.2, 0.],
        [0.5, 0.1, 0.4],
        [-0.3, 0.2, -0.8],
        [0.5, 0., 1e-9],
    ] {
        for initial in [[0., 0., 0.], [8., -13., 2.9]] {
            let samples: Vec<_> = [0., 0.02, 0.1, 0.7, 1.2, 2., 3.1, 4.8, 6.]
                .into_iter()
                .map(|time_s| {
                    let mut pose = advance_planar(initial, twist, time_s);
                    pose[2] = pose[2].sin().atan2(pose[2].cos());
                    TimedPlanarPose { time_s, pose }
                })
                .collect();
            let fit = PlanarPrediction::fit(&samples).unwrap();
            for i in 0..3 {
                assert!((fit.twist[i] - twist[i]).abs() < 1e-12);
            }
            let predicted = fit.predict(*samples.last().unwrap(), 300.).unwrap();
            let expected = advance_planar(initial, twist, 300.);
            for i in 0..2 {
                assert!((predicted[i] - expected[i]).abs() < 1e-9);
            }
            assert!((predicted[2] - expected[2]).sin().abs() < 1e-10);
        }
    }
}

#[test]
fn complete_circle_has_motion_but_no_net_displacement() {
    let samples: Vec<_> = (0..=100)
        .map(|i| {
            let time_s = i as f64 / 10.;
            TimedPlanarPose {
                time_s,
                pose: advance_planar([0.; 3], [0.5, 0., 0.1], time_s),
            }
        })
        .collect();
    let fit = PlanarPrediction::fit(&samples).unwrap();
    let end = fit
        .predict(samples[0], std::f64::consts::TAU / 0.1)
        .unwrap();
    assert!(end[0].hypot(end[1]) < 1e-12);
    assert!(fit.observed_path_length_m > 4.99);
}

#[test]
fn rejects_bad_times_and_nonfinite_data() {
    let a = TimedPlanarPose {
        time_s: 0.,
        pose: [0.; 3],
    };
    let b = TimedPlanarPose {
        time_s: 1.,
        pose: [1., 0., 0.],
    };
    assert!(PlanarPrediction::fit(&[a]).is_err());
    assert!(PlanarPrediction::fit(&[a, a]).is_err());
    assert!(PlanarPrediction::fit(&[b, a]).is_err());
    assert!(
        PlanarPrediction::fit(&[
            a,
            TimedPlanarPose {
                pose: [f64::NAN; 3],
                ..b
            }
        ])
        .is_err()
    );
    let fit = PlanarPrediction::fit(&[a, b]).unwrap();
    assert!(fit.predict(b, 0.).is_err());
    assert!(fit.predict(b, f64::INFINITY).is_err());
}

#[test]
fn heading_trend_recovers_constant_turn_with_nonuniform_wrapped_samples() {
    let twist = [0.5, -0.1, 0.8];
    let initial = [4., -3., 2.9];
    let samples: Vec<_> = [0., 0.01, 0.1, 0.7, 1.4, 2., 3.2, 4.8]
        .into_iter()
        .map(|time_s| {
            let mut pose = advance_planar(initial, twist, time_s);
            pose[2] = pose[2].sin().atan2(pose[2].cos());
            TimedPlanarPose {
                time_s: time_s + 1e8,
                pose,
            }
        })
        .collect();
    let model = PlanarTrendPrediction::fit(&samples).unwrap();
    for i in 0..3 {
        assert!((model.motion.twist[i] - twist[i]).abs() < 1e-8);
    }
    let predicted = model.predict(1e8 + 300.).unwrap();
    let expected = advance_planar(initial, twist, 300.);
    assert!((predicted[0] - expected[0]).hypot(predicted[1] - expected[1]) < 1e-5);
    assert!(model.heading_residual_rms_rad < 1e-8);
}

#[test]
fn regression_reduces_long_run_error_from_repeated_heading_noise() {
    // True path: x=0.5t,y=0. A 50mrad repeated heading observation error has
    // different phase at each endpoint. Regression should reduce the resulting
    // false turn substantially; finite-window regression still has bias.
    let samples: Vec<_> = (0..=500)
        .map(|i| {
            let time_s = 0.073 + i as f64 * 0.02;
            TimedPlanarPose {
                time_s,
                pose: [
                    0.5 * time_s,
                    0.,
                    0.05 * (std::f64::consts::TAU * time_s / 0.397).sin(),
                ],
            }
        })
        .collect();
    let old = PlanarPrediction::fit(&samples).unwrap();
    let trend = PlanarTrendPrediction::fit(&samples).unwrap();
    let old_end = old.predict(*samples.last().unwrap(), 300.).unwrap();
    let trend_end = trend.predict(300.).unwrap();
    let error = |p: [f64; 3]| (p[0] - 150.).hypot(p[1]);
    assert!(error(trend_end) < error(old_end) * 0.1);
    assert!((trend.heading_residual_rms_rad - 0.05 / 2_f64.sqrt()).abs() < 0.001);
    assert!(PlanarTrendPrediction::fit(&samples[..1]).is_err());
    assert!(trend.predict(samples[0].time_s).is_err());
}

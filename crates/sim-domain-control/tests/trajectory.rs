use sim_domain_control::trajectory::*;
fn config(interpolation: Interpolation) -> TrajectoryConfig {
    TrajectoryConfig {
        interpolation,
        keyframes: vec![
            Keyframe {
                time_s: 1.,
                values: vec![2., 4.],
            },
            Keyframe {
                time_s: 3.,
                values: vec![6., 0.],
            },
            Keyframe {
                time_s: 4.,
                values: vec![7., 1.],
            },
        ],
    }
}
#[test]
fn linear_interpolation_retains_endpoint_holds_and_channel_order() {
    let t = Trajectory::new(config(Interpolation::Linear)).unwrap();
    assert_eq!(t.sample(0.).unwrap().values, vec![2., 4.]);
    assert_eq!(t.sample(2.).unwrap().values, vec![4., 2.]);
    assert_eq!(t.sample(2.).unwrap().rates, vec![2., -2.]);
    assert_eq!(t.sample(3.).unwrap().rates, vec![1., 1.]);
    assert_eq!(t.sample(5.).unwrap().values, vec![7., 1.]);
    assert_eq!(t.sample(4.).unwrap().rates, vec![0., 0.]);
}
#[test]
fn quintic_has_analytic_midpoint_rest_endpoints_and_checked_derivatives() {
    let t = Trajectory::new(config(Interpolation::QuinticRestToRest)).unwrap();
    for x in [0., 1., 3., 4., 5.] {
        let s = t.sample(x).unwrap();
        assert_eq!(s.rates, vec![0.; 2]);
        assert_eq!(s.accelerations, vec![0.; 2]);
    }
    let s = t.sample(2.).unwrap();
    assert_eq!(s.values, vec![4., 2.]);
    assert_eq!(s.rates, vec![3.75, -3.75]);
    assert_eq!(s.accelerations, vec![0.; 2]);
    for x in [1.1, 1.5, 2.4, 2.9] {
        let h = 1e-5;
        let a = t.sample(x - h).unwrap();
        let b = t.sample(x + h).unwrap();
        let s = t.sample(x).unwrap();
        for i in 0..2 {
            assert!(((b.values[i] - a.values[i]) / (2. * h) - s.rates[i]).abs() < 1e-8);
            assert!(((b.rates[i] - a.rates[i]) / (2. * h) - s.accelerations[i]).abs() < 1e-8);
        }
    }
    let flat = Trajectory::new(TrajectoryConfig {
        interpolation: Interpolation::QuinticRestToRest,
        keyframes: vec![
            Keyframe {
                time_s: 0.,
                values: vec![1.],
            },
            Keyframe {
                time_s: 1e-300,
                values: vec![1.],
            },
        ],
    })
    .unwrap();
    assert_eq!(flat.sample(5e-301).unwrap().accelerations, vec![0.]);
}
#[test]
fn rejects_nonfinite_unsorted_and_dimension_mismatched_input() {
    let c = config(Interpolation::Linear);
    let mut b = c.clone();
    b.keyframes[1].time_s = 1.;
    assert!(Trajectory::new(b).is_err());
    let mut b = c.clone();
    b.keyframes[1].values.pop();
    assert!(Trajectory::new(b).is_err());
    let mut b = c.clone();
    b.keyframes[0].values[0] = f64::NAN;
    assert!(Trajectory::new(b).is_err());
    let mut b = c.clone();
    b.keyframes[0].time_s = -1.;
    assert!(Trajectory::new(b).is_err());
    let t = Trajectory::new(c).unwrap();
    assert!(t.sample(f64::NAN).is_err());
    assert!(t.sample(-1.).is_err());
}

#[test]
fn authored_value_bounds_cover_all_reference_segments() {
    for interpolation in [Interpolation::Linear, Interpolation::QuinticRestToRest] {
        let t = Trajectory::new(config(interpolation)).unwrap();
        t.validate_value_bounds(&[(Some(2.), Some(7.)), (Some(0.), Some(4.))])
            .unwrap();
        assert!(t
            .validate_value_bounds(&[(Some(2.), Some(5.)), (None, None)])
            .is_err());
        assert!(t
            .validate_value_bounds(&[(Some(f64::NAN), None), (None, None)])
            .is_err());
        assert!(t
            .validate_value_bounds(&[(Some(5.), Some(2.)), (None, None)])
            .is_err());
        assert!(t.validate_value_bounds(&[(None, None)]).is_err());
    }
}

fn periodic_controls() -> TrajectoryConfig {
    TrajectoryConfig {
        interpolation: Interpolation::PeriodicCubicBSpline,
        keyframes: [0., 1., 0., -1., 0.]
            .iter()
            .enumerate()
            .map(|(i, &q)| Keyframe {
                time_s: i as f64,
                values: vec![q, 2. * q + 3.],
            })
            .collect(),
    }
}

#[test]
fn traversal_bounds_match_analytic_linear_and_quintic_paths() {
    for interpolation in [Interpolation::Linear, Interpolation::QuinticRestToRest] {
        let t = Trajectory::new(config(interpolation)).unwrap();
        let bounds = t.rate_traversal_bounds(&[2., 4.], 64).unwrap();
        // First segment: 4/2 seconds. Second: 1/2 seconds. Both coordinates
        // are monotone on each segment; the first coordinate always dominates.
        assert!((bounds.duration_lower_s - 2.5).abs() < 1e-12);
        assert!(bounds.duration_upper_s >= 2.5 - 1e-12);
        // Upper-sum error is bounded by cell width times total variation of
        // the normalized quintic rate (2 * 1.875); linear rate is constant.
        assert!(bounds.duration_upper_s <= 2.5 * (1. + 3.75 / 64.) + 1e-12);
        assert_eq!(bounds.reference_duration_s, 3.); // Initial 1 s hold excluded.
        let expected_uniform = if matches!(interpolation, Interpolation::Linear) {
            3.
        } else {
            5.625
        };
        assert_eq!(bounds.uniform_duration_s, expected_uniform);
    }
}

#[test]
fn periodic_traversal_brackets_total_variation_and_refines() {
    let t = Trajectory::new(periodic_controls()).unwrap();
    let coarse = t.rate_traversal_bounds(&[1., 2.], 1).unwrap();
    let fine = t.rate_traversal_bounds(&[1., 2.], 64).unwrap();
    // Extrema are +/-2/3; a cycle covers total variation 8/3. The second
    // coordinate has twice the variation and twice the budget.
    let exact = 8. / 3.;
    assert!(fine.duration_lower_s <= exact + 1e-12);
    assert!(fine.duration_upper_s >= exact - 1e-12);
    assert!(fine.duration_upper_s - fine.duration_lower_s < 0.04);
    assert!(fine.duration_lower_s >= coarse.duration_lower_s - 1e-12);
    assert!(fine.duration_upper_s < coarse.duration_upper_s);
    assert_eq!(fine.uniform_duration_s, 4.);
    let double_budget = t.rate_traversal_bounds(&[2., 4.], 64).unwrap();
    assert_eq!(double_budget.duration_upper_s, fine.duration_upper_s / 2.);
}

#[test]
fn traversal_cells_cover_rate_switches_without_missing_interior_peaks() {
    let mut c = periodic_controls();
    for k in &mut c.keyframes {
        k.values[1] = (k.time_s * std::f64::consts::FRAC_PI_2).cos();
    }
    c.keyframes.last_mut().unwrap().values = c.keyframes[0].values.clone();
    let t = Trajectory::new(c).unwrap();
    let budgets = [1., 1.];
    let bounds = t.rate_traversal_bounds(&budgets, 8).unwrap();
    assert!(bounds.cells.iter().any(|c| c.limiting_coordinate == 0));
    assert!(bounds.cells.iter().any(|c| c.limiting_coordinate == 1));
    // Check the constructive piecewise constant phase-rate upper bound against
    // independently sampled derivatives, including cells crossing bottlenecks.
    for cell in bounds.cells {
        let h = cell.reference_end_s - cell.reference_start_s;
        let phase_rate = h / cell.duration_upper_s;
        for k in 0..101 {
            let sample = t
                .sample(cell.reference_start_s + h * k as f64 / 100.)
                .unwrap();
            assert!(sample
                .rates
                .iter()
                .zip(budgets)
                .all(|(r, b)| r.abs() * phase_rate <= b + 1e-12));
        }
    }
}

#[test]
fn traversal_rejects_invalid_budgets_and_unbounded_work() {
    let t = Trajectory::new(periodic_controls()).unwrap();
    for budgets in [
        vec![],
        vec![1.],
        vec![0., 1.],
        vec![-1., 1.],
        vec![f64::NAN, 1.],
        vec![f64::INFINITY, 1.],
    ] {
        assert!(t.rate_traversal_bounds(&budgets, 4).is_err());
    }
    assert!(t.rate_traversal_bounds(&[1., 2.], 0).is_err());
    assert!(t.rate_traversal_bounds(&[1., 2.], usize::MAX).is_err());
    assert!(t.rate_traversal_bounds(&[1., 2.], 100_001).is_err());
}

#[test]
fn redistributed_periodic_reference_preserves_phase_anchors_and_is_c2() {
    let source = Trajectory::new(periodic_controls()).unwrap();
    let config = RateRedistributionConfig {
        subdivisions_per_segment: 32,
        output_controls: 160,
        blend: 0.75,
        anchor_indices: vec![0, 1, 2, 3, 4],
        coordinate_intervals: None,
    };
    let result = source
        .redistribute_periodic_rates(&[1., 2.], &config)
        .unwrap();
    for i in 0..=4 {
        assert!((result.source_phases_s[40 * i] - i as f64).abs() < 1e-12);
    }
    assert!(result.source_phases_s.windows(2).all(|w| w[1] > w[0]));
    let curve = Trajectory::new(result.trajectory).unwrap();
    for time in [0., 1., 2., 3., 4.] {
        let left = curve.sample((time + 4. - 1e-8) % 4.).unwrap();
        let right = curve.sample((time + 1e-8) % 4.).unwrap();
        assert!((left.values[0] - right.values[0]).abs() < 1e-6);
        assert!((left.rates[0] - right.rates[0]).abs() < 1e-5);
        assert!((left.accelerations[0] - right.accelerations[0]).abs() < 1e-3);
    }
    let identity = source
        .redistribute_periodic_rates(
            &[1., 2.],
            &RateRedistributionConfig {
                blend: 0.,
                ..config.clone()
            },
        )
        .unwrap();
    for (i, phase) in identity.source_phases_s.iter().enumerate() {
        assert!((phase - i as f64 / 40.).abs() < 1e-12);
    }
    for anchors in [
        vec![],
        vec![1, 4],
        vec![0, 2, 1, 4],
        vec![0, 5],
        vec![0, 4, 4],
    ] {
        assert!(source
            .redistribute_periodic_rates(
                &[1., 2.],
                &RateRedistributionConfig {
                    anchor_indices: anchors,
                    ..config.clone()
                }
            )
            .is_err());
    }
    assert!(source
        .redistribute_periodic_rates(
            &[1., 2.],
            &RateRedistributionConfig {
                blend: 1.,
                ..config
            }
        )
        .is_err());
}

#[test]
fn selective_redistribution_keeps_inactive_channel_controls_on_original_phase() {
    let source = Trajectory::new(periodic_controls()).unwrap();
    let config = RateRedistributionConfig {
        subdivisions_per_segment: 32,
        output_controls: 160,
        blend: 0.75,
        anchor_indices: vec![0, 1, 2, 3, 4],
        coordinate_intervals: Some(vec![vec![[1, 2]], vec![]]),
    };
    let result = source
        .redistribute_periodic_rates(&[1., 2.], &config)
        .unwrap();
    let mut changed = false;
    for k in &result.trajectory.keyframes {
        let original = source.sample(k.time_s).unwrap().values;
        assert_eq!(k.values[1], original[1]);
        if k.time_s <= 1. || k.time_s >= 2. {
            assert_eq!(k.values[0], original[0]);
        } else {
            changed |= (k.values[0] - original[0]).abs() > 1e-4;
        }
    }
    assert!(changed);
    for intervals in [
        vec![],
        vec![vec![[1, 5]], vec![]],
        vec![vec![[1, 3], [2, 4]], vec![]],
    ] {
        assert!(source
            .redistribute_periodic_rates(
                &[1., 2.],
                &RateRedistributionConfig {
                    coordinate_intervals: Some(intervals),
                    ..config.clone()
                }
            )
            .is_err());
    }
}

#[test]
fn periodic_spline_has_analytic_values_and_continuous_cycle_derivatives() {
    let t = Trajectory::new(periodic_controls()).unwrap();
    let s = t.sample(0.).unwrap();
    assert_eq!(s.values, vec![0., 3.]);
    assert_eq!(s.rates, vec![1., 2.]);
    assert_eq!(s.accelerations, vec![0., 0.]);
    assert!((t.sample(1.).unwrap().values[0] - 2. / 3.).abs() < 1e-14);
    for knot in 1..=8 {
        let left = t.sample(knot as f64 - 1e-8).unwrap();
        let right = t.sample(knot as f64 + 1e-8).unwrap();
        for (a, b) in [
            (&left.values, &right.values),
            (&left.rates, &right.rates),
            (&left.accelerations, &right.accelerations),
        ] {
            for j in 0..2 {
                assert!((a[j] - b[j]).abs() < 1e-7);
            }
        }
    }
    for time in [0.125, 0.51, 1.3, 2.8, 3.9] {
        let s = t.sample(time).unwrap();
        let wrapped = t.sample(time + 12.).unwrap();
        let a = t.sample(time - 1e-5).unwrap();
        let b = t.sample(time + 1e-5).unwrap();
        for j in 0..2 {
            assert!((s.values[j] - wrapped.values[j]).abs() < 1e-14);
            assert!((s.rates[j] - wrapped.rates[j]).abs() < 1e-14);
            assert!((s.accelerations[j] - wrapped.accelerations[j]).abs() < 1e-14);
            assert!(((b.values[j] - a.values[j]) / 2e-5 - s.rates[j]).abs() < 1e-9);
            assert!(((b.rates[j] - a.rates[j]) / 2e-5 - s.accelerations[j]).abs() < 1e-9);
        }
    }
}

#[test]
fn periodic_spline_validates_closure_timing_and_control_hull_bounds() {
    let t = Trajectory::new(periodic_controls()).unwrap();
    t.validate_value_bounds(&[(Some(-1.), Some(1.)), (Some(1.), Some(5.))])
        .unwrap();
    for i in 0..1000 {
        let sample = t.sample(i as f64 / 100.).unwrap();
        assert!((-1. ..=1.).contains(&sample.values[0]));
        assert!((1. ..=5.).contains(&sample.values[1]));
    }
    for variant in 0..4 {
        let mut c = periodic_controls();
        match variant {
            0 => c.keyframes.pop().map(|_| ()).unwrap(),
            1 => c.keyframes[4].values[0] = 1e-8,
            2 => c.keyframes[2].time_s += 0.1,
            _ => {
                for k in &mut c.keyframes {
                    k.time_s += 1.;
                }
            }
        }
        assert!(Trajectory::new(c).is_err());
    }
}

#[test]
fn rate_bounds_include_interior_spline_extrema() {
    let mut c = periodic_controls();
    for (k, q) in c.keyframes.iter_mut().zip([-1., 0., 2., 1., -1.]) {
        k.values = vec![q];
    }
    let t = Trajectory::new(c).unwrap();
    assert!((t.maximum_absolute_rates().unwrap()[0] - 1.625).abs() < 1e-14);
    assert!((t.sample(1.25).unwrap().rates[0] - 1.625).abs() < 1e-14);
    for i in 0..1000 {
        assert!(t.sample(i as f64 / 250.).unwrap().rates[0].abs() <= 1.625 + 1e-14);
    }
    assert_eq!(
        Trajectory::new(config(Interpolation::Linear))
            .unwrap()
            .maximum_absolute_rates()
            .unwrap(),
        vec![2., 2.]
    );
    assert_eq!(
        Trajectory::new(config(Interpolation::QuinticRestToRest))
            .unwrap()
            .maximum_absolute_rates()
            .unwrap(),
        vec![3.75, 3.75]
    );
}

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
        assert!(
            t.validate_value_bounds(&[(Some(2.), Some(5.)), (None, None)])
                .is_err()
        );
        assert!(
            t.validate_value_bounds(&[(Some(f64::NAN), None), (None, None)])
                .is_err()
        );
        assert!(
            t.validate_value_bounds(&[(Some(5.), Some(2.)), (None, None)])
                .is_err()
        );
        assert!(t.validate_value_bounds(&[(None, None)]).is_err());
    }
}

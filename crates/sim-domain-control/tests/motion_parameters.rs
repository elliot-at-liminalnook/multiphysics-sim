use sim_core::QuantityKind as Q;
use sim_domain_control::{motion_parameters::*, trajectory::*};

fn constant(value: f64) -> Scalar {
    Scalar::Constant { value }
}
fn parameter(name: &str) -> Scalar {
    Scalar::Parameter { name: name.into() }
}
fn fixture() -> (TrajectoryTemplate, ParameterSpace, Values) {
    let template = TrajectoryTemplate {
        channels: vec![
            MotionChannel {
                name: "hinge".into(),
                kind: Q::Angle,
            },
            MotionChannel {
                name: "slide".into(),
                kind: Q::Length,
            },
        ],
        reference: TrajectoryConfig {
            interpolation: Interpolation::PeriodicCubicBSpline,
            keyframes: [
                [0.1, -0.0],
                [0.5, 0.3],
                [-0.2, 0.1],
                [0.8, -0.2],
                [0.1, -0.0],
            ]
            .into_iter()
            .enumerate()
            .map(|(i, values)| Keyframe {
                time_s: i as f64 * 0.25,
                values: values.into(),
            })
            .collect(),
        },
        transforms: vec![
            Transform::Affine {
                channels: vec!["hinge".into(), "slide".into()],
                scale: parameter("amplitude"),
                center: constant(0.),
                offset: constant(0.),
            },
            Transform::TimeScale {
                factor: parameter("duration"),
            },
            Transform::PeriodicShift {
                channels: vec!["hinge".into()],
                controls: parameter("phase"),
            },
            Transform::ControlOffset {
                channels: vec!["slide".into()],
                control: 0,
                offset: parameter("lift"),
            },
        ],
    };
    let space = ParameterSpace {
        parameters: vec![
            Parameter {
                name: "amplitude".into(),
                kind: Q::Dimensionless,
                bounds: [-3., 3.],
                integer: false,
            },
            Parameter {
                name: "duration".into(),
                kind: Q::Dimensionless,
                bounds: [0.1, 4.],
                integer: false,
            },
            Parameter {
                name: "phase".into(),
                kind: Q::Dimensionless,
                bounds: [-8., 8.],
                integer: true,
            },
            Parameter {
                name: "lift".into(),
                kind: Q::Length,
                bounds: [-1., 1.],
                integer: false,
            },
        ],
    };
    let values = space.named_values(&[1., 1., 0., 0.]).unwrap();
    (template, space, values)
}

#[test]
fn exact_identity_and_named_reordering_preserve_signed_zero() {
    let (mut t, mut p, v) = fixture();
    let identity = t.materialize(&p, &v).unwrap();
    for (a, b) in t.reference.keyframes.iter().zip(identity.keyframes) {
        assert_eq!(a.time_s.to_bits(), b.time_s.to_bits());
        assert_eq!(
            a.values.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            b.values.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
    }
    let mut changed = v.clone();
    changed.insert("amplitude".into(), -2.);
    changed.insert("phase".into(), 1.);
    let expected = t.materialize(&p, &changed).unwrap();
    t.channels.reverse();
    for k in &mut t.reference.keyframes {
        k.values.reverse();
    }
    p.parameters.reverse();
    let actual = t.materialize(&p, &changed).unwrap();
    for (a, b) in actual.keyframes.iter().zip(expected.keyframes) {
        assert_eq!(a.values, b.values.into_iter().rev().collect::<Vec<_>>());
    }
}

#[test]
fn affine_timing_phase_derivatives_follow_the_same_curve() {
    let (t, p, mut v) = fixture();
    v.insert("amplitude".into(), -1.7);
    v.insert("duration".into(), 0.4);
    v.insert("phase".into(), -1.);
    let base = Trajectory::new(t.reference.clone()).unwrap();
    let changed = Trajectory::new(t.materialize(&p, &v).unwrap()).unwrap();
    for time in [0.01, 0.049, 0.15, 0.31, 0.5] {
        let got = changed.sample(time).unwrap();
        for j in 0..2 {
            let source = base
                .sample((time / 0.4 + if j == 0 { -0.25 } else { 0. }).rem_euclid(1.))
                .unwrap();
            assert!((got.values[j] + 1.7 * source.values[j]).abs() < 1e-12);
            assert!((got.rates[j] + 1.7 / 0.4 * source.rates[j]).abs() < 1e-11);
            assert!((got.accelerations[j] + 1.7 / 0.16 * source.accelerations[j]).abs() < 1e-9);
        }
    }
    v.insert("lift".into(), 0.2);
    let c = t.materialize(&p, &v).unwrap();
    assert_eq!(c.keyframes[0].values, c.keyframes.last().unwrap().values);
    assert_eq!(c.keyframes[0].values[1], 0.2);
    let curve = Trajectory::new(c).unwrap();
    assert!(
        (curve.sample(0.).unwrap().values[1]
            - changed.sample(0.).unwrap().values[1]
            - 0.2 * 2. / 3.)
            .abs()
            < 1e-12
    );
}

#[test]
fn rejects_wrong_units_unknown_duplicate_unused_channels_and_fractional_phase() {
    let (t, p, v) = fixture();
    let mut bad = p.clone();
    bad.parameters[3].kind = Q::Angle;
    assert!(t.materialize(&bad, &v).unwrap_err().contains("kind"));
    let mut bad = p.clone();
    bad.parameters[2].integer = false;
    assert!(t.materialize(&bad, &v).is_err());
    let mut bad = p.clone();
    bad.parameters.push(bad.parameters[0].clone());
    assert!(t.materialize(&bad, &v).is_err());
    for (name, value) in [
        ("phase", 0.5),
        ("amplitude", 4.),
        ("duration", f64::NAN),
        ("unknown", 0.),
    ] {
        let mut bad = v.clone();
        bad.insert(name.into(), value);
        assert!(t.materialize(&p, &bad).is_err());
    }
    let mut bad = t.clone();
    bad.channels[1].name = "hinge".into();
    assert!(bad.materialize(&p, &v).is_err());
    let mut bad = t.clone();
    bad.transforms.push(Transform::Affine {
        channels: vec!["missing".into()],
        scale: constant(1.),
        center: constant(0.),
        offset: constant(0.),
    });
    assert!(bad.materialize(&p, &v).is_err());
    let mut bad = t.clone();
    bad.transforms.push(Transform::ControlOffset {
        channels: vec!["hinge".into()],
        control: 4,
        offset: constant(0.),
    });
    assert!(bad.materialize(&p, &v).is_err());
    let mut bad = t.clone();
    bad.transforms.push(Transform::TimeScale {
        factor: constant(0.),
    });
    assert!(bad.materialize(&p, &v).is_err());
    let mut bad = t.clone();
    bad.transforms.push(Transform::TimeScale {
        factor: constant(f64::MAX),
    });
    bad.transforms.push(Transform::TimeScale {
        factor: constant(2.),
    });
    assert!(bad.materialize(&p, &v).is_err());
    assert!(affine_value(f64::MAX, 2., 0., 0.).is_err());
    assert_eq!(
        affine_value(-0., 1., f64::MAX, -0.).unwrap().to_bits(),
        (-0f64).to_bits()
    );
}

#[test]
fn shared_registry_ports_and_parameter_declarations_retain_units() {
    let (_, p, _) = fixture();
    let declarations = p.declarations();
    assert_eq!(declarations[3].unit, "m");
    assert!(declarations[2].integer);
    let mut registry = sim_core::BehaviorRegistry::default();
    register(&mut registry).unwrap();
    let d = registry.get(&"control.affine_angle".into()).unwrap();
    assert_eq!(d.ports[0], sim_core::signal_in("value", Q::Angle));
    assert_eq!(d.ports[1], sim_core::signal_in("scale", Q::Dimensionless));
    assert_eq!(d.ports[4], sim_core::signal_out("result", Q::Angle));
    assert!(
        d.validate_parameters(&[("unexpected".into(), 1.)].into())
            .is_err()
    );
}

#[test]
fn typed_scaling_connects_duration_speed_and_acceleration_without_rounding() {
    let (_, p, mut v) = fixture();
    v.insert("duration".into(), 0.4);
    let scaled = |kind, power| {
        Scalar::Scaled {
            value: Box::new(constant(3.)),
            factor: Box::new(parameter("duration")),
            power,
        }
        .resolve(&p, &v, kind, false)
        .unwrap()
    };
    assert!((scaled(Q::Time, 1) - 1.2).abs() < 1e-15);
    assert_eq!(scaled(Q::LinearVelocity, -1), 7.5);
    assert!((scaled(Q::LinearAcceleration, -2) - 18.75).abs() < 1e-14);
    let mut expression = Scalar::Scaled {
        value: Box::new(parameter("lift")),
        factor: Box::new(parameter("duration")),
        power: -1,
    };
    assert_eq!(expression.parameter_names(), ["lift", "duration"].into());
    assert!(expression.resolve(&p, &v, Q::Angle, false).is_err());
    // Scaling preserves the receiving value's kind, even for inverse powers.
    assert!(expression.resolve(&p, &v, Q::Length, false).is_ok());
    expression = Scalar::Scaled {
        value: Box::new(constant(1.)),
        factor: Box::new(parameter("lift")),
        power: 0,
    };
    assert!(expression.resolve(&p, &v, Q::Time, false).is_err());
    expression = Scalar::Scaled {
        value: Box::new(constant(1.)),
        factor: Box::new(parameter("duration")),
        power: -1,
    };
    assert!(expression.resolve(&p, &v, Q::Dimensionless, true).is_err());
    v.insert("duration".into(), 0.5);
    assert_eq!(
        expression.resolve(&p, &v, Q::Dimensionless, true).unwrap(),
        2.
    );
    assert!(scaled_value(1., 0., -1).is_err());
    assert!(scaled_value(f64::MAX, 2., 1).is_err());
    assert_eq!(
        scaled_value(-0., 1., i32::MIN).unwrap().to_bits(),
        (-0f64).to_bits()
    );
    for _ in 0..64 {
        expression = Scalar::Scaled {
            value: Box::new(expression),
            factor: Box::new(constant(1.)),
            power: 1,
        };
    }
    assert!(
        expression
            .resolve(&p, &v, Q::Dimensionless, false)
            .unwrap_err()
            .contains("levels")
    );
    let mut registry = sim_core::BehaviorRegistry::default();
    register(&mut registry).unwrap();
    let d = registry
        .get(&"control.scale_power_linear_acceleration".into())
        .unwrap();
    assert_eq!(
        d.ports[0],
        sim_core::signal_in("value", Q::LinearAcceleration)
    );
    assert_eq!(d.ports[1], sim_core::signal_in("factor", Q::Dimensionless));
    assert_eq!(
        d.ports[2],
        sim_core::signal_out("result", Q::LinearAcceleration)
    );
    assert!(
        d.validate_parameters(&[("power".into(), -2.)].into())
            .is_ok()
    );
    assert!(
        d.validate_parameters(&[("power".into(), 0.5)].into())
            .is_err()
    );
}

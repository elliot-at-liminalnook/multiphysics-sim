use sim_core::{Behavior, BehaviorRegistry, Context};
use sim_domain_robot::effective_servo::{EFFECTIVE_SERVO, EffectiveServo};
use std::collections::BTreeMap;

fn parameters() -> BTreeMap<String, f64> {
    [
        ("stiffness", 10.0),
        ("damping", 0.5),
        ("stall_torque", 2.0),
        ("no_load_speed", 4.0),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v))
    .collect()
}

#[test]
fn positive_power_bound_and_backdrive_envelope_match_the_actuator_law() {
    let servo = EffectiveServo::new(&parameters()).unwrap();
    assert_eq!(servo.peak_motoring_power_w(), 2.0);
    for i in -100..=100 {
        let v = i as f64 / 10.0;
        let tau = servo.torque(0.0, v, 100.0 * v.signum());
        assert!(tau * v <= servo.peak_motoring_power_w() + 1e-12);
    }
    assert_eq!(servo.torque(0.0, 2.0, 100.0) * 2.0, servo.peak_motoring_power_w());
    assert_eq!(servo.torque_capacity(8.0, -1.0), 2.0);
    assert_eq!(servo.torque_capacity(8.0, 1.0), 0.0);
}

#[test]
fn bounded_torque_speed_curve_and_braking_do_not_create_an_ideal_motion_source() {
    let servo = EffectiveServo::new(&parameters()).unwrap();
    assert_eq!(servo.torque(0.0, 0.0, 0.1), 1.0);
    assert_eq!(servo.torque(0.0, 0.0, 1.0), 2.0);
    assert_eq!(servo.torque(0.0, 2.0, 1.0), 1.0);
    assert_eq!(servo.torque(0.0, 4.0, 1.0), 0.0);
    assert_eq!(servo.torque(0.0, 5.0, 1.0), 0.0);
    assert_eq!(servo.torque(0.0, -5.0, 1.0), 2.0);
    assert_eq!(servo.torque(1.0, 2.0, 1.0), -1.0);
    // A static load is held with a finite position error, not an imposed angle.
    assert!((servo.torque(0.8, 0.0, 0.9) - 1.0).abs() < 1e-14);
    for v in [-8.0, -4.0, -1.0, 0.0, 1.0, 4.0, 8.0] {
        for q in [-2.0, -0.2, 0.0, 0.2, 2.0] {
            let tau = servo.torque(q, v, 0.3);
            assert!(tau.abs() <= 2.0);
            assert!((tau + servo.torque(-q, -v, -0.3)).abs() < 1e-14);
            if tau * v > 0.0 {
                assert!(tau.abs() <= 2.0 * (1.0 - v.abs() / 4.0).max(0.0) + 1e-14);
            }
        }
    }
}

#[test]
fn registry_and_runtime_share_parameters_and_reject_missing_or_unsupported_physics() {
    let mut registry = BehaviorRegistry::default();
    sim_domain_robot::register(&mut registry).unwrap();
    let descriptor = registry.get(&EFFECTIVE_SERVO.into()).unwrap();
    assert_eq!(descriptor.ports.len(), 4);
    for name in ["stiffness", "damping", "stall_torque", "no_load_speed"] {
        let mut p = parameters();
        p.remove(name);
        assert!(EffectiveServo::new(&p).is_err());
        for value in [-1.0, f64::NAN, f64::INFINITY] {
            let mut p = parameters();
            p.insert(name.into(), value);
            assert!(EffectiveServo::new(&p).is_err());
        }
    }
    let mut p = parameters();
    p.insert("latency".into(), 0.01);
    assert!(EffectiveServo::new(&p).is_err());
}

#[test]
fn registered_component_applies_equal_opposite_shaft_and_housing_torques() {
    let servo = EffectiveServo::new(&parameters()).unwrap();
    for shift in [0.0, 0.7] {
        let mut through = [0.0; 4];
        let mut signals = [0.0; 4];
        let across = [0.2 + shift, 1.0 + shift, shift, shift];
        let mut ctx = Context::new(
            0.0,
            &[],
            &[],
            &[0, 2, 4, 4, 4],
            &[Some(1), None, Some(3), None],
            &across,
            &[0.0; 4],
            &[0.0, 0.0, 0.4, 0.0],
            &mut [],
            &mut through,
            &mut signals,
        );
        servo.residual(&mut ctx);
        assert!((through[0] + 1.5).abs() < 1e-14);
        assert!((through[2] - 1.5).abs() < 1e-14);
        assert!((signals[3] - 1.5).abs() < 1e-14);
        assert_eq!(through.iter().sum::<f64>(), 0.0);
    }
}

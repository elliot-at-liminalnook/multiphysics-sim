use sim_core::{Behavior, BehaviorRegistry, Context};
use sim_domain_robot::motor::MotorDynamics;
use std::collections::BTreeMap;

fn motor(mode: MotorDynamics) -> Box<dyn Behavior> {
    let mut registry = BehaviorRegistry::default();
    sim_domain_robot::register(&mut registry).unwrap();
    let descriptor = registry.get(&sim_domain_robot::MOTOR_UNIT.into()).unwrap();
    let mut parameters: BTreeMap<String, f64> = [
        ("resistance", 2.0),
        ("inductance", 0.004),
        ("torque_constant", 0.8),
        ("back_emf_constant", 0.7),
        ("rotor_inertia", 0.0002),
        ("gear_inertia", 0.0001),
        ("ratio", 1.0),
        ("efficiency", 1.0),
        ("gear_stiffness", 50.0),
        ("gear_damping", 0.1),
        ("backlash", 0.0),
        ("gear_friction", 0.0),
        ("no_load_current", 0.0),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v))
    .collect();
    parameters.extend(
        mode.parameter_flags()
            .into_iter()
            .map(|(k, v)| (k.into(), v)),
    );
    descriptor.validate_parameters(&parameters).unwrap();
    // Selecting a reduction must preserve the physical source parameters.
    assert_eq!(parameters["inductance"], 0.004);
    assert_eq!(parameters["rotor_inertia"], 0.0002);
    descriptor.equations.unwrap()(&parameters).unwrap()
}

fn residual(motor: &dyn Behavior, x: &[f64], rates: &[f64], voltage: f64) -> [f64; 3] {
    let mut residual = [0.0; 3];
    let mut through = [0.0; 5];
    let mut signals = [0.0; 3];
    motor.residual(&mut Context::new(
        0.0,
        x,
        rates,
        &[0, 1, 2, 4, 5],
        &[None, None, Some(3), None, None],
        &[voltage, 0.0, 0.0, 0.0, 293.15],
        &[0.0; 5],
        &[],
        &mut residual,
        &mut through,
        &mut signals,
    ));
    assert!((signals[1] - (50.0 * x[2] + 0.1 * x[1])).abs() < 1e-12);
    assert!((-through[4] - 2.0 * x[0] * x[0]).abs() < 1e-12);
    residual
}

#[test]
fn each_motor_reduction_matches_an_independent_linear_circuit_and_rotor_step() {
    use MotorDynamics::*;
    for mode in [Detailed, QuasistaticWinding, QuasistaticRotor, Quasistatic] {
        let motor = motor(mode);
        let l = if matches!(mode, QuasistaticWinding | Quasistatic) {
            0.0
        } else {
            0.004
        };
        let j = if matches!(mode, QuasistaticRotor | Quasistatic) {
            0.0
        } else {
            0.0003
        };
        for h in [0.0001, 0.001, 0.005] {
            let mut old = [0.0; 3];
            for tick in 0..40 {
                let voltage = if tick < 20 { 4.0 } else { -4.0 };
                let matrix = nalgebra::Matrix3::new(
                    l / h + 2.0,
                    0.7,
                    0.0,
                    -0.8,
                    j / h + 0.1,
                    50.0,
                    0.0,
                    -h,
                    1.0,
                );
                let expected = matrix
                    .lu()
                    .solve(&nalgebra::Vector3::new(
                        voltage + l / h * old[0],
                        j / h * old[1],
                        old[2],
                    ))
                    .unwrap();
                let mut x = old;
                sim_solve::solve_newton(&mut x, Default::default(), |x, r| {
                    let rates: Vec<_> = x.iter().zip(old).map(|(x, old)| (x - old) / h).collect();
                    r.copy_from_slice(&residual(&*motor, x, &rates, voltage));
                })
                .unwrap();
                for i in 0..3 {
                    assert!(
                        (x[i] - expected[i]).abs() < 1e-8,
                        "{mode:?} h={h} tick={tick} state={i}"
                    );
                }
                old = x;
            }
        }
    }
}

#[test]
fn quasistatic_transmission_keeps_compliance_and_converges_to_its_relaxation_law() {
    // I=(V-Ke*w)/R and Kt*I=k*theta+c*w imply
    // theta_dot=(Kt*V/R-k*theta)/(Kt*Ke/R+c).
    let motor = motor(MotorDynamics::Quasistatic);
    let (voltage, damping, stiffness) = (4.0, 0.8 * 0.7 / 2.0 + 0.1, 50.0);
    let equilibrium = 0.8 * voltage / (2.0 * stiffness);
    let horizon = 0.02;
    let exact = equilibrium * (1.0 - f64::exp(-horizon * stiffness / damping));
    let mut errors = Vec::new();
    for steps in [20, 40, 80] {
        let h = horizon / steps as f64;
        let mut state = [0.0; 3];
        for _ in 0..steps {
            let old = state;
            sim_solve::solve_newton(&mut state, Default::default(), |x, r| {
                r.copy_from_slice(&residual(
                    &*motor,
                    x,
                    &[0.0, 0.0, (x[2] - old[2]) / h],
                    voltage,
                ));
            })
            .unwrap();
        }
        let expected_be = equilibrium * (1.0 - (1.0 + h * stiffness / damping).powi(-steps));
        assert!((state[2] - expected_be).abs() < 1e-10);
        errors.push((state[2] - exact).abs());
    }
    assert!(errors.windows(2).all(|pair| pair[1] < 0.6 * pair[0]));
}

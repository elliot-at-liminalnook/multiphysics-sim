use sim_core::{Behavior, BehaviorRegistry, Context, Input, LocalJacobian, Output, View};
use std::collections::BTreeMap;
const OFFSETS: [usize; 5] = [0, 1, 2, 4, 5];
const RATE_MAP: [Option<usize>; 5] = [None, None, Some(3), None, None];
fn motor(analytic: bool, inductance: f64) -> Box<dyn Behavior> {
    motor_with_backlash(analytic, inductance, 0.01)
}
fn motor_with_backlash(analytic: bool, inductance: f64, backlash: f64) -> Box<dyn Behavior> {
    motor_with_dynamics(analytic, inductance, backlash, sim_domain_robot::motor::MotorDynamics::Detailed)
}
fn motor_with_dynamics(analytic: bool, inductance: f64, backlash: f64, dynamics: sim_domain_robot::motor::MotorDynamics) -> Box<dyn Behavior> {
    let mut registry = BehaviorRegistry::default();
    sim_domain_robot::register(&mut registry).unwrap();
    let mut params: BTreeMap<_, _> = [
        ("resistance", 2.0),
        ("torque_constant", 0.8),
        ("back_emf_constant", 0.7),
        ("inductance", inductance),
        ("ratio", 3.0),
        ("efficiency", 0.73),
        ("no_load_current", 0.1),
        ("backlash", backlash),
        ("gear_stiffness", 50.0),
        ("gear_damping", 0.1),
        ("gear_friction", 0.03),
        ("temp_coeff", 0.004),
        ("derating", 0.001),
        ("jacobian.analytic", if analytic { 1.0 } else { 0.0 }),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v))
    .collect();
    params.extend(dynamics.parameter_flags().into_iter().map(|(k,v)|(k.into(),v)));
    (registry
        .get(&sim_domain_robot::MOTOR_UNIT.into())
        .unwrap()
        .equations
        .unwrap())(&params)
    .unwrap()
}

#[test]
fn quasistatic_motor_partials_match_each_reduced_equation() {
    use sim_domain_robot::motor::MotorDynamics::*;
    for mode in [Detailed,QuasistaticWinding,QuasistaticRotor,Quasistatic] {
        let motor=motor_with_dynamics(true,0.003,0.01,mode);
        for gap in [-0.02,0.0,0.02] {
            let x=[0.3,0.7,gap,5.0,1.0,0.0,0.1,310.0,0.4,-0.7,0.9,0.1,0.2,-0.06,0.4,0.5];
            let j=jacobian(&*motor,&x);
            for col in 0..16 {
                let h=1e-6*(1.0+x[col].abs());
                let mut p=x;let mut n=x;p[col]+=h;n[col]-=h;
                for (row,(p,n)) in residual(&*motor,&p).iter().zip(residual(&*motor,&n)).enumerate() {
                    let fd=(p-n)/(2.0*h);
                    assert!((fd-j[row][col]).abs()<2e-6+2e-5*fd.abs(),"{mode:?} row {row} col {col}: {fd} != {}",j[row][col]);
                }
            }
            assert_eq!(j[0][8]==0.0,matches!(mode,QuasistaticWinding|Quasistatic));
            assert_eq!(j[1][9]==0.0,matches!(mode,QuasistaticRotor|Quasistatic));
        }
    }
}
fn view(x: &[f64; 16]) -> View<'_> {
    View {
        time: 0.3,
        states: &x[..3],
        offsets: &OFFSETS,
        rate_map: &RATE_MAP,
        across: &x[3..8],
        across_rates: &x[11..],
        signals_in: &[],
    }
}
fn residual(m: &dyn Behavior, x: &[f64; 16]) -> Vec<f64> {
    let mut s = [0.0; 3];
    let mut t = [0.0; 5];
    let mut o = [0.0; 3];
    let mut ctx = Context::new(
        0.3,
        &x[..3],
        &x[8..11],
        &OFFSETS,
        &RATE_MAP,
        &x[3..8],
        &x[11..],
        &[],
        &mut s,
        &mut t,
        &mut o,
    );
    m.residual(&mut ctx);
    s.into_iter().chain(t).chain(o).collect()
}
fn jacobian(m: &dyn Behavior, x: &[f64; 16]) -> Vec<Vec<f64>> {
    let mut j = LocalJacobian::default();
    assert!(m.jacobian(&view(x), &mut j));
    let mut matrix = vec![vec![0.0; 16]; 11];
    for (o, i, v) in j.entries {
        let row = match o {
            Output::State(i) => i,
            Output::Through(p, l) => 3 + OFFSETS[p] + l,
            Output::Signal(i) => 8 + i,
        };
        let col = match i {
            Input::State(i) => i,
            Input::StateRate(i) => 8 + i,
            Input::Across(p, l) => 3 + OFFSETS[p] + l,
            Input::AcrossDerivative(p, l) => 11 + OFFSETS[p] + l,
            Input::AcrossRate(p, l) => RATE_MAP[OFFSETS[p] + l]
                .map(|i| 3 + i)
                .unwrap_or(11 + OFFSETS[p] + l),
            Input::Signal(_) => panic!("no input signals"),
        };
        matrix[row][col] += v;
    }
    matrix
}
#[test]
fn all_motor_partials_match_independent_differences_in_smooth_modes() {
    for inductance in [0.0, 0.003] {
        let m = motor(true, inductance);
        let ordinary = motor(false, inductance);
        for (current, speed, temp) in [
            (0.3, 0.7, 310.0),
            (-0.4, 0.1, 350.0),
            (0.05, -0.4, 1100.0),
            (-0.0639998667, 0.1, 293.15),
        ] {
            for gap in [-0.02, 0.0, 0.02] {
                let x = [
                    current, speed, gap, 5.0, 1.0, 0.0, 77.0, temp, 0.4, -0.7, 0.9, 0.1, 0.2,
                    -0.06, 0.4, 0.5,
                ];
                assert_eq!(residual(&*m, &x), residual(&*ordinary, &x));
                let j = jacobian(&*m, &x);
                for eps in [1e-5, 1e-6, 1e-7] {
                    for col in 0..16 {
                        let h = eps * (1.0 + x[col].abs());
                        let mut p = x;
                        let mut n = x;
                        p[col] += h;
                        n[col] -= h;
                        let (p, n) = (residual(&*m, &p), residual(&*m, &n));
                        for row in 0..11 {
                            let fd = (p[row] - n[row]) / (2.0 * h);
                            assert!(
                                (fd - j[row][col]).abs() < 2e-6 + 2e-5 * fd.abs(),
                                "row {row} col {col}: {fd} != {}",
                                j[row][col]
                            );
                        }
                    }
                }
            }
        }
    }
}
#[test]
fn backlash_probe_crossing_reproduces_false_stiffness_but_local_derivative_is_zero() {
    let m = motor(true, 0.003);
    // A saved robot failure was 9.66 nrad inside engagement with 0.104 rad/s
    // relative speed. Preserve that scale in this standalone component fixture.
    let x = [
        0.05,
        0.312,
        0.005 - 9.66e-9,
        1.0,
        0.0,
        0.0,
        0.0,
        293.15,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
    ];
    let base = residual(&*m, &x);
    let j = jacobian(&*m, &x);
    assert_eq!(j[9][2], 0.0); // published coupling torque / gear angle
    let mut crossing = x;
    crossing[2] += 1e-8;
    assert!((residual(&*m, &crossing)[9] - base[9]) / 1e-8 > 900_000.0);
    let mut local = x;
    local[2] += 1e-10;
    assert_eq!(residual(&*m, &local)[9], base[9]);
    for sign in [-1.0, 1.0] {
        let mut edge = x;
        edge[2] = sign * 0.005;
        let mut inside = edge;
        inside[2] -= sign * 1e-10;
        assert_eq!(residual(&*m, &edge)[9], residual(&*m, &inside)[9]);
        assert_eq!(jacobian(&*m, &edge)[9][2], 0.0);
    }
}
#[test]
fn exact_motor_partials_require_explicit_opt_in() {
    let m = motor(false, 0.003);
    let mut j = LocalJacobian::default();
    let mut x = [0.0; 16];
    x[7] = 293.15;
    assert!(!m.jacobian(&view(&x), &mut j));
}

#[test]
fn zero_backlash_keeps_the_spring_derivative_at_zero_deflection() {
    let m = motor_with_backlash(true, 0.003, 0.0);
    let mut x = [0.0; 16];
    x[7] = 293.15;
    let j = jacobian(&*m, &x);
    assert_eq!(j[9][2], 50.0);
    assert_eq!(j[9][5], -50.0);
    for h in [1e-5, 1e-7, 1e-9] {
        let mut p = x;
        let mut n = x;
        p[2] += h;
        n[2] -= h;
        let fd = (residual(&*m, &p)[9] - residual(&*m, &n)[9]) / (2.0 * h);
        assert!((fd - 50.0).abs() < 1e-10);
    }
}

#[test]
fn discontinuous_backlash_damping_can_leave_an_implicit_step_without_a_root() {
    // A single positive inertia driven against this exact coupling law.
    // q_new=q_old+h*v_new, M*(v_new-v_old)/h=drive-coupling(q_new,v_new).
    // Solve each affine branch independently and check its admissible interval.
    let m = motor(true, 0.003);
    let (mass, h, half, v0) = (0.01, 0.0005, 0.005, 0.104);
    let (k, c_inside, c_outside) = (50.0, 0.005, 0.1);
    let q0 = half - h * v0;
    let drive = 0.5 * (c_inside + c_outside) * v0;
    let inside_v = (mass * v0 / h + drive) / (mass / h + c_inside);
    let positive_v = (mass * v0 / h + drive + k * (half - q0)) / (mass / h + c_outside + k * h);
    let negative_v = (mass * v0 / h + drive - k * (half + q0)) / (mass / h + c_outside + k * h);
    assert!(q0 + h * inside_v > half, "free-gap root violates its mode");
    assert!(
        q0 + h * positive_v < half,
        "positive-contact root violates its mode"
    );
    assert!(
        q0 + h * negative_v > -half,
        "negative-contact root violates its mode"
    );
    let f = |q: f64| {
        let v = (q - q0) / h;
        let x = [
            0.0,
            3.0 * v,
            q,
            0.0,
            0.0,
            0.0,
            0.0,
            293.15,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ];
        mass * (v - v0) / h - drive + residual(&*m, &x)[9]
    };
    assert!(f(half) < -0.004);
    assert!(f(half + 1e-12) > 0.004);
    // Therefore zero lies in the residual jump, not on a physical branch.
    // More Newton iterations or a different Jacobian cannot supply that root.
}

#[test]
fn reciprocal_motor_accounts_for_heat_and_stored_energy_in_both_power_directions() {
    let mut registry = BehaviorRegistry::default();
    sim_domain_robot::register(&mut registry).unwrap();
    let parameters: BTreeMap<String, f64> = [
        ("resistance", 2.), ("inductance", 0.003), ("torque_constant", 0.8),
        ("back_emf_constant", 0.8), ("derating", 0.), ("temp_coeff", 0.),
        ("ratio", 3.), ("efficiency", 0.73), ("no_load_current", 0.1),
        ("rotor_inertia", 0.02), ("gear_inertia", 0.01),
        ("gear_stiffness", 50.), ("gear_damping", 0.1), ("gear_friction", 0.03),
    ].into_iter().map(|(k,v)|(k.into(),v)).collect();
    let motor = registry.get(&sim_domain_robot::MOTOR_UNIT.into()).unwrap().equations.unwrap()(&parameters).unwrap();
    for current in [-2., 2.] {
        let mut x = [current, 7., 0.2, 12., 0., 0.1, 0., 293.15, 0., 0., 0., 0., 0., 0.5, 0., 0.];
        let r = residual(&*motor, &x);
        x[8] = -r[0]/0.003;
        x[9] = -r[1]/((0.02*9.+0.01)/3.);
        x[10] = -r[2];
        let r = residual(&*motor,&x);
        assert!(r[..3].iter().all(|v|v.abs()<1e-10));
        let input_power = 12.*current + r[5]*x[13] + r[7];
        let h = 1e-7;
        let mut before=x; let mut after=x;
        for k in 0..3 { before[k]-=h*x[8+k];after[k]+=h*x[8+k]; }
        before[5]-=h*x[13];after[5]+=h*x[13];
        let storage_rate=(motor.energy(&view(&after))-motor.energy(&view(&before)))/(2.*h);
        assert!((input_power-storage_rate).abs()<1e-6,"current={current}: net input {input_power}, storage {storage_rate}");
        assert!(r[7]<0.,"motor must emit dissipated heat");
    }
}

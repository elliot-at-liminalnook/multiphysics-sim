use sim_dynamics::{Ode, System, sdirk};
use sim_solve::NewtonConfig;
use std::cell::RefCell;
fn tight() -> NewtonConfig {
    NewtonConfig {
        absolute_tolerance: 1e-12,
        relative_tolerance: 1e-12,
        ..Default::default()
    }
}
struct Decay(f64);
impl Ode for Decay {
    fn dimension(&self) -> usize {
        1
    }
    fn derivative(&self, _: f64, x: &[f64], d: &mut [f64]) {
        d[0] = -self.0 * x[0];
    }
}
#[test]
fn smooth_decay_converges_second_order_and_stiff_modes_damp() {
    let integrate = |h: f64| {
        let mut x = vec![1.];
        for i in 0..(1. / h).round() as usize {
            x = sdirk::step(&Decay(2.), i as f64 * h, h, &x, tight())
                .unwrap()
                .state;
        }
        x[0]
    };
    let e1 = (integrate(0.05) - (-2f64).exp()).abs();
    let e2 = (integrate(0.025) - (-2f64).exp()).abs();
    assert!(e1 / e2 > 3.8 && e1 / e2 < 4.3, "{e1} / {e2}");
    for stiffness in [10., 1000., 1e6] {
        let s = sdirk::step(&Decay(stiffness), 0., 0.02, &[1.], tight()).unwrap();
        let z = -stiffness * 0.02;
        let g = sdirk::GAMMA;
        let exact_discrete = (1. + (1. - 2. * g) * z) / (1. - g * z).powi(2);
        assert!((s.state[0] - exact_discrete).abs() < 1e-9);
        assert!(s.state[0].abs() < 1.);
    }
}
#[test]
fn stage_times_and_algebraic_rows_use_real_endpoints() {
    struct Dae(RefCell<Vec<f64>>);
    impl System for Dae {
        fn dimension(&self) -> usize {
            2
        }
        fn algebraic(&self) -> Option<Vec<bool>> {
            Some(vec![false, true])
        }
        fn residual(&self, t: f64, x: &[f64], v: &[f64], r: &mut [f64]) {
            self.0.borrow_mut().push(t);
            r[0] = v[0] - t;
            r[1] = x[1] - x[0].powi(2);
        }
    }
    let sys = Dae(RefCell::new(vec![]));
    let x = [0.2, 0.04];
    let result = sdirk::step(&sys, 0.3, 0.1, &x, tight()).unwrap();
    assert!((result.state[0] - 0.235).abs() < 1e-11);
    assert!((result.state[1] - 0.235f64.powi(2)).abs() < 1e-11);
    for t in sys.0.borrow().iter() {
        assert!(
            (*t - (0.3 + sdirk::GAMMA * 0.1)).abs() < 1e-15 || (*t - 0.4).abs() < 1e-15,
            "{t}"
        );
    }
    assert_eq!(result.stages.len(), 2);
    assert_eq!(x, [0.2, 0.04]);
}
#[test]
fn failed_second_stage_does_not_mutate_the_input() {
    struct Fails;
    impl System for Fails {
        fn dimension(&self) -> usize {
            1
        }
        fn residual(&self, t: f64, _: &[f64], v: &[f64], r: &mut [f64]) {
            r[0] = if t > 0.05 { f64::NAN } else { v[0] - 1. };
        }
    }
    let initial = [0.];
    let failed = sdirk::step(&Fails, 0., 0.1, &initial, tight()).unwrap_err();
    assert_eq!(initial, [0.]);
    assert_eq!(failed.stages.len(), 2);
    assert!(failed.stages[0].solve_succeeded);
    assert!(!failed.stages[1].solve_succeeded);
    assert!(sdirk::step(&Decay(1.), 0., 0., &initial, tight()).is_err());
    assert!(sdirk::step(&Decay(1.), 1e20, 0.01, &initial, tight()).is_err());
}
#[test]
fn constant_acceleration_preserves_exact_velocity_and_position() {
    struct Force;
    impl Ode for Force {
        fn dimension(&self) -> usize {
            2
        }
        fn derivative(&self, _: f64, x: &[f64], d: &mut [f64]) {
            d[0] = x[1];
            d[1] = 3.;
        }
    }
    let s = sdirk::step(&Force, 0., 0.2, &[0.5, 2.], tight()).unwrap();
    assert!((s.state[0] - 0.96).abs() < 1e-11);
    assert!((s.state[1] - 2.6).abs() < 1e-11);
}

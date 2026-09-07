use sim_dynamics::jacobian_check::{check_jacobian, CheckConfig};
use sim_dynamics::{JacobianParts, System};

struct Coupled {
    wrong: bool,
}
impl System for Coupled {
    fn dimension(&self) -> usize {
        2
    }
    fn residual(&self, _: f64, x: &[f64], r: &[f64], out: &mut [f64]) {
        out[0] = x[0].sin() + x[0] * x[1] + (1.0 + x[1] * x[1]) * r[0];
        out[1] = x[0].exp() - 2.0 * x[1] + r[0] * r[1];
    }
    fn jacobian(&self, _: f64, x: &[f64], r: &[f64], j: &mut JacobianParts) -> bool {
        j.clear();
        j.dx(0, 0, x[0].cos() + x[1]);
        j.dx(0, 1, x[0] + 2.0 * x[1] * r[0]);
        j.dx(1, 0, x[0].exp());
        j.dx(1, 1, if self.wrong { 2.0 } else { -2.0 });
        j.drate(0, 0, 1.0 + x[1] * x[1]);
        j.drate(1, 0, r[1]);
        j.drate(1, 1, r[0]);
        true
    }
}
#[test]
fn exact_partials_pass_and_a_sign_regression_is_localized() {
    let x = [0.3, -0.7];
    let r = [1.2, -0.1];
    let config = CheckConfig::default();
    let good = check_jacobian(&Coupled { wrong: false }, 0.0, &x, &r, &config).unwrap();
    assert!(good.passed, "{good:?}");
    let bad = check_jacobian(&Coupled { wrong: true }, 0.0, &x, &r, &config).unwrap();
    assert!(!bad.passed && bad.mismatches > 0, "{bad:?}");
    assert!(bad
        .differences
        .iter()
        .any(|d| d.row == 1 && d.probe == "state[1]"));
    let directional = CheckConfig {
        columns: false,
        ..config
    };
    assert!(
        check_jacobian(&Coupled { wrong: false }, 0.0, &x, &r, &directional)
            .unwrap()
            .passed
    );
    assert!(
        !check_jacobian(&Coupled { wrong: true }, 0.0, &x, &r, &directional)
            .unwrap()
            .passed
    );
}
struct Contact;
impl System for Contact {
    fn dimension(&self) -> usize {
        1
    }
    fn residual(&self, _: f64, x: &[f64], _: &[f64], out: &mut [f64]) {
        out[0] = (-x[0]).max(0.0);
    }
    fn jacobian(&self, _: f64, x: &[f64], _: &[f64], j: &mut JacobianParts) -> bool {
        j.clear();
        j.dx(0, 0, if x[0] < 0.0 { -1.0 } else { 0.0 });
        true
    }
}
#[test]
fn contact_boundary_is_inconclusive_not_silently_passed() {
    let config = CheckConfig::default();
    for x in [-0.1, 0.1] {
        assert!(
            check_jacobian(&Contact, 0.0, &[x], &[0.0], &config)
                .unwrap()
                .passed
        );
    }
    let at_kink = check_jacobian(&Contact, 0.0, &[0.0], &[0.0], &config).unwrap();
    assert!(!at_kink.passed && at_kink.inconclusive > 0, "{at_kink:?}");
    assert!(at_kink
        .differences
        .iter()
        .any(|d| d.reason.contains("branch boundary")));
}
#[test]
fn invalid_scales_and_empty_checks_are_rejected() {
    for config in [
        CheckConfig {
            state_scales: vec![0.0],
            ..Default::default()
        },
        CheckConfig {
            columns: false,
            directions: 0,
            ..Default::default()
        },
    ] {
        assert!(check_jacobian(&Contact, 0.0, &[0.1], &[0.0], &config).is_err());
    }
}

#[test]
fn unresolved_reference_reports_roundoff_separately_from_stencil_error() {
    // This derivative is correct, but small probes disappear when added to the
    // large residual offset. The report must expose reference resolution rather
    // than implying that a zero numerical slope proves a bad supplied Jacobian.
    struct Offset;
    impl System for Offset {
        fn dimension(&self) -> usize { 1 }
        fn residual(&self, _: f64, x: &[f64], _: &[f64], out: &mut [f64]) {
            out[0] = 1e12 + x[0];
        }
        fn jacobian(&self, _: f64, _: &[f64], _: &[f64], j: &mut JacobianParts) -> bool {
            j.clear(); j.dx(0, 0, 1.0); true
        }
    }
    let config = CheckConfig { directions: 0, ..Default::default() };
    let report = check_jacobian(&Offset, 0.0, &[0.0], &[0.0], &config).unwrap();
    let difference = report.differences.iter().find(|d| d.probe == "state[0]").unwrap();
    assert!(!report.passed && difference.inconclusive);
    assert_eq!(difference.stencil_step, config.step);
    assert_eq!(difference.numerical_coarse, 0.0);
    assert_eq!(difference.numerical_fine, 0.0);
    assert_eq!(difference.reference_truncation_estimate, 0.0);
    assert!(difference.reference_roundoff_estimate > difference.tolerance);
    assert!(difference.reason.contains("roundoff"));
    assert_eq!(report.inconclusive_by_reason.values().sum::<usize>(), report.inconclusive);
}

#[test]
fn numerical_reference_bypasses_bad_derivatives_and_bad_sparsity() {
    use sim_dynamics::{Integrator, Simulation};
    struct Decay;
    impl System for Decay {
        fn dimension(&self) -> usize {
            1
        }
        fn residual(&self, _: f64, x: &[f64], r: &[f64], out: &mut [f64]) {
            out[0] = r[0] + 2.0 * x[0];
        }
        fn jacobian(&self, _: f64, _: &[f64], _: &[f64], _: &mut JacobianParts) -> bool {
            panic!("numerical reference must never call this derivative")
        }
        fn sparsity(&self) -> Option<sim_dynamics::jacobian::Sparsity> {
            Some(sim_dynamics::jacobian::Sparsity::new(vec![vec![]]))
        }
    }
    let mut sim = Simulation::new(Decay, Integrator::implicit_midpoint(), vec![1.0]);
    sim.set_numerical_jacobian(true);
    sim.run(0.2, 0.001).unwrap();
    assert!((sim.state[0] - (-0.4f64).exp()).abs() < 1e-7);
}

#[test]
fn intersecting_branches_are_detected_even_when_each_axis_looks_smooth() {
    // Homogeneous and odd along each line, but not differentiable at the
    // origin: central columns alone miss this type of switching intersection.
    struct Intersection;
    impl System for Intersection {
        fn dimension(&self) -> usize {
            2
        }
        fn residual(&self, _: f64, x: &[f64], _: &[f64], out: &mut [f64]) {
            out[0] = if x[0] == 0.0 && x[1] == 0.0 {
                0.0
            } else {
                x[0] * x[1] * x[1] / (x[0] * x[0] + x[1] * x[1])
            };
            out[1] = 0.0;
        }
        fn jacobian(&self, _: f64, _: &[f64], _: &[f64], j: &mut JacobianParts) -> bool {
            j.clear();
            true
        }
    }
    let report = check_jacobian(
        &Intersection,
        0.0,
        &[0.0; 2],
        &[0.0; 2],
        &CheckConfig::default(),
    )
    .unwrap();
    assert!(!report.passed && report.inconclusive > 0, "{report:?}");
    assert!(report
        .differences
        .iter()
        .any(|d| d.reason.contains("branch intersection")));
}

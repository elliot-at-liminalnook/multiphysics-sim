use sim_domain_multibody::contact::{regularized_coulomb, regularized_coulomb_scalar};

#[test]
fn isotropic_friction_has_combined_capacity_and_dissipates_power() {
    for capacity in [0.0, 1e-10, 1.0, 1e4] {
        for epsilon in [1e-5, 1e-3, 0.1] {
            for slip in [
                [0.0; 3],
                [1e-15, 0.0, 0.0],
                [0.3, -0.4, 0.5],
                [-2.0, 0.0, 1.0],
            ] {
                let force = regularized_coulomb(slip, capacity, epsilon);
                let norm = force.iter().fold(0.0_f64, |n, f| n.hypot(*f));
                let power: f64 = force.iter().zip(slip).map(|(f, v)| f * v).sum();
                assert!(norm <= capacity * (1.0 + 1e-14));
                assert!(power <= 0.0);
                let reverse = regularized_coulomb(slip.map(|v| -v), capacity, epsilon);
                for (a, b) in force.iter().zip(reverse) {
                    assert!((a + b).abs() < 1e-12 * capacity.max(1.0));
                }
            }
        }
    }
    let diagonal = regularized_coulomb([1.0, 1.0, 1.0], 3.0, 1e-3);
    assert!((diagonal[0] + 3.0_f64.sqrt()).abs() < 1e-14);
}

#[test]
fn scalar_limit_and_subcapacity_creep_are_explicit() {
    for v in [-1.0, -1e-5, 0.0, 1e-5, 1.0] {
        assert!(
            (regularized_coulomb([v], 2.0, 0.001)[0] - regularized_coulomb_scalar(v, 2.0, 0.001))
                .abs()
                < 1e-15
        );
    }
    let epsilon = 0.001;
    let creep = epsilon * 0.5_f64.atanh();
    assert!((regularized_coulomb([creep], 10.0, epsilon)[0] + 5.0).abs() < 1e-14);
    for epsilon in [0.0, -1.0, f64::NAN] {
        assert!(regularized_coulomb([1.0], 1.0, epsilon)[0].is_nan());
    }
    assert!(regularized_coulomb([f64::INFINITY], 1.0, 0.001)[0].is_nan());
    assert!(regularized_coulomb([0.0], -1.0, 0.001)[0].is_nan());
}

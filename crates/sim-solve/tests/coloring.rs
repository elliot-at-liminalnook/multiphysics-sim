use sim_solve::{
    BlockDiagonalColoring, SparseJacobian, solve_newton, solve_newton_numeric_colored,
};
use std::cell::Cell;

fn equations(x: &[f64], r: &mut [f64]) {
    r[0] = x[0] * x[0] + 3.0 * x[0] + 0.2 * x[1] - 1.0;
    r[1] = 0.1 * x[0] + 2.0 * x[1] + 0.1 * x[1].powi(3) + 0.3;
    r[2] = x[2].tanh() - 0.2;
    r[3] = 3.0 * x[3] + x[4] * x[4] - 0.1;
    r[4] = 0.2 * x[3] + 2.0 * x[4] - x[5] + 0.1;
    r[5] = x[4] + 3.0 * x[5] + 0.2 * x[5].powi(3) - 0.2;
}

#[test]
fn unequal_blocks_recover_identical_columns_with_fewer_probes() {
    let pattern = BlockDiagonalColoring::new(&[2, 1, 3]).unwrap();
    assert_eq!(pattern.dimension(), 6);
    assert_eq!(pattern.colors(), 3);
    for initial in [[0.0; 6], [0.3, -0.5, 0.9, -0.2, 0.8, -0.7]] {
        let mut x = initial;
        let mut base = [0.0; 6];
        equations(&x, &mut base);
        let calls = Cell::new(0);
        let mut colored = SparseJacobian::new(6);
        pattern
            .assemble(
                &mut x,
                &base,
                |x, r| {
                    calls.set(calls.get() + 1);
                    equations(x, r);
                },
                &mut colored,
            )
            .unwrap();
        assert_eq!(x, initial);
        assert_eq!(calls.get(), 3);
        let mut ordinary = SparseJacobian::new(6);
        for col in 0..6 {
            let h = 1e-6 * (1.0 + x[col].abs());
            x[col] += h;
            let mut r = [0.0; 6];
            equations(&x, &mut r);
            x[col] = initial[col];
            for row in 0..6 {
                ordinary.add(row, col, (r[row] - base[row]) / h);
            }
        }
        let nonzero = |j: &SparseJacobian| {
            j.summed()
                .into_iter()
                .filter(|(_, _, v)| *v != 0.0)
                .collect::<Vec<_>>()
        };
        assert_eq!(nonzero(&colored), nonzero(&ordinary));
        for epsilon in [1e-4, 1e-6, 1e-8] {
            assert_eq!(
                pattern.audit_off_block(&x, epsilon, equations).unwrap(),
                0.0
            );
        }
    }
}

#[test]
fn compressed_newton_preserves_nonlinear_solution_and_acceptance() {
    let pattern = BlockDiagonalColoring::new(&[2, 1, 3]).unwrap();
    let mut a = [0.0; 6];
    let mut b = a;
    let ca = Cell::new(0);
    let cb = Cell::new(0);
    let da = solve_newton(&mut a, Default::default(), |x, r| {
        ca.set(ca.get() + 1);
        equations(x, r);
    })
    .unwrap();
    let db = solve_newton_numeric_colored(
        &mut b,
        Default::default(),
        |x, r| {
            cb.set(cb.get() + 1);
            equations(x, r);
        },
        &pattern,
    )
    .unwrap();
    assert_eq!(a, b);
    assert_eq!(da.iterations, db.iterations);
    assert_eq!(da.residual_norm, db.residual_norm);
    assert!(cb.get() < ca.get(), "{} vs {}", cb.get(), ca.get());
}

#[test]
fn independent_audit_detects_a_false_structural_declaration() {
    let pattern = BlockDiagonalColoring::new(&[1, 1]).unwrap();
    let wrong = |x: &[f64], r: &mut [f64]| {
        r[0] = x[0] + 2.0 * x[1];
        r[1] = x[1];
    };
    assert!((pattern.audit_off_block(&[0.2, -0.1], 1e-6, wrong).unwrap() - 2.0).abs() < 1e-8);
    assert!(BlockDiagonalColoring::new(&[2, 0]).is_err());
    assert!(BlockDiagonalColoring::new(&[usize::MAX, 1]).is_err());
    assert!(pattern.audit_off_block(&[0.0], 1e-6, wrong).is_err());
}

#[test]
fn nonfinite_probes_restore_unknowns_and_fail_closed() {
    let pattern = BlockDiagonalColoring::new(&[1, 1]).unwrap();
    let mut x = [0.2, 0.3];
    let before = x;
    assert!(
        pattern
            .assemble(
                &mut x,
                &[0.0; 2],
                |_, r| r.fill(f64::NAN),
                &mut SparseJacobian::new(2)
            )
            .is_err()
    );
    assert_eq!(x, before);
    assert!(
        solve_newton_numeric_colored(&mut [0.0], Default::default(), |_, _| {}, &pattern).is_err()
    );
}

#[test]
fn affine_rate_coordinates_use_physical_correction_scales_without_relaxing_residuals() {
    use sim_solve::{
        NewtonAudit, NewtonConfig, solve_newton_numeric_colored_scaled_audited,
        solve_newton_numeric_scaled_cached_audited,
    };
    let config = NewtonConfig {
        absolute_tolerance: 1e-12,
        relative_tolerance: 1e-10,
        max_iterations: 40,
        ..Default::default()
    };
    let pattern = BlockDiagonalColoring::new(&[1]).unwrap();
    for h in [1e-2, 1e-6, 1e-9] {
        for colored in [false, true] {
            let old = 1.0;
            let mut rate = [0.2 / h];
            let mut audit = NewtonAudit::default();
            let residual = |r: &[f64], out: &mut [f64]| {
                let x = old + h * r[0];
                out[0] = x * x - 2.0;
            };
            let scale = |_: usize, r: f64| (1.0 + (old + h * r).abs()) / h;
            let result = if colored {
                solve_newton_numeric_colored_scaled_audited(
                    &mut rate,
                    config,
                    residual,
                    &pattern,
                    &scale,
                    Some(&mut audit),
                )
            } else {
                solve_newton_numeric_scaled_cached_audited(
                    &mut rate,
                    config,
                    residual,
                    &scale,
                    &mut None,
                    Some(&mut audit),
                )
            };
            result.unwrap();
            let state = old + h * rate[0];
            assert!((state - 2.0_f64.sqrt()).abs() < 1e-11);
            let mut r = [0.0];
            residual(&rate, &mut r);
            assert!(r[0].abs() <= audit.residual_limits[0]);
            assert!(
                audit
                    .iterations
                    .iter()
                    .filter_map(|i| i.correction.as_ref())
                    .all(|c| c
                        .largest_unknowns
                        .iter()
                        .all(|(_, _, bound, _)| h * bound < 3e-10))
            );
        }
    }
    // A large coordinate scale cannot turn an inconsistent equation into an
    // accepted root. Fresh raw residual checks remain authoritative.
    for colored in [false, true] {
        let mut x = [1.0];
        let residual = |x: &[f64], r: &mut [f64]| r[0] = x[0] * x[0] + 1.0;
        let scale = |_: usize, _: f64| 1e20;
        let result = if colored {
            solve_newton_numeric_colored_scaled_audited(
                &mut x, config, residual, &pattern, &scale, None,
            )
        } else {
            solve_newton_numeric_scaled_cached_audited(
                &mut x, config, residual, &scale, &mut None, None,
            )
        };
        assert!(result.is_err());
    }
}

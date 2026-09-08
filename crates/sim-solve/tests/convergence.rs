//! Convergence, backtracking and stale-Jacobian recovery regressions.
use sim_solve::{solve_newton_with_jacobian, NewtonConfig};

#[test]
fn fresh_final_iterations_finish_a_contracting_tail_at_the_same_tolerances() {
    use sim_solve::{solve_newton_numeric_cached_audited, NewtonAudit};
    for refresh in [false,true] {
        let mut x=[1.0];
        let mut audit=NewtonAudit::default();
        let config=NewtonConfig{refresh_before_iteration_limit:refresh,..Default::default()};
        let result=solve_newton_numeric_cached_audited(&mut x,config,
            |x,r|r[0]=x[0]*x[0]-2.0,&mut None,Some(&mut audit));
        if refresh {
            let solved=result.unwrap();
            assert!(solved.iterations<=config.max_iterations);
            assert!((x[0]-2.0_f64.sqrt()).abs()<1e-12);
            assert!((x[0]*x[0]-2.0).abs()<config.absolute_tolerance);
            assert!(audit.iterations.iter().filter(|i|i.iteration>=config.max_iterations-2).all(|i|i.fresh_jacobian));
        } else {
            assert!(result.is_err(),"fixture must expose the old exhausted tail");
        }
    }
}

#[test]
fn numerical_audit_preserves_residual_calls_state_and_cache_reuse() {
    use sim_solve::{solve_newton_numeric_cached_audited, NewtonAudit};
    let run = |record: bool| {
        let calls = std::cell::Cell::new(0);
        let mut x = [1.0];
        let mut cache = None;
        let mut traces = Vec::new();
        for (target, nonlinear) in [(2.0, false), (2.1, false), (2.1, true)] {
            let mut audit = NewtonAudit::default();
            let result = solve_newton_numeric_cached_audited(&mut x,
                NewtonConfig::default(), |x,r| {
                    calls.set(calls.get()+1);
                    r[0]=if nonlinear { x[0]*x[0]-target } else { x[0]-target };
                }, &mut cache, record.then_some(&mut audit));
            // Preserve failures too: this fixture can reach the existing
            // correction/residual acceptance disagreement with a stale matrix.
            traces.push((format!("{result:?}"),audit));
        }
        (x,calls.get(),traces)
    };
    let plain=run(false);
    let audited=run(true);
    assert_eq!(plain.0,audited.0);
    assert_eq!(plain.1,audited.1);
    for (a,b) in plain.2.iter().zip(&audited.2) {
        assert_eq!(a.0,b.0);
        assert!(a.1.iterations.is_empty());
        assert!(!b.1.iterations.is_empty());
    }
    assert!(!audited.2[1].1.iterations[0].fresh_jacobian);
}

#[test]
fn a_jump_with_no_root_must_not_be_accepted_as_converged() {
    // Each smooth branch has slope one, but its root lies outside that branch.
    // Consequently |F(x)| >= 1 for every finite x: there is no root to accept.
    let f = |x: f64| if x < 0.0 { x - 1.0 } else { x + 1.0 };
    for refresh in [false, true] {
    for broyden_updates in [false, true] {
    let mut x = [-1e-10];
    let result = solve_newton_with_jacobian(
        &mut x,
        NewtonConfig{refresh_before_iteration_limit:refresh,broyden_updates,..Default::default()},
        |x, r| r[0] = f(x[0]),
        |x, base, jacobian| {
            let h = 1e-8 * (1.0 + x[0].abs());
            jacobian[(0, 0)] = (f(x[0] + h) - base[0]) / h;
        },
    );
    assert!(
        result.is_err(),
        "accepted a nonexistent root: {result:?}, x={x:?}, F={}",
        f(x[0])
    );
    }
    }
}

#[test]
fn secants_reduce_derivative_probes_with_the_same_raw_acceptance() {
    use sim_solve::{solve_newton_numeric_cached_audited, NewtonAudit};
    let mut costs = Vec::new();
    for broyden_updates in [false, true] {
        let mut x = [1.0];
        let mut audit = NewtonAudit::default();
        let calls = std::cell::Cell::new(0);
        let config = NewtonConfig { broyden_updates, refresh_before_iteration_limit: true, ..Default::default() };
        solve_newton_numeric_cached_audited(&mut x, config, |x,r| {
            calls.set(calls.get()+1); r[0] = x[0]*x[0]-2.0;
        }, &mut None, Some(&mut audit)).unwrap();
        assert!((x[0]-2.0_f64.sqrt()).abs() < 1e-12);
        assert!((x[0]*x[0]-2.0).abs() <= config.absolute_tolerance);
        if broyden_updates { assert!(audit.iterations.iter().any(|i| i.decision == "broyden_update")); }
        costs.push((calls.get(), audit.iterations.iter().filter(|i| i.fresh_jacobian).count()));
    }
    assert!(costs[1].0 < costs[0].0 && costs[1].1 < costs[0].1, "{costs:?}");
}

#[test]
fn stiff_coupled_equations_with_different_row_units_still_converge() {
    for scale in [1e-9, 1.0, 1e12] {
        let mut x = [0.1, 0.2];
        solve_newton_with_jacobian(
            &mut x,
            NewtonConfig::default(),
            |x, r| {
                r[0] = scale * (x[0] + x[1] - 3.0);
                r[1] = (x[0] - x[1] - 1.0) / scale;
            },
            |_, _, j| {
                j[(0, 0)] = scale;
                j[(0, 1)] = scale;
                j[(1, 0)] = 1.0 / scale;
                j[(1, 1)] = -1.0 / scale;
            },
        )
        .unwrap();
        assert!((x[0] - 2.0).abs() < 1e-12 && (x[1] - 1.0).abs() < 1e-12);
    }
}

#[test]
fn a_false_small_correction_refreshes_before_returning_a_solution() {
    let mut x = [0.0];
    let mut builds = 0;
    solve_newton_with_jacobian(
        &mut x,
        NewtonConfig::default(),
        |x, r| r[0] = x[0] - 1.0,
        |_, _, j| {
            builds += 1;
            // An inflated first derivative would previously cause immediate
            // false success near x=0. A refresh supplies the correct slope.
            j[(0, 0)] = if builds == 1 { 1e12 } else { 1.0 };
        },
    )
    .unwrap();
    assert!(builds >= 2);
    assert_eq!(x[0], 1.0);
}


#[test]
fn stale_full_step_failure_refreshes_without_discarded_probes() {
    check_stale_refresh(false);
}

#[test]
fn stale_refresh_does_not_probe_discarded_domain_holes() {
    check_stale_refresh(true);
}

fn check_stale_refresh(domain_hole: bool) {
    use sim_solve::solve_newton_cached;
    use std::cell::RefCell;
    let mut cache = None;
    let mut x = [1.0];
    let scale = |_: usize, value: f64| 1.0 + value.abs();
    // Obtain a real factorization from another equation. Its sign is wrong
    // for the subsequent equation F(x)=1-x.
    solve_newton_cached(&mut x, NewtonConfig::default(),
        |x,r| r[0]=x[0], |_,_,j| j.add(0,0,1.0), &scale, &mut cache).unwrap();
    assert_eq!(x[0],0.0);
    assert!(cache.is_some());
    let probes = RefCell::new(Vec::new());
    let mut builds = 0;
    let result = solve_newton_cached(&mut x, NewtonConfig::default(),
        |x,r| {
            probes.borrow_mut().push(x[0]);
            // The failed full trial is finite. Backtracked trials would enter
            // an undefined region, but none can be used with a stale matrix.
            r[0] = if domain_hole && x[0] > -0.75 && x[0] < 0.0 {
                f64::NAN
            } else {1.0-x[0]};
        },
        |x,r,j| {
            builds += 1;
            assert_eq!((x[0],r[0]),(0.0,1.0), "refresh must restore both point and residual");
            j.add(0,0,-1.0);
        }, &scale, &mut cache);
    assert!(result.is_ok(), "{result:?}; probes={:?}",probes.borrow());
    assert_eq!(x[0],1.0);
    assert_eq!(builds,1);
    assert_eq!(probes.into_inner(),vec![0.0,-1.0,1.0]);
    assert_eq!(result.unwrap().line_search_reductions,0);
}


#[test]
fn fresh_jacobian_keeps_backtracking_for_a_nonlinear_root() {
    let mut x = [0.1];
    let result = solve_newton_with_jacobian(&mut x, NewtonConfig::default(),
        |x,r| r[0] = x[0].powi(3)-1.0,
        |x,_,j| j[(0,0)] = 3.0*x[0]*x[0]).unwrap();
    // The initial full Newton trial is ~33.4, far worse than the starting
    // point. A fresh matrix must still backtrack and reach the analytic root.
    assert!(result.line_search_reductions > 0);
    assert!((x[0]-1.0).abs() < 1e-7);
}

#[test]
fn correction_audit_exposes_motion_hidden_by_small_equation_residuals() {
    use sim_solve::{solve_newton_cached_audited, NewtonAudit};
    // A tiny residual in the first equation still requires a unit displacement.
    // Auditing must expose that distinction without changing the solve.
    let mut audited_result = None;
    for enabled in [false, true] {
        let mut x = [0.0, 0.0];
        let mut audit = NewtonAudit::default();
        let result = solve_newton_cached_audited(&mut x, NewtonConfig::default(),
            |x,r| { r[0]=1e-12*(x[0]-1.0); r[1]=x[1]-2.0; },
            |_,_,j| { j.add(0,0,1e-12); j.add(1,1,1.0); },
            &|_,_| 1.0, &mut None, enabled.then_some(&mut audit)).unwrap();
        if enabled {
            assert_eq!(audited_result, Some((x, result.iterations, result.line_search_reductions)));
            let c = audit.iterations[0].correction.as_ref().unwrap();
            assert!(!c.negligible && c.tight && !c.at_floor);
            assert_eq!(c.largest_unknowns.len(), 2);
            assert_eq!(c.largest_unknowns[0].0, 1);
            assert_eq!(c.largest_unknowns[1].0, 0);
            assert_eq!(c.largest_unknowns[1].1, 1.0);
            assert_eq!(c.largest_unknowns[1].2, NewtonConfig::default().relative_tolerance);
            assert_eq!(c.largest_unknowns[1].3, 1.0/NewtonConfig::default().relative_tolerance);
            assert!(1e-12 < audit.residual_limits[0]);
        } else {
            audited_result = Some((x, result.iterations, result.line_search_reductions));
        }
        assert_eq!(x, [1.0, 2.0]);
    }
}

#[test]
fn backtracking_audit_preserves_probes_and_identifies_the_selected_trial() {
    use sim_solve::{solve_newton_cached_audited, NewtonAudit};
    use std::cell::RefCell;
    let mut baseline = None;
    for enabled in [false, true] {
        let mut x = [0.1_f64];
        let probes = RefCell::new(Vec::new());
        let mut audit = NewtonAudit::default();
        let result = solve_newton_cached_audited(&mut x, NewtonConfig::default(),
            |x, r| { probes.borrow_mut().push(x[0]); r[0] = x[0].powi(3) - 1.0; },
            |x, _, j| j.add(0, 0, 3.0 * x[0] * x[0]),
            &|_, x| 1.0 + x.abs(), &mut None, enabled.then_some(&mut audit)).unwrap();
        assert!((x[0] - 1.0).abs() < 1e-7);
        let signature = (x, result.iterations, result.line_search_reductions, probes.into_inner());
        if enabled {
            assert_eq!(baseline.as_ref(), Some(&signature), "audit changed solver work");
            let first = &audit.iterations[0];
            let search = first.line_search.as_ref().unwrap();
            assert!(search.trials.len() > 2);
            assert_eq!(search.trials[0].alpha, 1.0);
            assert_eq!(search.trials.last().unwrap().alpha, NewtonConfig::default().min_line_search);
            let best = search.trials.iter().min_by(|a, b|
                a.scaled_residual_norm.total_cmp(&b.scaled_residual_norm)).unwrap();
            assert_eq!(search.selected_alpha, Some(best.alpha));
            assert_ne!(search.selected_alpha, Some(search.trials.last().unwrap().alpha));
            // Verify recorded norms against the actual residual probes, using
            // this iteration's row scale rather than the final matrix scale.
            let row_scale = first.largest_rows[0].2 / first.largest_rows[0].1.abs();
            for (trial, point) in search.trials.iter().zip(&signature.3[1..]) {
                let expected = (point.powi(3) - 1.0).abs() * row_scale;
                assert!((trial.scaled_residual_norm - expected).abs() <= 1e-12 * (1.0 + expected));
            }
        } else {
            baseline = Some(signature);
        }
    }
}

#[test]
fn guarded_backtracking_saves_probes_but_keeps_the_noise_floor_reference() {
    use sim_solve::{solve_newton_cached_audited, NewtonAudit};
    use std::cell::Cell;
    // The unique real root is `scale`. Scaling the coordinate moves the
    // row-equilibrated merit below the guard without weakening raw acceptance.
    for scale in [1.0, 1e-10] {
        let mut runs = Vec::new();
        for guarded in [false, true] {
            let mut x = [0.1 * scale];
            let calls = Cell::new(0);
            let mut audit = NewtonAudit::default();
            let config = NewtonConfig { guarded_backtracking: guarded,
                min_line_search: 1.0 / 4096.0, relative_tolerance: 1e-14,
                max_iterations: 40, ..Default::default() };
            solve_newton_cached_audited(&mut x, config,
                |x, r| { calls.set(calls.get() + 1); r[0] = (x[0] / scale).powi(3) - 1.0; },
                |x, _, j| j.add(0, 0, 3.0 * (x[0] / scale).powi(2) / scale),
                &|_, _| scale, &mut None, Some(&mut audit)).unwrap();
            assert!(((x[0] / scale).powi(3) - 1.0).abs() < 1e-10);
            let search = audit.iterations[0].line_search.as_ref().unwrap();
            assert_eq!(search.bracketed, guarded && scale == 1.0);
            if !search.bracketed {
                assert_eq!(search.trials.last().unwrap().alpha, config.min_line_search);
            }
            runs.push((x[0].to_bits(), calls.get(), search.selected_alpha));
        }
        assert_eq!(runs[0].0, runs[1].0, "root differs at scale {scale}");
        assert_eq!(runs[0].2, runs[1].2, "first selected step differs");
        if scale == 1.0 { assert!(runs[1].1 < runs[0].1); }
        else { assert_eq!(runs[1].1, runs[0].1); }
    }
}

#[test]
fn domain_aware_backtracking_rejects_invalid_trials_without_accepting_them() {
    let law = |x: &[f64], r: &mut [f64]| r[0] = x[0].ln() - 1.0;
    let mut baseline = [10.0];
    assert!(sim_solve::solve_newton(&mut baseline, Default::default(), law).is_err());
    let config = sim_solve::NewtonConfig {
        reject_nonfinite_trials: true,
        max_iterations: 40,
        ..Default::default()
    };
    let mut x = [10.0];
    let answer = sim_solve::solve_newton(&mut x, config, law).unwrap();
    assert!((x[0] - std::f64::consts::E).abs() < 1e-8);
    assert!(answer.line_search_reductions > 0);
    let mut invalid_start = [-1.0];
    assert_eq!(sim_solve::solve_newton(&mut invalid_start, config, law).unwrap_err(), sim_solve::SolveError::NonFinite);
    let mut nowhere = [1.0];
    let error = sim_solve::solve_newton(&mut nowhere, config, |x, r| {
        r[0] = if x[0] < 1.01 { x[0] - 100.0 } else { f64::NAN };
    }).unwrap_err();
    assert_eq!(error, sim_solve::SolveError::NonFinite);
    assert_eq!(nowhere, [1.0]);
}

#[test]
fn numerical_cache_snapshots_refresh_and_handle_dimension_changes() {
    use sim_solve::solve_newton_numeric_cached;
    use std::cell::Cell;
    let cfg = NewtonConfig::default();
    let mut cache = None;
    let calls = Cell::new(0);
    let solve = |x: &mut [f64], slope: f64, cache: &mut Option<sim_solve::JacobianCache>| {
        solve_newton_numeric_cached(x, cfg, |x, r| {
            calls.set(calls.get() + 1);
            for (r, x) in r.iter_mut().zip(x) { *r = slope * (*x - 2.0); }
        }, cache).unwrap();
        assert!(x.iter().all(|x| (*x - 2.0).abs() < 1e-9));
    };
    solve(&mut [0.0], 1.0, &mut cache);
    let fresh_calls = calls.replace(0);
    let accepted = cache.clone();
    let accepted_uses = accepted.as_ref().unwrap().uses;
    solve(&mut [0.0], 1.0, &mut cache);
    assert!(calls.replace(0) < fresh_calls);
    // A changed derivative gives an uphill stale correction. It must refresh.
    solve(&mut [0.0], -10.0, &mut cache);
    assert_eq!(accepted.as_ref().unwrap().uses, accepted_uses);
    solve(&mut [0.0, 0.0], 3.0, &mut cache);
    let mut trial = accepted.clone();
    assert!(solve_newton_numeric_cached(&mut [0.0], cfg,
        |_, r| r.fill(f64::NAN), &mut trial).is_err());
    let mut repeat = accepted;
    solve(&mut [0.0], 1.0, &mut repeat);
}

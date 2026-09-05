//! The island's assembled Jacobian — analytic where an element reports its
//! derivatives, element-local differences elsewhere — must agree with the
//! whole-island finite-difference Jacobian.

use nalgebra::DMatrix;
use sim_dynamics::System;
use sim_phenomena::scenarios::{language_independence, leg_seam, quadruped_gait};
use sim_phenomena::world::registry;

fn compare(name: &str, rt: &sim_compile::Runtime, x: &[f64], rate: &[f64]) {
    let system = &rt.islands[0].system;
    eprintln!("{name}: {} unknowns solved of {} in the store", system.reduced_dimension(), system.state_ids.len());
    let label = |i: usize| rt.model.state.entry(system.state_ids[i]).map(|e| e.name.clone()).unwrap_or_default();
    let n = x.len();
    let mut parts = sim_dynamics::JacobianParts::default();
    assert!(system.jacobian(0.0, x, rate, &mut parts));
    let (d_dx, d_drate): (DMatrix<f64>, DMatrix<f64>) = parts.dense(n);
    // Whole-island differences, one column at a time.
    let mut base = vec![0.0; n];
    system.residual(0.0, x, rate, &mut base);
    let mut worst = (0.0_f64, 0usize, 0usize, 0.0, 0.0);
    let mut xp = x.to_vec();
    let mut rp = rate.to_vec();
    let mut out = vec![0.0; n];
    for col in 0..n {
        let eps = 1.0e-6 * (1.0 + x[col].abs());
        xp[col] = x[col] + eps;
        system.residual(0.0, &xp, rate, &mut out);
        xp[col] = x[col];
        for row in 0..n {
            let fd = (out[row] - base[row]) / eps;
            let scale = 1.0 + fd.abs().max(d_dx[(row, col)].abs());
            let err = (fd - d_dx[(row, col)]).abs() / scale;
            if err > worst.0 { worst = (err, row, col, fd, d_dx[(row, col)]); }
        }
        let eps = 1.0e-6 * (1.0 + rate[col].abs());
        rp[col] = rate[col] + eps;
        system.residual(0.0, x, &rp, &mut out);
        rp[col] = rate[col];
        for row in 0..n {
            let fd = (out[row] - base[row]) / eps;
            let scale = 1.0 + fd.abs().max(d_drate[(row, col)].abs());
            let err = (fd - d_drate[(row, col)]).abs() / scale;
            if err > worst.0 { worst = (err, row, col, fd, d_drate[(row, col)]); }
        }
    }
    eprintln!("{name}: worst relative mismatch {:.2e} at ({} `{}`, {} `{}`): fd {} vs assembled {}", worst.0, worst.1, label(worst.1), worst.2, label(worst.2), worst.3, worst.4);
    // Forward differences on a regularised friction kink (tanh with ε = 1e-3) are themselves only good to ~1e-3.
    assert!(worst.0 < 2.0e-3, "{name}: assembled Jacobian disagrees with differences: {worst:?}");
}

#[test]
fn motor_loop_jacobian_matches() {
    let registry = registry();
    let (rt, _, _, _) = language_independence::plant(&registry, 2.0e-3);
    let island = &rt.islands[0];
    let mut x = island.state.clone();
    for (k, v) in x.iter_mut().enumerate() { *v += 0.1 * (k as f64 + 1.0).sin(); }
    let rate: Vec<f64> = (0..x.len()).map(|k| 0.3 * (k as f64).cos()).collect();
    compare("motor loop", &rt, &x, &rate);
}

#[test]
fn leg_jacobian_matches() {
    let registry = registry();
    let leg = leg_seam::Leg { compliant: true, ..leg_seam::Leg::default() };
    let plant = leg.model(&registry);
    let island = &plant.runtime.islands[0];
    let mut x = island.state.clone();
    for (k, v) in x.iter_mut().enumerate() { *v += 0.01 * (k as f64 + 1.0).sin(); }
    let rate: Vec<f64> = (0..x.len()).map(|k| 0.2 * (k as f64).cos()).collect();
    compare("leg", &plant.runtime, &x, &rate);
}

#[test]
fn quadruped_jacobian_matches() {
    let registry = registry();
    let plant = quadruped_gait::Quadruped::default().model(&registry);
    let island = &plant.runtime.islands[0];
    let mut x = island.state.clone();
    for (k, v) in x.iter_mut().enumerate() { *v += 0.01 * (k as f64 + 1.0).sin(); }
    let rate: Vec<f64> = (0..x.len()).map(|k| 0.2 * (k as f64).cos()).collect();
    compare("quadruped", &plant.runtime, &x, &rate);
}

#[test]
fn free_leg_jacobian_matches() {
    let registry = registry();
    let leg = leg_seam::Leg { ground: false, ..leg_seam::Leg::default() };
    let plant = leg.model(&registry);
    let island = &plant.runtime.islands[0];
    let x = island.state.clone();
    let rate = vec![0.0; x.len()];
    compare("free leg at rest", &plant.runtime, &x, &rate);
    let mut x = island.state.clone();
    for (k, v) in x.iter_mut().enumerate() { *v += 0.01 * (k as f64 + 1.0).sin(); }
    let rate: Vec<f64> = (0..x.len()).map(|k| 0.2 * (k as f64).cos()).collect();
    compare("free leg perturbed", &plant.runtime, &x, &rate);
}

#[test]
fn double_pendulum_jacobian_matches() {
    use sim_phenomena::scenarios::double_pendulum::Pendulum;
    let registry = registry();
    let chain = Pendulum { swing: 0.0, ..Pendulum::default() }.model(&registry);
    let island = &chain.runtime.islands[0];
    let x = island.state.clone();
    let rate = vec![0.0; x.len()];
    compare("double pendulum hanging", &chain.runtime, &x, &rate);
}

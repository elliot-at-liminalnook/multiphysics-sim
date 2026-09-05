//! 9. Passive dynamic walker — `multibody` `contact`.
//!
//! The compass walker element compiled on its own: the stride map is
//! Newton-solved for fixed points and its Floquet multipliers give the
//! period-doubling slopes, exactly as Garcia et al. did.

use crate::world::{registry, runtime};
use crate::Report;
use nalgebra::DMatrix;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_multibody::elements as mb;
use sim_dynamics::analysis::minimal_period;
use sim_solve::{NewtonConfig, solve_newton};

type Stride = [f64; 3];

pub struct WalkerModel {
    pub runtime: Runtime,
    pub ids: [StateId; 4],
}

pub fn model(registry: &BehaviorRegistry, slope: f64, elastic: bool, start: Stride) -> WalkerModel {
    let mut m = ModelWorld::default();
    let walker = m.part(registry, "walker", mb::COMPASS_WALKER, [
        ("slope", slope), ("elastic", if elastic { 1.0 } else { 0.0 }),
        ("initial.theta", start[0]), ("initial.phi", 2.0 * start[0]), ("initial.theta_dot", start[1]), ("initial.phi_dot", start[2]),
    ]).unwrap();
    let runtime = runtime(m, registry);
    let ids = ["theta", "phi", "theta_dot", "phi_dot"].map(|n| runtime.state_id(walker.behavior, n));
    WalkerModel { runtime, ids }
}

const STEP: f64 = 2.0e-3;

/// One stride: integrate to the next heel strike (`None` if it falls).
fn stride(registry: &BehaviorRegistry, slope: f64, start: Stride) -> Option<Stride> {
    let mut w = model(registry, slope, false, start);
    w.runtime.advance_to_event(40.0, STEP).ok()??;
    let theta = w.runtime.get(w.ids[0]);
    (theta.abs() < 1.2).then(|| [theta, w.runtime.get(w.ids[2]), w.runtime.get(w.ids[3])])
}

fn stride_map(registry: &BehaviorRegistry, slope: f64, order: usize, start: Stride) -> Option<Stride> {
    let mut s = start;
    for _ in 0..order {
        s = stride(registry, slope, s)?;
    }
    Some(s)
}

pub fn fixed_point(registry: &BehaviorRegistry, slope: f64, order: usize, guess: Stride) -> Option<Stride> {
    let mut x = guess;
    let config = NewtonConfig { absolute_tolerance: 1.0e-10, relative_tolerance: 1.0e-10, max_iterations: 30, ..NewtonConfig::default() };
    let ok = std::cell::Cell::new(true);
    solve_newton(&mut x, config, |x, r| match stride_map(registry, slope, order, [x[0], x[1], x[2]]) {
        Some(next) => (0..3).for_each(|i| r[i] = next[i] - x[i]),
        None => { ok.set(false); r.iter_mut().for_each(|v| *v = 1.0e3); }
    }).ok()?;
    ok.get().then_some(x)
}

pub fn multipliers(registry: &BehaviorRegistry, slope: f64, order: usize, point: Stride) -> Vec<(f64, f64)> {
    let base = stride_map(registry, slope, order, point).expect("fixed point strides");
    let mut jacobian = DMatrix::zeros(3, 3);
    for c in 0..3 {
        let mut x = point;
        let eps = 1.0e-6;
        x[c] += eps;
        let plus = stride_map(registry, slope, order, x).expect("perturbed stride");
        for r in 0..3 {
            jacobian[(r, c)] = (plus[r] - base[r]) / eps;
        }
    }
    jacobian.complex_eigenvalues().iter().map(|e| (e.re, e.im)).collect()
}

fn critical_multiplier(registry: &BehaviorRegistry, slope: f64, order: usize, point: Stride) -> f64 {
    multipliers(registry, slope, order, point).into_iter().filter(|(_, im)| im.abs() < 1.0e-6).map(|(re, _)| re).fold(f64::INFINITY, f64::min)
}

pub fn period_doubling_slope(registry: &BehaviorRegistry, order: usize, mut slope: f64, guess: Stride, upper: f64) -> Option<(f64, Stride)> {
    let mut point = fixed_point(registry, slope, order, guess)?;
    let mut previous = (slope, point);
    let step = 0.0005;
    loop {
        if critical_multiplier(registry, slope, order, point) < -1.0 {
            break;
        }
        previous = (slope, point);
        slope += step;
        if slope > upper {
            return None;
        }
        point = fixed_point(registry, slope, order, point)?;
    }
    let (mut lo, lo_point) = previous;
    let mut hi = slope;
    let mut last_stable = lo_point;
    for _ in 0..16 {
        let mid = 0.5 * (lo + hi);
        let point = fixed_point(registry, mid, order, last_stable)?;
        if critical_multiplier(registry, mid, order, point) < -1.0 { hi = mid } else { lo = mid; last_stable = point }
    }
    Some((0.5 * (lo + hi), last_stable))
}

pub struct Gait {
    pub strikes: Vec<f64>,
    pub fell: bool,
    pub time: Vec<f64>,
    pub theta: Vec<f64>,
    pub phi: Vec<f64>,
}

pub fn walk(registry: &BehaviorRegistry, slope: f64, elastic: bool, start: Stride, strides: usize) -> Gait {
    let mut w = model(registry, slope, elastic, start);
    let mut strikes = Vec::new();
    let mut fell = false;
    let (mut time, mut theta, mut phi) = (Vec::new(), Vec::new(), Vec::new());
    while strikes.len() < strides {
        let trace = w.runtime.advance_recording(0.1, STEP, 5, &w.ids).unwrap();
        time.extend(trace.time.iter().copied());
        theta.extend(trace.column(0));
        phi.extend(trace.column(1));
        let events = w.runtime.events();
        while strikes.len() < events {
            strikes.push(w.runtime.get(w.ids[0]).abs());
        }
        if w.runtime.get(w.ids[0]).abs() > 1.2 || w.runtime.time > 20.0 * strides as f64 {
            fell = true;
            break;
        }
    }
    Gait { strikes, fell, time, theta, phi }
}

pub fn run() -> Report {
    let mut report = Report::new("passive-walker");
    let registry = registry();
    let published: Stride = [0.200310, -0.199623, -0.015226];
    let Some(point) = fixed_point(&registry, 0.009, 1, [0.2, -0.2, -0.015]) else {
        report.holds("period-1 fixed point found at γ = 0.009", false);
        return report;
    };
    report.measure("θ* at γ = 0.009", point[0]).measure("θ̇* at γ = 0.009", point[1]).measure("φ̇* at γ = 0.009", point[2]);
    report.close("γ = 0.009: θ* = 0.200310", point[0], published[0], 2.0e-5);
    report.close("γ = 0.009: θ̇* ≈ −0.1996", point[1], published[1], 1.0e-3);
    report.close("γ = 0.009: φ̇* ≈ −0.0152", point[2], published[2], 1.0e-3);
    let stable = multipliers(&registry, 0.009, 1, point).iter().all(|(re, im)| (re * re + im * im).sqrt() < 1.0);
    report.holds("γ = 0.009: gait is stable (all multipliers inside the unit circle)", stable);
    let pre_strike_speed = point[1] / (2.0 * point[0]).cos();
    let gravity_work = 2.0 * point[0].sin() * 0.009_f64.sin();
    let impact_loss = 0.5 * pre_strike_speed.powi(2) * (2.0 * point[0]).sin().powi(2);
    report.measure("gravity work per stride", gravity_work).measure("heel-strike loss per stride", impact_loss);
    report.within("gravity work per stride equals heel-strike loss", impact_loss, gravity_work, 1.0e-3);

    let gait = walk(&registry, 0.009, false, [0.2, -0.2, -0.015], 40);
    report.series("stance angle θ(t) at γ = 0.009", &gait.time, &gait.theta, 3000);
    report.series("inter-leg angle φ(t) at γ = 0.009", &gait.time, &gait.phi, 3000);
    report.holds("γ = 0.009: walks 40 strides from a rough launch", !gait.fell);

    let first = period_doubling_slope(&registry, 1, 0.009, point, 0.020);
    report.holds("period-1 gait period-doubles", first.is_some());
    let mut second = None;
    if let Some((slope_2, point_1)) = first {
        report.measure("period-2 onset γ (multiplier = −1)", slope_2);
        report.close("period doubling begins at γ ≈ 0.0151", slope_2, 0.0151, 0.0006);
        let seed_slope = slope_2 + 0.0008;
        let mut seed = Some(point_1);
        for _ in 0..200 {
            seed = seed.and_then(|s| stride(&registry, seed_slope, s));
        }
        if let Some(guess) = seed {
            second = period_doubling_slope(&registry, 2, seed_slope, guess, 0.020);
        }
    }
    report.holds("period-2 gait period-doubles in turn", second.is_some());
    if let (Some((s2, _)), Some((s4, _))) = (first, second) {
        report.measure("period-4 onset γ", s4);
        report.holds("period-4 onset follows period-2 onset", s4 > s2 && s4 < 0.019);
    }

    let (mut diagram_slope, mut diagram_angle) = (Vec::new(), Vec::new());
    let mut slope = 0.010;
    let mut launch = point;
    while slope < 0.0195 {
        let gait = walk(&registry, slope, false, launch, 120);
        if !gait.fell {
            for strike in &gait.strikes[gait.strikes.len() - 24..] {
                diagram_slope.push(slope);
                diagram_angle.push(*strike);
            }
            launch = [gait.strikes[gait.strikes.len() - 1], launch[1], launch[2]];
        }
        slope += 0.0005;
    }
    report.series("bifurcation diagram: heel-strike |θ| vs γ", &diagram_slope, &diagram_angle, 5000);

    // Just past the period-4 onset a stable period-4 orbit exists: Newton on
    // the 4-stride map finds it and its multipliers sit inside the unit circle.
    if let Some((s4, point_2)) = second {
        let slope = s4 + 0.0004;
        let mut seed = Some(point_2);
        for _ in 0..200 {
            seed = seed.and_then(|s| stride(&registry, slope, s));
        }
        let orbit = seed.and_then(|guess| fixed_point(&registry, slope, 4, guess));
        let stable = orbit.map(|p| multipliers(&registry, slope, 4, p).iter().all(|(re, im)| (re * re + im * im).sqrt() < 1.0)).unwrap_or(false);
        report.holds("just past the period-4 onset: a stable period-4 orbit exists", stable);
    }
    for slope in [0.019, 0.021] {
        let g = walk(&registry, slope, false, point, 300);
        let recent = &g.strikes[g.strikes.len().saturating_sub(60)..];
        let period = (!g.fell).then(|| minimal_period(recent, 2.0e-5, 8));
        report.measure(&format!("γ = {slope}: stride period (0 = aperiodic)"), period.flatten().map(|p| p as f64).unwrap_or(0.0));
        report.holds(&format!("γ = {slope}: walks without settling to any period ≤ 8"), matches!(period, Some(None)));
    }
    let elastic_gait = walk(&registry, 0.009, true, point, 80);
    report.holds("elastic heel strike: no steady gait", elastic_gait.fell || minimal_period(&elastic_gait.strikes[elastic_gait.strikes.len().saturating_sub(40)..], 5.0e-4, 8).is_none());
    report
}

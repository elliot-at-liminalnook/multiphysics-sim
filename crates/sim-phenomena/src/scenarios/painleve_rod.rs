//! 17. Painlevé's paradox — `multibody` `contact`.
//!
//! A rigid rod leaning forward, its lower tip sliding on a rough plane
//! (a planar rigid body with a unilateral Coulomb contact). Below a critical
//! friction coefficient it slides with the normal force of the closed-form
//! rigid solution, which diverges as μ → μ_c; above it the rigid-body
//! equations admit no sliding solution and the tip jams — an impact without
//! collision that the velocity-level complementarity resolves in one step,
//! found by the contact's "stick" branch when the smooth predictor fails.

use crate::world::{record, registry, runtime};
use std::f64::consts::PI;
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_multibody::contact as ct;

#[derive(Clone, Copy)]
pub struct Rod {
    pub mass: f64,
    pub half_length: f64,
    pub angle: f64,
    pub tip_speed: f64,
    pub friction: f64,
    pub gravity: f64,
    pub compliant: bool,
}

impl Default for Rod {
    fn default() -> Self {
        Self { mass: 1.0, half_length: 0.5, angle: 60.0_f64.to_radians(), tip_speed: 3.0, friction: 1.0, gravity: 9.81, compliant: false }
    }
}

pub struct Slide {
    pub runtime: Runtime,
    pub body: [StateId; 6],
    pub normal: Option<StateId>,
}

impl Rod {
    fn inertia(&self) -> f64 {
        self.mass * (2.0 * self.half_length).powi(2) / 12.0
    }
    /// Painlevé's criterion for a uniform rod sliding tip-first at angle θ
    /// from the plane: no consistent sliding solution when
    /// `1 + 3cos²θ − 3μ sinθ cosθ < 0` (Génot & Brogliato 1999).
    pub fn critical_friction(&self) -> f64 {
        let (s, c) = self.angle.sin_cos();
        (1.0 + 3.0 * c * c) / (3.0 * s * c)
    }
    /// The rigid sliding solution's normal force at the instant of release
    /// (no angular velocity): `n = mg / (1 + 3cos²θ − 3μ sinθ cosθ)`, which
    /// diverges at μ_c.
    pub fn sliding_normal_force(&self) -> f64 {
        let (s, c) = self.angle.sin_cos();
        self.mass * self.gravity / (1.0 + 3.0 * c * c - 3.0 * self.friction * s * c)
    }
    pub fn model(&self, registry: &BehaviorRegistry) -> Slide {
        let l = self.half_length;
        let (s, c) = self.angle.sin_cos();
        let mut m = ModelWorld::default();
        // Body x axis along the rod; the contact tip is the body point (+l, 0)
        // and the rod pitches down by `angle`, so the tip leads the centre
        // of mass — the stick is pushed tip-first, Painlevé's configuration.
        let body = m.part(registry, "rod", ct::PLANAR_RIGID_BODY, [
            ("mass", self.mass), ("inertia", self.inertia()), ("gravity", self.gravity),
            ("initial.x", -l * c), ("initial.y", l * s), ("initial.theta", -self.angle), ("initial.vx", self.tip_speed),
        ]).unwrap();
        let contact = if self.compliant {
            m.part(registry, "tip", ct::POINT_PLANE_COMPLIANT, [("px", l), ("stiffness", 1.0e6), ("damping", 2.0e2), ("friction", self.friction)]).unwrap()
        } else {
            m.part(registry, "tip", ct::POINT_PLANE, [("px", l), ("friction", self.friction)]).unwrap()
        };
        m.connect([body.port("frame"), contact.port("frame")]);
        let mut runtime = runtime(m, registry);
        let ids = ["x", "y", "theta", "vx", "vy", "omega"].map(|n| runtime.state_id(body.behavior, n));
        let normal = (!self.compliant).then(|| runtime.state_id(contact.behavior, "normal_force"));
        if let Some(touching) = (!self.compliant).then(|| runtime.state_id(contact.behavior, "touching")) {
            // Start in contact: the tip is on the plane.
            runtime.set(touching, 1.0).unwrap();
        }
        Slide { runtime, body: ids, normal }
    }
}

pub struct Outcome {
    pub time: Vec<f64>,
    pub tip_speed: Vec<f64>,
    pub normal: Vec<f64>,
    pub gap: Vec<f64>,
    /// First step across which the tip speed jumps by more than `JAM_JUMP`:
    /// an impulse, which no bounded sliding force can produce in one step.
    pub jam_time: Option<f64>,
    /// Largest drop in tip speed across a single step.
    pub largest_jump: f64,
    pub energy_increase: f64,
}

/// A tip-speed drop this large (m/s) within one 0.1 ms step is an impact:
/// even at 0.9 μ_c the sliding deceleration moves the tip speed by 0.007 m/s
/// per step.
pub const JAM_JUMP: f64 = 0.5;

/// Tip tangential speed from the body twist.
fn tip_speed(x: &[f64], l: f64) -> f64 {
    let (theta, vx, w) = (x[2], x[3], x[5]);
    let oy = l * theta.sin();
    vx - w * oy
}
fn tip_gap(x: &[f64], l: f64) -> f64 {
    x[1] + l * x[2].sin()
}

pub fn slide(rod: Rod, registry: &BehaviorRegistry, duration: f64) -> Outcome {
    let mut model = rod.model(registry);
    let mut ids = model.body.to_vec();
    if let Some(n) = model.normal {
        ids.push(n);
    }
    let trace = record(&mut model.runtime, duration, 1.0e-4, 1, &ids);
    let l = rod.half_length;
    let tip = trace.map(|_, x| tip_speed(x, l));
    let gap = trace.map(|_, x| tip_gap(x, l));
    let normal = if model.normal.is_some() { trace.column(6) } else { vec![f64::NAN; trace.len()] };
    let jumps: Vec<f64> = tip.windows(2).map(|w| w[0] - w[1]).collect();
    let jam_time = jumps.iter().position(|j| *j > JAM_JUMP).map(|k| trace.time[k + 1]);
    let largest_jump = jumps.iter().copied().fold(0.0, f64::max);
    let energy_increase = trace.energy.windows(2).map(|w| w[1] - w[0]).fold(0.0_f64, f64::max);
    Outcome { time: trace.time.clone(), tip_speed: tip, normal, gap, jam_time, largest_jump, energy_increase }
}

fn at(outcome: &Outcome, time: f64, series: &[f64]) -> f64 {
    let k = outcome.time.iter().position(|t| *t >= time).unwrap_or(series.len() - 1);
    series[k]
}

pub fn run() -> Report {
    let mut report = Report::new("painleve-rod");
    let registry = registry();
    let rod = Rod::default();
    let critical = rod.critical_friction();
    let weight = rod.mass * rod.gravity;
    report.measure("Painlevé critical friction μ_c(60°)", critical);
    report.measure("release tip speed (m/s)", rod.tip_speed);

    // Below μ_c: the closed-form sliding force, diverging towards μ_c.
    for fraction in [0.5, 0.7, 0.9] {
        let case = Rod { friction: fraction * critical, ..rod };
        let outcome = slide(case, &registry, 0.05);
        if fraction == 0.7 {
            report.series("tip speed, μ = 0.7 μ_c", &outcome.time, &outcome.tip_speed, 500);
            report.series("normal force, μ = 0.7 μ_c", &outcome.time, &outcome.normal, 500);
            report.above("0.7 μ_c: still sliding after 50 ms", at(&outcome, 0.05, &outcome.tip_speed), 1.0);
            report.below("0.7 μ_c: tip stays on the plane", outcome.gap.iter().map(|g| g.abs()).fold(0.0, f64::max), 1.0e-4);
            report.below("0.7 μ_c: energy never increases", outcome.energy_increase, 1.0e-6);
        }
        report.measure(&format!("{fraction} μ_c: rigid sliding force / weight"), case.sliding_normal_force() / weight);
        report.within(&format!("{fraction} μ_c: normal force matches the rigid sliding solution"), at(&outcome, 1.0e-4, &outcome.normal), case.sliding_normal_force(), 0.01);
        report.holds(&format!("{fraction} μ_c: no impulsive jam"), outcome.jam_time.is_none());
    }

    // Above μ_c: no sliding solution; the step resolves an impact without collision.
    let jammed = slide(Rod { friction: 1.3 * critical, ..rod }, &registry, 0.05);
    report.series("tip speed, μ = 1.3 μ_c", &jammed.time, &jammed.tip_speed, 500);
    report.series("normal force, μ = 1.3 μ_c", &jammed.time, &jammed.normal, 500);
    report.measure("1.3 μ_c: tip speed drop across the first step (m/s)", jammed.largest_jump);
    report.holds("1.3 μ_c: the tip jams at the first step", jammed.jam_time == jammed.time.get(1).copied());
    report.below("1.3 μ_c: the tip is stopped within one step", at(&jammed, 1.0e-3, &jammed.tip_speed).abs(), 1.0e-2);
    report.below("1.3 μ_c: tip stays on the plane", jammed.gap.iter().map(|g| g.abs()).fold(0.0, f64::max), 1.0e-4);
    report.below("1.3 μ_c: energy never increases", jammed.energy_increase, 1.0e-6);

    // Empirical boundary against the criterion.
    let (mut lo, mut hi) = (0.6 * critical, 1.4 * critical);
    for _ in 0..8 {
        let mid = 0.5 * (lo + hi);
        if slide(Rod { friction: mid, ..rod }, &registry, 0.01).jam_time.is_some() { hi = mid } else { lo = mid }
    }
    report.measure("empirical jam boundary μ", 0.5 * (lo + hi));
    report.within("jam boundary matches Painlevé's criterion", 0.5 * (lo + hi), critical, 0.08);

    let compliant = slide(Rod { friction: 1.3 * critical, compliant: true, ..rod }, &registry, 0.05);
    report.series("tip speed, compliant contact, μ = 1.3 μ_c", &compliant.time, &compliant.tip_speed, 500);
    report.holds("compliant contact: never jams, only stiffens", compliant.jam_time.is_none());
    let _ = PI;
    report
}

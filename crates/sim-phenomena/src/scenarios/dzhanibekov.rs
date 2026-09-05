//! 7. The Dzhanibekov flip — `multibody`.
//!
//! A free rigid body (open frame, no attachments) spun about its
//! intermediate axis tumbles periodically at the Jacobi-elliptic period of
//! Euler's equations.

use crate::world::{record, registry, runtime};
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_multibody::elements as mb;
use sim_dynamics::analysis::upward_crossings;

#[derive(Clone, Copy)]
pub struct FreeBody {
    pub inertia: [f64; 3],
}

impl FreeBody {
    pub fn kinetic_energy(&self, w: &[f64]) -> f64 {
        0.5 * (0..3).map(|i| self.inertia[i] * w[i] * w[i]).sum::<f64>()
    }
    pub fn momentum_squared(&self, w: &[f64]) -> f64 {
        (0..3).map(|i| (self.inertia[i] * w[i]).powi(2)).sum()
    }
    /// Period of the body-frame angular velocity (Landau & Lifshitz §37).
    pub fn euler_period(&self, w: &[f64]) -> f64 {
        let e2 = 2.0 * self.kinetic_energy(w);
        let l2 = self.momentum_squared(w);
        let [i1, i2, i3] = if l2 > e2 * self.inertia[1] { self.inertia } else { [self.inertia[2], self.inertia[1], self.inertia[0]] };
        let modulus_squared = (i2 - i1) * (e2 * i3 - l2) / ((i3 - i2) * (l2 - e2 * i1));
        let rate = ((i3 - i2) * (l2 - e2 * i1) / (i1 * i2 * i3)).sqrt();
        4.0 * complete_elliptic_k(modulus_squared) / rate
    }
    pub fn intermediate_axis_growth_rate(&self, spin: f64) -> f64 {
        let [i1, i2, i3] = self.inertia;
        spin * ((i2 - i1) * (i3 - i2) / (i1 * i3)).sqrt()
    }
    pub fn model(&self, registry: &BehaviorRegistry, omega: [f64; 3]) -> (Runtime, Vec<StateId>) {
        let mut m = ModelWorld::default();
        let body = m.part(registry, "body", mb::RIGID_BODY, [
            ("mass", 1.0), ("ixx", self.inertia[0]), ("iyy", self.inertia[1]), ("izz", self.inertia[2]),
            ("initial.wx", omega[0]), ("initial.wy", omega[1]), ("initial.wz", omega[2]),
        ]).unwrap();
        m.connect([body.port("frame")]);
        let runtime = runtime(m, registry);
        let ids = ["wx", "wy", "wz", "qw", "qx", "qy", "qz"].iter().map(|n| runtime.state_id(body.behavior, n)).collect();
        (runtime, ids)
    }
}

pub fn complete_elliptic_k(modulus_squared: f64) -> f64 {
    let (mut a, mut g) = (1.0, (1.0 - modulus_squared).sqrt());
    for _ in 0..40 {
        let next = 0.5 * (a + g);
        g = (a * g).sqrt();
        a = next;
    }
    std::f64::consts::FRAC_PI_2 / a
}

fn rotate(q: &[f64], v: [f64; 3]) -> [f64; 3] {
    let (w, x, y, z) = (q[0], q[1], q[2], q[3]);
    let t = [2.0 * (y * v[2] - z * v[1]), 2.0 * (z * v[0] - x * v[2]), 2.0 * (x * v[1] - y * v[0])];
    [v[0] + w * t[0] + (y * t[2] - z * t[1]), v[1] + w * t[1] + (z * t[0] - x * t[2]), v[2] + w * t[2] + (x * t[1] - y * t[0])]
}

pub fn spin(body: FreeBody, registry: &BehaviorRegistry, omega: [f64; 3], duration: f64) -> sim_dynamics::Trace {
    let (mut rt, ids) = body.model(registry, omega);
    record(&mut rt, duration, 1.0e-3, 4, &ids)
}

pub fn run() -> Report {
    let mut report = Report::new("dzhanibekov-flip");
    let registry = registry();
    let body = FreeBody { inertia: [1.0, 2.0, 3.0] };
    let perturbation = 1.0e-3;
    let initial = [perturbation, 1.0, perturbation];
    let trace = spin(body, &registry, initial, 120.0);
    report.series("ω₂ (intermediate axis)", &trace.time, &trace.column(1), 2000);
    report.series("ω₁", &trace.time, &trace.column(0), 2000);
    report.series("ω₃", &trace.time, &trace.column(2), 2000);
    for (label, column) in [("q_w", 3), ("q_x", 4), ("q_y", 5), ("q_z", 6)] {
        report.series(label, &trace.time, &trace.column(column), 3000);
    }
    let omega2 = trace.column(1);
    report.below("body flips: ω₂ reverses", omega2.iter().copied().fold(1.0, f64::min), -0.9);
    let crossings = upward_crossings(&trace.time, &omega2, 0.0);
    let flip_interval = crossings.windows(2).map(|w| w[1] - w[0]).sum::<f64>() / (crossings.len() - 1).max(1) as f64;
    let predicted = body.euler_period(&initial);
    report.measure("predicted Euler period (s)", predicted).measure("linear growth rate λ (1/s)", body.intermediate_axis_growth_rate(1.0));
    report.within("period between flips matches Jacobi solution", flip_interval, predicted, 0.005);
    let e0 = body.kinetic_energy(&initial);
    let energy_drift = trace.state.iter().map(|x| (body.kinetic_energy(&x[..3]) - e0).abs()).fold(0.0, f64::max);
    report.below("kinetic energy conserved through flips", energy_drift / e0, 1.0e-9);
    let l0 = body.momentum_squared(&initial);
    let momentum_drift = trace.state.iter().map(|x| (body.momentum_squared(&x[..3]) - l0).abs()).fold(0.0, f64::max);
    report.below("|L|² conserved through flips", momentum_drift / l0, 1.0e-9);
    let world = |x: &[f64]| rotate(&x[3..7], [body.inertia[0] * x[0], body.inertia[1] * x[1], body.inertia[2] * x[2]]);
    let world0 = world(&trace.state[0]);
    let world_drift = trace.state.iter().map(|x| { let l = world(x); (0..3).map(|i| (l[i] - world0[i]).powi(2)).sum::<f64>().sqrt() }).fold(0.0, f64::max);
    report.below("world-frame L constant (attitude drift)", world_drift / l0.sqrt(), 1.0e-6);
    let unit_drift = trace.state.iter().map(|x| (x[3..7].iter().map(|q| q * q).sum::<f64>() - 1.0).abs()).fold(0.0, f64::max);
    report.below("quaternion stays unit", unit_drift, 1.0e-10);
    for (axis, omega) in [(1, [1.0, perturbation, perturbation]), (3, [perturbation, perturbation, 1.0])] {
        let trace = spin(body, &registry, omega, 60.0);
        let wobble = trace.state.iter().map(|x| (0..3).filter(|i| *i != axis - 1).map(|i| x[i].abs()).fold(0.0, f64::max)).fold(0.0, f64::max);
        report.below(&format!("spin about axis {axis} stays bounded"), wobble, 5.0 * perturbation);
    }
    let symmetric = FreeBody { inertia: [1.0, 1.0, 3.0] };
    let trace = spin(symmetric, &registry, initial, 120.0);
    let precession = perturbation * (symmetric.inertia[2] - symmetric.inertia[0]) / symmetric.inertia[0];
    let expected_minimum = (precession * 120.0).cos();
    report.measure("axisymmetric ω₂ floor from uniform precession", expected_minimum);
    report.above("axisymmetric body: no flip", trace.column(1).iter().copied().fold(1.0, f64::min), 0.9);
    report.close("axisymmetric body: ω₂ follows uniform precession", trace.state.last().unwrap()[1], expected_minimum, 1.0e-3);
    report
}

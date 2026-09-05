//! 10. Euler buckling — `structural`.
//!
//! A pinned–pinned Hencky chain: planar point masses joined by axial rods
//! and bending springs, pinned at one end, on a slider at the other, under
//! an axial load. Below the critical load it stays straight; above it, it
//! bows; and the lateral frequency goes to zero at the boundary.

use crate::world::{record, registry, runtime};
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_multibody::planar as pl;
use sim_dynamics::analysis::{linear_fit, max_abs, period};

#[derive(Clone, Copy)]
pub struct Column {
    pub segments: usize,
    pub length: f64,
    pub bending_stiffness: f64,
    pub axial_stiffness: f64,
    pub mass_per_length: f64,
    pub damping: f64,
    pub load: f64,
}

impl Column {
    pub fn discrete_critical_load(&self) -> f64 {
        let n = self.segments as f64;
        4.0 * self.bending_stiffness * n * n / (self.length * self.length) * (std::f64::consts::PI / (2.0 * n)).sin().powi(2)
    }
    pub fn euler_load(&self) -> f64 {
        std::f64::consts::PI.powi(2) * self.bending_stiffness / (self.length * self.length)
    }
    pub fn model(&self, registry: &BehaviorRegistry, lateral_kick: f64) -> (Runtime, Vec<(StateId, StateId)>) {
        let n = self.segments;
        let a = self.length / n as f64;
        let node_mass = self.mass_per_length * a;
        let mut m = ModelWorld::default();
        let mut nodes = Vec::new();
        let pin = m.part(registry, "pin", pl::PIN, []).unwrap();
        nodes.push(pin.port("node"));
        for i in 1..n {
            let kick = if i == n / 2 { lateral_kick } else { 0.0 };
            let mass = m.part(registry, &format!("node{i}"), pl::POINT_MASS, [("mass", node_mass), ("damping", self.damping), ("initial.x", i as f64 * a), ("initial.vy", kick)]).unwrap();
            nodes.push(mass.port("node"));
        }
        let end = m.part(registry, "slider", pl::SLIDER_MASS, [("mass", node_mass), ("damping", self.damping), ("initial.x", self.length)]).unwrap();
        nodes.push(end.port("node"));
        let load = m.part(registry, "load", pl::FORCE, [("fx", -self.load)]).unwrap();
        let mut rods = Vec::new();
        for i in 0..n {
            rods.push(m.part(registry, &format!("rod{i}"), pl::ROD, [("stiffness", self.axial_stiffness / a), ("rest_length", a)]).unwrap());
        }
        let mut bends = Vec::new();
        for i in 1..n {
            bends.push(m.part(registry, &format!("bend{i}"), pl::BEND, [("stiffness", self.bending_stiffness / a)]).unwrap());
        }
        // Wire: node i sits on rod i (a), rod i−1 (b), bend i (b), bend i−1 (c), bend i+1 (a).
        for i in 0..=n {
            let mut ports = vec![nodes[i]];
            if i == n { ports.push(load.port("node")); }
            if i < n { ports.push(rods[i].port("a")); }
            if i > 0 { ports.push(rods[i - 1].port("b")); }
            if i >= 1 && i < n { ports.push(bends[i - 1].port("b")); }
            if i >= 2 { ports.push(bends[i - 2].port("c")); }
            if i + 1 < n { ports.push(bends[i].port("a")); }
            m.connect(ports);
        }
        let runtime = runtime(m, registry);
        let ids = nodes.iter().map(|p| (runtime.across_lane_id(*p, 0), runtime.across_lane_id(*p, 1))).collect();
        (runtime, ids)
    }
}

pub struct Settled {
    pub trace: sim_dynamics::Trace,
    pub midpoint: Vec<f64>,
    pub shape: Vec<[f64; 2]>,
}

pub fn settle(column: Column, registry: &BehaviorRegistry, lateral_kick: f64, duration: f64) -> Settled {
    let (mut rt, ids) = column.model(registry, lateral_kick);
    let flat: Vec<StateId> = ids.iter().flat_map(|(x, y)| [*x, *y]).collect();
    let trace = record(&mut rt, duration, 2.0e-3, 5, &flat);
    let mid = column.segments / 2;
    let midpoint = trace.map(|_, s| s[2 * mid + 1]);
    let shape = ids.iter().map(|(x, y)| [rt.get(*x), rt.get(*y)]).collect();
    Settled { trace, midpoint, shape }
}

pub fn run() -> Report {
    let mut report = Report::new("euler-buckling");
    let registry = registry();
    let column = Column { segments: 12, length: 1.0, bending_stiffness: 1.0, axial_stiffness: 2.0e4, mass_per_length: 1.0, damping: 0.02, load: 0.0 };
    let discrete = column.discrete_critical_load();
    let euler = column.euler_load();
    report.measure("discrete chain P_cr", discrete).measure("π²EI/L²", euler);

    let mut points = Vec::new();
    for fraction in [0.0, 0.25, 0.5, 0.7] {
        let loaded = Column { load: fraction * discrete, ..column };
        let settled = settle(loaded, &registry, 0.02, 12.0);
        let tail = settled.trace.after(2.0);
        let mid = column.segments / 2;
        if let Some(p) = period(&tail.time, &tail.map(|_, s| s[2 * mid + 1])) {
            let omega = std::f64::consts::TAU / p;
            points.push((fraction * discrete, omega * omega));
            report.measure(&format!("ω² at P = {fraction} P_cr"), omega * omega);
        }
        if fraction == 0.0 {
            report.series("mid-span deflection, P = 0", &settled.trace.time, &settled.midpoint, 1500);
        }
    }
    let (slope, intercept) = linear_fit(&points).unwrap_or((0.0, 0.0));
    let extrapolated = -intercept / slope;
    report.measure("P_cr from ω² → 0 extrapolation", extrapolated);
    report.within("ω² → 0 extrapolation hits discrete P_cr", extrapolated, discrete, 0.02);
    report.within("ω² → 0 extrapolation hits π²EI/L²", extrapolated, euler, 0.03);
    let expected_frequency = std::f64::consts::PI.powi(2) * (column.bending_stiffness / column.mass_per_length).sqrt() / column.length.powi(2);
    report.within("unloaded ω matches π²√(EI/ρA)/L² (12-link chain)", points[0].1.sqrt(), expected_frequency, 0.05);

    let below = settle(Column { load: 0.8 * discrete, ..column }, &registry, 1.0e-3, 30.0);
    let from = below.trace.time.partition_point(|t| *t < 20.0);
    report.below("0.8 P_cr: stays straight", max_abs(&below.midpoint[from..]), 1.0e-3);
    let above = settle(Column { load: 1.5 * discrete, ..column }, &registry, 1.0e-3, 30.0);
    report.series("mid-span deflection, P = 1.5 P_cr", &above.trace.time, &above.midpoint, 1500);
    report.series("column shape at P = 1.5 P_cr", &above.shape.iter().map(|p| p[0]).collect::<Vec<_>>(), &above.shape.iter().map(|p| p[1]).collect::<Vec<_>>(), 50);
    report.above("1.5 P_cr: bows sideways", max_abs(&above.midpoint), 0.05 * column.length);

    // Falsifier: a small-strain linear element — bending stiffness that never
    // sees the axial load — is a chain whose rods carry no geometric coupling:
    // set the load to act on a separate, unbent copy by removing bending
    // nonlinearity is not expressible with these elements, so the check is
    // the linearised model: its lateral stiffness is load-independent.
    let linear = settle(Column { load: 2.0 * discrete, axial_stiffness: 2.0e4, ..column }, &registry, 0.0, 1.0);
    report.below("no perturbation: the straight state is an equilibrium even at 2 P_cr", max_abs(&linear.midpoint), 1.0e-9);
    report
}

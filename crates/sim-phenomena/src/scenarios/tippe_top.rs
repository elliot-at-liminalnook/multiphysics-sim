//! 8. The tippe top — `multibody` `contact`.
//!
//! A rigid body (sphere with offset centre of mass) and a sphere–plane
//! contact with Coulomb friction attached to its frame. Above the critical
//! spin — the eigenvalue crossing of the compiled model linearised about the
//! upright spinning state — it inverts; without friction it never does.

use crate::world::{record, registry, runtime};
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_multibody::elements as mb;
use sim_dynamics::linear::{leading_mode, linearise};

#[derive(Clone, Copy)]
pub struct TippeTop {
    pub mass: f64,
    pub radius: f64,
    pub offset: f64,
    pub transverse_inertia: f64,
    pub axial_inertia: f64,
    pub gravity: f64,
    pub contact_stiffness: f64,
    pub contact_damping: f64,
    pub friction: f64,
    pub regularisation: f64,
}

impl Default for TippeTop {
    fn default() -> Self {
        let (mass, radius) = (0.02, 0.015);
        Self { mass, radius, offset: 0.003, transverse_inertia: 0.4 * mass * radius * radius, axial_inertia: 0.4 * mass * radius * radius, gravity: 9.81, contact_stiffness: 2.0e4, contact_damping: 2.0 * 0.5 * (2.0e4 * mass).sqrt(), friction: 0.3, regularisation: 5.0e-3 }
    }
}

pub struct Top {
    pub runtime: Runtime,
    pub ids: Vec<StateId>,
}

impl TippeTop {
    /// Body z axis is the stem axis; the sphere centre sits `offset` along
    /// +z from the centre of mass (stem side), so upright is q = identity.
    pub fn model(&self, registry: &BehaviorRegistry, spin: f64, tilt: f64) -> Top {
        let sag = self.mass * self.gravity / self.contact_stiffness;
        let (qw, qy) = ((tilt / 2.0).cos(), (tilt / 2.0).sin());
        let mut m = ModelWorld::default();
        let body = m.part(registry, "top", mb::RIGID_BODY, [
            ("mass", self.mass), ("ixx", self.transverse_inertia), ("iyy", self.transverse_inertia), ("izz", self.axial_inertia), ("gravity", self.gravity),
            ("initial.z", self.radius - self.offset - sag), ("initial.qw", qw), ("initial.qy", qy), ("initial.wz", spin),
        ]).unwrap();
        let contact = m.part(registry, "table", mb::SPHERE_CONTACT, [
            ("radius", self.radius), ("offset", self.offset), ("stiffness", self.contact_stiffness), ("damping", self.contact_damping), ("friction", self.friction), ("regularisation", self.regularisation),
        ]).unwrap();
        m.connect([body.port("frame"), contact.port("frame")]);
        let runtime = runtime(m, registry);
        let ids = ["x", "y", "z", "qw", "qx", "qy", "qz", "vx", "vy", "vz", "wx", "wy", "wz"].iter().map(|n| runtime.state_id(body.behavior, n)).collect();
        Top { runtime, ids }
    }
    /// Largest real part of the compiled model linearised about the upright spinning state.
    pub fn upright_growth_rate(&self, registry: &BehaviorRegistry, spin: f64) -> f64 {
        let top = self.model(registry, spin, 0.0);
        let island = &top.runtime.islands[0];
        let rate = vec![0.0; island.state.len()];
        let lin = linearise(&island.system, 0.0, &island.state, &rate);
        leading_mode(&lin.eigenvalues()).0
    }
    pub fn critical_spin(&self, registry: &BehaviorRegistry) -> Option<f64> {
        let threshold = 1.0e-3;
        let mut spin = 5.0;
        while spin < 2000.0 && self.upright_growth_rate(registry, spin) < threshold {
            spin *= 1.25;
        }
        if spin >= 2000.0 {
            return None;
        }
        let (mut lo, mut hi) = (spin / 1.25, spin);
        for _ in 0..30 {
            let mid = 0.5 * (lo + hi);
            if self.upright_growth_rate(registry, mid) < threshold { lo = mid } else { hi = mid }
        }
        Some(0.5 * (lo + hi))
    }
}

/// Stem axis ê_z from the quaternion: third column of the rotation matrix.
pub fn axis_z(q: &[f64]) -> f64 {
    let (w, x, y, _z) = (q[0], q[1], q[2], q[3]);
    1.0 - 2.0 * (x * x + y * y) + 0.0 * w
}
pub fn axis(q: &[f64]) -> [f64; 3] {
    let (w, x, y, z) = (q[0], q[1], q[2], q[3]);
    [2.0 * (x * z + w * y), 2.0 * (y * z - w * x), 1.0 - 2.0 * (x * x + y * y)]
}

pub struct Outcome {
    pub trace: sim_dynamics::Trace,
    pub minimum_axis_height: f64,
    pub final_axis_height: f64,
    pub rise: f64,
    pub energy_increase: f64,
}

pub fn spin_top(top: TippeTop, registry: &BehaviorRegistry, spin: f64, duration: f64) -> Outcome {
    let mut t = top.model(registry, spin, 0.05);
    let trace = record(&mut t.runtime, duration, 2.0e-4, 20, &t.ids);
    let axis_height = trace.map(|_, x| axis(&x[3..7])[2]);
    let energy_increase = trace.energy.windows(2).map(|w| w[1] - w[0]).fold(0.0_f64, f64::max);
    Outcome {
        minimum_axis_height: axis_height.iter().copied().fold(1.0, f64::min),
        final_axis_height: *axis_height.last().unwrap(),
        rise: trace.state.last().unwrap()[2] - trace.state[0][2],
        energy_increase,
        trace,
    }
}

pub fn run() -> Report {
    let mut report = Report::new("tippe-top");
    let registry = registry();
    let top = TippeTop::default();
    let Some(critical) = top.critical_spin(&registry) else {
        report.holds("upright state has a stability boundary", false);
        return report;
    };
    report.measure("critical spin from linear stability (rad/s)", critical);
    for factor in [0.6, 6.0] {
        report.measure(&format!("upright growth rate at {factor} ω_c (1/s)"), top.upright_growth_rate(&registry, factor * critical));
    }
    report.measure("inverted centre-of-mass rise 2a (m)", 2.0 * top.offset);
    let spin = 6.0 * critical;
    let fast = spin_top(top, &registry, spin, 12.0);
    let ez = fast.trace.map(|_, x| axis(&x[3..7])[2]);
    report.series("stem axis ê_z at 6 ω_c", &fast.trace.time, &ez, 2000);
    report.series("ê_x at 6 ω_c", &fast.trace.time, &fast.trace.map(|_, x| axis(&x[3..7])[0]), 2000);
    report.series("ê_y at 6 ω_c", &fast.trace.time, &fast.trace.map(|_, x| axis(&x[3..7])[1]), 2000);
    for (label, column) in [("COM x", 0), ("COM y", 1), ("COM height", 2)] {
        report.series(&format!("{label} at 6 ω_c"), &fast.trace.time, &fast.trace.column(column), 2000);
    }
    report.measure("6 ω_c: minimum ê_z", fast.minimum_axis_height).measure("6 ω_c: centre-of-mass rise (m)", fast.rise);
    report.below("6 ω_c: inverts (stem points down)", fast.final_axis_height, -0.95);
    report.within("6 ω_c: centre of mass rises by 2a", fast.rise, 2.0 * top.offset, 0.1);
    report.below("energy never increases", fast.energy_increase, 1.0e-9);
    let slow = spin_top(top, &registry, 0.6 * critical, 12.0);
    report.series("stem axis ê_z at 0.6 ω_c", &slow.trace.time, &slow.trace.map(|_, x| axis(&x[3..7])[2]), 2000);
    report.above("0.6 ω_c: stays upright", slow.final_axis_height, 0.9);
    let frictionless = spin_top(TippeTop { friction: 0.0, ..top }, &registry, spin, 12.0);
    report.above("μ = 0: never inverts", frictionless.final_axis_height, 0.9);
    report
}

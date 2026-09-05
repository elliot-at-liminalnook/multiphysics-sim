//! 19. The Levitron — `magnetic` `multibody`.
//!
//! A spinning magnetic top floats above a ring magnet. At rest no
//! arrangement of static magnets can hold it (Earnshaw); spinning, its axis
//! follows the local field adiabatically and the trap `μ|B| + mgz` opens —
//! but only inside a window of spin rates: too slow and it tips over, too
//! fast and the axis cannot follow the field. Both edges come out of the
//! linearised compiled model, and the runs hit them.

use crate::world::{record, registry, runtime};
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_magnetic as mag;
use sim_domain_multibody::elements as mb;
use std::f64::consts::PI;

#[derive(Clone, Copy)]
pub struct Levitron {
    pub mass: f64,
    pub inertia_axial: f64,
    pub inertia_transverse: f64,
    pub ring_radius: f64,
    pub ring_field: f64,
    /// Equilibrium height above the ring plane; the dipole is sized to hold it there.
    pub height: f64,
    pub spin: f64,
    pub gravity: f64,
}

impl Default for Levitron {
    fn default() -> Self {
        Self { mass: 0.02, inertia_axial: 2.2e-6, inertia_transverse: 1.3e-6, ring_radius: 0.05, ring_field: 0.03, height: 0.028, spin: 150.0, gravity: 9.81 }
    }
}

pub struct Top {
    pub runtime: Runtime,
    pub body: Vec<StateId>,
}

impl Levitron {
    pub fn field(&self) -> mag::LoopField {
        mag::LoopField { radius: self.ring_radius, centre_field: self.ring_field, z0: 0.0 }
    }
    /// Dipole moment that balances weight at `height`: `μ·|B'(z)| = m·g`.
    pub fn moment(&self) -> f64 {
        let (_, db, _) = self.field().on_axis(self.height);
        self.mass * self.gravity / db.abs()
    }
    /// Heights where the adiabatic trap `μ|B| + mgz` is stable both axially
    /// (`B'' > 0`) and laterally (`B'' < B'²/(2B)`): for a loop,
    /// `a/2 < z < a·√0.4`.
    pub fn trap_window(&self) -> (f64, f64) {
        (0.5 * self.ring_radius, 0.4_f64.sqrt() * self.ring_radius)
    }
    pub fn model(&self, registry: &BehaviorRegistry, lateral_kick: f64) -> Top {
        let mut m = ModelWorld::default();
        let body = m.part(registry, "top", mb::RIGID_BODY, [
            ("mass", self.mass), ("ixx", self.inertia_transverse), ("iyy", self.inertia_transverse), ("izz", self.inertia_axial), ("gravity", self.gravity),
            ("initial.x", lateral_kick), ("initial.z", self.height), ("initial.qw", 1.0), ("initial.wz", self.spin),
        ]).unwrap();
        // Dipole along −z: antiparallel to the ring's field above its plane, so it is repelled.
        let magnet = m.part(registry, "magnet", mag::MAGNETIC_TOP, [("moment", -self.moment()), ("ring_radius", self.ring_radius), ("ring_field", self.ring_field)]).unwrap();
        m.connect([body.port("frame"), magnet.port("frame")]);
        let runtime = runtime(m, registry);
        let body = ["x", "y", "z", "qw", "qx", "qy", "qz", "vx", "vy", "vz", "wx", "wy", "wz"].iter().map(|n| runtime.state_id(body.behavior, n)).collect();
        Top { runtime, body }
    }
    /// Largest growth rate of the levitating state. A spinning top is not
    /// a fixed point in quaternion coordinates — it is a periodic orbit,
    /// and since a full turn returns the quaternion as −q its period is
    /// *two* turns, 4π/Ω — so stability is Floquet's: the multipliers of
    /// the compiled model's monodromy over that period, with the two
    /// neutral directions (spin phase, quaternion norm) projected out.
    pub fn growth_rate(&self, registry: &BehaviorRegistry) -> f64 {
        let period = 4.0 * PI / self.spin.max(1.0e-9);
        let h = (period / 200.0).min(2.0e-4);
        let mut top = self.model(registry, 0.0);
        let ids = top.body.clone();
        let x0: Vec<f64> = ids.iter().map(|id| top.runtime.get(*id)).collect();
        let flow = |x: &[f64]| {
            for (id, value) in ids.iter().zip(x) {
                top.runtime.set(*id, *value).unwrap();
            }
            top.runtime.advance(period, h).unwrap();
            ids.iter().map(|id| top.runtime.get(*id)).collect::<Vec<f64>>()
        };
        let q = [x0[3], x0[4], x0[5], x0[6]];
        // Spin phase: δq = ½ q ⊗ (0, 0, 0, 1); norm: δq ∝ q.
        let mut phase = vec![0.0; 13];
        phase[3] = -0.5 * q[3];
        phase[4] = 0.5 * q[2];
        phase[5] = -0.5 * q[1];
        phase[6] = 0.5 * q[0];
        let mut norm = vec![0.0; 13];
        norm[3..7].copy_from_slice(&q);
        let multipliers = sim_dynamics::analysis::floquet_multipliers(flow, &x0, 1.0e-3, &[phase, norm]);
        multipliers.iter().map(|m| m.norm().ln() / period).fold(f64::NEG_INFINITY, f64::max)
    }
    /// Growth rates below this are the monodromy's noise floor: a
    /// conservative orbit's multipliers sit *on* the unit circle, and the
    /// difference quotients over two turns resolve them to a few 1e-3.
    pub const STABLE_BELOW: f64 = 0.25;

    /// The spin window `(ω_min, ω_max)` from the Floquet growth rate, by scan and bisection.
    pub fn spin_window(&self, registry: &BehaviorRegistry) -> (f64, f64) {
        let stable = |spin: f64| Levitron { spin, ..*self }.growth_rate(registry) <= Self::STABLE_BELOW;
        let grid: Vec<f64> = (0..40).map(|k| 20.0 * 1.15_f64.powi(k)).collect();
        let flags: Vec<bool> = grid.iter().map(|s| stable(*s)).collect();
        let first = flags.iter().position(|f| *f).expect("some spin is stable");
        let last = flags.iter().rposition(|f| *f).unwrap();
        let bisect = |mut lo: f64, mut hi: f64, want_lo_stable: bool| {
            for _ in 0..24 {
                let mid = (lo * hi).sqrt();
                if stable(mid) == want_lo_stable { lo = mid } else { hi = mid }
            }
            (lo * hi).sqrt()
        };
        let low = if first == 0 { grid[0] } else { bisect(grid[first - 1], grid[first], false) };
        let high = if last + 1 == grid.len() { grid[last] } else { bisect(grid[last], grid[last + 1], true) };
        (low, high)
    }
}

pub struct Flight {
    pub time: Vec<f64>,
    pub height: Vec<f64>,
    pub lateral: Vec<f64>,
    pub tilt: Vec<f64>,
    /// First time the top left the trap (2 cm off in height or sideways).
    pub escape: Option<f64>,
}

pub fn fly(levitron: Levitron, registry: &BehaviorRegistry, duration: f64, kick: f64) -> Flight {
    let mut top = levitron.model(registry, kick);
    let trace = record(&mut top.runtime, duration, 2.0e-4, 10, &top.body);
    let height = trace.column(2);
    let lateral = trace.map(|_, x| (x[0] * x[0] + x[1] * x[1]).sqrt());
    let tilt = trace.map(|_, x| {
        let q = nalgebra::UnitQuaternion::from_quaternion(nalgebra::Quaternion::new(x[3], x[4], x[5], x[6]));
        let axis = q * nalgebra::Vector3::new(0.0, 0.0, 1.0);
        axis.z.clamp(-1.0, 1.0).acos()
    });
    let escape = trace.time.iter().zip(height.iter().zip(&lateral)).find(|(_, (z, r))| (**z - levitron.height).abs() > 0.02 || **r > 0.02).map(|(t, _)| *t);
    Flight { time: trace.time.clone(), height, lateral, tilt, escape }
}

pub fn run() -> Report {
    let mut report = Report::new("levitron");
    let registry = registry();
    let top = Levitron::default();
    let (z_low, z_high) = top.trap_window();
    report.measure("dipole moment holding the top (A·m²)", top.moment());
    report.measure("adiabatic trap: lowest stable height (m)", z_low);
    report.measure("adiabatic trap: highest stable height (m)", z_high);
    report.holds("the top sits inside the adiabatic trap window", top.height > z_low && top.height < z_high);

    let (low, high) = top.spin_window(&registry);
    report.measure("spin window: lower edge (rpm)", low * 60.0 / (2.0 * PI));
    report.measure("spin window: upper edge (rpm)", high * 60.0 / (2.0 * PI));
    report.holds("window edges of the toy's order (Simon et al. 1997: ~1 000–3 000 rpm)", low * 60.0 / (2.0 * PI) > 300.0 && high * 60.0 / (2.0 * PI) < 10_000.0);

    let kick = 1.0e-3;
    let cases = [
        ("no spin (Earnshaw)", 0.0, true),
        ("below the window", 0.8 * low, true),
        ("just below the lower edge", 0.9 * low, true),
        ("just above the lower edge", 1.1 * low, false),
        ("inside the window", (low * high).sqrt(), false),
        ("just below the upper edge", 0.9 * high, false),
        ("just above the upper edge", 1.15 * high, true),
        ("above the window", 1.3 * high, true),
    ];
    for (label, spin, escapes) in cases {
        let duration = 8.0;
        let flight = fly(Levitron { spin, ..top }, &registry, duration, kick);
        if matches!(label, "below the window" | "inside the window" | "above the window" | "no spin (Earnshaw)") {
            report.series(&format!("height (m), {label}"), &flight.time, &flight.height, 1200);
            report.series(&format!("lateral offset (m), {label}"), &flight.time, &flight.lateral, 1200);
            report.series(&format!("axis tilt (rad), {label}"), &flight.time, &flight.tilt, 1200);
        }
        match flight.escape {
            Some(t) => report.measure(&format!("{label}: left the trap at (s)"), t),
            None => report.measure(&format!("{label}: still flying after (s)"), duration),
        };
        report.holds(&format!("{label}: {}", if escapes { "falls or flies off" } else { "stays aloft" }), flight.escape.is_some() == escapes);
    }
    report
}

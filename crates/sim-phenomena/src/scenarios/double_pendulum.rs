//! 26. The double pendulum's two notes — `multibody` `joints`.
//!
//! Two equal rods hung one from the other by revolute joints. Rung gently
//! it plays two notes at once: the in-phase and counter-phase modes at
//! `ω² = (2 ∓ √2)·g/L`. Weld the knee and there is one note, at the
//! frequency of a single stiff rod of twice the length.

use crate::world::{record, registry, runtime};
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_multibody::contact as ct;
use sim_domain_multibody::planar as pl;
use sim_dynamics::analysis::power_spectrum;
use sim_dynamics::linear::linearise;

#[derive(Clone, Copy)]
pub struct Pendulum {
    pub length: f64,
    pub mass: f64,
    pub gravity: f64,
    /// Initial angle of the upper rod from the vertical (rad).
    pub swing: f64,
    pub welded: bool,
}

impl Default for Pendulum {
    fn default() -> Self {
        Self { length: 1.0, mass: 1.0, gravity: 9.81, swing: 0.02, welded: false }
    }
}

pub struct Chain {
    pub runtime: Runtime,
    pub upper: [StateId; 6],
    pub lower: [StateId; 6],
}

impl Pendulum {
    /// Small-angle modes of two equal point masses on equal massless rods.
    pub fn mode_frequencies(&self) -> (f64, f64) {
        let base = self.gravity / self.length;
        (((2.0 - 2.0_f64.sqrt()) * base).sqrt(), ((2.0 + 2.0_f64.sqrt()) * base).sqrt())
    }
    /// Welded: one rod of length 2L with the two masses at L and 2L.
    pub fn welded_frequency(&self) -> f64 {
        let l = self.length;
        (self.gravity * (l + 2.0 * l) / (l * l + 4.0 * l * l)).sqrt()
    }
    pub fn model(&self, registry: &BehaviorRegistry) -> Chain {
        let l = self.length;
        let (s, c) = self.swing.sin_cos();
        let mut m = ModelWorld::default();
        // Point-mass rods: tiny inertia about the mass, mass at the rod's tip.
        let upper = m.part(registry, "upper", ct::PLANAR_RIGID_BODY, [
            ("mass", self.mass), ("inertia", 1.0e-6 * self.mass * l * l), ("gravity", self.gravity),
            ("initial.x", l * s), ("initial.y", -l * c), ("initial.theta", self.swing),
        ]).unwrap();
        // The lower rod starts swung the other way, so both notes sound.
        let (s2, c2) = (-self.swing).sin_cos();
        let lower = m.part(registry, "lower", ct::PLANAR_RIGID_BODY, [
            ("mass", self.mass), ("inertia", 1.0e-6 * self.mass * l * l), ("gravity", self.gravity),
            ("initial.x", l * s + l * s2), ("initial.y", -l * c - l * c2), ("initial.theta", -self.swing),
        ]).unwrap();
        // The pivot: a massive body pinned to the world.
        let pivot = m.part(registry, "pivot", ct::PLANAR_RIGID_BODY, [("mass", 1.0e6), ("inertia", 1.0e6), ("gravity", 0.0)]).unwrap();
        let ceiling = m.part(registry, "ceiling", pl::FIXED, [("stabilisation", 50.0)]).unwrap();
        // Rod frames: the mass at the origin, the hinge at (0, +L) in the body (up the rod).
        let shoulder = m.part(registry, "shoulder", pl::REVOLUTE, [("bx", 0.0), ("by", l), ("stabilisation", 50.0)]).unwrap();
        let knee = m.part(registry, "knee", if self.welded { pl::FIXED } else { pl::REVOLUTE }, [("bx", 0.0), ("by", l), ("stabilisation", 50.0)]).unwrap();
        m.connect([pivot.port("frame"), ceiling.port("b"), shoulder.port("a")]);
        m.connect([upper.port("frame"), shoulder.port("b"), knee.port("a")]);
        m.connect([lower.port("frame"), knee.port("b")]);
        // The ceiling joint's `a` side is the world: an anchored frame body.
        let world = m.part(registry, "world", ct::PLANAR_RIGID_BODY, [("mass", 1.0e6), ("inertia", 1.0e6), ("gravity", 0.0)]).unwrap();
        m.connect([world.port("frame"), ceiling.port("a")]);
        let runtime = runtime(m, registry);
        let ids = |body: &sim_core::Instance| ["x", "y", "theta", "vx", "vy", "omega"].map(|n| runtime.state_id(body.behavior, n));
        Chain { upper: ids(&upper), lower: ids(&lower), runtime }
    }
    /// Mode frequencies from the compiled model's linearisation at the hanging state.
    pub fn compiled_modes(&self, registry: &BehaviorRegistry) -> Vec<f64> {
        let chain = Pendulum { swing: 0.0, ..*self }.model(registry);
        let island = &chain.runtime.islands[0];
        let rate = vec![0.0; island.state.len()];
        let lin = linearise(&island.system, 0.0, &island.state, &rate);
        // The 10⁶ kg pivot bodies add a slow wobble far below the rods' notes.
        let mut freqs: Vec<f64> = lin.eigenvalues().iter().filter(|e| e.im > 0.5 && e.norm() < 1.0e3).map(|e| e.im).collect();
        freqs.sort_by(|a, b| a.total_cmp(b));
        freqs
    }
    /// The same pendulum authored in minimal coordinates (`multibody.chain`)
    /// and linearised: a regular pencil, so both notes are resolved.
    pub fn chain_modes(&self, registry: &BehaviorRegistry) -> Vec<f64> {
        use sim_domain_multibody::chain::CHAIN;
        let l = self.length;
        let mut m = ModelWorld::default();
        let pivot = m.part(registry, "pivot", ct::PLANAR_RIGID_BODY, [("mass", 1.0e6), ("inertia", 1.0e6), ("gravity", 0.0)]).unwrap();
        let chain = m.part(registry, "chain", CHAIN, [
            ("joint.shoulder", 0.0), ("joint.elbow", 1.0), ("gravity", self.gravity),
            ("link0.mass", self.mass), ("link0.length", l), ("link0.com", l), ("link0.inertia", 1.0e-6 * self.mass * l * l),
            ("link1.mass", self.mass), ("link1.length", l), ("link1.com", l), ("link1.inertia", 1.0e-6 * self.mass * l * l),
            ("initial.joint.shoulder.angle", -std::f64::consts::FRAC_PI_2),
        ]).unwrap();
        m.connect([pivot.port("frame"), chain.port("base")]);
        m.connect([chain.port("tip")]);
        m.connect([chain.port("joint.shoulder")]);
        m.connect([chain.port("joint.elbow")]);
        let rt = runtime(m, registry);
        let island = &rt.islands[0];
        let rate = vec![0.0; island.state.len()];
        let lin = linearise(&island.system, 0.0, &island.state, &rate);
        let mut freqs: Vec<f64> = lin.eigenvalues().iter().filter(|e| e.im > 0.5 && e.norm() < 1.0e3).map(|e| e.im).collect();
        freqs.sort_by(|a, b| a.total_cmp(b));
        freqs.dedup_by(|a, b| (*a - *b).abs() < 1.0e-6);
        freqs
    }
}

pub struct Ring {
    pub time: Vec<f64>,
    pub lower_x: Vec<f64>,
    pub peaks: Vec<f64>,
    pub energy_drift: f64,
}

pub fn ring(pendulum: Pendulum, registry: &BehaviorRegistry, duration: f64) -> Ring {
    let mut chain = pendulum.model(registry);
    let ids = [chain.lower[0]];
    let trace = record(&mut chain.runtime, duration, 2.0e-3, 2, &ids);
    let lower_x = trace.column(0);
    let spectrum = power_spectrum(&trace.time, &lower_x);
    // Spectral peaks above a tenth of the largest.
    let top = spectrum.iter().map(|(_, p)| *p).fold(0.0, f64::max);
    let mut peaks = Vec::new();
    for k in 1..spectrum.len() - 1 {
        if spectrum[k].1 > 0.1 * top && spectrum[k].1 > spectrum[k - 1].1 && spectrum[k].1 > spectrum[k + 1].1 {
            peaks.push(2.0 * std::f64::consts::PI * spectrum[k].0);
        }
    }
    let energy_drift = (trace.energy.last().unwrap() - trace.energy[0]).abs() / trace.energy[0].abs().max(1.0e-12);
    Ring { time: trace.time.clone(), lower_x, peaks, energy_drift }
}

pub fn run() -> Report {
    let mut report = Report::new("double-pendulum");
    let registry = registry();
    let base = Pendulum::default();
    let (slow, fast) = base.mode_frequencies();
    report.measure("in-phase mode ω₁ = √((2−√2)g/L) (rad/s)", slow);
    report.measure("counter-phase mode ω₂ = √((2+√2)g/L) (rad/s)", fast);
    let modes = base.compiled_modes(&registry);
    report.measure("compiled linearisation: lowest mode (rad/s)", modes.first().copied().unwrap_or(f64::NAN));
    report.measure("compiled linearisation: next mode (rad/s)", modes.get(1).copied().unwrap_or(f64::NAN));
    // The multiplier joints make an index-2 pencil whose finite eigenvalues
    // are not trustworthy from shift-invert (they move with rounding); the
    // values above are recorded, not checked. The same pendulum in minimal
    // coordinates linearises cleanly, and the ring below is the evidence
    // for the joints themselves.
    let chain_modes = base.chain_modes(&registry);
    report.measure("minimal-coordinate chain: lowest mode (rad/s)", chain_modes.first().copied().unwrap_or(f64::NAN));
    report.measure("minimal-coordinate chain: next mode (rad/s)", chain_modes.get(1).copied().unwrap_or(f64::NAN));
    report.within("the chain's linearisation gives the in-phase note", chain_modes.first().copied().unwrap_or(f64::NAN), slow, 1.0e-3);
    report.within("the chain's linearisation gives the counter-phase note", chain_modes.get(1).copied().unwrap_or(f64::NAN), fast, 1.0e-3);

    let rung = ring(base, &registry, 60.0);
    report.series("lower mass x (m)", &rung.time, &rung.lower_x, 1500);
    report.measure("spectral peaks found", rung.peaks.len() as f64);
    let nearest = |target: f64| rung.peaks.iter().map(|p| (p - target).abs() / target).fold(f64::INFINITY, f64::min);
    report.below("the ring shows the in-phase note", nearest(slow), 0.03);
    report.below("the ring shows the counter-phase note", nearest(fast), 0.03);
    report.below("energy is conserved through the joints", rung.energy_drift, 2.0e-3);

    let welded = ring(Pendulum { welded: true, ..base }, &registry, 60.0);
    report.series("lower mass x (m), knee welded", &welded.time, &welded.lower_x, 1500);
    report.measure("welded: single-rod frequency (rad/s)", base.welded_frequency());
    report.measure("welded: spectral peaks found", welded.peaks.len() as f64);
    let nearest_w = |target: f64| welded.peaks.iter().map(|p| (p - target).abs() / target).fold(f64::INFINITY, f64::min);
    report.below("welded: one note, at the stiff rod's frequency", nearest_w(base.welded_frequency()), 0.03);
    report.holds("welded: the counter-phase note is gone", nearest_w(fast) > 0.1);
    report
}

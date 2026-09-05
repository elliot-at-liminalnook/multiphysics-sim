//! 25. Stochastic resonance — `translational` `stochastic`.
//!
//! A particle in a double well, driven too weakly to cross the barrier,
//! plus thermal noise. With little noise nothing happens; with too much it
//! hops at random; in between, the hops synchronise with the drive and the
//! periodic signal comes out of the noise *stronger* than it went in. The
//! optimum is where the Kramers rate matches the drive.

use crate::world::{record, registry, runtime};
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_translational::elements as tr;
use sim_dynamics::analysis::power_spectrum;

#[derive(Clone, Copy)]
pub struct Well {
    pub a: f64,
    pub b: f64,
    pub mass: f64,
    pub damping: f64,
    /// Bath temperature in energy units, kT.
    pub temperature: f64,
    pub drive_amplitude: f64,
    pub drive_frequency: f64,
    pub seed: u64,
}

impl Default for Well {
    fn default() -> Self {
        Self { a: 1.0, b: 1.0, mass: 0.02, damping: 1.0, temperature: 0.1, drive_amplitude: 0.1, drive_frequency: 0.05 / (2.0 * std::f64::consts::PI), seed: 7 }
    }
}

pub struct Particle {
    pub runtime: Runtime,
    pub position: StateId,
}

impl Well {
    pub fn barrier(&self) -> f64 {
        self.a * self.a / (4.0 * self.b)
    }
    /// Kramers' escape rate for the overdamped well: `√(U''_min·|U''_max|)/(2πγ)·e^{−ΔU/kT}`.
    pub fn kramers_rate(&self) -> f64 {
        (2.0 * self.a * self.a).sqrt() / (2.0 * std::f64::consts::PI * self.damping) * (-self.barrier() / self.temperature).exp()
    }
    /// The temperature at which two Kramers hops per period match the drive: `2r_K = Ω`.
    pub fn optimal_temperature(&self) -> f64 {
        let omega = 2.0 * std::f64::consts::PI * self.drive_frequency;
        self.barrier() / ((2.0 * self.a * self.a).sqrt() / (std::f64::consts::PI * self.damping * omega)).ln()
    }
    pub fn model(&self, registry: &BehaviorRegistry) -> Particle {
        let mut m = ModelWorld::default();
        let start = if self.a > 0.0 { (self.a / self.b).sqrt() } else { 0.0 };
        let mass = m.part(registry, "particle", tr::MASS, [("mass", self.mass), ("initial.position", start)]).unwrap();
        let well = m.part(registry, "well", tr::DOUBLE_WELL, [("a", self.a), ("b", self.b)]).unwrap();
        let bath = m.part(registry, "bath", tr::LANGEVIN, [("damping", self.damping), ("intensity", 2.0 * self.damping * self.temperature), ("drive_amplitude", self.drive_amplitude), ("drive_frequency", self.drive_frequency)]).unwrap();
        m.connect([mass.port("axis"), well.port("axis"), bath.port("axis")]);
        let mut runtime = runtime(m, registry);
        runtime.seed(self.seed);
        let position = runtime.across_id(mass.port("axis"));
        Particle { runtime, position }
    }
}

pub struct Outcome {
    pub time: Vec<f64>,
    pub position: Vec<f64>,
    /// Output power at the drive frequency: the spectral amplification.
    pub signal: f64,
    pub snr: f64,
    pub hops: usize,
}

pub fn jiggle(well: Well, registry: &BehaviorRegistry, periods: f64) -> Outcome {
    let mut particle = well.model(registry);
    let duration = periods / well.drive_frequency;
    let trace = record(&mut particle.runtime, duration, 0.05, 1, &[particle.position]);
    let position = trace.column(0);
    let spectrum = power_spectrum(&trace.time, &position);
    let target = well.drive_frequency;
    let df = spectrum[1].0 - spectrum[0].0;
    let peak = spectrum.iter().filter(|(f, _)| (f - target).abs() <= 1.5 * df).map(|(_, p)| *p).fold(0.0, f64::max);
    let floor: Vec<f64> = spectrum.iter().filter(|(f, _)| (f - target).abs() > 4.0 * df && *f > 0.5 * target && *f < 2.0 * target).map(|(_, p)| *p).collect();
    let snr = peak / sim_dynamics::analysis::mean(&floor).max(1.0e-300);
    let hops = position.windows(2).filter(|w| (w[0] > 0.0) != (w[1] > 0.0)).count();
    Outcome { time: trace.time.clone(), position, signal: peak, snr, hops }
}

pub fn run() -> Report {
    let mut report = Report::new("stochastic-resonance");
    let registry = registry();
    let base = Well::default();
    let optimum = base.optimal_temperature();
    report.measure("barrier ΔU", base.barrier());
    report.measure("drive amplitude / static threshold", base.drive_amplitude / (2.0 / 27.0_f64.sqrt() * base.a.powf(1.5) / base.b.sqrt()));
    report.measure("Kramers optimum kT (2r_K = Ω)", optimum);
    let temperatures = [0.3, 0.5, 0.7, 1.0, 1.4, 2.0, 3.0].map(|f| f * optimum);
    let mut signals = Vec::new();
    for (k, kt) in temperatures.iter().enumerate() {
        let outcome = jiggle(Well { temperature: *kt, ..base }, &registry, 20.0);
        if k == 0 || k == 3 || k == 6 {
            report.series(&format!("position, kT = {:.2} × optimum", kt / optimum), &outcome.time, &outcome.position, 1500);
        }
        report.measure(&format!("kT = {:.2} × optimum: output power at the drive frequency", kt / optimum), outcome.signal);
        report.measure(&format!("kT = {:.2} × optimum: hops per drive period", kt / optimum), outcome.hops as f64 / 20.0);
        signals.push(outcome.signal);
    }
    let best = signals.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map(|(k, _)| k).unwrap();
    report.measure("strongest output at kT / optimum", temperatures[best] / optimum);
    report.holds("the output at the drive frequency peaks at an intermediate noise level", best > 0 && best + 1 < temperatures.len());
    report.holds("the optimum is within a factor 2 of Kramers' 2r_K = Ω", (temperatures[best] / optimum).ln().abs() < 2.0_f64.ln() + 1.0e-9);
    report.above("the right noise amplifies the signal: peak output over the quiet case", signals[best] / signals[0], 5.0);
    // Falsifier: one well — a linear response that noise can only bury.
    let mut single = Vec::new();
    for kt in [0.3 * optimum, 1.0 * optimum, 3.0 * optimum] {
        let outcome = jiggle(Well { a: -base.a, temperature: kt, ..base }, &registry, 20.0);
        report.measure(&format!("single well, kT = {:.2} × optimum: output power / SNR", kt / optimum), outcome.signal);
        single.push((outcome.signal, outcome.snr));
    }
    let (lo, hi) = (single.iter().map(|s| s.0).fold(f64::INFINITY, f64::min), single.iter().map(|s| s.0).fold(0.0, f64::max));
    report.below("single well: no amplification worth the name (within a factor 3 across the same noise range)", hi / lo, 3.0);
    report.holds("single well: noise only ever hurts the signal-to-noise ratio", single[0].1 > single[1].1 && single[1].1 > single[2].1);
    report
}

//! 23. Vortex-induced vibration lock-in — `line` `fluid`.
//!
//! A taut cable in a cross-flow sheds vortices at the Strouhal frequency
//! `St·U/D`, which walks up with the flow speed. Near a cable mode the
//! shedding does not pass through resonance and move on: it *locks* to the
//! mode across a band of speeds and the amplitude plateaus. The wake is an
//! oscillator that listens to the cable; take away its ear and the plateau
//! vanishes.

use crate::world::{record, registry, runtime};
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_fluid as fl;
use sim_domain_line as line;
use sim_domain_translational::elements as tr;

#[derive(Clone, Copy)]
pub struct Cable {
    pub length: f64,
    pub diameter: f64,
    pub tension: f64,
    pub mass_per_length: f64,
    pub damping_per_length: f64,
    pub cells: usize,
    pub density: f64,
    pub speed: f64,
    /// Facchinetti's wake–structure coupling; 0 removes the feedback.
    pub coupling: f64,
}

impl Default for Cable {
    fn default() -> Self {
        Self { length: 10.0, diameter: 0.1, tension: 4000.0, mass_per_length: 20.0, damping_per_length: 1.0, cells: 8, density: 1000.0, speed: 1.0, coupling: 12.0 }
    }
}

pub struct Span {
    pub runtime: Runtime,
    pub midpoint: StateId,
    pub displacements: Vec<StateId>,
}

impl Cable {
    pub fn natural_frequency(&self) -> f64 {
        1.0 / (2.0 * self.length) * (self.tension / self.mass_per_length).sqrt()
    }
    pub fn shedding_frequency(&self) -> f64 {
        0.2 * self.speed / self.diameter
    }
    pub fn reduced_velocity(&self) -> f64 {
        self.speed / (self.natural_frequency() * self.diameter)
    }
    pub fn model(&self, registry: &BehaviorRegistry) -> Span {
        let mut m = ModelWorld::default();
        let mut params: Vec<(&'static str, f64)> = vec![("length", self.length), ("tension", self.tension), ("mass_per_length", self.mass_per_length), ("damping_per_length", self.damping_per_length), ("cells", self.cells as f64), ("initial.shape", 0.01 * self.diameter)];
        // A wake oscillator at every cell.
        let mut taps = Vec::new();
        for i in 0..self.cells {
            let key: &'static str = Box::leak(format!("tap.c{i:02}").into_boxed_str());
            params.push((key, (i as f64 + 1.0) / (self.cells as f64 + 1.0)));
            taps.push(key);
        }
        let cable = m.part(registry, "cable", line::STRING, params).unwrap();
        for (i, tap) in taps.iter().enumerate() {
            let wake = m.part(registry, &format!("wake {i}"), fl::WAKE_OSCILLATOR, [
                ("density", self.density), ("speed", self.speed), ("diameter", self.diameter), ("coupling", self.coupling), ("length", self.length / (self.cells as f64 + 1.0)),
            ]).unwrap();
            m.connect([cable.port(tap), wake.port("structure")]);
        }
        for end in ["left", "right"] {
            let anchor = m.part(registry, &format!("{end} anchor"), tr::GROUND, []).unwrap();
            m.connect([cable.port(end), anchor.port("axis")]);
        }
        let runtime = runtime(m, registry);
        let displacements: Vec<StateId> = (0..self.cells).map(|i| runtime.state_id(cable.behavior, &format!("y{i}"))).collect();
        let midpoint = displacements[self.cells / 2];
        Span { runtime, midpoint, displacements }
    }
}

pub struct Response {
    pub time: Vec<f64>,
    pub midpoint: Vec<f64>,
    /// Steady amplitude over the last third, as a fraction of the diameter.
    pub amplitude_ratio: f64,
    pub frequency: f64,
}

pub fn respond(cable: Cable, registry: &BehaviorRegistry, duration: f64) -> Response {
    let mut span = cable.model(registry);
    let trace = record(&mut span.runtime, duration, 4.0e-3, 5, &[span.midpoint]);
    let midpoint = trace.column(0);
    let tail = 2 * midpoint.len() / 3;
    let amplitude = sim_dynamics::analysis::max_abs(&midpoint[tail..]);
    let frequency = sim_dynamics::analysis::period(&trace.time[tail..], &midpoint[tail..]).map(|p| 1.0 / p).unwrap_or(f64::NAN);
    Response { time: trace.time.clone(), midpoint, amplitude_ratio: amplitude / cable.diameter, frequency }
}

pub fn run() -> Report {
    let mut report = Report::new("viv-lock-in");
    let registry = registry();
    let base = Cable::default();
    let fn1 = base.natural_frequency();
    report.measure("cable first mode (Hz)", fn1);
    report.measure("speed where St·U/D = f₁ (m/s)", fn1 * base.diameter / 0.2);
    let speeds: Vec<f64> = (0..12).map(|k| (0.55 + 0.15 * k as f64) * fn1 * base.diameter / 0.2).collect();
    let mut locked = Vec::new();
    let mut deaf = Vec::new();
    for (k, speed) in speeds.iter().enumerate() {
        let with = respond(Cable { speed: *speed, ..base }, &registry, 40.0);
        let without = respond(Cable { speed: *speed, coupling: 0.0, ..base }, &registry, 40.0);
        if k == 4 {
            report.series("midpoint (m), wake listening, at St·U/D ≈ 1.15 f₁", &with.time, &with.midpoint, 1500);
            report.series("midpoint (m), wake deaf, at St·U/D ≈ 1.15 f₁", &without.time, &without.midpoint, 1500);
        }
        report.measure(&format!("U/(f₁D) = {:.2}: A/D listening / deaf", Cable { speed: *speed, ..base }.reduced_velocity()), with.amplitude_ratio);
        report.measure(&format!("U/(f₁D) = {:.2}: response frequency / f₁ (listening)", Cable { speed: *speed, ..base }.reduced_velocity()), with.frequency / fn1);
        locked.push(with);
        deaf.push(without);
    }
    let peak = |r: &[Response]| r.iter().map(|x| x.amplitude_ratio).fold(0.0_f64, f64::max);
    let band = |r: &[Response]| {
        let p = peak(r);
        r.iter().filter(|x| x.amplitude_ratio > 0.5 * p).count()
    };
    let (peak_locked, peak_deaf) = (peak(&locked), peak(&deaf));
    let (band_locked, band_deaf) = (band(&locked), band(&deaf));
    report.measure("peak A/D with the wake listening", peak_locked);
    report.measure("peak A/D with the wake deaf (plain resonance)", peak_deaf);
    report.measure("speeds within half the peak, listening", band_locked as f64);
    report.measure("speeds within half the peak, deaf", band_deaf as f64);
    report.holds("peak amplitude of the order Facchinetti et al. find (0.3–1 D)", peak_locked > 0.3 && peak_locked < 1.0);
    report.above("lock-in: the plateau spans at least twice the resonance band", band_locked as f64 / band_deaf.max(1) as f64, 2.0);
    // Inside the band the cable rings at its own mode, not at St·U/D.
    let centre = &locked[4];
    report.within("inside the band the response sits on the cable mode", centre.frequency, fn1, 0.1);
    report.above("…while the shedding frequency has walked past it", Cable { speed: speeds[4], ..base }.shedding_frequency() / fn1, 1.1);
    report
}

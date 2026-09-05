//! 29. Quantisation limit cycle — `control` `sensing` `electrical` `rotational`.
//!
//! A PI position loop on a motor with an encoder, asked to hold a position
//! half a count from a count edge. With a continuous angle the loop settles
//! and goes quiet; with the angle quantised it never settles — the encoder
//! reads one count or the next, never the target, the integrator winds
//! back and forth, and the shaft hunts for ever at a fraction of a count.
//! Around the edge the quantiser is a relay of height `q/2`, whose
//! describing function `N(A) = 2q/(πA)` gives the amplitude
//! `A = 2q·|L(jω_c)|/π` at the phase crossover `ω_c` of the linear part
//! `L(s) = (Kp + Ki/s)·K/(s(1 + τs))·e^{−sT}` (two holds): the amplitude scales with
//! the count, the frequency does not.

use crate::Report;
use crate::world::{registry, runtime};
use nalgebra::Complex;
use sim_compile::Runtime;
use sim_core::{BehaviorId, BehaviorRegistry, FnCoupler, ModelWorld, StateId};
use sim_domain_bridges::elements as bridge;
use sim_domain_control::external::EXTERNAL;
use sim_domain_electrical::elements as el;
use sim_domain_rotational::elements as rot;
use sim_domain_sensing as sense;
use sim_dynamics::analysis::power_spectrum;
use std::f64::consts::{PI, TAU};

#[derive(Clone, Copy)]
pub struct Hunt {
    pub resistance: f64,
    pub torque_constant: f64,
    pub inertia: f64,
    pub viscous_drag: f64,
    pub period: f64,
    /// Encoder counts per turn; 0 for a continuous angle.
    pub counts: f64,
    pub kp: f64,
    pub ki: f64,
    pub start_angle: f64,
}

impl Default for Hunt {
    fn default() -> Self {
        Self { resistance: 0.6, torque_constant: 0.05, inertia: 2.0e-4, viscous_drag: 2.0e-4, period: 1.0e-2, counts: 1024.0, kp: 3.0, ki: 15.0, start_angle: 0.05 }
    }
}

pub struct Plant {
    pub runtime: Runtime,
    pub controller: BehaviorId,
    pub angle: StateId,
    pub measured: StateId,
}

impl Hunt {
    pub fn time_constant(&self) -> f64 {
        self.inertia * self.resistance / (self.viscous_drag * self.resistance + self.torque_constant * self.torque_constant)
    }
    pub fn gain(&self) -> f64 {
        self.torque_constant / (self.viscous_drag * self.resistance + self.torque_constant * self.torque_constant)
    }
    pub fn quantum(&self) -> f64 {
        if self.counts > 0.0 { TAU / self.counts } else { 0.0 }
    }

    /// The linear part from measured angle back to angle: two zero-order
    /// holds (the encoder's and the controller's), half a sample each.
    pub fn linear(&self, omega: f64) -> Complex<f64> {
        let s = Complex::new(0.0, omega);
        let controller = self.kp + self.ki / s;
        let plant = self.gain() / (s * (1.0 + self.time_constant() * s));
        let hold = Complex::new(0.0, -omega * self.period).exp();
        controller * plant * hold
    }

    /// The target: half a count past zero, so the encoder can never read it.
    pub fn setpoint(&self) -> f64 {
        if self.counts > 0.0 { 0.5 * self.quantum() } else { 0.5 * TAU / 1024.0 }
    }

    /// Phase crossover: the frequency where the linear part's phase is −π.
    pub fn crossover(&self) -> f64 {
        // Unwrapped: the phase starts near −π/2 and only falls.
        let phase = |w: f64| { let a = self.linear(w).arg(); if a > 0.0 { a - TAU } else { a } };
        let (mut lo, mut hi) = (1.0, 1.0);
        while phase(hi) > -PI && hi < 1.0e6 {
            hi *= 1.5;
        }
        while phase(lo) < -PI && lo > 1.0e-6 {
            lo /= 1.5;
        }
        for _ in 0..80 {
            let mid = (lo * hi).sqrt();
            if phase(mid) > -PI { lo = mid } else { hi = mid }
        }
        (lo * hi).sqrt()
    }

    /// Describing function of the quantiser seen from a count edge — a
    /// relay of height `q/2` — at amplitude `a`.
    pub fn describing(&self, a: f64) -> f64 {
        2.0 * self.quantum() / (PI * a)
    }

    /// Predicted limit cycle `(amplitude, frequency in Hz)`: `N(A)·|L(jω_c)| = 1`.
    pub fn predicted_cycle(&self) -> Option<(f64, f64)> {
        let wc = self.crossover();
        (self.quantum() > 0.0).then(|| (2.0 * self.quantum() * self.linear(wc).norm() / PI, wc / TAU))
    }

    pub fn model(&self, registry: &BehaviorRegistry) -> Plant {
        let mut m = ModelWorld::default();
        let source = m.part(registry, "source", el::CONTROLLED_VOLTAGE_SOURCE, []).unwrap();
        let ground = m.part(registry, "ground", el::GROUND, []).unwrap();
        let motor = m.part(registry, "motor", bridge::BRUSHED_MOTOR, [("resistance", self.resistance), ("inductance", 0.0), ("torque_constant", self.torque_constant), ("back_emf_constant", self.torque_constant)]).unwrap();
        let rotor = m.part(registry, "rotor", rot::INERTIA, [("inertia", self.inertia), ("damping", self.viscous_drag), ("initial.angle", self.start_angle)]).unwrap();
        let mount = m.part(registry, "mount", rot::GROUND, []).unwrap();
        let encoder = m.part(registry, "encoder", sense::ENCODER, [("counts", self.counts), ("period", if self.counts > 0.0 { self.period } else { 0.0 })]).unwrap();
        let controller = m.part(registry, "controller", EXTERNAL, [("period", self.period), ("sense.angle", 0.0), ("act.voltage", 0.0)]).unwrap();
        m.connect([source.port("p"), motor.port("p")]);
        m.connect([source.port("n"), motor.port("n"), ground.port("pin")]);
        m.connect([motor.port("shaft"), rotor.port("shaft"), encoder.port("shaft")]);
        m.connect([motor.port("case"), mount.port("flange")]);
        m.connect([encoder.port("angle"), controller.port("sense.angle")]);
        m.connect([controller.port("act.voltage"), source.port("voltage")]);
        let mut runtime = runtime(m, registry);
        let (kp, ki, period, setpoint) = (self.kp, self.ki, self.period, self.setpoint());
        let mut integral = 0.0;
        runtime
            .attach(controller.behavior, Box::new(FnCoupler(move |_t: f64, s: &[f64], a: &mut [f64]| {
                let error = setpoint - s[0];
                integral += ki * period * error;
                a[0] = kp * error + integral;
            })))
            .unwrap();
        let angle = runtime.across_id(rotor.port("shaft"));
        let measured = runtime.signal_id(encoder.port("angle"));
        Plant { runtime, controller: controller.behavior, angle, measured }
    }

    /// Run for `duration` and measure the residual motion over the last 40%:
    /// `(amplitude, frequency in Hz, time, angle)`.
    pub fn measure(&self, registry: &BehaviorRegistry, duration: f64) -> (f64, f64, Vec<f64>, Vec<f64>) {
        let mut plant = self.model(registry);
        let trace = plant.runtime.advance_recording(duration, self.period / 4.0, 4, &[plant.angle]).unwrap();
        let angle = trace.column(0).to_vec();
        let from = (0.6 * angle.len() as f64) as usize;
        let tail = &angle[from..];
        let (lo, hi) = tail.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        let mean = tail.iter().sum::<f64>() / tail.len() as f64;
        let centred: Vec<f64> = tail.iter().map(|v| v - mean).collect();
        let spectrum = power_spectrum(&trace.time[from..], &centred);
        let frequency = spectrum.iter().skip(1).fold((0.0, 0.0), |best, (f, p)| if *p > best.1 { (*f, *p) } else { best }).0;
        (0.5 * (hi - lo), frequency, trace.time.clone(), angle)
    }
}

pub fn run() -> Report {
    let mut report = Report::new("quantisation-hunt");
    let registry = registry();
    let base = Hunt::default();
    let wc = base.crossover();
    report.measure("phase crossover of the linear loop (Hz)", wc / TAU);
    report.measure("|L(jω_c)| (below one: the linear loop is stable)", base.linear(wc).norm());
    let (amplitude, frequency) = base.predicted_cycle().unwrap_or((f64::NAN, f64::NAN));
    report.measure("describing function: predicted amplitude (counts)", amplitude / base.quantum());
    report.measure("describing function: predicted frequency (Hz)", frequency);
    let (a, f, time, angle) = base.measure(&registry, 5.0);
    report.series("angle (counts), 1024-count encoder", &time, &angle.iter().map(|v| v / base.quantum()).collect::<Vec<_>>(), 1200);
    report.measure("1024 counts: hunt amplitude (counts)", a / base.quantum());
    report.measure("1024 counts: hunt frequency (Hz)", f);
    report.above("the quantised loop never settles: it hunts by at least a tenth of a count", a / base.quantum(), 0.1);
    report.within("hunt amplitude matches the describing function", a, amplitude, 0.35);
    report.within("hunt frequency is near the phase crossover (a sampled relay cycle rounds to whole samples)", f, frequency, 0.4);
    // The knob: more counts, the same hunt at a smaller amplitude.
    let fine = Hunt { counts: 4096.0, ..base };
    let (a4, f4, time4, angle4) = fine.measure(&registry, 5.0);
    report.series("angle (counts), 4096-count encoder", &time4, &angle4.iter().map(|v| v / fine.quantum()).collect::<Vec<_>>(), 1200);
    report.measure("4096 counts: hunt amplitude (counts)", a4 / fine.quantum());
    report.measure("4096 counts: hunt frequency (Hz)", f4);
    report.below("four times the counts: the hunt shrinks with the quantum", a4, 0.5 * a);
    report.holds("four times the counts: still a fraction of a count", a4 / fine.quantum() > 0.1 && a4 / fine.quantum() < 1.0);
    report.within("four times the counts: the frequency is the same", f4, f, 0.25);
    // Falsifier: a continuous angle, and the same loop goes quiet.
    let smooth = Hunt { counts: 0.0, ..base };
    let (a0, _, time0, angle0) = smooth.measure(&registry, 5.0);
    report.series("angle (rad), continuous encoder", &time0, &angle0, 1200);
    report.measure("continuous: residual amplitude (rad)", a0);
    report.below("falsifier: remove the quantiser and the loop settles", a0, 0.1 * base.quantum());
    report
}

//! 28. Latency-induced instability — `control` `electrical` `rotational`.
//!
//! A proportional speed loop closed through the external-controller seam.
//! Nothing about the gain changes; only the bus latency — whole samples
//! between the sensor frame and the command — and a loop that is stable at
//! zero latency grows without bound two samples later. The first-order
//! plant sampled with a zero-order hold is `x⁺ = a·x + b·u`, `a = e^{−T/τ}`,
//! `b = K(1 − a)`; with `u[k] = −Kp·x[k − d]` the characteristic polynomial
//! is `z^{d+1} − a·z^d + L(1 − a)`, `L = Kp·K`, and the loop goes unstable
//! where a root reaches the unit circle.

use crate::Report;
use crate::world::{registry, runtime};
use nalgebra::DMatrix;
use sim_compile::Runtime;
use sim_core::{BehaviorId, BehaviorRegistry, FnCoupler, ModelWorld, StateId};
use sim_domain_bridges::elements as bridge;
use sim_domain_control::external::EXTERNAL;
use sim_domain_electrical::elements as el;
use sim_domain_rotational::elements as rot;

#[derive(Clone, Copy)]
pub struct Loop {
    pub resistance: f64,
    pub torque_constant: f64,
    pub inertia: f64,
    pub viscous_drag: f64,
    pub period: f64,
    /// Bus latency in whole samples.
    pub latency: usize,
    /// Loop gain `Kp·K` (dimensionless).
    pub loop_gain: f64,
}

impl Default for Loop {
    fn default() -> Self {
        Self { resistance: 0.6, torque_constant: 0.05, inertia: 2.0e-4, viscous_drag: 2.0e-4, period: 5.0e-3, latency: 2, loop_gain: 5.0 }
    }
}

pub struct Plant {
    pub runtime: Runtime,
    pub controller: BehaviorId,
    pub speed: StateId,
    pub angle: StateId,
}

impl Loop {
    pub fn time_constant(&self) -> f64 {
        self.inertia * self.resistance / (self.viscous_drag * self.resistance + self.torque_constant * self.torque_constant)
    }
    pub fn gain(&self) -> f64 {
        self.torque_constant / (self.viscous_drag * self.resistance + self.torque_constant * self.torque_constant)
    }
    pub fn kp(&self) -> f64 {
        self.loop_gain / self.gain()
    }

    /// Spectral radius of `z^{d+1} − a·z^d + L(1 − a)`: above one the
    /// sampled loop grows.
    pub fn spectral_radius(&self) -> f64 {
        let a = (-self.period / self.time_constant()).exp();
        let n = self.latency + 1;
        // Companion matrix of the monic polynomial z^n − a z^{n−1} + c.
        let mut m = DMatrix::zeros(n, n);
        m[(0, 0)] = a;
        if n > 1 {
            m[(0, n - 1)] = -self.loop_gain * (1.0 - a);
            for k in 1..n {
                m[(k, k - 1)] = 1.0;
            }
        } else {
            m[(0, 0)] = a - self.loop_gain * (1.0 - a);
        }
        m.complex_eigenvalues().iter().map(|e| e.norm()).fold(0.0, f64::max)
    }

    /// The loop gain at which the spectral radius reaches one for this latency.
    pub fn critical_gain(&self) -> f64 {
        let (mut lo, mut hi) = (0.0, 1.0);
        while (Self { loop_gain: hi, ..*self }).spectral_radius() < 1.0 {
            hi *= 2.0;
        }
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if (Self { loop_gain: mid, ..*self }).spectral_radius() < 1.0 { lo = mid } else { hi = mid }
        }
        0.5 * (lo + hi)
    }

    /// Voltage source → brushed motor → inertia, tachometer into the seam,
    /// the seam's command back to the source, the seam's input delayed by
    /// `latency` samples.
    pub fn model(&self, registry: &BehaviorRegistry) -> Plant {
        let mut m = ModelWorld::default();
        let source = m.part(registry, "source", el::CONTROLLED_VOLTAGE_SOURCE, []).unwrap();
        let ground = m.part(registry, "ground", el::GROUND, []).unwrap();
        let motor = m.part(registry, "motor", bridge::BRUSHED_MOTOR, [("resistance", self.resistance), ("inductance", 0.0), ("torque_constant", self.torque_constant), ("back_emf_constant", self.torque_constant)]).unwrap();
        let rotor = m.part(registry, "rotor", rot::INERTIA, [("inertia", self.inertia), ("damping", self.viscous_drag), ("initial.speed", 1.0)]).unwrap();
        let mount = m.part(registry, "mount", rot::GROUND, []).unwrap();
        let tacho = m.part(registry, "tacho", rot::SPEED_SENSOR, []).unwrap();
        let controller = m.part(registry, "controller", EXTERNAL, [("period", self.period), ("input_delay", self.latency as f64), ("sense.speed", 0.0), ("act.voltage", 0.0)]).unwrap();
        m.connect([source.port("p"), motor.port("p")]);
        m.connect([source.port("n"), motor.port("n"), ground.port("pin")]);
        m.connect([motor.port("shaft"), rotor.port("shaft"), tacho.port("shaft")]);
        m.connect([motor.port("case"), mount.port("flange")]);
        m.connect([tacho.port("speed"), controller.port("sense.speed")]);
        m.connect([controller.port("act.voltage"), source.port("voltage")]);
        let mut runtime = runtime(m, registry);
        let kp = self.kp();
        runtime.attach(controller.behavior, Box::new(FnCoupler(move |_t: f64, s: &[f64], a: &mut [f64]| a[0] = -kp * s[0]))).unwrap();
        let speed = runtime.state_id(rotor.behavior, "speed");
        let angle = runtime.across_id(rotor.port("shaft"));
        Plant { runtime, controller: controller.behavior, speed, angle }
    }

    /// Measured growth rate (1/s) of the speed's envelope over `samples`
    /// samples: the log ratio of the RMS over the last third to the first.
    pub fn growth_rate(&self, registry: &BehaviorRegistry, samples: usize) -> (f64, Vec<f64>, Vec<f64>) {
        let mut plant = self.model(registry);
        let h = self.period / 5.0;
        let trace = plant.runtime.advance_recording(samples as f64 * self.period, h, 1, &[plant.speed]).unwrap();
        let speed = trace.column(0).to_vec();
        let third = speed.len() / 3;
        let rms = |s: &[f64]| (s.iter().map(|v| v * v).sum::<f64>() / s.len() as f64).sqrt();
        let (early, late) = (rms(&speed[..third]), rms(&speed[speed.len() - third..]));
        let span = trace.time[speed.len() - third] - trace.time[0];
        ((late / early).ln() / span, trace.time.clone(), speed)
    }
}

pub fn run() -> Report {
    let mut report = Report::new("latency-instability");
    let registry = registry();
    let base = Loop::default();
    report.measure("plant time constant τ (s)", base.time_constant());
    report.measure("sample period T (s)", base.period);
    // The unit-circle crossing for each latency, at fixed loop gain.
    for latency in 0..=5 {
        let critical = Loop { latency, ..base }.critical_gain();
        report.measure(&format!("latency {latency} samples: critical loop gain"), critical);
    }
    // Sweep the latency at fixed gain: the loop is stable until the
    // predicted latency and grows past it.
    let mut first_unstable_measured = None;
    let mut first_unstable_predicted = None;
    for latency in 0..=5 {
        let case = Loop { latency, ..base };
        let (rate, time, speed) = case.growth_rate(&registry, 120);
        let predicted = case.spectral_radius().ln() / case.period;
        report.measure(&format!("latency {latency}: measured growth rate (1/s)"), rate);
        report.measure(&format!("latency {latency}: predicted growth rate (1/s)"), predicted);
        if latency <= 3 {
            report.series(&format!("speed (rad/s), latency {latency} samples"), &time, &speed, 600);
        }
        report.holds(&format!("latency {latency}: the loop grows iff the polynomial says so"), (rate > 0.0) == (predicted > 0.0));
        if predicted > 0.0 && first_unstable_predicted.is_none() {
            first_unstable_predicted = Some(latency);
        }
        if rate > 0.0 && first_unstable_measured.is_none() {
            first_unstable_measured = Some(latency);
        }
    }
    report.measure("first unstable latency, predicted (samples)", first_unstable_predicted.map(|l| l as f64).unwrap_or(f64::NAN));
    report.measure("first unstable latency, measured (samples)", first_unstable_measured.map(|l| l as f64).unwrap_or(f64::NAN));
    report.holds("the latency that tips the loop is the one the unit circle predicts", first_unstable_measured.is_some() && first_unstable_measured == first_unstable_predicted);
    // The published number: bisect the measured critical gain at two samples
    // of latency and compare with the polynomial's.
    let case = Loop { latency: 2, ..base };
    let (mut lo, mut hi) = (0.5 * case.critical_gain(), 2.0 * case.critical_gain());
    for _ in 0..8 {
        let mid = 0.5 * (lo + hi);
        let (rate, _, _) = Loop { loop_gain: mid, ..case }.growth_rate(&registry, 120);
        if rate > 0.0 { hi = mid } else { lo = mid }
    }
    report.measure("latency 2: critical loop gain, measured", 0.5 * (lo + hi));
    report.within("latency 2: measured critical gain matches the unit-circle crossing", 0.5 * (lo + hi), case.critical_gain(), 0.05);
    // Falsifier: the same gain with no latency is comfortably stable — the
    // instability is the bus's, not the gain's.
    let (rate0, _, _) = Loop { latency: 0, loop_gain: hi, ..base }.growth_rate(&registry, 120);
    report.measure("latency 0 at that gain: growth rate (1/s)", rate0);
    report.below("falsifier: remove the latency and the same gain is stable", rate0, 0.0);
    report
}

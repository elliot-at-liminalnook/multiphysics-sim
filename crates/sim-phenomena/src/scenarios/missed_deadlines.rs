//! 30. Missed deadlines — `control` `electrical` `rotational`.
//!
//! Real-time mode: the Python controller of plate 27 runs behind the
//! `RealTime` coupler, which gives it one sample period of wall clock to
//! answer. A controller that computes faster than the period is invisible;
//! one that takes longer misses every deadline, its commands land a sample
//! late, and a speed loop whose gain is safe at zero latency but past the
//! one-sample limit of plate 28 turns from decay to growth. The knob is the
//! controller's compute time; the boundary is the sample period.

use crate::Report;
use crate::scenarios::language_independence::{plant, spawn_python};
use crate::world::registry;
use sim_core::BehaviorRegistry;
use sim_couple::RealTime;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[derive(Clone, Copy)]
pub struct Deadline {
    pub period: f64,
    /// Loop gain Kp·K: below the zero-latency limit, above the one-sample limit.
    pub loop_gain: f64,
    /// Wall-clock seconds the controller spends per sample.
    pub busy: f64,
    pub setpoint: f64,
    pub samples: usize,
}

impl Default for Deadline {
    fn default() -> Self {
        // Between the zero-latency and one-sample limits of plate 28's
        // polynomial at this period: kept deadlines decay, missed ones grow.
        let (zero, one) = Self::limits(1.0e-2);
        Self { period: 1.0e-2, loop_gain: (zero * one).sqrt(), busy: 0.0, setpoint: 0.0, samples: 80 }
    }
}

impl Deadline {
    /// Critical loop gains at zero and one sample of latency for `period`.
    pub fn limits(period: f64) -> (f64, f64) {
        use crate::scenarios::latency_instability::Loop;
        let base = Loop { period, ..Loop::default() };
        (Loop { latency: 0, ..base }.critical_gain(), Loop { latency: 1, ..base }.critical_gain())
    }
}

/// Plant gain of the motor loop plate 27 builds (rad/s per volt).
pub fn plant_gain() -> f64 {
    let (drag, r, kt) = (2.0e-4, 0.6, 0.05);
    kt / (drag * r + kt * kt)
}

pub struct Outcome {
    pub missed_fraction: f64,
    pub growth_rate: f64,
    pub time: Vec<f64>,
    pub speed: Vec<f64>,
}

impl Deadline {
    pub fn kp(&self) -> f64 {
        self.loop_gain / plant_gain()
    }

    pub fn run(&self, registry: &BehaviorRegistry) -> std::io::Result<Outcome> {
        let (mut rt, seam, speed, _) = plant(registry, self.period);
        rt.set(speed, 1.0).unwrap();
        let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../clients/python/examples/pi_controller.py");
        let inner = spawn_python(&[script.to_str().unwrap(), "--kp", &self.kp().to_string(), "--ki", "0", "--setpoint", &self.setpoint.to_string(), "--sensor", "speed", "--actuator", "voltage", "--busy", &self.busy.to_string()])?;
        let realtime = RealTime::new(Box::new(inner), Duration::from_secs_f64(self.period));
        let missed = realtime.missed();
        rt.attach(seam, Box::new(realtime)).unwrap();
        let trace = rt.advance_recording(self.samples as f64 * self.period, self.period / 4.0, 1, &[speed]).unwrap();
        let s = trace.column(0).to_vec();
        let third = s.len() / 3;
        let rms = |x: &[f64]| (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt();
        let growth_rate = (rms(&s[s.len() - third..]) / rms(&s[..third])).ln() / (trace.time[s.len() - third] - trace.time[0]);
        Ok(Outcome { missed_fraction: missed.load(Ordering::Relaxed) as f64 / self.samples as f64, growth_rate, time: trace.time.clone(), speed: s })
    }
}

pub fn run() -> Report {
    let mut report = Report::new("missed-deadlines");
    let registry = registry();
    let base = Deadline::default();
    report.measure("sample period = deadline (s)", base.period);
    let (zero, one) = Deadline::limits(base.period);
    report.measure("critical loop gain at zero latency", zero);
    report.measure("critical loop gain at one sample of latency", one);
    report.measure("loop gain used (between the two)", base.loop_gain);
    let cases = [0.0, 0.002, 0.005, 0.015, 0.025];
    let mut boundary_low = None;
    let mut boundary_high = None;
    for busy in cases {
        let case = Deadline { busy, ..base };
        match case.run(&registry) {
            Ok(outcome) => {
                let label = format!("compute time {:.0} ms", busy * 1.0e3);
                report.series(&format!("speed (rad/s), {label}"), &outcome.time, &outcome.speed, 400);
                report.measure(&format!("{label}: missed deadlines (fraction of samples)"), outcome.missed_fraction);
                report.measure(&format!("{label}: growth rate (1/s)"), outcome.growth_rate);
                if busy < base.period {
                    report.below(&format!("{label}: faster than the period, deadlines kept"), outcome.missed_fraction, 0.2);
                    report.below(&format!("{label}: the loop decays"), outcome.growth_rate, 0.0);
                    boundary_low = Some(busy);
                } else {
                    report.above(&format!("{label}: slower than the period, deadlines missed"), outcome.missed_fraction, 0.8);
                    report.above(&format!("{label}: the loop grows"), outcome.growth_rate, 0.0);
                    if boundary_high.is_none() {
                        boundary_high = Some(busy);
                    }
                }
            }
            Err(e) => {
                report.holds(&format!("python3 available ({e})"), false);
            }
        }
    }
    report.measure("last compute time that kept up (s)", boundary_low.unwrap_or(f64::NAN));
    report.measure("first compute time that fell behind (s)", boundary_high.unwrap_or(f64::NAN));
    report.holds("the boundary is the sample period", matches!((boundary_low, boundary_high), (Some(lo), Some(hi)) if lo < base.period && hi >= base.period));
    report
}

//! 1. Kapitza's inverted pendulum — `mechanical`.
//!
//! A pendulum on a pivot whose vertical acceleration is a sinusoidal
//! signal. Fast enough, the inverted position becomes stable and the bob
//! oscillates slowly about it at the frequency the averaged equations predict.

use crate::world::{record, registry, runtime};
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_control::elements as ctl;
use sim_domain_multibody::elements as mb;
use sim_dynamics::analysis::{max_abs, period};
use std::f64::consts::TAU;

#[derive(Clone, Copy)]
pub struct DrivenPendulum {
    pub length: f64,
    pub gravity: f64,
    pub drive_amplitude: f64,
    pub drive_frequency: f64,
}

impl DrivenPendulum {
    pub fn inverted_is_stable(&self) -> bool {
        (self.drive_amplitude * self.drive_frequency).powi(2) > 2.0 * self.gravity * self.length
    }
    pub fn slow_frequency(&self) -> f64 {
        let (a, w, l, g) = (self.drive_amplitude, self.drive_frequency, self.length, self.gravity);
        ((a * w).powi(2) / (2.0 * l * l) - g / l).sqrt()
    }
    /// Pivot height `a·cos(Ωt)` ⇒ acceleration `−aΩ²·cos(Ωt)`, fed as a signal.
    pub fn model(&self, registry: &BehaviorRegistry, initial_angle: f64) -> (Runtime, StateId) {
        let mut m = ModelWorld::default();
        let drive = m.part(registry, "pivot drive", ctl::SINE, [("amplitude", -self.drive_amplitude * self.drive_frequency.powi(2)), ("frequency", self.drive_frequency / TAU)]).unwrap();
        let pendulum = m.part(registry, "pendulum", mb::DRIVEN_PENDULUM, [("length", self.length), ("gravity", self.gravity), ("initial.angle", initial_angle)]).unwrap();
        m.connect([drive.port("value"), pendulum.port("pivot_acceleration")]);
        let runtime = runtime(m, registry);
        let angle = runtime.state_id(pendulum.behavior, "angle");
        (runtime, angle)
    }
}

pub struct Outcome {
    pub excursion: f64,
    pub slow_period: Option<f64>,
    pub time: Vec<f64>,
    pub angle: Vec<f64>,
}

pub fn shake(pendulum: DrivenPendulum, registry: &BehaviorRegistry, duration: f64, stroboscopic: bool) -> Outcome {
    let (mut rt, angle) = pendulum.model(registry, 0.1);
    let step = TAU / pendulum.drive_frequency / 200.0;
    let trace = record(&mut rt, duration, step, if stroboscopic { 200 } else { 2 }, &[angle]);
    let angle = trace.column(0);
    Outcome { excursion: max_abs(&angle), slow_period: period(&trace.time, &angle), time: trace.time.clone(), angle }
}

pub fn run() -> Report {
    let mut report = Report::new("kapitza-pendulum");
    let registry = registry();
    let fast = DrivenPendulum { length: 0.2, gravity: 9.81, drive_amplitude: 0.01, drive_frequency: TAU * 50.0 };
    report.measure("a²Ω² (m²/s²)", (fast.drive_amplitude * fast.drive_frequency).powi(2)).measure("2gL (m²/s²)", 2.0 * fast.gravity * fast.length).measure("predicted slow frequency (rad/s)", fast.slow_frequency());
    report.holds("fast drive satisfies a²Ω² > 2gL", fast.inverted_is_stable());

    let full = shake(fast, &registry, 1.5, false);
    report.series("angle at full rate, first 1.5 s", &full.time, &full.angle, 3000);
    report.series("pivot height, first 1.5 s", &full.time, &full.time.iter().map(|t| fast.drive_amplitude * (fast.drive_frequency * t).cos()).collect::<Vec<_>>(), 3000);
    let upright = shake(fast, &registry, 12.0, true);
    report.series("stroboscopic angle, fast drive", &upright.time, &upright.angle, 1000);
    report.below("fast drive: bob stays near upright", upright.excursion, 0.3);
    let measured = upright.slow_period.map(|p| TAU / p).unwrap_or(0.0);
    report.within("slow oscillation frequency", measured, fast.slow_frequency(), 0.03);

    let slow = DrivenPendulum { drive_frequency: TAU * 20.0, ..fast };
    report.holds("slow drive violates a²Ω² > 2gL", !slow.inverted_is_stable());
    let fallen = shake(slow, &registry, 3.0, true);
    report.series("stroboscopic angle, slow drive", &fallen.time, &fallen.angle, 1000);
    report.above("slow drive: bob falls", fallen.excursion, 1.0);
    report
}

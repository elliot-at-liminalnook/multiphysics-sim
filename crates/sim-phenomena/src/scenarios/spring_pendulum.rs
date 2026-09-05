//! 11. The 2:1 spring pendulum — `mechanical`.
//!
//! A planar point mass under gravity on a spring anchored at the origin.
//! With the bounce at twice the swing frequency a pure bounce becomes a
//! swing and back.

use crate::world::{record, registry, runtime};
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_multibody::planar as pl;
use sim_dynamics::analysis::{max_abs, period};

#[derive(Clone, Copy)]
pub struct SpringPendulum {
    pub mass: f64,
    pub stiffness: f64,
    pub rest_length: f64,
    pub gravity: f64,
}

impl SpringPendulum {
    pub fn hanging_length(&self) -> f64 {
        self.rest_length + self.mass * self.gravity / self.stiffness
    }
    pub fn bounce_frequency(&self) -> f64 {
        (self.stiffness / self.mass).sqrt()
    }
    pub fn swing_frequency(&self) -> f64 {
        (self.gravity / self.hanging_length()).sqrt()
    }
    pub fn tuned(mass: f64, rest_length: f64, gravity: f64, ratio: f64) -> Self {
        Self { mass, stiffness: mass * gravity * (ratio * ratio - 1.0) / rest_length, rest_length, gravity }
    }
    /// y is measured downward from the pivot at the origin, so gravity acts
    /// along +y and the spring's anchor is (0, 0).
    pub fn model(&self, registry: &BehaviorRegistry, bounce_amplitude: f64) -> (Runtime, StateId, StateId) {
        let l = self.hanging_length();
        let mut m = ModelWorld::default();
        let bob = m.part(registry, "bob", pl::POINT_MASS, [("mass", self.mass), ("gravity", -self.gravity), ("initial.x", 1.0e-4 * l), ("initial.y", l + bounce_amplitude)]).unwrap();
        let spring = m.part(registry, "spring", pl::ANCHORED_SPRING, [("stiffness", self.stiffness), ("rest_length", self.rest_length)]).unwrap();
        m.connect([bob.port("node"), spring.port("node")]);
        let runtime = runtime(m, registry);
        let x = runtime.across_lane_id(bob.port("node"), 0);
        let y = runtime.across_lane_id(bob.port("node"), 1);
        (runtime, x, y)
    }
}

pub struct Outcome {
    pub lateral_peak: f64,
    pub time: Vec<f64>,
    pub lateral: Vec<f64>,
    pub vertical: Vec<f64>,
}

pub fn bounce(pendulum: SpringPendulum, registry: &BehaviorRegistry, bounce_amplitude: f64, duration: f64) -> Outcome {
    let l = pendulum.hanging_length();
    let (mut rt, x, y) = pendulum.model(registry, bounce_amplitude);
    let trace = record(&mut rt, duration, 1.0e-3, 10, &[x, y]);
    let lateral = trace.column(0);
    Outcome { lateral_peak: max_abs(&lateral), time: trace.time.clone(), lateral, vertical: trace.map(|_, s| s[1] - l) }
}

pub fn run() -> Report {
    let mut report = Report::new("spring-pendulum");
    let registry = registry();
    let (m, rest, g) = (1.0, 0.5, 9.81);
    let tuned = SpringPendulum::tuned(m, rest, g, 2.0);
    let amplitude = 0.1 * tuned.hanging_length();
    report.measure("bounce / swing frequency ratio", tuned.bounce_frequency() / tuned.swing_frequency()).measure("bounce amplitude (m)", amplitude);
    report.close("tuned to k/m = 4g/L", tuned.bounce_frequency() / tuned.swing_frequency(), 2.0, 1.0e-9);

    let resonant = bounce(tuned, &registry, amplitude, 120.0);
    report.series("lateral x, tuned 2:1", &resonant.time, &resonant.lateral, 3000);
    report.series("vertical bounce, tuned 2:1", &resonant.time, &resonant.vertical, 3000);
    report.above("tuned: bounce becomes swing", resonant.lateral_peak, amplitude);
    let peak_index = resonant.lateral.iter().map(|x| x.abs()).enumerate().fold((0, 0.0), |m, (i, v)| if v > m.1 { (i, v) } else { m }).0;
    report.above("tuned: bounce recovers after the swing", max_abs(&resonant.vertical[peak_index..]), 0.7 * amplitude);
    if let (Some(b), Some(s)) = (period(&resonant.time, &resonant.vertical), period(&resonant.time, &resonant.lateral)) {
        report.within("observed swing period is twice the bounce period", s / b, 2.0, 0.02);
    }
    let detuned = bounce(SpringPendulum::tuned(m, rest, g, 2.2), &registry, amplitude, 120.0);
    report.series("lateral x, detuned 10%", &detuned.time, &detuned.lateral, 3000);
    report.below("10% detuned: transfer collapses", detuned.lateral_peak, 0.3 * resonant.lateral_peak);
    let far = bounce(SpringPendulum::tuned(m, rest, g, 3.0_f64.sqrt()), &registry, amplitude, 120.0);
    report.below("k/m = 3g/L: swing stays at perturbation level", far.lateral_peak, 0.02 * amplitude);
    report
}

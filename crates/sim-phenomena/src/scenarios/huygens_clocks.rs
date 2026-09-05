//! 2. Huygens' coupled clocks — `mechanical` `structural`.
//!
//! A beam (translational mass, spring and damper to ground) carrying two
//! escapement pendulums. Whatever their starting phases they lock in
//! anti-phase; a rigid beam (no translational node motion) leaves them alone.

use crate::world::{record, registry, runtime};
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_multibody::elements as mb;
use sim_domain_translational::elements as tr;
use sim_dynamics::analysis::{max_abs, upward_crossings};
use std::f64::consts::PI;

#[derive(Clone, Copy)]
pub struct ClocksOnBeam {
    pub pendulum_mass: f64,
    pub pendulum_length: f64,
    pub pendulum_damping: f64,
    pub escapement_kick: f64,
    pub beam_mass: f64,
    pub beam_stiffness: f64,
    pub beam_damping: f64,
    pub gravity: f64,
    pub rigid: bool,
}

impl Default for ClocksOnBeam {
    fn default() -> Self {
        let mass = 0.5;
        let length = 0.994;
        let omega = (9.81_f64 / length).sqrt();
        let damping = mass * length * length * omega / 100.0;
        let (beam_mass, beam_frequency, beam_zeta) = (5.0, 1.0, 0.5);
        Self {
            pendulum_mass: mass,
            pendulum_length: length,
            pendulum_damping: damping,
            escapement_kick: PI * damping * 0.1 / 2.0 / (mass * length * length),
            beam_mass,
            beam_stiffness: beam_mass * (2.0 * PI * beam_frequency).powi(2),
            beam_damping: 2.0 * beam_zeta * beam_mass * 2.0 * PI * beam_frequency,
            gravity: 9.81,
            rigid: false,
        }
    }
}

pub struct Clocks {
    pub runtime: Runtime,
    pub beam: StateId,
    pub angles: [StateId; 2],
    pub rates: [StateId; 2],
}

impl ClocksOnBeam {
    pub fn pendulum_period(&self) -> f64 {
        2.0 * PI * (self.pendulum_length / self.gravity).sqrt()
    }
    pub fn model(&self, registry: &BehaviorRegistry, initial_phase: f64) -> Clocks {
        let amplitude = 0.1;
        let omega = 2.0 * PI / self.pendulum_period();
        let mut m = ModelWorld::default();
        let beam = m.part(registry, "beam", tr::MASS, [("mass", self.beam_mass)]).unwrap();
        let spring = m.part(registry, "beam spring", tr::SPRING, [("stiffness", self.beam_stiffness)]).unwrap();
        let damper = m.part(registry, "beam damper", tr::DAMPER, [("damping", self.beam_damping)]).unwrap();
        let wall = m.part(registry, "wall", tr::GROUND, []).unwrap();
        let pendulum = |m: &mut ModelWorld, name: &str, angle: f64, rate: f64| {
            m.part(registry, name, mb::PENDULUM_ON_CART, [
                ("mass", self.pendulum_mass), ("length", self.pendulum_length), ("damping", self.pendulum_damping), ("gravity", self.gravity),
                ("escapement_kick", self.escapement_kick), ("initial.angle", angle), ("initial.rate", rate),
            ]).unwrap()
        };
        let clock1 = pendulum(&mut m, "clock 1", amplitude, 0.0);
        let clock2 = pendulum(&mut m, "clock 2", amplitude * initial_phase.cos(), -amplitude * omega * initial_phase.sin());
        if self.rigid {
            // The clocks hang from the wall itself: the beam node cannot move.
            m.connect([wall.port("axis"), clock1.port("cart"), clock2.port("cart"), spring.port("b"), damper.port("b")]);
            m.connect([beam.port("axis"), spring.port("a"), damper.port("a")]);
        } else {
            m.connect([beam.port("axis"), spring.port("a"), damper.port("a"), clock1.port("cart"), clock2.port("cart")]);
            m.connect([spring.port("b"), damper.port("b"), wall.port("axis")]);
        }
        let runtime = runtime(m, registry);
        let beam_id = runtime.across_id(if self.rigid { wall.port("axis") } else { beam.port("axis") });
        let angles = [runtime.state_id(clock1.behavior, "angle"), runtime.state_id(clock2.behavior, "angle")];
        let rates = [runtime.state_id(clock1.behavior, "rate"), runtime.state_id(clock2.behavior, "rate")];
        Clocks { runtime, beam: beam_id, angles, rates }
    }
}

pub struct Outcome {
    pub phase_difference: f64,
    pub beam_excursion: f64,
    pub amplitudes: (f64, f64),
    pub kicks: usize,
    pub phase_time: Vec<f64>,
    pub phase_trace: Vec<f64>,
    pub trace: sim_dynamics::Trace,
}

fn phase_between(period: f64, t1: &[f64], t2: &[f64]) -> Vec<(f64, f64)> {
    t1.iter().filter_map(|a| {
        let b = t2.iter().min_by(|p, q| (*p - a).abs().total_cmp(&(*q - a).abs()))?;
        let phase = (b - a) / period * 2.0 * PI;
        Some((*a, (phase + PI).rem_euclid(2.0 * PI) - PI))
    }).collect()
}

pub fn tick(clocks: ClocksOnBeam, registry: &BehaviorRegistry, initial_phase: f64, duration: f64) -> Outcome {
    let mut c = clocks.model(registry, initial_phase);
    let ids = [c.beam, c.angles[0], c.angles[1], c.rates[0], c.rates[1]];
    let trace = record(&mut c.runtime, duration, 4.0e-3, 5, &ids);
    let period = clocks.pendulum_period();
    let c1 = upward_crossings(&trace.time, &trace.column(1), 0.0);
    let c2 = upward_crossings(&trace.time, &trace.column(2), 0.0);
    let phases = phase_between(period, &c1, &c2);
    let last = &phases[phases.len().saturating_sub(20)..];
    let tail = trace.after(duration - 100.0);
    Outcome {
        phase_difference: last.iter().map(|(_, p)| p.abs()).sum::<f64>() / last.len() as f64,
        beam_excursion: max_abs(&tail.column(0)),
        amplitudes: (max_abs(&tail.column(1)), max_abs(&tail.column(2))),
        kicks: c.runtime.events(),
        phase_time: phases.iter().map(|(t, _)| *t).collect(),
        phase_trace: phases.iter().map(|(_, p)| p.abs()).collect(),
        trace,
    }
}

pub fn run() -> Report {
    let mut report = Report::new("huygens-clocks");
    let registry = registry();
    let clocks = ClocksOnBeam::default();
    report.measure("pendulum period (s)", clocks.pendulum_period());
    for (label, start) in [("near in-phase", 0.5), ("quadrature", PI / 2.0)] {
        let outcome = tick(clocks, &registry, start, 2400.0);
        report.series(&format!("|phase difference| from {label} start"), &outcome.phase_time, &outcome.phase_trace, 1200);
        report.measure(&format!("final |phase difference| from {label}"), outcome.phase_difference);
        report.measure(&format!("final amplitudes from {label}: clock 1"), outcome.amplitudes.0);
        report.measure(&format!("final amplitudes from {label}: clock 2"), outcome.amplitudes.1);
        report.measure(&format!("escapement kicks from {label}"), outcome.kicks as f64);
        if start == 0.5 {
            let opening = outcome.trace.after(0.0);
            let n = opening.time.partition_point(|t| *t < 12.0);
            report.series("θ₁, first 12 s", &opening.time[..n], &opening.column(1)[..n], 1200);
            report.series("θ₂, first 12 s", &opening.time[..n], &opening.column(2)[..n], 1200);
            report.series("beam x, first 12 s", &opening.time[..n], &opening.column(0)[..n], 1200);
            let closing = outcome.trace.after(2388.0);
            report.series("θ₁, last 12 s", &closing.time, &closing.column(1), 1200);
            report.series("θ₂, last 12 s", &closing.time, &closing.column(2), 1200);
            report.series("beam x, last 12 s", &closing.time, &closing.column(0), 1200);
        }
        report.close(&format!("{label}: locks in anti-phase within the hour"), outcome.phase_difference, PI, 0.05);
        report.below(&format!("{label}: beam moves by tens of micrometres once locked"), outcome.beam_excursion, 1.0e-4);
    }
    let rigid = tick(ClocksOnBeam { rigid: true, ..clocks }, &registry, 0.5, 2400.0);
    report.close("rigid beam: phase never changes", rigid.phase_difference, 0.5, 0.02);
    report
}

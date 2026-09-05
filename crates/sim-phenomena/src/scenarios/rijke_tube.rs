//! 4. The Rijke tube — `thermal` `acoustic`.
//!
//! An open duct's Galerkin modes with a tap at the gauze, a lag chain
//! delaying the tap velocity, and King's-law heat release injected back at
//! the tap. The tube sings only with the gauze in the lower half.

use crate::world::{record, registry, runtime};
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_acoustic as ac;
use sim_domain_control::elements as ctl;
use sim_dynamics::analysis::{envelope_rate, max_abs, period};

#[derive(Clone, Copy)]
pub struct RijkeTube {
    pub modes: usize,
    pub heater_position: f64,
    pub heater_power: f64,
    pub time_lag: f64,
    pub lag_stages: usize,
}

impl Default for RijkeTube {
    fn default() -> Self {
        Self { modes: 3, heater_position: 0.25, heater_power: 1.0, time_lag: 0.2, lag_stages: 8 }
    }
}

pub struct Tube {
    pub runtime: Runtime,
    pub duct: sim_core::Instance,
    pub heat: StateId,
    pub velocity: StateId,
    pub pressure_at_tap: StateId,
    /// Modal velocities η̇_j, for reconstructing p(x).
    pub eta_dot: Vec<StateId>,
}

impl RijkeTube {
    /// Modal pressure at `x` from the recorded η̇ values.
    pub fn pressure_from_modes(&self, eta_dot: &[f64], x: f64) -> f64 {
        -(0..self.modes).map(|j| { let k = (j + 1) as f64 * std::f64::consts::PI; eta_dot[j] / k * (k * x).sin() }).sum::<f64>()
    }
    pub fn model(&self, registry: &BehaviorRegistry, initial_amplitude: f64) -> Tube {
        let mut m = ModelWorld::default();
        let duct = m.part(registry, "duct", ac::DUCT_MODES, [("modes", self.modes as f64), ("tap", self.heater_position), ("initial.amplitude", initial_amplitude)]).unwrap();
        let lag = m.part(registry, "lag", ctl::LAG_CHAIN, [("stages", self.lag_stages as f64), ("delay", self.time_lag)]).unwrap();
        let heater = m.part(registry, "gauze", ac::HEAT_RELEASE, [("power", self.heater_power)]).unwrap();
        m.connect([duct.port("tap"), heater.port("tap")]);
        m.connect([duct.port("velocity"), lag.port("input")]);
        m.connect([lag.port("output"), heater.port("velocity")]);
        let runtime = runtime(m, registry);
        let heat = runtime.signal_id(heater.port("heat"));
        let velocity = runtime.signal_id(duct.port("velocity"));
        let pressure_at_tap = runtime.across_id(duct.port("tap"));
        let eta_dot = (0..self.modes).map(|j| runtime.state_id(duct.behavior, &format!("eta_dot{j}"))).collect();
        Tube { runtime, duct, heat, velocity, pressure_at_tap, eta_dot }
    }
}

pub struct Outcome {
    pub growth_rate: f64,
    pub limit_amplitude: f64,
    pub frequency: f64,
    pub rayleigh_index: f64,
    pub time: Vec<f64>,
    pub pressure: Vec<f64>,
    pub tail: sim_dynamics::Trace,
}

/// Rayleigh index ⟨p′q′⟩ at the heater over the first cycles of a small
/// oscillation: positive means heat is added in phase with pressure.
pub fn rayleigh_index(tube: RijkeTube, registry: &BehaviorRegistry) -> f64 {
    let mut t = tube.model(registry, 0.02);
    let ids = [t.pressure_at_tap, t.heat];
    let trace = record(&mut t.runtime, 10.0, 2.0e-3, 4, &ids);
    let window = trace.after(2.0);
    window.state.iter().map(|x| x[0] * x[1]).sum::<f64>() / window.len() as f64
}

pub fn sing(tube: RijkeTube, registry: &BehaviorRegistry, duration: f64) -> Outcome {
    let mut t = tube.model(registry, 1.0e-3);
    let eta0 = t.runtime.state_id(t.duct.behavior, "eta0");
    let mut ids = vec![eta0, t.pressure_at_tap, t.heat, t.velocity];
    ids.extend(t.eta_dot.iter().copied());
    let trace = record(&mut t.runtime, duration, 2.0e-3, 4, &ids);
    // Pressure at L/4 from the modal velocities.
    let p_quarter = |x: &[f64]| -(0..tube.modes).map(|j| { let k = (j + 1) as f64 * std::f64::consts::PI; x[4 + j] / k * (k * 0.25).sin() }).sum::<f64>();
    let pressure = trace.map(|_, x| p_quarter(x));
    let early = trace.after(2.0);
    let early_end = early.time.partition_point(|t| *t < 14.0);
    let early_pressure = early.map(|_, x| p_quarter(x));
    let growth_rate = envelope_rate(&early.time[..early_end], &early_pressure[..early_end]).unwrap_or(0.0);
    let tail = trace.after(duration - 20.0);
    let tail_pressure = tail.map(|_, x| p_quarter(x));
    let frequency = period(&tail.time, &tail.column(0)).map(|p| 1.0 / p).unwrap_or(0.0);
    Outcome { growth_rate, limit_amplitude: max_abs(&tail_pressure), frequency, rayleigh_index: rayleigh_index(tube, registry), time: trace.time.clone(), pressure, tail }
}

pub fn run() -> Report {
    let mut report = Report::new("rijke-tube");
    let registry = registry();
    let base = RijkeTube::default();

    let quarter = sing(base, &registry, 80.0);
    report.series("pressure at L/4, heater at L/4", &quarter.time, &quarter.pressure, 3000);
    {
        let cycle = quarter.tail.after(74.0);
        report.series("limit cycle: heat release vs pressure", &cycle.column(1), &cycle.column(2), 1500);
        report.series("acoustic velocity at heater, last 6 s", &cycle.time, &cycle.column(3), 1500);
        report.series("heat release, last 6 s", &cycle.time, &cycle.column(2), 1500);
    }
    report.measure("growth rate, heater at L/4", quarter.growth_rate).measure("limit-cycle pressure amplitude", quarter.limit_amplitude).measure("Rayleigh index ⟨p′q′⟩, heater at L/4", quarter.rayleigh_index);
    report.above("heater at L/4: grows", quarter.growth_rate, 0.05);
    report.above("heater at L/4: sustained tone", quarter.limit_amplitude, 0.3);
    report.within("tone at the fundamental f = c/2L", quarter.frequency, 0.5, 0.05);
    report.above("Rayleigh index positive where it sings", quarter.rayleigh_index, 0.0);

    let mut rates = Vec::new();
    for position in [0.10, 0.15, 0.25, 0.35, 0.45] {
        let outcome = sing(RijkeTube { heater_position: position, ..base }, &registry, 30.0);
        report.measure(&format!("growth rate, heater at {position} L"), outcome.growth_rate);
        rates.push((position, outcome.growth_rate));
    }
    report.series("growth rate vs heater position", &rates.iter().map(|(p, _)| *p).collect::<Vec<_>>(), &rates.iter().map(|(_, r)| *r).collect::<Vec<_>>(), 10);
    let strongest = rates.iter().fold((0.0, f64::NEG_INFINITY), |m, r| if r.1 > m.1 { *r } else { m });
    report.close("strongest growth near L/4", strongest.0, 0.25, 0.0);

    let middle = sing(RijkeTube { heater_position: 0.5, ..base }, &registry, 40.0);
    report.below("heater at L/2: silent", middle.limit_amplitude, 1.0e-3);
    let upper = sing(RijkeTube { heater_position: 0.75, ..base }, &registry, 40.0);
    report.series("pressure at L/4, heater at 3L/4", &upper.time, &upper.pressure, 3000);
    report.below("heater at 3L/4: silent", upper.limit_amplitude, 1.0e-4);
    report.below("Rayleigh index negative where it is silent", upper.rayleigh_index, 0.0);
    report
}

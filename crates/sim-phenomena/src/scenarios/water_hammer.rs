//! 5. Water hammer — `hydraulic` `structural`.
//!
//! A reservoir, a pipe of compressible volumes and fluid inertances, and a
//! closing valve to an outlet. Close faster than the round-trip wave time
//! and the pressure at the valve jumps by ρ·c·Δv.

use crate::world::{record, registry, runtime};
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_hydraulic as hy;
use sim_dynamics::analysis::{max, mean, period};

#[derive(Clone, Copy)]
pub struct Pipeline {
    pub cells: usize,
    pub length: f64,
    pub area: f64,
    pub density: f64,
    pub wave_speed: f64,
    pub reservoir_pressure: f64,
    pub outlet_pressure: f64,
    pub closure_time: f64,
}

impl Pipeline {
    pub fn new(cells: usize, closure_time: f64) -> Self {
        Self { cells, length: 100.0, area: 7.854e-3, density: 1000.0, wave_speed: 1200.0, reservoir_pressure: 2.0e5, outlet_pressure: 1.0e5, closure_time }
    }
    pub fn initial_velocity(&self) -> f64 {
        2.0
    }
    pub fn joukowsky(&self) -> f64 {
        self.density * self.wave_speed * self.initial_velocity()
    }
    pub fn round_trip_time(&self) -> f64 {
        2.0 * self.length / self.wave_speed
    }
    pub fn valve_conductance(&self) -> f64 {
        self.area * self.initial_velocity() / (self.reservoir_pressure - self.outlet_pressure)
    }
}

pub struct Pipe {
    pub runtime: Runtime,
    pub pressures: Vec<StateId>,
    pub valve_pressure: StateId,
    pub seated: StateId,
}

impl Pipeline {
    /// Reservoir → half inertance → [volume, inertance]×N … → valve → outlet.
    pub fn model(&self, registry: &BehaviorRegistry) -> Pipe {
        let n = self.cells;
        let dx = self.length / n as f64;
        let compliance = self.area * dx / (self.density * self.wave_speed * self.wave_speed);
        let inertance = self.density * dx / self.area;
        let flow = self.area * self.initial_velocity();
        let mut m = ModelWorld::default();
        let reservoir = m.part(registry, "reservoir", hy::RESERVOIR, [("pressure", self.reservoir_pressure)]).unwrap();
        let outlet = m.part(registry, "outlet", hy::RESERVOIR, [("pressure", self.outlet_pressure)]).unwrap();
        let mut volumes = Vec::new();
        let mut faces = Vec::new();
        for i in 0..n {
            volumes.push(m.part(registry, &format!("cell{i}"), hy::VOLUME, [("compliance", compliance), ("initial.pressure", self.reservoir_pressure)]).unwrap());
        }
        for i in 0..n {
            let l = if i == 0 { 0.5 * inertance } else { inertance };
            faces.push(m.part(registry, &format!("face{i}"), hy::INERTANCE, [("inertance", l), ("initial.flow", flow)]).unwrap());
        }
        // The valve carries the last half cell of water column.
        let valve = m.part(registry, "valve", hy::VALVE, [("conductance", self.valve_conductance()), ("closure_time", self.closure_time), ("inertance", 0.5 * inertance), ("initial.flow", flow)]).unwrap();
        m.connect([reservoir.port("port"), faces[0].port("a")]);
        for i in 0..n {
            let mut ports = vec![faces[i].port("b"), volumes[i].port("port")];
            ports.push(if i + 1 < n { faces[i + 1].port("a") } else { valve.port("a") });
            m.connect(ports);
        }
        m.connect([valve.port("b"), outlet.port("port")]);
        let runtime = runtime(m, registry);
        let pressures = volumes.iter().map(|v| runtime.across_id(v.port("port"))).collect::<Vec<_>>();
        let valve_pressure = runtime.across_id(valve.port("a"));
        let seated = runtime.state_id(valve.behavior, "seated");
        Pipe { runtime, pressures, valve_pressure, seated }
    }
}

pub struct Outcome {
    pub plateau_rise: f64,
    pub peak_rise: f64,
    pub oscillation_period: Option<f64>,
    pub time: Vec<f64>,
    pub valve_pressure: Vec<f64>,
    pub profiles: Vec<(f64, Vec<f64>)>,
}

pub fn slam(pipe: Pipeline, registry: &BehaviorRegistry, duration: f64) -> Outcome {
    let mut model = pipe.model(registry);
    let mut ids = vec![model.valve_pressure];
    ids.extend(model.pressures.iter().copied());
    let trace = record(&mut model.runtime, duration, 1.0e-3, 1, &ids);
    let valve_pressure = trace.column(0);
    let closure = pipe.closure_time;
    let round_trip = pipe.round_trip_time();
    let plateau = trace.time.iter().zip(&valve_pressure).filter(|(t, _)| **t > closure + 0.25 * round_trip && **t < closure + 0.75 * round_trip).map(|(_, p)| *p - pipe.reservoir_pressure).collect::<Vec<_>>();
    let after = trace.after(closure);
    let profiles = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5].iter().map(|f| {
        let t = closure + f * round_trip;
        let index = trace.time.partition_point(|s| *s < t).min(trace.len() - 1);
        (*f, trace.state[index][1..].to_vec())
    }).collect();
    Outcome {
        plateau_rise: mean(&plateau),
        peak_rise: max(&valve_pressure) - pipe.reservoir_pressure,
        oscillation_period: period(&after.time, &after.column(0)),
        time: trace.time.clone(),
        valve_pressure,
        profiles,
    }
}

pub fn run() -> Report {
    let mut report = Report::new("water-hammer");
    let registry = registry();
    let fast = Pipeline::new(40, 0.02);
    let joukowsky = fast.joukowsky();
    let round_trip = fast.round_trip_time();
    report.measure("ρ·c·Δv (Pa)", joukowsky).measure("round-trip time 2L/c (s)", round_trip);
    report.holds("fast closure is faster than 2L/c", fast.closure_time < round_trip);

    let sudden = slam(fast, &registry, 1.2);
    report.series("valve pressure, fast closure", &sudden.time, &sudden.valve_pressure, 1200);
    let positions = (0..fast.cells).map(|i| (i as f64 + 0.5) * fast.length / fast.cells as f64).collect::<Vec<_>>();
    for (f, profile) in &sudden.profiles {
        report.series(&format!("pressure along pipe at t = closure + {f}·2L/c"), &positions, profile, 200);
    }
    report.measure("plateau pressure rise (Pa)", sudden.plateau_rise);
    report.within("plateau rise matches Joukowsky", sudden.plateau_rise, joukowsky, 0.03);
    report.above("peak is many times the static head", sudden.peak_rise / (fast.reservoir_pressure - fast.outlet_pressure), 10.0);
    if let Some(p) = sudden.oscillation_period {
        report.measure("pressure oscillation period (s)", p);
        report.within("oscillation period is 4L/c", p, 2.0 * round_trip, 0.03);
    } else {
        report.holds("pressure oscillation detected", false);
    }

    let slow = Pipeline::new(40, 10.0 * round_trip);
    let gentle = slam(slow, &registry, 10.0 * round_trip + 0.5);
    report.series("valve pressure, slow closure", &gentle.time, &gentle.valve_pressure, 1200);
    report.measure("slow closure peak / Joukowsky", gentle.peak_rise / joukowsky);
    report.below("slow closure: spike collapses", gentle.peak_rise / joukowsky, 0.3);
    report
}

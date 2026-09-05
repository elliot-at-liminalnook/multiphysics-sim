//! 24. Janssen's silo — `granular`.
//!
//! Pour grain into a tall silo and watch the stress on its floor: it rises
//! with the fill like a fluid's would, then stops rising. The walls carry
//! the rest through friction, and the floor never feels more than
//! `ρgD/(4μK)` however much is poured — Janssen's saturation.

use crate::world::{damped_runtime, record, registry};
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_granular::{self as gr, G};
use sim_dynamics::analysis::linear_fit;

#[derive(Clone, Copy)]
pub struct Silo {
    pub diameter: f64,
    pub density: f64,
    pub friction: f64,
    pub janssen_k: f64,
    pub pour_rate: f64,
    /// Drain orifice at the floor (None: closed).
    pub orifice: Option<(f64, f64)>,
    pub initial_mass: f64,
}

impl Default for Silo {
    fn default() -> Self {
        Self { diameter: 1.0, density: 1500.0, friction: 0.4, janssen_k: 0.5, pour_rate: 100.0, orifice: None, initial_mass: 0.0 }
    }
}

pub struct Bin {
    pub runtime: Runtime,
    pub mass: StateId,
    pub base_stress: StateId,
}

impl Silo {
    pub fn column(&self) -> gr::Column {
        gr::Column { diameter: self.diameter, density: self.density, friction: self.friction, janssen_k: self.janssen_k, initial_mass: self.initial_mass }
    }
    pub fn model(&self, registry: &BehaviorRegistry) -> Bin {
        let mut m = ModelWorld::default();
        let column = m.part(registry, "silo", gr::COLUMN, [("diameter", self.diameter), ("density", self.density), ("friction", self.friction), ("janssen_k", self.janssen_k), ("initial.mass", self.initial_mass)]).unwrap();
        let hopper = m.part(registry, "hopper", gr::HOPPER, [("rate", self.pour_rate)]).unwrap();
        m.connect([hopper.port("out"), column.port("top")]);
        match self.orifice {
            Some((diameter, grain)) => {
                let orifice = m.part(registry, "orifice", gr::ORIFICE, [("diameter", diameter), ("grain", grain), ("density", self.density)]).unwrap();
                let sink = m.part(registry, "ground", gr::SINK, []).unwrap();
                m.connect([column.port("base"), orifice.port("in")]);
                m.connect([orifice.port("out"), sink.port("in")]);
            }
            None => m.connect([column.port("base")]),
        }
        // First-order filling: backward Euler keeps the floor stress consistent with the fill at every record.
        let runtime = damped_runtime(m, registry);
        let mass = runtime.state_id(column.behavior, "mass");
        let base_stress = runtime.across_id(column.port("base"));
        Bin { runtime, mass, base_stress }
    }
}

pub struct Fill {
    pub time: Vec<f64>,
    pub height: Vec<f64>,
    pub base_stress: Vec<f64>,
    pub mass: Vec<f64>,
}

pub fn pour(silo: Silo, registry: &BehaviorRegistry, duration: f64) -> Fill {
    let mut bin = silo.model(registry);
    let trace = record(&mut bin.runtime, duration, 0.5, 1, &[bin.mass, bin.base_stress]);
    let column = silo.column();
    let mass = trace.column(0);
    Fill { height: mass.iter().map(|m| column.height(*m)).collect(), base_stress: trace.column(1), time: trace.time.clone(), mass }
}

pub fn run() -> Report {
    let mut report = Report::new("janssen-silo");
    let registry = registry();
    let base = Silo::default();
    let column = base.column();
    report.measure("Janssen saturation stress ρgD/(4μK) (Pa)", column.saturation_stress());
    report.measure("Janssen depth scale D/(4μK) (m)", column.depth_scale());

    let fill = pour(base, &registry, 250.0);
    report.series("floor stress (Pa) vs time, μ = 0.4", &fill.time, &fill.base_stress, 1200);
    report.series("fill height (m) vs time, μ = 0.4", &fill.time, &fill.height, 1200);
    let final_height = *fill.height.last().unwrap();
    let final_stress = *fill.base_stress.last().unwrap();
    report.measure("fill height at the end (m)", final_height);
    report.measure("floor stress at the end (Pa)", final_stress);
    report.measure("a fluid that deep would press (Pa)", base.density * G * final_height);
    report.within("floor stress saturates at Janssen's ρgD/(4μK)", final_stress, column.saturation_stress(), 1.0e-3);
    // The approach: ln(1 − σ/σ_sat) = −h/λ.
    let points: Vec<(f64, f64)> = fill.height.iter().zip(&fill.base_stress).filter(|(h, s)| **h > 0.1 && **s < 0.98 * column.saturation_stress()).map(|(h, s)| (*h, (1.0 - s / column.saturation_stress()).ln())).collect();
    let slope = linear_fit(&points).map(|(m, _)| m).unwrap_or(f64::NAN);
    report.measure("depth scale from the fill (m)", -1.0 / slope);
    report.within("the approach to saturation has Janssen's depth scale", -1.0 / slope, column.depth_scale(), 1.0e-2);
    // Twice the grain, no more load.
    let doubled = pour(Silo { pour_rate: 2.0 * base.pour_rate, ..base }, &registry, 250.0);
    report.measure("twice the grain: floor stress (Pa)", *doubled.base_stress.last().unwrap());
    report.within("adding grain adds no load", *doubled.base_stress.last().unwrap(), final_stress, 5.0e-3);

    // Falsifier: frictionless walls, hydrostatic at every depth.
    let fluid = pour(Silo { friction: 0.0, ..base }, &registry, 250.0);
    report.series("floor stress (Pa) vs time, μ = 0", &fluid.time, &fluid.base_stress, 1200);
    let worst = fluid.height.iter().zip(&fluid.base_stress).filter(|(h, _)| **h > 0.5).map(|(h, s)| (s / (base.density * G * h) - 1.0).abs()).fold(0.0, f64::max);
    report.below("μ = 0: hydrostatic at every depth", worst, 1.0e-6);

    // Beverloo: draining through an orifice, the rate does not care how full the silo is.
    let full = Silo { initial_mass: 1500.0 * std::f64::consts::FRAC_PI_4 * 15.0, pour_rate: 0.0, orifice: Some((0.1, 0.005)), ..base };
    let drain = pour(full, &registry, 300.0);
    let rates: Vec<f64> = drain.mass.windows(2).map(|w| (w[0] - w[1]) / 0.5).collect();
    let (early, late) = (rates[10], rates[rates.len() / 2]);
    report.measure("Beverloo drain rate (kg/s), silo nearly full", early);
    report.measure("Beverloo drain rate (kg/s), silo half empty", late);
    report.close("the hourglass principle: the drain rate ignores the fill", late, early, 1.0e-3 * early.abs().max(1.0e-9));
    let jammed = Silo { orifice: Some((0.03, 0.025)), ..full };
    let stuck = pour(jammed, &registry, 50.0);
    report.close("an opening under 1.5 grains jams", *stuck.mass.last().unwrap(), stuck.mass[0], 1.0e-9);
    report
}

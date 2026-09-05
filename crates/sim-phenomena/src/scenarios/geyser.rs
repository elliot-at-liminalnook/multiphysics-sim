//! 20. The geyser — `fluid` (two-phase) `thermal`.
//!
//! A vertical water column heated at the bottom and fed from an aquifer.
//! The weight of the water above holds the bottom below its boiling point
//! until it finally boils; the first steam lightens the column, the
//! pressure on the fluid below drops, and the whole column flashes and
//! erupts. Refilled with cold water, it starts over. The period falls as
//! the heat rises — the energy to bring the refill to boiling is the clock.

use crate::world::{damped_runtime, record, registry};
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_fluid::twophase::{self as tp, Water, ATMOSPHERE, G};
use sim_domain_thermal as th;
use sim_dynamics::analysis::linear_fit;

#[derive(Clone, Copy)]
pub struct Geyser {
    pub segments: usize,
    pub segment_height: f64,
    pub diameter: f64,
    /// Heat into the bottom segment (W).
    pub heat: f64,
    /// Aquifer head above the column top (m of water).
    pub recharge_head: f64,
    pub recharge_conductance: f64,
    pub recharge_temperature: f64,
    pub pool_temperature: f64,
    /// Falsifier: lay the column flat so the water above weighs nothing.
    pub flat: bool,
    /// The open basin at the top: rim height above its floor, and its area.
    pub basin_height: f64,
    pub basin_area: f64,
}

impl Default for Geyser {
    fn default() -> Self {
        Self { segments: 5, segment_height: 2.0, diameter: 0.3, heat: 1.0e5, recharge_head: 0.5, recharge_conductance: 2.2e-5, recharge_temperature: 293.15, pool_temperature: 293.15, flat: false, basin_height: 1.0, basin_area: 0.5 }
    }
}

pub struct Conduit {
    pub runtime: Runtime,
    pub pressures: Vec<StateId>,
    pub enthalpies: Vec<StateId>,
    /// Spill over the basin's rim (kg/s).
    pub outflow: StateId,
    pub basin_mass: StateId,
    pub basin_pressure: StateId,
    pub basin_enthalpy: StateId,
}

impl Geyser {
    pub fn area(&self) -> f64 {
        std::f64::consts::FRAC_PI_4 * self.diameter * self.diameter
    }
    pub fn height(&self) -> f64 {
        self.segments as f64 * self.segment_height
    }
    pub fn model(&self, registry: &BehaviorRegistry) -> Conduit {
        let rise = if self.flat { 0.0 } else { self.segment_height };
        let column = if self.flat { 0.0 } else { self.height() };
        let rho = Water::liquid_density(self.pool_temperature, ATMOSPHERE);
        let cold = Water::liquid_enthalpy(self.recharge_temperature);
        let mut m = ModelWorld::default();
        let mut volumes = Vec::new();
        let mut nodes: Vec<Vec<sim_core::PortId>> = Vec::new();
        for k in 0..self.segments {
            let depth = column - (k as f64 + 0.5) * rise + self.basin_height;
            let volume = m.part(registry, &format!("segment {k}"), tp::VOLUME_PH, [
                ("volume", self.area() * self.segment_height), ("initial.pressure", ATMOSPHERE + rho * G * depth), ("initial.enthalpy", cold),
            ]).unwrap();
            nodes.push(vec![volume.port("node")]);
            volumes.push(volume);
        }
        for k in 0..self.segments - 1 {
            let pipe = m.part(registry, &format!("throat {k}"), tp::PIPE_PH, [("length", self.segment_height), ("diameter", self.diameter), ("friction", 0.03), ("rise", rise)]).unwrap();
            nodes[k].push(pipe.port("a"));
            nodes[k + 1].push(pipe.port("b"));
        }
        // The basin at the top: an open tank with a free surface, initially
        // full to the rim, that spills what the column pushes up.
        let basin_level = if self.flat { self.basin_height } else { self.basin_height };
        let basin = m.part(registry, "basin", tp::TANK_PH, [
            ("area", self.basin_area), ("height", self.basin_height), ("initial.level", basin_level), ("initial.enthalpy", Water::liquid_enthalpy(self.pool_temperature)),
        ]).unwrap();
        let mouth = m.part(registry, "mouth", tp::PIPE_PH, [("length", 0.5 * self.segment_height), ("diameter", self.diameter), ("friction", 0.03), ("rise", 0.5 * rise)]).unwrap();
        nodes[self.segments - 1].push(mouth.port("a"));
        m.connect([mouth.port("b"), basin.port("bottom")]);
        // Recharge from the aquifer into the bottom segment.
        let aquifer = m.part(registry, "aquifer", tp::RESERVOIR_PH, [("pressure", ATMOSPHERE + rho * G * (column + self.basin_height + self.recharge_head)), ("enthalpy", cold)]).unwrap();
        let inlet = m.part(registry, "inlet", tp::VALVE_PH, [("conductance", self.recharge_conductance)]).unwrap();
        m.connect([aquifer.port("node"), inlet.port("a")]);
        nodes[0].push(inlet.port("b"));
        // Heat: a source into a small hot-rock capacitance, conducted into the bottom water.
        let burner = m.part(registry, "burner", th::HEAT_SOURCE, [("power", self.heat)]).unwrap();
        let rock = m.part(registry, "rock", th::CAPACITANCE, [("heat_capacity", 1.0e4), ("initial.temperature", self.recharge_temperature + self.heat / 1.0e4)]).unwrap();
        let wall = m.part(registry, "wall", tp::WALL_HEAT, [("conductance", 1.0e4)]).unwrap();
        m.connect([burner.port("node"), rock.port("node"), wall.port("wall")]);
        nodes[0].push(wall.port("fluid"));
        for node in nodes {
            m.connect(node);
        }
        // The column's acoustics (8 ms) are not the story; backward Euler damps them.
        let runtime = damped_runtime(m, registry);
        let pressures = volumes.iter().map(|v| runtime.state_id(v.behavior, "pressure")).collect();
        let enthalpies = volumes.iter().map(|v| runtime.state_id(v.behavior, "enthalpy")).collect();
        let outflow = runtime.signal_id(basin.port("spill"));
        let basin_mass = runtime.state_id(basin.behavior, "mass");
        let basin_pressure = runtime.state_id(basin.behavior, "pressure");
        let basin_enthalpy = runtime.state_id(basin.behavior, "enthalpy");
        Conduit { runtime, pressures, enthalpies, outflow, basin_mass, basin_pressure, basin_enthalpy }
    }
}

pub struct Cycle {
    pub time: Vec<f64>,
    pub bottom_temperature: Vec<f64>,
    pub bottom_quality: Vec<f64>,
    pub outflow: Vec<f64>,
    pub eruptions: Vec<f64>,
    pub period: Option<f64>,
    /// Mouth outflow while the column is still filling — the recharge rate.
    pub recharge_rate: f64,
    /// Hottest the bottom gets while still liquid (quality below 1 %).
    pub liquid_superheat: f64,
}

pub fn erupt(geyser: Geyser, registry: &BehaviorRegistry, duration: f64) -> Cycle {
    let mut conduit = geyser.model(registry);
    let mut ids = vec![conduit.pressures[0], conduit.enthalpies[0], conduit.outflow];
    ids.extend(conduit.pressures.iter().skip(1).copied());
    ids.extend(conduit.enthalpies.iter().skip(1).copied());
    let trace = record(&mut conduit.runtime, duration, 0.05, 10, &ids);
    let bottom_temperature = trace.map(|_, x| Water::state(x[0], x[1]).temperature);
    let bottom_quality = trace.map(|_, x| Water::state(x[0], x[1]).quality);
    let outflow: Vec<f64> = trace.column(2);
    // An eruption: the mouth's outflow rising through ten times the recharge.
    let threshold = 0.5;
    let eruptions: Vec<f64> = outflow.windows(2).enumerate().filter(|(_, w)| w[0] < threshold && w[1] >= threshold).map(|(k, _)| trace.time[k + 1]).collect();
    let period = (eruptions.len() >= 3).then(|| (eruptions[eruptions.len() - 1] - eruptions[1]) / (eruptions.len() - 2) as f64);
    let first = eruptions.first().copied().unwrap_or(duration);
    let filling: Vec<f64> = trace.time.iter().zip(&outflow).filter(|(t, _)| **t > 0.3 * first && **t < 0.8 * first).map(|(_, m)| *m).collect();
    let recharge_rate = if filling.is_empty() { f64::NAN } else { sim_dynamics::analysis::mean(&filling) };
    let liquid_superheat = bottom_temperature.iter().zip(&bottom_quality).filter(|(_, x)| **x < 0.01).map(|(t, _)| *t).fold(f64::NEG_INFINITY, f64::max);
    Cycle { time: trace.time.clone(), bottom_temperature, bottom_quality, outflow, eruptions, period, recharge_rate, liquid_superheat }
}

pub fn run() -> Report {
    let mut report = Report::new("geyser");
    let registry = registry();
    let base = Geyser::default();
    let boiling_at_bottom = Water::saturation_temperature(ATMOSPHERE + 1000.0 * G * (base.height() + base.basin_height));
    report.measure("column height (m)", base.height());
    report.measure("boiling point at the bottom (°C)", boiling_at_bottom - 273.15);
    report.measure("boiling point at the surface (°C)", Water::saturation_temperature(ATMOSPHERE) - 273.15);
    let mut points = Vec::new();
    for heat in [1.0e5, 1.5e5, 2.0e5, 3.0e5] {
        let cycle = erupt(Geyser { heat, ..base }, &registry, 3000.0);
        let label = format!("{:.0} kW", heat / 1000.0);
        if heat == 1.0e5 {
            report.series("bottom temperature (K), 100 kW", &cycle.time, &cycle.bottom_temperature, 1500);
            report.series("bottom steam quality, 100 kW", &cycle.time, &cycle.bottom_quality, 1500);
            report.series("spill over the rim (kg/s), 100 kW", &cycle.time, &cycle.outflow, 1500);
            report.measure("recharge rate (kg/s)", cycle.recharge_rate);
        }
        // The recharge carries heat away; what is left is the clock.
        let cooling = cycle.recharge_rate * Water::CP_LIQUID * (boiling_at_bottom - base.recharge_temperature);
        let first = cycle.eruptions.first().copied();
        report.measure(&format!("{label}: hottest liquid at the bottom (°C)"), cycle.liquid_superheat - 273.15);
        report.measure(&format!("{label}: time to the first eruption (s)"), first.unwrap_or(f64::NAN));
        report.measure(&format!("{label}: eruptions in 3 000 s"), cycle.eruptions.len() as f64);
        report.measure(&format!("{label}: interval between later eruptions (s)"), cycle.period.unwrap_or(f64::NAN));
        report.measure(&format!("{label}: net heat after the recharge's cooling (kW)"), (heat - cooling) / 1000.0);
        report.above(&format!("{label}: the water above holds the bottom liquid past the surface boiling point"), cycle.liquid_superheat - 273.15, 110.0);
        report.holds(&format!("{label}: erupts"), cycle.eruptions.len() >= 2);
        if let Some(first) = first {
            points.push(((heat - cooling).ln(), first.ln()));
        }
    }
    let slope = linear_fit(&points).map(|(m, _)| m).unwrap_or(f64::NAN);
    report.measure("d ln(time to first eruption) / d ln(net heat)", slope);
    report.within("the clock is the energy to bring the bottom to boiling: time ∝ 1/(net heat)", slope, -1.0, 0.2);

    // Falsifier: lay the column flat — no water above the bottom — and the
    // bottom can never store superheat: it boils at the surface boiling
    // point as soon as it gets there.
    let flat = erupt(Geyser { flat: true, ..base }, &registry, 1500.0);
    let basin_boiling = Water::saturation_temperature(ATMOSPHERE + 1000.0 * G * base.basin_height) - 273.15;
    report.series("bottom temperature (K), column laid flat", &flat.time, &flat.bottom_temperature, 1500);
    report.series("spill over the rim (kg/s), column laid flat", &flat.time, &flat.outflow, 1500);
    report.measure("flat: boiling point under the basin alone (°C)", basin_boiling);
    report.measure("flat: hottest liquid at the bottom (°C)", flat.liquid_superheat - 273.15);
    report.below("flat column: no superheat, it boils at the basin's own boiling point", flat.liquid_superheat - 273.15, basin_boiling + 1.5);
    report
}

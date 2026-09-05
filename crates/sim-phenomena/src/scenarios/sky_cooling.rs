//! 22. Cooling below ambient under the sun — `radiative` `thermal`.
//!
//! A surface that emits strongly in the 8–13 µm atmospheric window and
//! reflects nearly all sunlight faces a clear sky. Through the window it
//! radiates to space, which is far colder than the air; outside the window
//! the atmosphere radiates back at the air's temperature. Insulated from
//! convection, it settles *below* the air temperature — under direct sun.

use crate::world::{record, registry, runtime};
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_radiative::{self as rad, Band};
use sim_domain_thermal as th;

#[derive(Clone, Copy)]
pub struct Radiator {
    pub area: f64,
    pub window_emissivity: f64,
    pub outside_emissivity: f64,
    pub solar_absorptivity: f64,
    pub irradiance: f64,
    pub air_temperature: f64,
    /// Effective sky temperature seen through the window (space through a dry, clear atmosphere).
    pub window_sky_temperature: f64,
    /// Nonradiative (convective + conductive) coefficient, W/m²K.
    pub nonradiative: f64,
    pub heat_capacity: f64,
}

impl Default for Radiator {
    fn default() -> Self {
        // Raman, Anoma, Zhu, Rephaeli & Fan 2014: a selective emitter, ~3 % solar
        // absorption, ~890 W/m² sun, nonradiative coefficient ≈ 6.9 W/m²K.
        Self { area: 1.0, window_emissivity: 0.9, outside_emissivity: 0.1, solar_absorptivity: 0.03, irradiance: 890.0, air_temperature: 300.0, window_sky_temperature: 255.0, nonradiative: 6.9, heat_capacity: 5000.0 }
    }
}

pub const WINDOW: Band = Band { lo: 8.0, hi: 13.0 };
pub const BELOW: Band = Band { lo: 0.0, hi: 8.0 };
pub const ABOVE: Band = Band { lo: 13.0, hi: 1.0e9 };

pub struct Panel {
    pub runtime: Runtime,
    pub temperature: StateId,
}

impl Radiator {
    /// Net cooling power at surface temperature `t` (W): window emission to
    /// the cold sky, exchange with the opaque atmosphere elsewhere, minus
    /// absorbed sun and the nonradiative gain from the air.
    pub fn net_cooling(&self, t: f64) -> f64 {
        let window = self.window_emissivity * (WINDOW.emissive_power(t) - WINDOW.emissive_power(self.window_sky_temperature));
        let outside = self.outside_emissivity * ((BELOW.emissive_power(t) + ABOVE.emissive_power(t)) - (BELOW.emissive_power(self.air_temperature) + ABOVE.emissive_power(self.air_temperature)));
        self.area * (window + outside - self.solar_absorptivity * self.irradiance - self.nonradiative * (self.air_temperature - t))
    }
    /// Steady surface temperature from the energy balance alone.
    pub fn balance_temperature(&self) -> f64 {
        let (mut lo, mut hi) = (200.0, 400.0);
        for _ in 0..80 {
            let mid = 0.5 * (lo + hi);
            if self.net_cooling(mid) > 0.0 { hi = mid } else { lo = mid }
        }
        0.5 * (lo + hi)
    }
    pub fn model(&self, registry: &BehaviorRegistry) -> Panel {
        let mut m = ModelWorld::default();
        let slab = m.part(registry, "panel", th::CAPACITANCE, [("heat_capacity", self.heat_capacity), ("initial.temperature", self.air_temperature)]).unwrap();
        let sun = m.part(registry, "sun", th::HEAT_SOURCE, [("power", self.solar_absorptivity * self.irradiance * self.area)]).unwrap();
        let air_link = m.part(registry, "air link", th::CONDUCTANCE, [("conductance", self.nonradiative * self.area)]).unwrap();
        let air = m.part(registry, "air", th::AMBIENT, [("temperature", self.air_temperature)]).unwrap();
        let mut thermal_node = vec![slab.port("node"), sun.port("node"), air_link.port("a")];
        for (name, band, emissivity, sky) in [
            ("window", WINDOW, self.window_emissivity, self.window_sky_temperature),
            ("below the window", BELOW, self.outside_emissivity, self.air_temperature),
            ("above the window", ABOVE, self.outside_emissivity, self.air_temperature),
        ] {
            let face = m.part(registry, &format!("face, {name}"), rad::SURFACE, [("area", self.area), ("emissivity", emissivity), ("band_lo", band.lo), ("band_hi", band.hi)]).unwrap();
            let view = m.part(registry, &format!("view, {name}"), rad::VIEW, [("area", self.area), ("factor", 1.0)]).unwrap();
            let sky_node = m.part(registry, &format!("sky, {name}"), rad::SKY, [("temperature", sky), ("band_lo", band.lo), ("band_hi", band.hi)]).unwrap();
            m.connect([face.port("face"), view.port("a")]);
            m.connect([view.port("b"), sky_node.port("node")]);
            thermal_node.push(face.port("heat"));
        }
        m.connect(thermal_node);
        m.connect([air_link.port("b"), air.port("node")]);
        let runtime = runtime(m, registry);
        let temperature = runtime.across_id(slab.port("node"));
        Panel { runtime, temperature }
    }
}

pub struct Outcome {
    pub time: Vec<f64>,
    pub temperature: Vec<f64>,
    pub settled: f64,
}

pub fn settle(radiator: Radiator, registry: &BehaviorRegistry, duration: f64) -> Outcome {
    let mut panel = radiator.model(registry);
    let trace = record(&mut panel.runtime, duration, 2.0, 5, &[panel.temperature]);
    let temperature = trace.column(0);
    Outcome { settled: *temperature.last().unwrap(), time: trace.time.clone(), temperature }
}

pub fn run() -> Report {
    let mut report = Report::new("sky-cooling");
    let registry = registry();
    let base = Radiator::default();
    report.measure("window band share of a 300 K blackbody", WINDOW.fraction(300.0));
    let selective = settle(base, &registry, 6000.0);
    report.series("panel temperature (K), selective emitter under the sun", &selective.time, &selective.temperature, 1200);
    let depression = base.air_temperature - selective.settled;
    report.measure("selective emitter: settles below the air by (K)", depression);
    report.measure("energy balance alone: below the air by (K)", base.air_temperature - base.balance_temperature());
    report.within("the compiled network agrees with the energy balance", selective.settled, base.balance_temperature(), 1.0e-4);
    report.above("below ambient under direct sun", depression, 2.0);
    report.within("of the order Raman et al. measured (≈ 5 K)", depression, 4.9, 0.6);

    // Night: no sun, further below.
    let night = settle(Radiator { irradiance: 0.0, ..base }, &registry, 6000.0);
    report.measure("selective emitter at night: below the air by (K)", base.air_temperature - night.settled);
    report.above("colder still at night", (base.air_temperature - night.settled) - depression, 1.0);

    // Falsifier: a grey emitter is black in the solar band too — it absorbs the sun.
    let grey = settle(Radiator { window_emissivity: 0.9, outside_emissivity: 0.9, solar_absorptivity: 0.9, ..base }, &registry, 6000.0);
    report.series("panel temperature (K), grey emitter under the sun", &grey.time, &grey.temperature, 1200);
    report.measure("grey emitter under the sun: above the air by (K)", grey.settled - base.air_temperature);
    report.above("grey emitter: heats above ambient instead", grey.settled - base.air_temperature, 20.0);
    // And even a grey emitter that somehow reflected the sun: the window
    // selectivity is worth several kelvin of extra depression at night.
    let grey_night = settle(Radiator { irradiance: 0.0, window_emissivity: 0.9, outside_emissivity: 0.9, ..base }, &registry, 6000.0);
    report.measure("grey emitter at night: below the air by (K)", base.air_temperature - grey_night.settled);
    report
}

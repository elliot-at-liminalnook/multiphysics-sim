//! 21. Semenov ignition — `chemical` `thermal`.
//!
//! A reacting mixture in a vessel whose wall is held at T_w. Heat comes
//! from an Arrhenius reaction, `Q·V·A·c·e^{−E/RT}`, and leaves through the
//! wall, `hS·(T − T_w)`. Below a critical wall temperature the two balance
//! and the vessel simmers a few kelvin warm; above it, nothing can balance
//! the exponential and it ignites. The threshold is Semenov's.

use crate::world::{registry, runtime};
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_chemical::{self as chem, GAS_CONSTANT as R};
use sim_domain_thermal as th;

#[derive(Clone, Copy)]
pub struct Vessel {
    pub pre_exponential: f64,
    pub activation_energy: f64,
    /// Heat released per mole reacted (J/mol, positive).
    pub heat_of_reaction: f64,
    pub concentration: f64,
    pub volume: f64,
    pub wall_conductance: f64,
    pub heat_capacity: f64,
    pub wall_temperature: f64,
}

impl Default for Vessel {
    fn default() -> Self {
        Self { pre_exponential: 1.0e10, activation_energy: 1.0e5, heat_of_reaction: 2.0e5, concentration: 100.0, volume: 1.0e-3, wall_conductance: 1.0, heat_capacity: 100.0, wall_temperature: 420.0 }
    }
}

impl Vessel {
    pub fn generation(&self, t: f64) -> f64 {
        self.heat_of_reaction * self.volume * self.pre_exponential * self.concentration * (-self.activation_energy / (R * t)).exp()
    }
    pub fn loss(&self, t: f64) -> f64 {
        self.wall_conductance * (t - self.wall_temperature)
    }
    /// Semenov's parameter at the wall temperature: critical at 1/e.
    pub fn semenov_psi(&self) -> f64 {
        let tw = self.wall_temperature;
        self.activation_energy / (R * tw * tw) * self.generation(tw) / self.wall_conductance
    }
    /// Wall temperature at which `ψ = 1/e`.
    pub fn semenov_wall_temperature(&self) -> f64 {
        bisect(300.0, 700.0, |tw| Vessel { wall_temperature: tw, ..*self }.semenov_psi() - (-1.0_f64).exp())
    }
    /// The exact tangency: the generation curve touches the loss line —
    /// `q_gen(T*) = hS(T* − T_w)` and `q_gen'(T*) = hS` — solved for T_w.
    pub fn tangency_wall_temperature(&self) -> f64 {
        // For each T*, the slope condition fixes T*; walk T* until the loss line passes through T_w.
        let slope = |t: f64| self.activation_energy / (R * t * t) * self.generation(t);
        let t_star = bisect(300.0, 900.0, |t| slope(t) - self.wall_conductance);
        t_star - self.generation(t_star) / self.wall_conductance
    }
    pub fn model(&self, registry: &BehaviorRegistry) -> Batch {
        let mut m = ModelWorld::default();
        let fuel = m.part(registry, "fuel", chem::RESERVOIR, [("concentration", self.concentration), ("reference", self.concentration)]).unwrap();
        let product = m.part(registry, "product", chem::RESERVOIR, [("concentration", self.concentration), ("reference", self.concentration)]).unwrap();
        let reaction = m.part(registry, "reaction", chem::REACTION, [
            ("pre_exponential", self.pre_exponential), ("activation_energy", self.activation_energy), ("enthalpy", -self.heat_of_reaction), ("volume", self.volume), ("reference", self.concentration),
        ]).unwrap();
        let contents = m.part(registry, "contents", th::CAPACITANCE, [("heat_capacity", self.heat_capacity), ("initial.temperature", self.wall_temperature)]).unwrap();
        let wall = m.part(registry, "wall", th::CONDUCTANCE, [("conductance", self.wall_conductance)]).unwrap();
        let bath = m.part(registry, "bath", th::AMBIENT, [("temperature", self.wall_temperature)]).unwrap();
        m.connect([fuel.port("node"), reaction.port("reactant")]);
        m.connect([product.port("node"), reaction.port("product")]);
        m.connect([contents.port("node"), reaction.port("heat"), fuel.port("heat"), product.port("heat"), wall.port("a")]);
        m.connect([wall.port("b"), bath.port("node")]);
        let runtime = runtime(m, registry);
        let temperature = runtime.across_id(contents.port("node"));
        Batch { runtime, temperature }
    }
}

fn bisect(mut lo: f64, mut hi: f64, f: impl Fn(f64) -> f64) -> f64 {
    let flo = f(lo);
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        if (f(mid) > 0.0) == (flo > 0.0) { lo = mid } else { hi = mid }
    }
    0.5 * (lo + hi)
}

pub struct Batch {
    pub runtime: Runtime,
    pub temperature: StateId,
}

pub struct Outcome {
    pub time: Vec<f64>,
    pub temperature: Vec<f64>,
    pub ignition: Option<f64>,
    pub final_excess: f64,
}

pub fn heat(vessel: Vessel, registry: &BehaviorRegistry, duration: f64) -> Outcome {
    let mut batch = vessel.model(registry);
    let ids = [batch.temperature];
    // Stop once it has clearly run away — the exponential goes to infinity
    // within a step after that, and the runtime rightly refuses it.
    let ceiling = vessel.wall_temperature + 100.0;
    let mut time = vec![0.0];
    let mut temperature = vec![batch.runtime.get(batch.temperature)];
    let step: f64 = 0.2;
    let mut t = 0.0;
    let mut ignition = None;
    while t < duration {
        let chunk = step.min(duration - t);
        match batch.runtime.advance_recording(chunk, chunk / 4.0, 4, &ids) {
            Ok(trace) => {
                t += chunk;
                let last = trace.state.last().unwrap()[0];
                time.push(t);
                temperature.push(last);
                if last >= ceiling {
                    ignition = Some(t);
                    break;
                }
            }
            Err(_) => {
                ignition = Some(t);
                break;
            }
        }
    }
    Outcome { time, final_excess: temperature.last().unwrap() - vessel.wall_temperature, temperature, ignition }
}

pub fn run() -> Report {
    let mut report = Report::new("semenov-ignition");
    let registry = registry();
    let base = Vessel::default();
    let semenov = base.semenov_wall_temperature();
    let exact = base.tangency_wall_temperature();
    report.measure("Semenov ψ = 1/e wall temperature (K)", semenov);
    report.measure("exact tangency wall temperature (K)", exact);
    report.measure("RT_w/E at the threshold", R * exact / base.activation_energy);

    for (label, tw) in [("well below", exact - 20.0), ("just below", exact - 1.0), ("just above", exact + 1.0), ("well above", exact + 20.0)] {
        let outcome = heat(Vessel { wall_temperature: tw, ..base }, &registry, 4000.0);
        report.series(&format!("vessel temperature (K), wall {label} the threshold"), &outcome.time, &outcome.temperature, 1200);
        match outcome.ignition {
            Some(t) => report.measure(&format!("wall {label} ({tw:.1} K): ignites at (s)"), t),
            None => report.measure(&format!("wall {label} ({tw:.1} K): settles (K above the wall)"), outcome.final_excess),
        };
        report.holds(&format!("wall {label} the threshold: {}", if tw > exact { "ignites" } else { "settles" }), outcome.ignition.is_some() == (tw > exact));
    }
    // The run's own boundary, by bisection on the wall temperature.
    let (mut lo, mut hi) = (exact - 15.0, exact + 15.0);
    for _ in 0..10 {
        let mid = 0.5 * (lo + hi);
        if heat(Vessel { wall_temperature: mid, ..base }, &registry, 4000.0).ignition.is_some() { hi = mid } else { lo = mid }
    }
    let boundary = 0.5 * (lo + hi);
    report.measure("ignition boundary from the runs (K)", boundary);
    report.within("boundary matches the exact tangency condition", boundary, exact, 2.0e-3);
    report.within("boundary matches Semenov's ψ = 1/e", boundary, semenov, 0.03);

    // Falsifier: no activation energy, no threshold — the steady excess is
    // the same at every wall temperature.
    let mut excesses = Vec::new();
    for tw in [360.0, 400.0, 440.0, 480.0] {
        let linear = Vessel { activation_energy: 0.0, pre_exponential: base.generation(exact) / (base.heat_of_reaction * base.volume * base.concentration), wall_temperature: tw, ..base };
        let outcome = heat(linear, &registry, 2000.0);
        report.holds(&format!("no activation energy, wall {tw:.0} K: settles"), outcome.ignition.is_none());
        excesses.push(outcome.final_excess);
    }
    let spread = excesses.iter().copied().fold(f64::NEG_INFINITY, f64::max) - excesses.iter().copied().fold(f64::INFINITY, f64::min);
    report.measure("no activation energy: steady excess over the wall (K)", excesses[0]);
    report.below("no activation energy: the steady state moves smoothly (same excess everywhere)", spread / excesses[0].abs().max(1.0e-9), 1.0e-3);
    report
}

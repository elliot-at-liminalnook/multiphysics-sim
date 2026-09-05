//! 16. Thermoelastic damping — `structural` `thermal`.
//!
//! One flexural mode (an inertia and a spring on a curvature node) coupled
//! through sixteen thermoelastic-layer bridges to a thermal chain of
//! capacitances and conductances. No damper anywhere; the beam stops.

use crate::world::{record, registry};
use crate::Report;
use sim_compile::Runtime;
use sim_core::{BehaviorRegistry, ModelWorld, StateId};
use sim_domain_bridges::elements as bridge;
use sim_domain_rotational::elements as rot;
use sim_domain_thermal as th;
use sim_dynamics::analysis::{envelope_rate, period};
use std::f64::consts::PI;

#[derive(Clone, Copy)]
pub struct Material {
    pub youngs_modulus: f64,
    pub expansion: f64,
    pub density: f64,
    pub specific_heat: f64,
    pub conductivity: f64,
    pub temperature: f64,
}

impl Material {
    pub const ALUMINIUM: Self = Self { youngs_modulus: 70.0e9, expansion: 23.0e-6, density: 2700.0, specific_heat: 900.0, conductivity: 237.0, temperature: 300.0 };
    pub fn diffusivity(&self) -> f64 {
        self.conductivity / (self.density * self.specific_heat)
    }
    pub fn relaxation_strength(&self) -> f64 {
        self.youngs_modulus * self.expansion * self.expansion * self.temperature / (self.density * self.specific_heat)
    }
}

#[derive(Clone, Copy)]
pub struct ThermoelasticBeam {
    pub material: Material,
    pub thickness: f64,
    pub width: f64,
    pub layers: usize,
    pub frequency: f64,
}

pub struct Beam {
    pub runtime: Runtime,
    pub curvature: StateId,
    pub layer_temperatures: Vec<StateId>,
    pub layer_conductance: f64,
    /// Entropy production of every element, from the store.
    pub productions: Vec<StateId>,
}

impl ThermoelasticBeam {
    fn bending_stiffness(&self) -> f64 {
        self.material.youngs_modulus * self.width * self.thickness.powi(3) / 12.0
    }
    pub fn zener_time(&self) -> f64 {
        self.thickness * self.thickness / (PI * PI * self.material.diffusivity())
    }
    pub fn lifshitz_roukes_loss(&self) -> f64 {
        let xi = self.thickness * (self.frequency / (2.0 * self.material.diffusivity())).sqrt();
        self.material.relaxation_strength() * (6.0 / (xi * xi) - 6.0 / xi.powi(3) * (xi.sinh() + xi.sin()) / (xi.cosh() + xi.cos()))
    }
    pub fn zener_loss(&self) -> f64 {
        let wt = self.frequency * self.zener_time();
        self.material.relaxation_strength() * wt / (1.0 + wt * wt)
    }
    pub fn model(&self, registry: &BehaviorRegistry) -> Beam {
        self.model_with_sign(registry, 1.0)
    }

    /// `sign` flips the thermoelastic coupling; only +1 is physical.
    pub fn model_with_sign(&self, registry: &BehaviorRegistry, sign: f64) -> Beam {
        self.build(registry, sign, 1.0).expect("model compiles")
    }

    /// Conductances scaled by `sign`: −1 carries heat uphill, which the
    /// runtime's entropy accounting must refuse.
    pub fn model_with_conductance_sign(&self, registry: &BehaviorRegistry, sign: f64) -> Option<Beam> {
        self.build(registry, 1.0, sign)
    }

    fn build(&self, registry: &BehaviorRegistry, sign: f64, conductance_sign: f64) -> Option<Beam> {
        let m = self.material;
        let ei = self.bending_stiffness();
        let modal_inertia = ei / (self.frequency * self.frequency);
        let dy = self.thickness / self.layers as f64;
        let layer_capacity = m.density * m.specific_heat * self.width * dy;
        let layer_conductance = m.conductivity * self.width / dy;
        let mut w = ModelWorld::default();
        let mode = w.part(registry, "mode", rot::INERTIA, [("inertia", modal_inertia), ("initial.angle", 1.0e-3)]).unwrap();
        let stiffness = w.part(registry, "EI", rot::SPRING, [("stiffness", ei)]).unwrap();
        let root = w.part(registry, "root", rot::GROUND, []).unwrap();
        w.connect([stiffness.port("b"), root.port("flange")]);
        let mut bending_ports = vec![mode.port("shaft"), stiffness.port("a")];
        let mut layer_nodes = Vec::new();
        for l in 0..self.layers {
            let y = -0.5 * self.thickness + (l as f64 + 0.5) * dy;
            let cap = w.part(registry, &format!("layer{l}"), th::CAPACITANCE, [("heat_capacity", layer_capacity), ("initial.temperature", m.temperature)]).unwrap();
            let coupling = w.part(registry, &format!("coupling{l}"), bridge::THERMOELASTIC_LAYER, [
                ("height", y), ("thickness", dy), ("width", self.width), ("youngs_modulus", m.youngs_modulus), ("expansion", m.expansion), ("temperature", m.temperature), ("sign", sign),
            ]).unwrap();
            bending_ports.push(coupling.port("bending"));
            let mut node_ports = vec![cap.port("node"), coupling.port("layer")];
            if l > 0 {
                let conduction = w.part(registry, &format!("conduction{l}"), th::CONDUCTANCE, [("conductance", conductance_sign * layer_conductance)]).unwrap();
                let previous: &mut Vec<sim_core::PortId> = layer_nodes.last_mut().unwrap();
                previous.push(conduction.port("a"));
                node_ports.push(conduction.port("b"));
            }
            layer_nodes.push(node_ports);
        }
        w.connect(bending_ports);
        let cap_ports: Vec<sim_core::PortId> = layer_nodes.iter().map(|ports| ports[0]).collect();
        for ports in layer_nodes {
            w.connect(ports);
        }
        let runtime = Runtime::new(w, registry, sim_dynamics::Integrator::ImplicitMidpoint(sim_solve::NewtonConfig { max_iterations: 40, min_line_search: 1.0 / 4096.0, ..Default::default() })).ok()?;
        let curvature = runtime.across_id(mode.port("shaft"));
        let layer_temperatures = cap_ports.iter().map(|p| runtime.across_id(*p)).collect();
        let productions = runtime.model.behaviors.keys().map(|b| runtime.entropy_production_id(b)).collect();
        Some(Beam { runtime, curvature, layer_temperatures, layer_conductance, productions })
    }
    /// Entropy production rate `Σ G·(ΔT)²/T₀²` between adjacent layers.
    pub fn entropy_production(&self, beam: &Beam, temperatures: &[f64]) -> f64 {
        temperatures.windows(2).map(|w| beam.layer_conductance * (w[1] - w[0]).powi(2)).sum::<f64>() / self.material.temperature.powi(2)
    }
    /// Mechanical energy plus thermal free energy — the quantity T₀·σ drains.
    pub fn available_energy(&self, curvature: f64, curvature_rate: f64, temperatures: &[f64]) -> f64 {
        let m = self.material;
        let dy = self.thickness / self.layers as f64;
        let ei = self.bending_stiffness();
        let modal_inertia = ei / (self.frequency * self.frequency);
        0.5 * modal_inertia * curvature_rate * curvature_rate + 0.5 * ei * curvature * curvature
            + 0.5 * m.density * m.specific_heat / m.temperature * self.width * dy * temperatures.iter().map(|t| t * t).sum::<f64>()
    }
}

pub struct Outcome {
    pub loss_factor: f64,
    pub frequency: f64,
    pub entropy_identity_error: f64,
    pub time: Vec<f64>,
    pub curvature: Vec<f64>,
    pub profile: (Vec<f64>, Vec<f64>),
}

pub fn ring(beam: ThermoelasticBeam, registry: &BehaviorRegistry, cycles: f64) -> Outcome {
    let mut b = beam.model(registry);
    let rate_id = b.runtime.state_id(b.runtime.model.behaviors.iter().find(|(_, x)| x.kind.0 == rot::INERTIA).map(|(id, _)| id).unwrap(), "speed");
    let layers = beam.layers;
    let mut ids = vec![b.curvature, rate_id];
    ids.extend(b.layer_temperatures.iter().copied());
    let production_from = ids.len();
    ids.extend(b.productions.iter().copied());
    let cycle = 2.0 * PI / beam.frequency;
    let step = cycle / 80.0;
    let trace = record(&mut b.runtime, cycles * cycle, step, 1, &ids);
    // Layer nodes carry absolute temperature; the analysis wants the excursion.
    let t0 = beam.material.temperature;
    let temps = |x: &[f64], layers: usize| -> Vec<f64> { x[2..2 + layers].iter().map(|t| t - t0).collect() };
    // T₀ times the total entropy production the compiler reports, integrated.
    let total = |x: &[f64]| x[production_from..].iter().sum::<f64>();
    let mut dissipated = 0.0;
    for w in trace.state.windows(2) {
        dissipated += 0.5 * (total(&w[0]) + total(&w[1])) * step * beam.material.temperature;
    }
    let first = &trace.state[0];
    let last = trace.state.last().unwrap();
    let e0 = beam.available_energy(first[0], first[1], &temps(first, layers));
    let e1 = beam.available_energy(last[0], last[1], &temps(last, layers));
    let curvature = trace.column(0);
    let rate = envelope_rate(&trace.time, &curvature).unwrap_or(0.0);
    let frequency = period(&trace.time, &curvature).map(|p| 2.0 * PI / p).unwrap_or(beam.frequency);
    let start = trace.time.partition_point(|t| *t < 10.0 * cycle);
    let snapshot = (start..trace.len()).max_by(|a, b| curvature[*a].total_cmp(&curvature[*b])).unwrap_or(start);
    let profile = ((0..layers).map(|l| -0.5 + (l as f64 + 0.5) / layers as f64).collect(), temps(&trace.state[snapshot], layers));
    Outcome { loss_factor: -2.0 * rate / frequency, frequency, entropy_identity_error: ((e0 - e1) - dissipated).abs() / e0, time: trace.time.clone(), curvature, profile }
}

pub fn run() -> Report {
    let mut report = Report::new("thermoelastic-damping");
    let registry = registry();
    let material = Material::ALUMINIUM;
    let frequency = 2.0 * PI * 10.0e3;
    let peak_thickness = (PI * PI * material.diffusivity() / frequency).sqrt();
    let beam = |thickness: f64| ThermoelasticBeam { material, thickness, width: 1.0e-3, layers: 16, frequency };
    let reference = beam(peak_thickness);
    report.measure("relaxation strength Δ_E", material.relaxation_strength()).measure("thickness at ωτ = 1 (m)", peak_thickness).measure("Zener Q⁻¹ at ωτ = 1", reference.zener_loss()).measure("Lifshitz–Roukes Q⁻¹ at ωτ = 1", reference.lifshitz_roukes_loss());

    let mut curve = Vec::new();
    for (label, scale) in [("h/3", 1.0 / 3.0), ("h/√3", 1.0 / 3.0_f64.sqrt()), ("h", 1.0), ("√3·h", 3.0_f64.sqrt()), ("3h", 3.0)] {
        let b = beam(scale * peak_thickness);
        let outcome = ring(b, &registry, 400.0);
        curve.push((scale, outcome.loss_factor));
        report.measure(&format!("measured Q⁻¹ at {label}"), outcome.loss_factor);
        report.within(&format!("Q⁻¹ at {label} matches Lifshitz–Roukes"), outcome.loss_factor, b.lifshitz_roukes_loss(), 0.05);
        if scale == 1.0 {
            report.series("curvature ringing down, ωτ = 1", &outcome.time, &outcome.curvature, 4000);
            report.series("temperature across thickness (K) at peak curvature", &outcome.profile.0, &outcome.profile.1, 50);
            report.below("T₀ × compiler-reported entropy production equals energy lost", outcome.entropy_identity_error, 5.0e-3);
            report.within("frequency stays at the mode frequency", outcome.frequency, frequency, 0.01);
        }
    }
    report.series("measured Q⁻¹ vs thickness / h(ωτ=1)", &curve.iter().map(|(s, _)| *s).collect::<Vec<_>>(), &curve.iter().map(|(_, q)| *q).collect::<Vec<_>>(), 10);
    let peak = curve.iter().fold((0.0, 0.0), |m, c| if c.1 > m.1 { *c } else { m });
    report.close("loss peaks at ωτ = 1", peak.0, 1.0, 0.0);
    report.within("peak loss is ≈ 0.494 Δ_E", peak.1, 0.494 * material.relaxation_strength(), 0.05);

    let inert = ThermoelasticBeam { material: Material { expansion: 0.0, ..material }, ..reference };
    let outcome = ring(inert, &registry, 400.0);
    report.below("α = 0: rings indefinitely", outcome.loss_factor.abs(), 1.0e-6);

    // Falsifier for the entropy lane: a layer conductance that carries heat
    // uphill produces negative entropy, and the runtime refuses the step.
    let rebuilt = ThermoelasticBeam { ..reference }.model_with_conductance_sign(&registry, -1.0);
    let rejected = rebuilt.map(|mut b| b.runtime.advance(2.0 * PI / frequency, 2.0 * PI / frequency / 80.0));
    report.holds("heat carried uphill: rejected by the second-law check", matches!(rejected, Some(Err(sim_compile::RuntimeError::SecondLaw { .. }))));
    report
}

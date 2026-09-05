//! Chemical domain: one species per connector, across the chemical
//! potential μ (J/mol), through the molar flow (mol/s). Every element that
//! needs a concentration reads the temperature off a thermal port and uses
//! the ideal relation `c = c₀·exp(μ/RT)`, so the coupling to heat is the
//! ordinary one — no special cases.

use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, Provision, QuantityKind, RegistryError,
    StateDeclaration, acausal, param, param_or,
};
use std::collections::BTreeMap;

pub const RESERVOIR: &str = "chem.reservoir";
pub const SPECIES: &str = "chem.species";
pub const REACTION: &str = "chem.reaction";
pub const DIFFUSION: &str = "chem.diffusion";
pub const ELECTRODE: &str = "bridge.electrode";

pub const GAS_CONSTANT: f64 = 8.314_462_618;
pub const FARADAY: f64 = 96_485.332_12;

type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;

/// Ideal concentration from a chemical potential at temperature `t`.
pub fn concentration(potential: f64, t: f64, reference: f64) -> f64 {
    reference * (potential / (GAS_CONSTANT * t)).exp()
}
pub fn potential(concentration: f64, t: f64, reference: f64) -> f64 {
    GAS_CONSTANT * t * (concentration / reference).ln()
}

/// A species held at fixed `concentration` (a large bath, a fed reactor):
/// its potential follows the temperature it reads.
pub struct Reservoir {
    pub concentration: f64,
    pub reference: f64,
}
impl Behavior for Reservoir {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("molar_flow", QuantityKind::MolarFlow, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        let t = ctx.across(1);
        ctx.set_state_residual(0, ctx.across(0) - potential(self.concentration, t, self.reference));
        ctx.add_through(0, ctx.state(0));
    }
}

/// A well-mixed amount of one species in `volume`: its potential *is* the
/// node's; the amount `n = V·c₀·exp(μ/RT)` changes by the molar flows in.
pub struct Species {
    pub volume: f64,
    pub reference: f64,
    pub initial_concentration: f64,
    pub initial_temperature: f64,
}
impl Behavior for Species {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("potential", QuantityKind::ChemicalPotential, potential(self.initial_concentration, self.initial_temperature, self.reference))]
    }
    fn provides(&self) -> Vec<Provision> {
        vec![Provision { port: 0, lane: 0, state: 0 }]
    }
    fn residual(&self, ctx: &mut Context) {
        let (mu, t) = (ctx.state(0), ctx.across(1));
        let n = self.volume * concentration(mu, t, self.reference);
        let dn = n * (ctx.state_rate(0) / (GAS_CONSTANT * t) - mu * ctx.across_derivative(1, 0) / (GAS_CONSTANT * t * t));
        ctx.add_through(0, dn);
    }
}

/// `reactant → product` at the Arrhenius rate `A·exp(−E/RT)·c` in `volume`,
/// releasing `−enthalpy` per mole through the thermal port. The heat leaves
/// at the reacting temperature, so the entropy it carries is the
/// production — nothing to declare.
pub struct Reaction {
    pub pre_exponential: f64,
    pub activation_energy: f64,
    pub enthalpy: f64,
    pub volume: f64,
    pub reference: f64,
}
impl Reaction {
    pub fn rate(&self, concentration: f64, t: f64) -> f64 {
        self.pre_exponential * (-self.activation_energy / (GAS_CONSTANT * t)).exp() * concentration
    }
}
impl Behavior for Reaction {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let t = ctx.across(2);
        let c = concentration(ctx.across(0), t, self.reference);
        let flow = self.rate(c, t) * self.volume;
        ctx.add_through(0, flow);
        ctx.add_through(1, -flow);
        ctx.add_through(2, self.enthalpy * flow);
    }
}

/// Fickian exchange between two nodes of the same species: `D·(c_a − c_b)`.
pub struct Diffusion {
    pub conductance: f64,
    pub reference: f64,
}
impl Behavior for Diffusion {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let t = ctx.across(2);
        let flow = self.conductance * (concentration(ctx.across(0), t, self.reference) - concentration(ctx.across(1), t, self.reference));
        ctx.add_through(0, flow);
        ctx.add_through(1, -flow);
    }
}

/// Butler–Volmer electrode: current `i₀A[e^{αzFη/RT} − e^{−(1−α)zFη/RT}]`
/// at overpotential `η = (v_p − v_n) − E₀ − μ/zF`, moving `i/zF` moles of
/// the ion and dumping `i·η` as heat.
pub struct Electrode {
    pub exchange_current: f64,
    pub area: f64,
    pub transfer: f64,
    pub charge: f64,
    pub standard_potential: f64,
}
impl Behavior for Electrode {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let t = ctx.across(3);
        let eta = ctx.across(0) - ctx.across(1) - self.standard_potential - ctx.across(2) / (self.charge * FARADAY);
        let x = self.charge * FARADAY * eta / (GAS_CONSTANT * t);
        let current = self.exchange_current * self.area * ((self.transfer * x).exp() - (-(1.0 - self.transfer) * x).exp());
        ctx.add_through(0, current);
        ctx.add_through(1, -current);
        ctx.add_through(2, current / (self.charge * FARADAY));
        ctx.add_through(3, -current * eta);
    }
}

fn reservoir(p: &Params) -> Made {
    Ok(Box::new(Reservoir { concentration: param(p, "concentration")?, reference: param_or(p, "reference", 1.0) }))
}
fn species(p: &Params) -> Made {
    Ok(Box::new(Species { volume: param(p, "volume")?, reference: param_or(p, "reference", 1.0), initial_concentration: param_or(p, "initial.concentration", 1.0), initial_temperature: param_or(p, "initial.temperature", 298.15) }))
}
fn reaction(p: &Params) -> Made {
    Ok(Box::new(Reaction { pre_exponential: param(p, "pre_exponential")?, activation_energy: param(p, "activation_energy")?, enthalpy: param(p, "enthalpy")?, volume: param(p, "volume")?, reference: param_or(p, "reference", 1.0) }))
}
fn diffusion(p: &Params) -> Made {
    Ok(Box::new(Diffusion { conductance: param(p, "conductance")?, reference: param_or(p, "reference", 1.0) }))
}
fn electrode(p: &Params) -> Made {
    Ok(Box::new(Electrode { exchange_current: param(p, "exchange_current")?, area: param_or(p, "area", 1.0), transfer: param_or(p, "transfer", 0.5), charge: param_or(p, "charge", 1.0), standard_potential: param_or(p, "standard_potential", 0.0) }))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    use ConnectorKind::{Chemical as C, Electrical as E, Thermal as H};
    for descriptor in [
        BehaviorDescriptor::new(RESERVOIR, "Fixed-concentration bath", vec![acausal("node", C), acausal("heat", H)], reservoir).with_parameters(vec![P::required("concentration", "mol/m³").positive(), P::optional("reference", "mol/m³", 1.0).positive()]),
        BehaviorDescriptor::new(SPECIES, "Well-mixed amount of a species", vec![acausal("node", C), acausal("heat", H)], species).with_parameters(vec![P::required("volume", "m³").positive(), P::optional("reference", "mol/m³", 1.0).positive(), P::optional("initial.concentration", "mol/m³", 1.0).positive(), P::optional("initial.temperature", "K", 298.15).positive()]),
        BehaviorDescriptor::new(REACTION, "Arrhenius reaction", vec![acausal("reactant", C), acausal("product", C), acausal("heat", H)], reaction).with_parameters(vec![P::required("pre_exponential", "1/s").nonnegative(), P::required("activation_energy", "J/mol"), P::required("enthalpy", "J/mol"), P::required("volume", "m³").positive(), P::optional("reference", "mol/m³", 1.0).positive()]),
        BehaviorDescriptor::new(DIFFUSION, "Fickian exchange", vec![acausal("a", C), acausal("b", C), acausal("heat", H)], diffusion).with_parameters(vec![P::required("conductance", "m³/s").nonnegative(), P::optional("reference", "mol/m³", 1.0).positive()]),
        BehaviorDescriptor::new(ELECTRODE, "Butler–Volmer electrode", vec![acausal("p", E), acausal("n", E), acausal("ion", C), acausal("heat", H)], electrode).with_parameters(vec![P::required("exchange_current", "A/m²").nonnegative(), P::optional("area", "m²", 1.0).positive(), P::optional("transfer", "1", 0.5).nonnegative().at_most(1.0), P::optional("charge", "1", 1.0).positive(), P::optional("standard_potential", "V", 0.0)]),
    ] {
        registry.register(descriptor)?;
    }
    Ok(())
}

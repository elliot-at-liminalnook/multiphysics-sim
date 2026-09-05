//! Lumped thermal elements. Heat flow is positive into an element at its
//! first port; a node's heat flows sum to zero.

use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, QuantityKind, RegistryError,
    StateDeclaration, View, acausal, param, param_or,
};
use std::collections::BTreeMap;

pub const CAPACITANCE: &str = "thermal.capacitance";
pub const CONDUCTANCE: &str = "thermal.conductance";
pub const AMBIENT: &str = "thermal.ambient";
pub const HEAT_SOURCE: &str = "thermal.heat_source";

type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;

/// Heat capacity at a node; supply `initial.temperature` to set the node.
pub struct Capacitance {
    pub heat_capacity: f64,
}
impl Behavior for Capacitance {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let stored = self.heat_capacity * ctx.across_rate(0);
        ctx.add_through(0, stored);
        ctx.store_entropy(stored / ctx.across(0));
    }
    fn energy(&self, view: &View) -> f64 {
        self.heat_capacity * view.across(0)
    }
}
fn capacitance(p: &Params) -> Made {
    Ok(Box::new(Capacitance { heat_capacity: param(p, "heat_capacity")? }))
}

/// Conduction `G·(T_a − T_b)`; give `resistance` instead of `conductance` if preferred.
pub struct Conductance {
    pub conductance: f64,
}
impl Behavior for Conductance {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let flow = self.conductance * (ctx.across(0) - ctx.across(1));
        ctx.add_through(0, flow);
        ctx.add_through(1, -flow);
    }
}
fn conductance(p: &Params) -> Made {
    let g = match p.get("conductance") {
        Some(g) => *g,
        None => 1.0 / param(p, "resistance")?,
    };
    Ok(Box::new(Conductance { conductance: g }))
}

/// Fixed temperature; the heat it absorbs is its multiplier state.
pub struct Ambient {
    pub temperature: f64,
}
impl Behavior for Ambient {
    fn pinned(&self) -> Vec<(usize, usize, f64)> {
        vec![(0, 0, self.temperature)]
    }
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("absorbed", QuantityKind::HeatFlow, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.set_state_residual(0, ctx.across(0) - self.temperature);
        let absorbed = ctx.state(0);
        ctx.add_through(0, absorbed);
        // A reservoir stores what it absorbs, reversibly.
        ctx.store_entropy(absorbed / self.temperature);
    }
}
fn ambient(p: &Params) -> Made {
    Ok(Box::new(Ambient { temperature: param(p, "temperature")? }))
}

pub struct HeatSource {
    pub power: f64,
}
impl Behavior for HeatSource {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.add_through(0, -self.power);
    }
}
fn heat_source(p: &Params) -> Made {
    Ok(Box::new(HeatSource { power: param_or(p, "power", 0.0) }))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    use ConnectorKind::Thermal as H;
    for descriptor in [
        BehaviorDescriptor::new(CAPACITANCE, "Thermal capacitance", vec![acausal("node", H)], capacitance).with_parameters(vec![P::required("heat_capacity", "J/K").positive()]),
        BehaviorDescriptor::new(CONDUCTANCE, "Thermal conductance", vec![acausal("a", H), acausal("b", H)], conductance).with_parameters(vec![P::alternative("conductance", "W/K"), P::alternative("resistance", "K/W")]),
        BehaviorDescriptor::new(AMBIENT, "Fixed temperature", vec![acausal("node", H)], ambient).with_parameters(vec![P::required("temperature", "K").nonnegative()]),
        BehaviorDescriptor::new(HEAT_SOURCE, "Constant heat source", vec![acausal("node", H)], heat_source).with_parameters(vec![P::optional("power", "W", 0.0)]),
    ] {
        registry.register(descriptor)?;
    }
    Ok(())
}

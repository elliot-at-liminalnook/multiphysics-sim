//! Lumped compressible hydraulics: volumes, inertances, reservoirs, valves.
//! Volume flow is positive into an element at its first port.

use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, QuantityKind, RegistryError,
    StateDeclaration, View, acausal, param, param_or,
};
use std::collections::BTreeMap;

pub const VOLUME: &str = "hydraulic.volume";
pub const INERTANCE: &str = "hydraulic.inertance";
pub const RESERVOIR: &str = "hydraulic.reservoir";
pub const VALVE: &str = "hydraulic.valve";

type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;

/// Compressible volume: `C·ṗ` of flow stored, `C = V/(ρc²)`.
pub struct Volume {
    pub compliance: f64,
}
impl Behavior for Volume {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let stored = self.compliance * ctx.across_rate(0);
        ctx.add_through(0, stored);
    }
    fn energy(&self, view: &View) -> f64 {
        0.5 * self.compliance * view.across(0).powi(2)
    }
}
fn volume(p: &Params) -> Made {
    Ok(Box::new(Volume { compliance: param(p, "compliance")? }))
}

/// Fluid inertia in a pipe segment: `L·q̇ = p_a − p_b`, `L = ρℓ/A`.
pub struct Inertance {
    pub inertance: f64,
    pub initial_flow: f64,
}
impl Behavior for Inertance {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("flow", QuantityKind::VolumeFlow, self.initial_flow)]
    }
    fn residual(&self, ctx: &mut Context) {
        let q = ctx.state(0);
        ctx.set_state_residual(0, self.inertance * ctx.state_rate(0) - (ctx.across(0) - ctx.across(1)));
        ctx.add_through(0, q);
        ctx.add_through(1, -q);
    }
    fn energy(&self, view: &View) -> f64 {
        0.5 * self.inertance * view.state(0).powi(2)
    }
}
fn inertance(p: &Params) -> Made {
    Ok(Box::new(Inertance { inertance: param(p, "inertance")?, initial_flow: param_or(p, "initial.flow", 0.0) }))
}

pub struct Reservoir {
    pub pressure: f64,
}
impl Behavior for Reservoir {
    fn pinned(&self) -> Vec<(usize, usize, f64)> {
        vec![(0, 0, self.pressure)]
    }
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("supplied", QuantityKind::VolumeFlow, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.set_state_residual(0, ctx.across(0) - self.pressure);
        ctx.add_through(0, ctx.state(0));
    }
}
fn reservoir(p: &Params) -> Made {
    Ok(Box::new(Reservoir { pressure: param(p, "pressure")? }))
}

/// Valve at the end of a water column: the column's inertance `L` and a
/// linear conductance `K(t)` closing from t = 0 over `closure_time`. Seating
/// is a guard-and-jump: the flow state is zeroed and frozen, which is what
/// a shut valve does to the column behind it.
pub struct Valve {
    pub conductance: f64,
    pub closure_time: f64,
    pub inertance: f64,
    pub floor: f64,
    pub initial_flow: f64,
}
impl Valve {
    pub fn open_fraction(&self, t: f64) -> f64 {
        (1.0 - t / self.closure_time).clamp(self.floor, 1.0)
    }
}
impl Behavior for Valve {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![
            StateDeclaration::new("flow", QuantityKind::VolumeFlow, self.initial_flow),
            StateDeclaration::new("seated", QuantityKind::Dimensionless, 0.0),
        ]
    }
    fn residual(&self, ctx: &mut Context) {
        let q = ctx.state(0);
        ctx.set_state_residual(1, ctx.state_rate(1));
        if ctx.state(1) > 0.5 {
            ctx.set_state_residual(0, ctx.state_rate(0));
        } else {
            let drop = q / (self.conductance * self.open_fraction(ctx.time));
            ctx.set_state_residual(0, self.inertance * ctx.state_rate(0) - (ctx.across(0) - ctx.across(1) - drop));
        }
        ctx.add_through(0, q);
        ctx.add_through(1, -q);
    }
    fn energy(&self, view: &View) -> f64 {
        0.5 * self.inertance * view.state(0).powi(2)
    }
    fn guards(&self, view: &View, out: &mut Vec<f64>) {
        out.push(if view.state(1) > 0.5 { 1.0 } else { self.closure_time - view.time });
    }
    fn jump(&mut self, _index: usize, _view: &View, states: &mut [f64]) {
        states[0] = 0.0;
        states[1] = 1.0;
    }
}
fn valve(p: &Params) -> Made {
    Ok(Box::new(Valve {
        conductance: param(p, "conductance")?,
        closure_time: param(p, "closure_time")?,
        inertance: param(p, "inertance")?,
        floor: param_or(p, "floor", 1.0e-4),
        initial_flow: param_or(p, "initial.flow", 0.0),
    }))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    use ConnectorKind::Hydraulic as Y;
    for descriptor in [
        BehaviorDescriptor::new(VOLUME, "Compressible volume", vec![acausal("port", Y)], volume).with_parameters(vec![P::required("compliance", "m³/Pa").positive()]),
        BehaviorDescriptor::new(INERTANCE, "Fluid inertance", vec![acausal("a", Y), acausal("b", Y)], inertance).with_parameters(vec![P::required("inertance", "Pa·s²/m³").positive(), P::optional("initial.flow", "m³/s", 0.0)]),
        BehaviorDescriptor::new(RESERVOIR, "Constant-pressure reservoir", vec![acausal("port", Y)], reservoir).with_parameters(vec![P::required("pressure", "Pa")]),
        BehaviorDescriptor::new(VALVE, "Closing valve with its water column", vec![acausal("a", Y), acausal("b", Y)], valve).with_parameters(vec![P::required("conductance", "m³/(Pa·s)").positive(), P::required("closure_time", "s").positive(), P::required("inertance", "Pa·s²/m³").positive(), P::optional("floor", "1", 1.0e-4).positive().at_most(1.0), P::optional("initial.flow", "m³/s", 0.0)]),
    ] {
        registry.register(descriptor)?;
    }
    Ok(())
}

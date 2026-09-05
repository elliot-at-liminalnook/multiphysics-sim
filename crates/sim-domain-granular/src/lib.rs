//! Granular domain: across the stress on a plane (Pa), through the mass
//! flow of grain (kg/s). A column carries its own Janssen stress profile;
//! an orifice drains at Beverloo's rate whatever the load above — or not
//! at all, if it is too small for the grain.

use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, QuantityKind, RegistryError,
    StateDeclaration, View, acausal, param, param_or,
};
use std::collections::BTreeMap;

pub const HOPPER: &str = "granular.hopper";
pub const COLUMN: &str = "granular.column";
pub const ORIFICE: &str = "granular.orifice";
pub const SINK: &str = "granular.sink";

pub const G: f64 = 9.81;

type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;

/// Pours grain at a set rate.
pub struct Hopper {
    pub rate: f64,
}
impl Behavior for Hopper {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.add_through(0, -self.rate);
    }
}

/// A silo of `diameter` filled with grain of bulk `density`: the free
/// surface at `top` (zero stress), the floor at `base`, where the stress is
/// Janssen's `σ_sat·(1 − e^{−h/λ})` with `σ_sat = ρgD/(4μK)` and
/// `λ = D/(4μK)` — the walls carry the rest. With `friction = 0` it is a
/// fluid: `ρgh`.
pub struct Column {
    pub diameter: f64,
    pub density: f64,
    pub friction: f64,
    pub janssen_k: f64,
    pub initial_mass: f64,
}
impl Column {
    pub fn area(&self) -> f64 {
        std::f64::consts::FRAC_PI_4 * self.diameter * self.diameter
    }
    pub fn height(&self, mass: f64) -> f64 {
        mass / (self.density * self.area())
    }
    pub fn saturation_stress(&self) -> f64 {
        self.density * G * self.diameter / (4.0 * self.friction * self.janssen_k)
    }
    pub fn depth_scale(&self) -> f64 {
        self.diameter / (4.0 * self.friction * self.janssen_k)
    }
    pub fn base_stress(&self, mass: f64) -> f64 {
        let h = self.height(mass);
        if self.friction * self.janssen_k <= 0.0 {
            self.density * G * h
        } else {
            self.saturation_stress() * (1.0 - (-h / self.depth_scale()).exp())
        }
    }
}
impl Behavior for Column {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![
            StateDeclaration::new("mass", QuantityKind::Mass, self.initial_mass),
            StateDeclaration::new("inflow", QuantityKind::MassFlow, 0.0),
            StateDeclaration::new("outflow", QuantityKind::MassFlow, 0.0),
        ]
    }
    fn residual(&self, ctx: &mut Context) {
        let (mass, inflow, outflow) = (ctx.state(0), ctx.state(1), ctx.state(2));
        ctx.set_state_residual(0, ctx.state_rate(0) - (inflow - outflow));
        // Free surface: no stress. Floor: Janssen.
        ctx.set_state_residual(1, ctx.across(0));
        ctx.set_state_residual(2, ctx.across(1) - self.base_stress(mass.max(0.0)));
        ctx.add_through(0, inflow);
        ctx.add_through(1, -outflow);
    }
    fn energy(&self, view: &View) -> f64 {
        let mass = view.state(0).max(0.0);
        0.5 * mass * G * self.height(mass)
    }
}

/// Beverloo's orifice: `C·ρ·√g·(D − k·d)^{5/2}` from `in` to `out`,
/// independent of the stress above it; nothing at all once the opening is
/// less than `k` grain diameters.
pub struct Orifice {
    pub diameter: f64,
    pub grain: f64,
    pub density: f64,
    pub coefficient: f64,
    pub k: f64,
}
impl Orifice {
    pub fn rate(&self) -> f64 {
        let open = self.diameter - self.k * self.grain;
        if open <= 0.0 { 0.0 } else { self.coefficient * self.density * G.sqrt() * open.powf(2.5) }
    }
    pub fn jammed(&self) -> bool {
        self.diameter <= self.k * self.grain
    }
}
impl Behavior for Orifice {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        // Drains only while something is above it (stress > 0).
        let loaded = 0.5 * (1.0 + (ctx.across(0) / 10.0).tanh());
        let flow = self.rate() * loaded;
        ctx.add_through(0, flow);
        ctx.add_through(1, -flow);
    }
}

/// Free fall: takes any flow at zero stress.
pub struct Sink;
impl Behavior for Sink {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("flow", QuantityKind::MassFlow, 0.0)]
    }
    fn pinned(&self) -> Vec<(usize, usize, f64)> {
        vec![(0, 0, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.set_state_residual(0, ctx.across(0));
        ctx.add_through(0, ctx.state(0));
    }
}

fn sink(_: &Params) -> Made {
    Ok(Box::new(Sink))
}
fn hopper(p: &Params) -> Made {
    Ok(Box::new(Hopper { rate: param(p, "rate")? }))
}
fn column(p: &Params) -> Made {
    Ok(Box::new(Column { diameter: param(p, "diameter")?, density: param(p, "density")?, friction: param_or(p, "friction", 0.0), janssen_k: param_or(p, "janssen_k", 0.5), initial_mass: param_or(p, "initial.mass", 0.0) }))
}
fn orifice(p: &Params) -> Made {
    Ok(Box::new(Orifice { diameter: param(p, "diameter")?, grain: param(p, "grain")?, density: param(p, "density")?, coefficient: param_or(p, "coefficient", 0.58), k: param_or(p, "k", 1.5) }))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    use ConnectorKind::Granular as Gr;
    for descriptor in [
        BehaviorDescriptor::new(HOPPER, "Grain source", vec![acausal("out", Gr)], hopper).with_parameters(vec![P::required("rate", "kg/s").nonnegative()]),
        BehaviorDescriptor::new(COLUMN, "Silo column with Janssen walls", vec![acausal("top", Gr), acausal("base", Gr)], column).with_parameters(vec![P::required("diameter", "m").positive(), P::required("density", "kg/m³").positive(), P::optional("friction", "1", 0.0).nonnegative(), P::optional("janssen_k", "1", 0.5).nonnegative(), P::optional("initial.mass", "kg", 0.0).nonnegative()]),
        BehaviorDescriptor::new(ORIFICE, "Beverloo orifice", vec![acausal("in", Gr), acausal("out", Gr)], orifice).with_parameters(vec![P::required("diameter", "m").positive(), P::required("grain", "m").positive(), P::required("density", "kg/m³").positive(), P::optional("coefficient", "1", 0.58).nonnegative(), P::optional("k", "1", 1.5).nonnegative()]),
        BehaviorDescriptor::new(SINK, "Free fall at zero stress", vec![acausal("in", Gr)], sink).with_parameters(vec![]),
    ] {
        registry.register(descriptor)?;
    }
    Ok(())
}

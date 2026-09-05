//! Lumped circuit elements as compiled behaviors. Current is positive into
//! an element at its `p` pin.

use sim_core::{
    Behavior, Input, LocalJacobian, Output, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, QuantityKind, RegistryError,
    StateDeclaration, View, acausal, param, param_or, signal_in,
};
use std::collections::BTreeMap;

pub const GROUND: &str = "electrical.ground";
pub const VOLTAGE_SOURCE: &str = "electrical.voltage_source";
pub const CONTROLLED_VOLTAGE_SOURCE: &str = "electrical.controlled_voltage_source";
pub const CURRENT_SOURCE: &str = "electrical.current_source";
pub const RESISTOR: &str = "electrical.resistor";
pub const CAPACITOR: &str = "electrical.capacitor";
pub const INDUCTOR: &str = "electrical.inductor";
pub const CHUA_DIODE: &str = "electrical.chua_diode";

type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;

pub struct Ground;
impl Behavior for Ground {
    fn pinned(&self) -> Vec<(usize, usize, f64)> {
        vec![(0, 0, 0.0)]
    }
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("current", QuantityKind::Current, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.set_state_residual(0, ctx.across(0));
        ctx.add_through(0, ctx.state(0));
    }
    fn jacobian(&self, _view: &View, out: &mut LocalJacobian) -> bool {
        out.set(Output::State(0), Input::Across(0, 0), 1.0);
        out.through(0, Input::State(0), 1.0);
        true
    }
}
fn ground(_: &Params) -> Made {
    Ok(Box::new(Ground))
}

/// Ideal voltage source: `v_p − v_n = V`; its current is a multiplier state.
pub struct VoltageSource {
    pub voltage: f64,
    pub controlled: bool,
}
impl Behavior for VoltageSource {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("current", QuantityKind::Current, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        let voltage = if self.controlled { ctx.signal_in(0) } else { self.voltage };
        ctx.set_state_residual(0, ctx.across(0) - ctx.across(1) - voltage);
        let i = ctx.state(0);
        ctx.add_through(0, i);
        ctx.add_through(1, -i);
    }
    fn jacobian(&self, _view: &View, out: &mut LocalJacobian) -> bool {
        out.set(Output::State(0), Input::Across(0, 0), 1.0);
        out.set(Output::State(0), Input::Across(1, 0), -1.0);
        if self.controlled {
            out.set(Output::State(0), Input::Signal(0), -1.0);
        }
        out.through(0, Input::State(0), 1.0);
        out.through(1, Input::State(0), -1.0);
        true
    }
}
fn voltage_source(p: &Params) -> Made {
    Ok(Box::new(VoltageSource { voltage: param(p, "voltage")?, controlled: false }))
}
fn controlled_voltage_source(_: &Params) -> Made {
    Ok(Box::new(VoltageSource { voltage: 0.0, controlled: true }))
}

/// Ideal current source pushing `current` out of `p` into the circuit.
pub struct CurrentSource {
    pub current: f64,
}
impl Behavior for CurrentSource {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.add_through(0, -self.current);
        ctx.add_through(1, self.current);
    }
    fn jacobian(&self, _view: &View, _out: &mut LocalJacobian) -> bool {
        true
    }
}
fn current_source(p: &Params) -> Made {
    Ok(Box::new(CurrentSource { current: param(p, "current")? }))
}

pub struct Resistor {
    pub resistance: f64,
}
impl Behavior for Resistor {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let i = (ctx.across(0) - ctx.across(1)) / self.resistance;
        ctx.add_through(0, i);
        ctx.add_through(1, -i);
    }
    fn jacobian(&self, _view: &View, out: &mut LocalJacobian) -> bool {
        let g = 1.0 / self.resistance;
        for (port, sign) in [(0, 1.0), (1, -1.0)] {
            out.through(port, Input::Across(0, 0), sign * g);
            out.through(port, Input::Across(1, 0), -sign * g);
        }
        true
    }
}
fn resistor(p: &Params) -> Made {
    Ok(Box::new(Resistor { resistance: param(p, "resistance")? }))
}

pub struct Capacitor {
    pub capacitance: f64,
}
impl Behavior for Capacitor {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let i = self.capacitance * (ctx.across_rate(0) - ctx.across_rate(1));
        ctx.add_through(0, i);
        ctx.add_through(1, -i);
    }
    fn jacobian(&self, _view: &View, out: &mut LocalJacobian) -> bool {
        for (port, sign) in [(0, 1.0), (1, -1.0)] {
            out.through(port, Input::AcrossRate(0, 0), sign * self.capacitance);
            out.through(port, Input::AcrossRate(1, 0), -sign * self.capacitance);
        }
        true
    }
    fn energy(&self, view: &View) -> f64 {
        0.5 * self.capacitance * (view.across(0) - view.across(1)).powi(2)
    }
}
fn capacitor(p: &Params) -> Made {
    Ok(Box::new(Capacitor { capacitance: param(p, "capacitance")? }))
}

pub struct Inductor {
    pub inductance: f64,
    pub initial_current: f64,
}
impl Behavior for Inductor {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("current", QuantityKind::Current, self.initial_current)]
    }
    fn residual(&self, ctx: &mut Context) {
        let i = ctx.state(0);
        ctx.set_state_residual(0, self.inductance * ctx.state_rate(0) - (ctx.across(0) - ctx.across(1)));
        ctx.add_through(0, i);
        ctx.add_through(1, -i);
    }
    fn jacobian(&self, _view: &View, out: &mut LocalJacobian) -> bool {
        out.state_rate(0, 0, self.inductance);
        out.set(Output::State(0), Input::Across(0, 0), -1.0);
        out.set(Output::State(0), Input::Across(1, 0), 1.0);
        out.through(0, Input::State(0), 1.0);
        out.through(1, Input::State(0), -1.0);
        true
    }
    fn energy(&self, view: &View) -> f64 {
        0.5 * self.inductance * view.state(0).powi(2)
    }
}
fn inductor(p: &Params) -> Made {
    Ok(Box::new(Inductor { inductance: param(p, "inductance")?, initial_current: param_or(p, "initial.current", 0.0) }))
}

/// Chua's piecewise-linear negative resistance: slope `m0` inside ±`breakpoint`, `m1` outside.
pub struct ChuaDiode {
    pub m0: f64,
    pub m1: f64,
    pub breakpoint: f64,
}
impl ChuaDiode {
    pub fn current(&self, v: f64) -> f64 {
        let b = self.breakpoint;
        self.m1 * v + 0.5 * (self.m0 - self.m1) * ((v + b).abs() - (v - b).abs())
    }
}
impl Behavior for ChuaDiode {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let i = self.current(ctx.across(0) - ctx.across(1));
        ctx.add_through(0, i);
        ctx.add_through(1, -i);
    }
}
fn chua_diode(p: &Params) -> Made {
    Ok(Box::new(ChuaDiode { m0: param(p, "m0")?, m1: param(p, "m1")?, breakpoint: param_or(p, "breakpoint", 1.0) }))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    use ConnectorKind::Electrical as E;
    let two = || vec![acausal("p", E), acausal("n", E)];
    for descriptor in [
        BehaviorDescriptor::new(GROUND, "Electrical ground", vec![acausal("pin", E)], ground).with_parameters(vec![]),
        BehaviorDescriptor::new(VOLTAGE_SOURCE, "Ideal voltage source", two(), voltage_source).with_parameters(vec![P::required("voltage", "V")]),
        BehaviorDescriptor::new(CONTROLLED_VOLTAGE_SOURCE, "Controlled voltage source", vec![acausal("p", E), acausal("n", E), signal_in("voltage", QuantityKind::Voltage)], controlled_voltage_source).with_parameters(vec![]),
        BehaviorDescriptor::new(CURRENT_SOURCE, "Ideal current source", two(), current_source).with_parameters(vec![P::required("current", "A")]),
        BehaviorDescriptor::new(RESISTOR, "Resistor", two(), resistor).with_parameters(vec![P::required("resistance", "Ω")]),
        BehaviorDescriptor::new(CAPACITOR, "Capacitor", two(), capacitor).with_parameters(vec![P::required("capacitance", "F").positive()]),
        BehaviorDescriptor::new(INDUCTOR, "Inductor", two(), inductor).with_parameters(vec![P::required("inductance", "H").positive(), P::optional("initial.current", "A", 0.0)]),
        BehaviorDescriptor::new(CHUA_DIODE, "Chua diode", two(), chua_diode).with_parameters(vec![P::required("m0", "S"), P::required("m1", "S"), P::optional("breakpoint", "V", 1.0).positive()]),
    ] {
        registry.register(descriptor)?;
    }
    Ok(())
}

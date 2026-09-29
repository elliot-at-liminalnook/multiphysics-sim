//! Lumped circuit elements as compiled behaviors. Current is positive into
//! an element at its `p` pin.

use sim_core::{
    Behavior, Input, LocalJacobian, Output, BehaviorDescriptor, BehaviorRegistry, Context, QuantityKind, RegistryError,
    StateDeclaration, View, acausal, param, param_or, signal_in,
};
use std::collections::BTreeMap;

pub const GROUND: &str = "electrical.ground";
pub const VOLTAGE_SOURCE: &str = "electrical.voltage_source";
pub const SWITCHED_VOLTAGE_SOURCE: &str = "electrical.switched_voltage_source";
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

/// A supply that switches on at `on_at` and off at `off_at` with exact
/// scheduled edges: power a motor, then cut it to see what the load does.
pub struct SwitchedVoltageSource {
    pub voltage: f64,
    pub on_at: f64,
    pub off_at: f64,
}
impl SwitchedVoltageSource {
    fn edge(&self, phase: f64) -> Option<f64> {
        if phase < 0.5 {
            Some(self.on_at)
        } else if phase < 1.5 && self.off_at.is_finite() {
            Some(self.off_at)
        } else {
            None
        }
    }
}
impl Behavior for SwitchedVoltageSource {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![
            StateDeclaration::new("current", QuantityKind::Current, 0.0),
            StateDeclaration::new("phase", QuantityKind::Dimensionless, if self.on_at <= 0. { 1. } else { 0. }),
        ]
    }
    fn residual(&self, ctx: &mut Context) {
        let on = ctx.state(1) == 1.;
        ctx.set_state_residual(0, ctx.across(0) - ctx.across(1) - if on { self.voltage } else { 0. });
        ctx.set_state_residual(1, ctx.state_rate(1));
        let i = ctx.state(0);
        ctx.add_through(0, i);
        ctx.add_through(1, -i);
    }
    fn jacobian(&self, _view: &View, out: &mut LocalJacobian) -> bool {
        out.set(Output::State(0), Input::Across(0, 0), 1.0);
        out.set(Output::State(0), Input::Across(1, 0), -1.0);
        out.state_rate(1, 1, 1.0);
        out.through(0, Input::State(0), 1.0);
        out.through(1, Input::State(0), -1.0);
        true
    }
    fn guards(&self, view: &View, out: &mut Vec<f64>) {
        out.push(self.edge(view.state(1)).map_or(1., |t| t - view.time));
    }
    fn scheduled_events(&self, view: &View, out: &mut Vec<(usize, f64)>) {
        if let Some(t) = self.edge(view.state(1)) {
            out.push((0, t));
        }
    }
    fn jump(&mut self, _: usize, _: &View, states: &mut [f64]) {
        states[1] += 1.;
    }
}
fn switched_voltage_source(p: &Params) -> Made {
    Ok(Box::new(SwitchedVoltageSource { voltage: param(p, "voltage")?, on_at: param_or(p, "on_at", 0.0), off_at: param_or(p, "off_at", f64::INFINITY) }))
}

static SWITCHED_SOURCE_NOTES: sim_core::ComponentNotes = sim_core::ComponentNotes { category: "Power",
    explanation: "An ideal supply behind a switch: 0 V until `on_at`, then the set voltage, then 0 V (a short, like a motor driver braking) from `off_at`. The edges are scheduled exactly in simulation time.",
    equations: &["v_p − v_n = V for on_at ≤ t < off_at, else 0"],
    limits: "No internal resistance or current limit; add a resistor for a real supply's source resistance.",
    parameters: &[("voltage", "Voltage while on."), ("on_at", "Switch-on time."), ("off_at", "Switch-off time; leave unset to stay on.")],
    typical: &[("voltage", 12.0)],
    pairs_with: &["bridge.brushed_motor", GROUND, RESISTOR],
    active: true,
    ..sim_core::ComponentNotes::new("A voltage supply that turns on and off at set times.")
};

static GROUND_NOTES: sim_core::ComponentNotes = sim_core::ComponentNotes { category: "Electrical",
    explanation: "The 0 V reference. Every circuit needs one node tied to ground so voltages are defined.",
    pairs_with: &[VOLTAGE_SOURCE, SWITCHED_VOLTAGE_SOURCE, RESISTOR, CAPACITOR],
    ..sim_core::ComponentNotes::new("The 0 V reference node.")
};
static VOLTAGE_SOURCE_NOTES: sim_core::ComponentNotes = sim_core::ComponentNotes { category: "Power",
    explanation: "Holds a fixed voltage between its pins whatever current flows, like an ideal bench supply or battery with no internal resistance.",
    equations: &["v_p − v_n = V"],
    limits: "No internal resistance, current limit or discharge; use robot.battery for a battery model.",
    typical: &[("voltage", 12.0)],
    pairs_with: &[GROUND, RESISTOR, "bridge.brushed_motor", CAPACITOR, INDUCTOR],
    active: true,
    ..sim_core::ComponentNotes::new("An ideal constant-voltage supply.")
};
static RESISTOR_NOTES: sim_core::ComponentNotes = sim_core::ComponentNotes { category: "Electrical",
    explanation: "Converts electrical power to heat in proportion to the current squared.",
    equations: &["v = R·i", "power = i²·R"],
    typical: &[("resistance", 10.0)],
    pairs_with: &[VOLTAGE_SOURCE, GROUND, CAPACITOR, INDUCTOR],
    ..sim_core::ComponentNotes::new("Ohm's law: voltage proportional to current.")
};

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
    use sim_core::connectors::Electrical as E;
    let two = || vec![acausal("p", E), acausal("n", E)];
    for descriptor in [
        BehaviorDescriptor::new(GROUND, "Electrical ground", vec![acausal("pin", E)], ground).with_parameters(vec![]).with_notes(&GROUND_NOTES),
        BehaviorDescriptor::new(VOLTAGE_SOURCE, "Ideal voltage source", two(), voltage_source).with_parameters(vec![P::required("voltage", "V")]).with_notes(&VOLTAGE_SOURCE_NOTES),
        BehaviorDescriptor::new(SWITCHED_VOLTAGE_SOURCE, "Switched voltage supply", two(), switched_voltage_source)
            .with_parameters(vec![P::required("voltage", "V"), P::optional("on_at", "s", 0.0).nonnegative(), P::optional("off_at", "s", f64::INFINITY).nonnegative()])
            .with_notes(&SWITCHED_SOURCE_NOTES),
        BehaviorDescriptor::new(CONTROLLED_VOLTAGE_SOURCE, "Controlled voltage source", vec![acausal("p", E), acausal("n", E), signal_in("voltage", QuantityKind::Voltage)], controlled_voltage_source).with_parameters(vec![]),
        BehaviorDescriptor::new(CURRENT_SOURCE, "Ideal current source", two(), current_source).with_parameters(vec![P::required("current", "A")]),
        BehaviorDescriptor::new(RESISTOR, "Resistor", two(), resistor).with_parameters(vec![P::required("resistance", "Ω")]).with_notes(&RESISTOR_NOTES),
        BehaviorDescriptor::new(CAPACITOR, "Capacitor", two(), capacitor).with_parameters(vec![P::required("capacitance", "F").positive()]),
        BehaviorDescriptor::new(INDUCTOR, "Inductor", two(), inductor).with_parameters(vec![P::required("inductance", "H").positive(), P::optional("initial.current", "A", 0.0)]),
        BehaviorDescriptor::new(CHUA_DIODE, "Chua diode", two(), chua_diode).with_parameters(vec![P::required("m0", "S"), P::required("m1", "S"), P::optional("breakpoint", "V", 1.0).positive()]),
    ] {
        registry.register(descriptor)?;
    }
    crate::voltage_history::register(registry)?;
    crate::semiconductors::register(registry)
}

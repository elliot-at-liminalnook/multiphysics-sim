//! Switching and rectifying elements with explicit heat ports, and an ideal
//! voltage sense. Current is positive into an element at its first pin.
//!
//! The diode and body-diode characteristics are smooth piecewise-linear
//! approximations (a softplus knee of width `smoothing` around the forward
//! voltage), not a Shockley exponential: they are numerically robust at large
//! steps and are labelled as such. Conduction losses flow into the `heat`
//! port, so switching parts warm their thermal network. Constant output and
//! junction capacitances keep switch nodes well posed when every device is
//! off; switching (capacitive) losses are not modelled.
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, Context, ParameterDeclaration as P, QuantityKind, RegistryError, StateDeclaration, acausal, param_or, signal_in,
    signal_out,
};
use std::collections::BTreeMap;

pub const DIODE: &str = "electrical.diode";
pub const MOSFET: &str = "electrical.mosfet";
pub const VOLTAGE_SENSE: &str = "electrical.voltage_sense";

type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;

/// Smooth rectifier: `i = (w/R)·ln(1 + exp((v − V_f)/w)) + G_off·v`.
#[derive(Debug, Clone, Copy)]
pub struct Rectifier {
    pub forward_voltage: f64,
    pub on_resistance: f64,
    pub off_conductance: f64,
    pub smoothing: f64,
}

impl Rectifier {
    pub fn current(&self, v: f64) -> f64 {
        let x = (v - self.forward_voltage) / self.smoothing;
        // Numerically stable softplus.
        let softplus = if x > 30. { x } else if x < -30. { x.exp() } else { x.exp().ln_1p() };
        self.smoothing * softplus / self.on_resistance + self.off_conductance * v
    }
    fn from(p: &Params) -> Self {
        Self {
            forward_voltage: param_or(p, "forward_voltage", 0.7),
            on_resistance: param_or(p, "on_resistance", 0.05),
            off_conductance: param_or(p, "off_conductance", 1e-9),
            smoothing: param_or(p, "smoothing", 0.02),
        }
    }
}

/// Diode from anode `p` to cathode `n`, with a constant junction
/// capacitance so a reverse-biased node is never left floating.
pub struct Diode(pub Rectifier, pub f64);
impl Behavior for Diode {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let i = self.0.current(ctx.across(0) - ctx.across(1)) + self.1 * (ctx.across_rate(0) - ctx.across_rate(1));
        ctx.add_through(0, i);
        ctx.add_through(1, -i);
    }
}
fn diode(p: &Params) -> Made {
    Ok(Box::new(Diode(Rectifier::from(p), param_or(p, "junction_capacitance", 100e-12))))
}

/// N-channel switch between `drain` and `source` with a body diode
/// (source → drain). The gate signal turns the channel on at ≥ 0.5.
/// On-resistance rises with the `heat` port temperature, and every watt the
/// part dissipates (`v_ds · i_d`) flows into `heat`.
pub struct Mosfet {
    /// Drain–source (output) capacitance, constant.
    pub output_capacitance: f64,
    pub on_resistance: f64,
    pub off_conductance: f64,
    pub temperature_coefficient: f64,
    pub reference: f64,
    pub body: Rectifier,
}
impl Mosfet {
    pub fn on_resistance_at(&self, temperature: f64) -> f64 {
        self.on_resistance * (1. + self.temperature_coefficient * (temperature - self.reference)).max(0.1)
    }
    /// Drain current for a drain–source voltage, gate state and temperature.
    pub fn drain_current(&self, v_ds: f64, on: bool, temperature: f64) -> f64 {
        let channel = if on { v_ds / self.on_resistance_at(temperature) } else { self.off_conductance * v_ds };
        channel - self.body.current(-v_ds)
    }
}
impl Behavior for Mosfet {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let v = ctx.across(0) - ctx.across(1);
        let conduction = self.drain_current(v, ctx.signal_in(0) >= 0.5, ctx.across(2));
        // Charging the output capacitance stores energy; it is not dissipated.
        let displacement = self.output_capacitance * (ctx.across_rate(0) - ctx.across_rate(1));
        ctx.add_through(0, conduction + displacement);
        ctx.add_through(1, -conduction - displacement);
        ctx.add_through(2, -v * conduction);
    }
}
fn mosfet(p: &Params) -> Made {
    Ok(Box::new(Mosfet {
        output_capacitance: param_or(p, "output_capacitance", 500e-12),
        on_resistance: param_or(p, "on_resistance", 0.02),
        off_conductance: param_or(p, "off_conductance", 1e-8),
        temperature_coefficient: param_or(p, "temperature_coefficient", 0.004),
        reference: param_or(p, "reference", 298.15),
        body: Rectifier {
            forward_voltage: param_or(p, "body.forward_voltage", 0.8),
            on_resistance: param_or(p, "body.on_resistance", 0.02),
            off_conductance: 0.,
            smoothing: param_or(p, "body.smoothing", 0.02),
        },
    }))
}

/// Ideal voltmeter: draws no current; `reading = (v_p − v_n) / scale`.
pub struct VoltageSense {
    pub scale: f64,
}
impl Behavior for VoltageSense {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let v = ctx.across(0) - ctx.across(1);
        ctx.set_signal(0, v / self.scale);
    }
}
fn voltage_sense(p: &Params) -> Made {
    Ok(Box::new(VoltageSense { scale: param_or(p, "scale", 1.0) }))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::connectors::{Electrical as E, Thermal as H};
    registry.register(
        BehaviorDescriptor::new(DIODE, "Diode (smooth piecewise-linear)", vec![acausal("p", E), acausal("n", E)], diode).with_parameters(vec![
            P::optional("forward_voltage", "V", 0.7).nonnegative(),
            P::optional("on_resistance", "Ω", 0.05).positive(),
            P::optional("off_conductance", "S", 1e-9).nonnegative(),
            P::optional("smoothing", "V", 0.02).positive(),
            P::optional("junction_capacitance", "F", 100e-12).nonnegative(),
        ]),
    )?;
    registry.register(
        BehaviorDescriptor::new(
            MOSFET,
            "N-channel MOSFET switch with body diode and heat",
            vec![acausal("drain", E), acausal("source", E), signal_in("gate", QuantityKind::Dimensionless), acausal("heat", H)],
            mosfet,
        )
        .with_parameters(vec![
            P::optional("on_resistance", "Ω", 0.02).positive(),
            P::optional("output_capacitance", "F", 500e-12).nonnegative(),
            P::optional("off_conductance", "S", 1e-8).nonnegative(),
            P::optional("temperature_coefficient", "1/K", 0.004),
            P::optional("reference", "K", 298.15).positive(),
            P::optional("body.forward_voltage", "V", 0.8).nonnegative(),
            P::optional("body.on_resistance", "Ω", 0.02).positive(),
            P::optional("body.smoothing", "V", 0.02).positive(),
        ]),
    )?;
    registry.register(
        BehaviorDescriptor::new(VOLTAGE_SENSE, "Voltage sense (reading = volts / scale)", vec![acausal("p", E), acausal("n", E), signal_out("reading", QuantityKind::Dimensionless)], voltage_sense)
            .with_parameters(vec![P::optional("scale", "V", 1.0).positive()]),
    )
}

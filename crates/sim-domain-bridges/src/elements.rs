//! Behaviors whose ports span two physical domains.

use sim_core::{signal_out,
    Behavior, Input, LocalJacobian, Output, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, QuantityKind, RegistryError,
    StateDeclaration, View, acausal, param, param_or,
};
use std::collections::BTreeMap;

pub const THERMISTOR: &str = "bridge.thermistor";
pub const BRUSHED_MOTOR: &str = "bridge.brushed_motor";
pub const MOTOR: &str = "bridge.motor";
pub const DUAL_DRIVE: &str = "bridge.dual_drive";
pub const THERMOELASTIC_LAYER: &str = "bridge.thermoelastic_layer";

type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;

/// Resistor whose resistance follows its own temperature,
/// `R = R₀·exp(α·(T − T_ref))`, dumping its dissipation into the thermal port.
pub struct Thermistor {
    pub resistance: f64,
    pub coefficient: f64,
    pub reference: f64,
}
impl Thermistor {
    pub fn resistance_at(&self, temperature: f64) -> f64 {
        self.resistance * (self.coefficient * (temperature - self.reference)).exp()
    }
}
impl Behavior for Thermistor {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let r = self.resistance_at(ctx.across(2));
        let v = ctx.across(0) - ctx.across(1);
        let i = v / r;
        ctx.add_through(0, i);
        ctx.add_through(1, -i);
        ctx.add_through(2, -v * i);
    }
}
fn thermistor(p: &Params) -> Made {
    Ok(Box::new(Thermistor { resistance: param(p, "resistance")?, coefficient: param(p, "coefficient")?, reference: param(p, "reference")? }))
}

/// Brushed DC motor between pins `p`, `n` and a shaft against its case.
/// With zero inductance the winding current is algebraic.
pub struct BrushedMotor {
    pub resistance: f64,
    pub inductance: f64,
    pub torque_constant: f64,
    pub back_emf_constant: f64,
}
impl Behavior for BrushedMotor {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("current", QuantityKind::Current, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        let i = ctx.state(0);
        let v = ctx.across(0) - ctx.across(1);
        let speed = ctx.across_rate(2) - ctx.across_rate(3);
        let back_emf = self.back_emf_constant * speed;
        let residual = if self.inductance > 0.0 {
            self.inductance * ctx.state_rate(0) - (v - self.resistance * i - back_emf)
        } else {
            self.resistance * i - (v - back_emf)
        };
        ctx.set_state_residual(0, residual);
        ctx.add_through(0, i);
        ctx.add_through(1, -i);
        let torque = self.torque_constant * i;
        ctx.add_through(2, -torque);
        ctx.add_through(3, torque);
    }
    fn jacobian(&self, _view: &View, out: &mut LocalJacobian) -> bool {
        // Winding row: L·i' + R·i − v + k_e·(ω₂ − ω₃) (or R·i − v + k_e·Δω).
        if self.inductance > 0.0 {
            out.state_rate(0, 0, self.inductance);
        }
        out.state_state(0, 0, self.resistance);
        out.set(Output::State(0), Input::Across(0, 0), -1.0);
        out.set(Output::State(0), Input::Across(1, 0), 1.0);
        out.set(Output::State(0), Input::AcrossRate(2, 0), self.back_emf_constant);
        out.set(Output::State(0), Input::AcrossRate(3, 0), -self.back_emf_constant);
        out.through(0, Input::State(0), 1.0);
        out.through(1, Input::State(0), -1.0);
        out.through(2, Input::State(0), -self.torque_constant);
        out.through(3, Input::State(0), self.torque_constant);
        true
    }
    fn energy(&self, view: &View) -> f64 {
        0.5 * self.inductance * view.state(0).powi(2)
    }
}
fn brushed_motor(p: &Params) -> Made {
    Ok(Box::new(BrushedMotor {
        resistance: param(p, "resistance")?,
        inductance: param_or(p, "inductance", 0.0),
        torque_constant: param(p, "torque_constant")?,
        back_emf_constant: param(p, "back_emf_constant")?,
    }))
}

/// A motor behind one `Motor` plug (`ConnectorKind::MOTOR`): winding
/// terminal with its return through the chassis, shaft, and case. The
/// winding resistance follows the thermistor law `R₀·exp(α(T − T_ref))`,
/// so a negative coefficient makes two motors on one drive hog exactly as
/// two thermistors do.
pub struct Motor {
    pub resistance: f64,
    pub coefficient: f64,
    pub reference: f64,
    pub torque_constant: f64,
}
impl Motor {
    pub fn resistance_at(&self, temperature: f64) -> f64 {
        self.resistance * (self.coefficient * (temperature - self.reference)).exp()
    }
}
impl Behavior for Motor {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let plug = ConnectorKind::MOTOR;
        let (winding, shaft, case) = (plug.member_offset(0), plug.member_offset(1), plug.member_offset(2));
        let v = ctx.across_lane(0, winding);
        let speed = ctx.across_rate_lane(0, shaft);
        let r = self.resistance_at(ctx.across_lane(0, case));
        let i = (v - self.torque_constant * speed) / r;
        ctx.add_through_lane(0, winding, i);
        ctx.add_through_lane(0, shaft, -self.torque_constant * i);
        ctx.add_through_lane(0, case, -i * i * r);
    }
}
fn motor(p: &Params) -> Made {
    Ok(Box::new(Motor {
        resistance: param(p, "resistance")?,
        coefficient: param_or(p, "coefficient", 0.0),
        reference: param_or(p, "reference", 293.15),
        torque_constant: param(p, "torque_constant")?,
    }))
}

/// A drive with two `Motor` sockets: it regulates the total winding current
/// and lets the two windings share it through one internal bus, touches
/// neither shaft nor case, and reports the hotter case — a drive that knows
/// its motor is hot.
pub struct DualDrive {
    pub current: f64,
}
impl Behavior for DualDrive {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("current_a", QuantityKind::Current, 0.5 * self.current)]
    }
    fn residual(&self, ctx: &mut Context) {
        let plug = ConnectorKind::MOTOR;
        let (winding, case) = (plug.member_offset(0), plug.member_offset(2));
        let current_a = ctx.state(0);
        // One bus: both sockets sit at the same potential.
        ctx.set_state_residual(0, ctx.across_lane(0, winding) - ctx.across_lane(1, winding));
        ctx.add_through_lane(0, winding, -current_a);
        ctx.add_through_lane(1, winding, -(self.current - current_a));
        ctx.set_signal(0, ctx.across_lane(0, case).max(ctx.across_lane(1, case)));
    }
}
fn dual_drive(p: &Params) -> Made {
    Ok(Box::new(DualDrive { current: param(p, "current")? }))
}

/// One layer of a bending beam's cross-section: strain rate heats it,
/// its temperature bends the beam back. Ports: bending (across curvature,
/// through moment) and the layer's thermal node.
pub struct ThermoelasticLayer {
    /// Sign of the coupling; anything but +1 is unphysical and exists only
    /// so a scenario can show the compiler rejecting it.
    pub sign: f64,
    pub height: f64,
    pub thickness: f64,
    pub width: f64,
    pub youngs_modulus: f64,
    pub expansion: f64,
    pub temperature: f64,
}
impl Behavior for ThermoelasticLayer {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let area = self.width * self.thickness;
        let coupling = self.sign * self.youngs_modulus * self.expansion * self.height * area;
        // Thermal moment into the beam's balance; heat out of the layer node.
        let moment = coupling * ctx.across(1);
        let heating = coupling * self.temperature * ctx.across_rate(0);
        ctx.add_through(0, moment);
        ctx.add_through(1, -heating);
        // Thermoelastic coupling is reversible: the heat it exchanges is
        // stored, not produced. (Reverse its sign and it is neither: the
        // compiler rejects the model.)
        ctx.store_entropy(-heating / ctx.across(1));
    }
}
fn thermoelastic_layer(p: &Params) -> Made {
    Ok(Box::new(ThermoelasticLayer {
        sign: param_or(p, "sign", 1.0),
        height: param(p, "height")?,
        thickness: param(p, "thickness")?,
        width: param(p, "width")?,
        youngs_modulus: param(p, "youngs_modulus")?,
        expansion: param(p, "expansion")?,
        temperature: param(p, "temperature")?,
    }))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    use ConnectorKind::{Electrical as E, Rotational as R, Thermal as H};
    for descriptor in [
        BehaviorDescriptor::new(THERMISTOR, "Temperature-dependent resistor", vec![acausal("p", E), acausal("n", E), acausal("heat", H)], thermistor).with_parameters(vec![P::required("resistance", "Ω").positive(), P::required("coefficient", "1/K"), P::required("reference", "K").positive()]),
        BehaviorDescriptor::new(BRUSHED_MOTOR, "Brushed DC motor", vec![acausal("p", E), acausal("n", E), acausal("shaft", R), acausal("case", R)], brushed_motor).with_parameters(vec![P::required("resistance", "Ω").positive(), P::optional("inductance", "H", 0.0).nonnegative(), P::required("torque_constant", "N·m/A"), P::required("back_emf_constant", "V·s/rad")]),
        BehaviorDescriptor::new(THERMOELASTIC_LAYER, "Thermoelastic beam layer", vec![acausal("bending", R), acausal("layer", H)], thermoelastic_layer).with_parameters(vec![P::optional("sign", "1", 1.0), P::required("height", "m"), P::required("thickness", "m").positive(), P::required("width", "m").positive(), P::required("youngs_modulus", "Pa").positive(), P::required("expansion", "1/K"), P::required("temperature", "K").positive()]),
        BehaviorDescriptor::new(MOTOR, "Motor behind one plug", vec![acausal("plug", ConnectorKind::MOTOR)], motor).with_parameters(vec![P::required("resistance", "Ω").positive(), P::optional("coefficient", "1/K", 0.0), P::optional("reference", "K", 293.15).positive(), P::required("torque_constant", "N·m/A")]),
        BehaviorDescriptor::new(DUAL_DRIVE, "Two-socket current drive", vec![acausal("a", ConnectorKind::MOTOR), acausal("b", ConnectorKind::MOTOR), signal_out("hottest", QuantityKind::Temperature)], dual_drive).with_parameters(vec![P::required("current", "A")]),
    ] {
        registry.register(descriptor)?;
    }
    Ok(())
}

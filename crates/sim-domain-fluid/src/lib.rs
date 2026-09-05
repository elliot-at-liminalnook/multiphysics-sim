//! Aerodynamic loads on structures.

pub mod twophase;

use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, QuantityKind, RegistryError, StateDeclaration, acausal,
    param, param_or,
};
use std::collections::BTreeMap;

pub const QUASI_STEADY_SECTION: &str = "fluid.quasi_steady_section";

type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;

/// Quasi-steady thin-airfoil loads on a pitch–plunge section:
/// `L = 2πρUb(Uα + ḣ + b(½ − a)α̇)`, moment `b(½ + a)·L` about the elastic
/// axis. Plunge is positive downward; the section's translational port
/// carries h and its rotational port α.
pub struct QuasiSteadySection {
    pub air_density: f64,
    pub airspeed: f64,
    pub semichord: f64,
    pub axis_offset: f64,
}
impl QuasiSteadySection {
    pub fn lift(&self, alpha: f64, plunge_rate: f64, pitch_rate: f64) -> f64 {
        let (u, b) = (self.airspeed, self.semichord);
        2.0 * std::f64::consts::PI * self.air_density * u * b * (u * alpha + plunge_rate + b * (0.5 - self.axis_offset) * pitch_rate)
    }
}
impl Behavior for QuasiSteadySection {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let lift = self.lift(ctx.across(1), ctx.across_rate(0), ctx.across_rate(1));
        let moment = self.semichord * (0.5 + self.axis_offset) * lift;
        // Lift pushes the section up (−h); the moment pitches it nose-up (+α).
        ctx.add_through(0, lift);
        ctx.add_through(1, -moment);
    }
}
fn quasi_steady_section(p: &Params) -> Made {
    Ok(Box::new(QuasiSteadySection {
        air_density: param_or(p, "air_density", 1.225),
        airspeed: param(p, "airspeed")?,
        semichord: param(p, "semichord")?,
        axis_offset: param(p, "axis_offset")?,
    }))
}

pub const WAKE_OSCILLATOR: &str = "fluid.wake_oscillator";

/// Facchinetti, de Langre & Biolley's wake oscillator (2004): the lift
/// coefficient of a cylinder of `diameter` in a cross-flow `speed` follows a
/// van der Pol variable `q` at the shedding frequency `2π·St·U/D`, forced by
/// the cylinder's own acceleration (`coupling`·ÿ/D). It lifts the structure
/// with `½ρU²D·C_L0·q/2` per length and damps it with `½ρUD·C_D·ẏ`. Set
/// `coupling = 0` and the wake no longer listens: resonance, but no lock-in.
pub struct WakeOscillator {
    pub density: f64,
    pub speed: f64,
    pub diameter: f64,
    pub strouhal: f64,
    pub lift_coefficient: f64,
    pub drag_coefficient: f64,
    pub epsilon: f64,
    pub coupling: f64,
    pub length: f64,
}
impl WakeOscillator {
    pub fn shedding_frequency(&self) -> f64 {
        2.0 * std::f64::consts::PI * self.strouhal * self.speed / self.diameter
    }
}
impl Behavior for WakeOscillator {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("q", QuantityKind::Dimensionless, 2.0), StateDeclaration::new("q_dot", QuantityKind::Frequency, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        let (q, qd) = (ctx.state(0), ctx.state(1));
        let omega = self.shedding_frequency();
        let velocity = ctx.across_rate(0);
        let acceleration = ctx.across_derivative(0, 1);
        ctx.set_state_residual(0, ctx.state_rate(0) - qd);
        ctx.set_state_residual(1, ctx.state_rate(1) + self.epsilon * omega * (q * q - 1.0) * qd + omega * omega * q - self.coupling * acceleration / self.diameter);
        let dynamic = 0.5 * self.density * self.speed * self.speed * self.diameter * self.length;
        let lift = dynamic * self.lift_coefficient * q / 2.0;
        let damping = 0.5 * self.density * self.speed * self.diameter * self.drag_coefficient * self.length * velocity;
        ctx.add_through(0, -(lift - damping));
    }
}
fn wake_oscillator(p: &Params) -> Made {
    Ok(Box::new(WakeOscillator {
        density: param_or(p, "density", 1000.0),
        speed: param(p, "speed")?,
        diameter: param(p, "diameter")?,
        strouhal: param_or(p, "strouhal", 0.2),
        lift_coefficient: param_or(p, "lift_coefficient", 0.3),
        drag_coefficient: param_or(p, "drag_coefficient", 1.2),
        epsilon: param_or(p, "epsilon", 0.3),
        coupling: param_or(p, "coupling", 12.0),
        length: param(p, "length")?,
    }))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    registry.register(BehaviorDescriptor::new(WAKE_OSCILLATOR, "Van der Pol wake oscillator", vec![acausal("structure", ConnectorKind::Translational)], wake_oscillator).with_parameters(vec![
        P::optional("density", "kg/m³", 1000.0).positive(), P::required("speed", "m/s").nonnegative(),
        P::required("diameter", "m").positive(), P::optional("strouhal", "1", 0.2).nonnegative(),
        P::optional("lift_coefficient", "1", 0.3), P::optional("drag_coefficient", "1", 1.2).nonnegative(),
        P::optional("epsilon", "1", 0.3), P::optional("coupling", "1", 12.0), P::required("length", "m").positive(),
    ]))?;
    registry.register(BehaviorDescriptor::new(
        QUASI_STEADY_SECTION,
        "Quasi-steady aerodynamic section",
        vec![acausal("plunge", ConnectorKind::Translational), acausal("pitch", ConnectorKind::Rotational)],
        quasi_steady_section,
    ).with_parameters(vec![P::optional("air_density", "kg/m³", 1.225).positive(),
        P::required("airspeed", "m/s").nonnegative(), P::required("semichord", "m").positive(),
        P::required("axis_offset", "1")]))
}

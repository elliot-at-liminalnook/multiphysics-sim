//! Galerkin acoustics: an open duct's modes with a tap where a heater (or
//! any volume source) couples in, plus King's-law heat release.

use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, QuantityKind, RegistryError,
    StateDeclaration, View, acausal, param, param_or, signal_in, signal_out,
};
use std::collections::BTreeMap;
use std::f64::consts::PI;

pub const DUCT_MODES: &str = "acoustic.duct_modes";
pub const HEAT_RELEASE: &str = "acoustic.heat_release";

type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;

/// Non-dimensional open–open duct (length 1, sound speed 1) with `modes`
/// Galerkin modes and damping `c1·j² + c2·√j`. The tap port's across is the
/// acoustic pressure at `tap`; its through is the volume source injected
/// there. Velocity at the tap is published as a signal.
pub struct DuctModes {
    pub modes: usize,
    pub tap: f64,
    pub c1: f64,
    pub c2: f64,
    pub initial_amplitude: f64,
}
impl DuctModes {
    fn k(j: usize) -> f64 {
        (j + 1) as f64 * PI
    }
    fn damping(&self, j: usize) -> f64 {
        let n = (j + 1) as f64;
        self.c1 * n * n + self.c2 * n.sqrt()
    }
    pub fn pressure_at(&self, states: &[f64], x: f64) -> f64 {
        -(0..self.modes).map(|j| states[self.modes + j] / Self::k(j) * (Self::k(j) * x).sin()).sum::<f64>()
    }
    pub fn velocity_at(&self, states: &[f64], x: f64) -> f64 {
        (0..self.modes).map(|j| states[j] * (Self::k(j) * x).cos()).sum()
    }
}
impl Behavior for DuctModes {
    fn states(&self) -> Vec<StateDeclaration> {
        let mut states = Vec::new();
        for j in 0..self.modes {
            states.push(StateDeclaration::new(format!("eta{j}"), QuantityKind::Dimensionless, if j == 0 { self.initial_amplitude } else { 0.0 }));
        }
        for j in 0..self.modes {
            states.push(StateDeclaration::new(format!("eta_dot{j}"), QuantityKind::Dimensionless, 0.0));
        }
        states.push(StateDeclaration::new("source", QuantityKind::Dimensionless, 0.0));
        states
    }
    fn residual(&self, ctx: &mut Context) {
        let n = self.modes;
        let source = ctx.state(2 * n);
        for j in 0..n {
            let k = Self::k(j);
            ctx.set_state_residual(j, ctx.state_rate(j) - ctx.state(n + j));
            let forcing = -2.0 * k * (k * self.tap).sin() * source;
            ctx.set_state_residual(n + j, ctx.state_rate(n + j) + k * k * ctx.state(j) + self.damping(j) * ctx.state(n + j) - forcing);
        }
        // The tap node's pressure is the modal pressure; the source is the
        // through the node pushes into the duct.
        let pressure = self.pressure_at(ctx.states(), self.tap);
        ctx.set_state_residual(2 * n, ctx.across(0) - pressure);
        ctx.add_through(0, source);
        let velocity = self.velocity_at(ctx.states(), self.tap);
        ctx.set_signal(0, velocity);
    }
    fn energy(&self, view: &View) -> f64 {
        (0..self.modes).map(|j| 0.5 * (view.state(self.modes + j).powi(2) + (Self::k(j) * view.state(j)).powi(2))).sum()
    }
}
fn duct_modes(p: &Params) -> Made {
    Ok(Box::new(DuctModes {
        modes: param_or(p, "modes", 3.0) as usize,
        tap: param(p, "tap")?,
        c1: param_or(p, "c1", 0.1),
        c2: param_or(p, "c2", 0.06),
        initial_amplitude: param_or(p, "initial.amplitude", 1.0e-3),
    }))
}

/// King's-law heat release `β(√|u₀ + u| − √u₀)` driven by the (delayed)
/// velocity on its input, injected as a volume source at its port.
pub struct HeatRelease {
    pub power: f64,
    pub mean_velocity: f64,
}
impl HeatRelease {
    pub fn release(&self, velocity: f64) -> f64 {
        self.power * ((self.mean_velocity + velocity).abs().sqrt() - self.mean_velocity.sqrt())
    }
}
impl Behavior for HeatRelease {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let q = self.release(ctx.signal_in(0));
        ctx.add_through(0, -q);
        ctx.set_signal(0, q);
    }
}
fn heat_release(p: &Params) -> Made {
    Ok(Box::new(HeatRelease { power: param(p, "power")?, mean_velocity: param_or(p, "mean_velocity", 1.0 / 3.0) }))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    use ConnectorKind::NormalizedAcoustic as A;
    use QuantityKind::Dimensionless as D;
    for descriptor in [
        BehaviorDescriptor::new(DUCT_MODES, "Normalized open duct Galerkin modes", vec![acausal("tap", A), signal_out("velocity", D)], duct_modes).with_parameters(vec![P::optional("modes", "1", 3.0).integer(1.0, 4096.0), P::required("tap", "1").nonnegative().at_most(1.0), P::optional("c1", "1", 0.1), P::optional("c2", "1", 0.06), P::optional("initial.amplitude", "1", 1.0e-3)]),
        BehaviorDescriptor::new(HEAT_RELEASE, "Normalized King's-law heat release", vec![acausal("tap", A), signal_in("velocity", D), signal_out("heat", D)], heat_release).with_parameters(vec![P::required("power", "1"), P::optional("mean_velocity", "1", 1.0/3.0).nonnegative()]),
    ] {
        registry.register(descriptor)?;
    }
    Ok(())
}

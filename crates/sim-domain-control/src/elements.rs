//! Signal sources, filters and controllers as compiled behaviors. Sampled
//! controllers hold their outputs in states with zero rate and update them
//! in jumps fired by a time guard — the same event machinery every domain uses.

use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, Context, QuantityKind, RegistryError, StateDeclaration, View,
    param, param_or, signal_in, signal_out,
};
use std::collections::BTreeMap;

pub const CONSTANT: &str = "control.constant";
pub const SINE: &str = "control.sine";
pub const PI_CONTROLLER: &str = "control.pi";
pub const SAMPLED_PROPORTIONAL: &str = "control.sampled_p";
pub const LAG_CHAIN: &str = "control.lag_chain";

type Params = BTreeMap<String, f64>;
type Made = Result<Box<dyn Behavior>, sim_core::EquationError>;

pub struct Constant {
    pub value: f64,
}
impl Behavior for Constant {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.set_signal(0, self.value);
    }
}
fn constant(p: &Params) -> Made {
    Ok(Box::new(Constant { value: param(p, "value")? }))
}

/// `amplitude · cos(2π f t + phase)`.
pub struct Sine {
    pub amplitude: f64,
    pub frequency: f64,
    pub phase: f64,
}
impl Behavior for Sine {
    fn states(&self) -> Vec<StateDeclaration> {
        Vec::new()
    }
    fn residual(&self, ctx: &mut Context) {
        let value = self.amplitude * (std::f64::consts::TAU * self.frequency * ctx.time + self.phase).cos();
        ctx.set_signal(0, value);
    }
}
fn sine(p: &Params) -> Made {
    Ok(Box::new(Sine { amplitude: param(p, "amplitude")?, frequency: param(p, "frequency")?, phase: param_or(p, "phase", 0.0) }))
}

/// Continuous PI regulator on `setpoint − measured`, with optional
/// derivative-style rate damping on a second input.
pub struct Pi {
    pub kp: f64,
    pub ki: f64,
    pub setpoint: f64,
}
impl Behavior for Pi {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("integral", QuantityKind::Dimensionless, 0.0)]
    }
    fn residual(&self, ctx: &mut Context) {
        let error = self.setpoint - ctx.signal_in(0);
        ctx.set_state_residual(0, ctx.state_rate(0) - error);
        let command = self.kp * error + self.ki * ctx.state(0);
        ctx.set_signal(0, command);
    }
}
fn pi(p: &Params) -> Made {
    Ok(Box::new(Pi { kp: param(p, "kp")?, ki: param_or(p, "ki", 0.0), setpoint: param_or(p, "setpoint", 0.0) }))
}

/// Zero-order-hold proportional controller sampled every `period`.
pub struct SampledProportional {
    pub gain: f64,
    pub period: f64,
    pub limit: f64,
    pub setpoint: f64,
}
impl Behavior for SampledProportional {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![
            StateDeclaration::new("held", QuantityKind::Dimensionless, 0.0),
            StateDeclaration::new("next_sample", QuantityKind::Time, 0.0),
        ]
    }
    fn residual(&self, ctx: &mut Context) {
        ctx.set_state_residual(0, ctx.state_rate(0));
        ctx.set_state_residual(1, ctx.state_rate(1));
        let held = ctx.state(0);
        ctx.set_signal(0, held);
    }
    fn guards(&self, view: &View, out: &mut Vec<f64>) {
        out.push(view.state(1) - view.time);
    }
    fn jump(&mut self, _index: usize, view: &View, states: &mut [f64]) {
        let command = self.gain * (self.setpoint - view.signal_in(0));
        states[0] = command.clamp(-self.limit, self.limit);
        states[1] += self.period;
    }
}
fn sampled_p(p: &Params) -> Made {
    Ok(Box::new(SampledProportional { gain: param(p, "gain")?, period: param(p, "period")?, limit: param_or(p, "limit", f64::INFINITY), setpoint: param_or(p, "setpoint", 0.0) }))
}

/// `stages` first-order lags in series with total time constant `delay`:
/// an Erlang approximation of a pure transport delay.
pub struct LagChain {
    pub stages: usize,
    pub delay: f64,
}
impl Behavior for LagChain {
    fn states(&self) -> Vec<StateDeclaration> {
        (0..self.stages).map(|k| StateDeclaration::new(format!("stage{k}"), QuantityKind::Dimensionless, 0.0)).collect()
    }
    fn residual(&self, ctx: &mut Context) {
        let rate = self.stages as f64 / self.delay;
        let mut input = ctx.signal_in(0);
        for k in 0..self.stages {
            let residual = ctx.state_rate(k) - (input - ctx.state(k)) * rate;
            ctx.set_state_residual(k, residual);
            input = ctx.state(k);
        }
        ctx.set_signal(0, input);
    }
}
fn lag_chain(p: &Params) -> Made {
    Ok(Box::new(LagChain { stages: param_or(p, "stages", 8.0) as usize, delay: param(p, "delay")? }))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    use QuantityKind::Dimensionless as D;
    for descriptor in [
        BehaviorDescriptor::new(CONSTANT, "Constant signal", vec![signal_out("value", D)], constant).with_parameters(vec![P::required("value", "1")]),
        BehaviorDescriptor::new(SINE, "Sinusoidal signal", vec![signal_out("value", D)], sine).with_parameters(vec![P::required("amplitude", "1"), P::required("frequency", "Hz"), P::optional("phase", "rad", 0.0)]),
        BehaviorDescriptor::new(PI_CONTROLLER, "PI regulator", vec![signal_in("measured", D), signal_out("command", D)], pi).with_parameters(vec![P::required("kp", "1"), P::optional("ki", "1/s", 0.0), P::optional("setpoint", "1", 0.0)]),
        BehaviorDescriptor::new(SAMPLED_PROPORTIONAL, "Sampled proportional controller", vec![signal_in("measured", D), signal_out("command", D)], sampled_p).with_parameters(vec![P::required("gain", "1"), P::required("period", "s").positive(), P::optional("limit", "1", f64::INFINITY).nonnegative(), P::optional("setpoint", "1", 0.0)]),
        BehaviorDescriptor::new(LAG_CHAIN, "Lag chain (transport delay)", vec![signal_in("input", D), signal_out("output", D)], lag_chain).with_parameters(vec![P::optional("stages", "1", 8.0).integer(1.0, 1024.0), P::required("delay", "s").positive()]),
    ] {
        registry.register(descriptor)?;
    }
    crate::external::register(registry)
}

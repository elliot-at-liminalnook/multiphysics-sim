//! Sampled angular bias with bounded state and slew, independent of any plant.
use serde::{Deserialize, Serialize};
use sim_core::{Behavior, BehaviorDescriptor, BehaviorRegistry, Context, QuantityKind,
    RegistryError, StateDeclaration, View, param, signal_in, signal_out};

pub const ANGLE_INTEGRAL: &str = "control.angle_integral";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AngleIntegralConfig {
    pub period_s: f64,
    pub integral_gain_per_s: f64,
    pub leak_rate_per_s: f64,
    pub maximum_bias_rad: f64,
    pub maximum_rate_rad_s: f64,
}

#[derive(Clone, Debug)]
pub struct AngleIntegral {
    config: AngleIntegralConfig,
}
impl AngleIntegral {
    pub fn new(config: AngleIntegralConfig) -> Result<Self, String> {
        if [config.period_s, config.maximum_bias_rad, config.maximum_rate_rad_s]
            .iter().any(|v| !v.is_finite() || *v <= 0.)
            || [config.integral_gain_per_s, config.leak_rate_per_s]
                .iter().any(|v| !v.is_finite() || *v < 0.)
            || ![config.period_s * config.integral_gain_per_s,
                config.period_s * config.leak_rate_per_s,
                config.period_s * config.maximum_rate_rad_s]
                .iter().all(|v| v.is_finite())
        {
            return Err("angle integral requires positive finite period/bounds/rate, nonnegative finite gains and finite sample increments".into());
        }
        Ok(Self { config })
    }

    /// The caller owns the bias state and calls once per declared period.
    /// Disabled updates slew toward zero. The only memory is the bounded bias;
    /// no hidden accumulator can wind up beyond saturation. Errors mutate nothing.
    pub fn update(&self, bias_rad: f64, error_rad: f64, enabled: bool) -> Result<f64, String> {
        if !bias_rad.is_finite() || bias_rad.abs() > self.config.maximum_bias_rad || !error_rad.is_finite() {
            return Err("angle integral requires finite error and bounded finite prior bias".into());
        }
        let c = &self.config;
        let desired = if enabled {
            (bias_rad + c.period_s * c.integral_gain_per_s * error_rad)
                / (1. + c.period_s * c.leak_rate_per_s)
        } else { 0. };
        let change = desired - bias_rad;
        if !desired.is_finite() || !change.is_finite() {
            return Err("nonfinite angle integral update".into());
        }
        let maximum_change = c.period_s * c.maximum_rate_rad_s;
        Ok((bias_rad + change.clamp(-maximum_change, maximum_change))
            .clamp(-c.maximum_bias_rad, c.maximum_bias_rad))
    }
}

struct Registered(AngleIntegral);
impl Behavior for Registered {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![StateDeclaration::new("samples", QuantityKind::Dimensionless, 0.),
            StateDeclaration::new("bias", QuantityKind::Angle, 0.)]
    }
    fn residual(&self, ctx: &mut Context) {
        for i in 0..2 { ctx.set_state_residual(i, ctx.state_rate(i)); }
        ctx.set_signal(0, ctx.state(1));
    }
    fn guards(&self, view: &View, out: &mut Vec<f64>) {
        out.push(view.state(0) * self.0.config.period_s - view.time);
    }
    fn scheduled_events(&self, view: &View, out: &mut Vec<(usize, f64)>) {
        out.push((0, view.state(0) * self.0.config.period_s));
    }
    fn jump(&mut self, _: usize, view: &View, states: &mut [f64]) {
        let gate = view.signal_in(1);
        states[1] = if gate.is_finite() {
            self.0.update(view.state(1), view.signal_in(0), gate >= 1.).unwrap_or(f64::NAN)
        } else { f64::NAN };
        states[0] = view.state(0) + 1.;
    }
}
fn make(p: &std::collections::BTreeMap<String, f64>) -> Result<Box<dyn Behavior>, sim_core::EquationError> {
    let config = AngleIntegralConfig {
        period_s: param(p, "period_s")?,
        integral_gain_per_s: param(p, "integral_gain_per_s")?,
        leak_rate_per_s: param(p, "leak_rate_per_s")?,
        maximum_bias_rad: param(p, "maximum_bias_rad")?,
        maximum_rate_rad_s: param(p, "maximum_rate_rad_s")?,
    };
    Ok(Box::new(Registered(AngleIntegral::new(config).map_err(|e|
        sim_core::EquationError::InvalidParameter("angle_integral".into(), e))?)))
}
pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    registry.register(BehaviorDescriptor::new(ANGLE_INTEGRAL, "Bounded sampled angular integral bias",
        vec![signal_in("error", QuantityKind::Angle), signal_in("enabled", QuantityKind::Dimensionless),
            signal_out("bias", QuantityKind::Angle)], make)
        .with_parameters(vec![P::required("period_s", "s").positive(),
            P::required("integral_gain_per_s", "1/s").nonnegative(),
            P::required("leak_rate_per_s", "1/s").nonnegative(),
            P::required("maximum_bias_rad", "rad").positive(),
            P::required("maximum_rate_rad_s", "rad/s").positive()]))
}

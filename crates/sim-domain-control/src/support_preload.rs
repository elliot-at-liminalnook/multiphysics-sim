//! Bounded sampled integral force feedback for a compliant position-controlled
//! support. Positive output requests additional extension; it applies no force.
use serde::{Deserialize, Serialize};
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, Context, QuantityKind, RegistryError,
    StateDeclaration, View, param, signal_in, signal_out,
};
pub const SUPPORT_PRELOAD: &str = "control.support_preload";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportPreloadConfig {
    pub period_s: f64,
    pub integral_gain_m_per_ns: f64,
    pub maximum_extension_m: f64,
}
#[derive(Clone, Debug)]
pub struct SupportPreload {
    config: SupportPreloadConfig,
}
impl SupportPreload {
    pub fn new(config: SupportPreloadConfig) -> Result<Self, String> {
        if [
            config.period_s,
            config.integral_gain_m_per_ns,
            config.maximum_extension_m,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.)
        {
            return Err("positive finite support-preload scales required".into());
        }
        Ok(Self { config })
    }
    pub fn update(
        &self,
        extension: f64,
        target_force: f64,
        measured_force: f64,
        enabled: bool,
    ) -> Result<f64, String> {
        if !extension.is_finite()
            || extension < 0.
            || extension > self.config.maximum_extension_m
            || [target_force, measured_force]
                .iter()
                .any(|v| !v.is_finite() || *v < 0.)
        {
            return Err("finite nonnegative loads and bounded extension required".into());
        }
        if !enabled {
            return Ok(0.);
        }
        let next = extension
            + self.config.period_s
                * self.config.integral_gain_m_per_ns
                * (target_force - measured_force);
        if !next.is_finite() {
            return Err("nonfinite support-preload update".into());
        }
        Ok(next.clamp(0., self.config.maximum_extension_m))
    }
}
struct Registered(SupportPreload);
impl Behavior for Registered {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![
            StateDeclaration::new("samples", QuantityKind::Dimensionless, 0.),
            StateDeclaration::new("extension", QuantityKind::Length, 0.),
        ]
    }
    fn residual(&self, ctx: &mut Context) {
        for i in 0..2 {
            ctx.set_state_residual(i, ctx.state_rate(i));
        }
        ctx.set_signal(0, ctx.state(1));
    }
    fn guards(&self, v: &View, out: &mut Vec<f64>) {
        out.push(v.state(0) * self.0.config.period_s - v.time)
    }
    fn scheduled_events(&self, v: &View, out: &mut Vec<(usize, f64)>) {
        out.push((0, v.state(0) * self.0.config.period_s))
    }
    fn jump(&mut self, _: usize, v: &View, states: &mut [f64]) {
        states[1] = self
            .0
            .update(
                v.state(1),
                v.signal_in(0),
                v.signal_in(1),
                v.signal_in(2) >= 1.,
            )
            .unwrap_or(f64::NAN);
        states[0] = v.state(0) + 1.;
    }
}
fn make(
    p: &std::collections::BTreeMap<String, f64>,
) -> Result<Box<dyn Behavior>, sim_core::EquationError> {
    let config = SupportPreloadConfig {
        period_s: param(p, "period_s")?,
        integral_gain_m_per_ns: param(p, "integral_gain_m_per_ns")?,
        maximum_extension_m: param(p, "maximum_extension_m")?,
    };
    Ok(Box::new(Registered(SupportPreload::new(config).map_err(
        |e| sim_core::EquationError::InvalidParameter("support_preload".into(), e),
    )?)))
}
pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    registry.register(
        BehaviorDescriptor::new(
            SUPPORT_PRELOAD,
            "Bounded support force preload",
            vec![
                signal_in("target_force", QuantityKind::Force),
                signal_in("measured_force", QuantityKind::Force),
                signal_in("enabled", QuantityKind::Dimensionless),
                signal_out("extension", QuantityKind::Length),
            ],
            make,
        )
        .with_parameters(vec![
            P::required("period_s", "s").positive(),
            P::required("integral_gain_m_per_ns", "m/(N*s)").positive(),
            P::required("maximum_extension_m", "m").positive(),
        ]),
    )
}

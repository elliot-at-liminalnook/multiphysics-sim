//! A load-weighted velocity objective, not a physical damping force.
use serde::{Deserialize, Serialize};
use sim_core::{Behavior, BehaviorDescriptor, BehaviorRegistry, Context, QuantityKind,
    RegistryError, param, signal_in, signal_out};

pub const LOAD_DAMPING: &str = "control.load_damping";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LoadDampingConfig {
    pub velocity_damping_s: f64,
    pub full_support_force_n: f64,
}

#[derive(Clone, Debug)]
pub struct LoadDamping { config: LoadDampingConfig }
impl LoadDamping {
    pub fn new(config: LoadDampingConfig) -> Result<Self, String> {
        if !config.velocity_damping_s.is_finite() || config.velocity_damping_s < 0.
            || !config.full_support_force_n.is_finite() || config.full_support_force_n <= 0. {
            return Err("load damping requires finite nonnegative seconds and positive support force".into());
        }
        Ok(Self { config })
    }
    /// Signed displacement opposing observed velocity, attenuated by normal
    /// load. The downstream controller owns position/joint/actuator limits.
    pub fn displacement(&self, velocity_m_s: f64, normal_force_n: f64) -> Result<f64, String> {
        if !velocity_m_s.is_finite() || !normal_force_n.is_finite() || normal_force_n < 0. {
            return Err("load damping requires finite velocity and nonnegative normal force".into());
        }
        let weight = (normal_force_n / self.config.full_support_force_n).min(1.);
        let value = -(self.config.velocity_damping_s * weight) * velocity_m_s;
        if !value.is_finite() { return Err("nonfinite load damping displacement".into()); }
        Ok(value)
    }
}

struct Registered(LoadDamping);
impl Behavior for Registered {
    fn states(&self) -> Vec<sim_core::StateDeclaration> { vec![] }
    fn residual(&self, ctx: &mut Context) {
        let value = self.0.displacement(ctx.signal_in(0), ctx.signal_in(1)).unwrap_or(f64::NAN);
        ctx.set_signal(0, value);
    }
}
fn make(p: &std::collections::BTreeMap<String, f64>) -> Result<Box<dyn Behavior>, sim_core::EquationError> {
    let config = LoadDampingConfig { velocity_damping_s: param(p, "velocity_damping_s")?,
        full_support_force_n: param(p, "full_support_force_n")? };
    Ok(Box::new(Registered(LoadDamping::new(config).map_err(|e|
        sim_core::EquationError::InvalidParameter("load_damping".into(), e))?)))
}
pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    registry.register(BehaviorDescriptor::new(LOAD_DAMPING, "Load-weighted velocity correction",
        vec![signal_in("velocity", QuantityKind::LinearVelocity), signal_in("normal_force", QuantityKind::Force),
            signal_out("displacement", QuantityKind::Length)], make)
        .with_parameters(vec![P::required("velocity_damping_s", "s").nonnegative(),
            P::required("full_support_force_n", "N").positive()]))
}

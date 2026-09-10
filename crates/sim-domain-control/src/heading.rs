//! Bounded shortest-angle feedback. Outputs a kinematic angle suggestion,
//! never a torque or a pose update; the caller owns actuator/contact dynamics.
use serde::{Deserialize, Serialize};
use sim_core::{Behavior, BehaviorDescriptor, BehaviorRegistry, Context, QuantityKind,
    RegistryError, param, signal_in, signal_out};

pub const HEADING_FEEDBACK: &str = "control.heading_feedback";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HeadingFeedbackConfig {
    pub position_gain: f64,
    pub velocity_damping_s: f64,
    pub maximum_correction_rad: f64,
}
#[derive(Clone, Debug)]
pub struct HeadingFeedback { config: HeadingFeedbackConfig }
impl HeadingFeedback {
    pub fn new(config: HeadingFeedbackConfig) -> Result<Self, String> {
        if !config.position_gain.is_finite() || config.position_gain < 0.
            || !config.velocity_damping_s.is_finite() || config.velocity_damping_s < 0.
            || !config.maximum_correction_rad.is_finite() || config.maximum_correction_rad <= 0. {
            return Err("heading feedback requires finite nonnegative gains and a positive angle cap".into());
        }
        Ok(Self { config })
    }
    pub fn correction(&self, target: f64, actual: f64, target_rate: f64, actual_rate: f64) -> Result<f64, String> {
        let error = shortest_angle_error(target, actual)?;
        if !target_rate.is_finite() || !actual_rate.is_finite() {
            return Err("finite heading rates required".into());
        }
        let value = self.config.position_gain * error
            + self.config.velocity_damping_s * (target_rate - actual_rate);
        if !value.is_finite() { return Err("nonfinite heading correction".into()); }
        Ok(value.clamp(-self.config.maximum_correction_rad, self.config.maximum_correction_rad))
    }
}
pub fn shortest_angle_error(target: f64, actual: f64) -> Result<f64, String> {
    let delta = target - actual;
    if !target.is_finite() || !actual.is_finite() || !delta.is_finite() {
        return Err("finite heading angles and difference required".into());
    }
    Ok(delta.sin().atan2(delta.cos()))
}

/// World-Z heading and its exact rate from a world-space forward direction
/// and angular velocity. A vertical forward axis has undefined heading.
pub fn world_z_heading(forward: [f64; 3], angular_velocity: [f64; 3]) -> Result<[f64; 2], String> {
    if forward.iter().chain(&angular_velocity).any(|v| !v.is_finite()) {
        return Err("finite heading direction and angular velocity required".into());
    }
    let [x, y, z] = forward;
    let [wx, wy, wz] = angular_velocity;
    let horizontal_squared = x * x + y * y;
    if !horizontal_squared.is_finite() || horizontal_squared <= 1e-12 {
        return Err("world-Z heading requires a nonvertical forward axis".into());
    }
    let rate = (x * (wz * x - wx * z) - y * (wy * z - wz * y)) / horizontal_squared;
    if !rate.is_finite() { return Err("nonfinite world-Z heading rate".into()); }
    Ok([y.atan2(x), rate])
}

struct Registered(HeadingFeedback);
impl Behavior for Registered {
    fn states(&self) -> Vec<sim_core::StateDeclaration> { vec![] }
    fn residual(&self, ctx: &mut Context) {
        let value = self.0.correction(ctx.signal_in(0), ctx.signal_in(1), ctx.signal_in(2), ctx.signal_in(3))
            .unwrap_or(f64::NAN);
        ctx.set_signal(0, value);
    }
}
fn make(p: &std::collections::BTreeMap<String, f64>) -> Result<Box<dyn Behavior>, sim_core::EquationError> {
    let config = HeadingFeedbackConfig { position_gain: param(p, "position_gain")?,
        velocity_damping_s: param(p, "velocity_damping_s")?, maximum_correction_rad: param(p, "maximum_correction_rad")? };
    Ok(Box::new(Registered(HeadingFeedback::new(config).map_err(|e|
        sim_core::EquationError::InvalidParameter("heading_feedback".into(), e))?)))
}
pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    use sim_core::ParameterDeclaration as P;
    registry.register(BehaviorDescriptor::new(HEADING_FEEDBACK, "Bounded shortest-angle heading feedback",
        vec![signal_in("target", QuantityKind::Angle), signal_in("actual", QuantityKind::Angle),
            signal_in("target_rate", QuantityKind::AngularVelocity), signal_in("actual_rate", QuantityKind::AngularVelocity),
            signal_out("correction", QuantityKind::Angle)], make)
        .with_parameters(vec![P::required("position_gain", "1").nonnegative(),
            P::required("velocity_damping_s", "s").nonnegative(),
            P::required("maximum_correction_rad", "rad").positive()]))
}

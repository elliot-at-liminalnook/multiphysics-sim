//! Explicit low-fidelity position servo for interactive/training profiles.
//! No winding, rotor, gearbox compliance/backlash, firmware clock, sensor
//! quantization, latency or heat state is inferred. External link inertia and
//! mechanism/contact physics remain the caller's articulated model.
use sim_core::{
    Behavior, BehaviorDescriptor, BehaviorRegistry, ConnectorKind, Context, EquationError,
    ParameterDeclaration as P, QuantityKind, RegistryError, StateDeclaration, acausal, param,
    signal_in, signal_out,
};
use std::collections::BTreeMap;

pub const EFFECTIVE_SERVO: &str = "robot.effective_servo";

#[derive(Clone, Debug)]
pub struct EffectiveServo {
    stiffness: f64,
    damping: f64,
    stall: f64,
    speed: f64,
}

impl EffectiveServo {
    pub fn new(p: &BTreeMap<String, f64>) -> Result<Self, EquationError> {
        for name in p.keys() {
            if !["stiffness", "damping", "stall_torque", "no_load_speed"].contains(&name.as_str()) {
                return Err(EquationError::InvalidParameter(
                    name.clone(),
                    "unsupported effective servo parameter".into(),
                ));
            }
        }
        let result = Self {
            stiffness: param(p, "stiffness")?,
            damping: param(p, "damping")?,
            stall: param(p, "stall_torque")?,
            speed: param(p, "no_load_speed")?,
        };
        // Registry metadata validates builds; direct runtime use follows exactly
        // the same domain restrictions through this constructor.
        for (name, v, positive) in [
            ("stiffness", result.stiffness, true),
            ("damping", result.damping, false),
            ("stall_torque", result.stall, true),
            ("no_load_speed", result.speed, true),
        ] {
            if !v.is_finite() || v < 0.0 || (positive && v == 0.0) {
                return Err(EquationError::InvalidParameter(
                    name.into(),
                    "must be finite and within the declared positive/nonnegative range".into(),
                ));
            }
        }
        Ok(result)
    }

    /// Positive torque increases relative output angle. The envelope limits
    /// motoring torque to a linear torque/speed curve; opposing motion can be
    /// braked at stall torque. External forces can still backdrive/overspeed it.
    /// This is an effective response law, not a winding or heat prediction.
    pub fn torque(&self, angle: f64, speed: f64, target: f64) -> f64 {
        let request = self.stiffness * (target - angle) - self.damping * speed;
        let available =
            self.stall * (1.0 - (request.signum() * speed / self.speed).max(0.0)).max(0.0);
        request.clamp(-available, available)
    }
}

impl Behavior for EffectiveServo {
    fn states(&self) -> Vec<StateDeclaration> {
        vec![]
    }
    fn residual(&self, ctx: &mut Context) {
        let torque = self.torque(
            ctx.across(0) - ctx.across(1),
            ctx.across_rate(0) - ctx.across_rate(1),
            ctx.signal_in(2),
        );
        ctx.add_through(0, -torque);
        ctx.add_through(1, torque);
        ctx.set_signal(3, torque);
    }
}
fn make(p: &BTreeMap<String, f64>) -> Result<Box<dyn Behavior>, EquationError> {
    Ok(Box::new(EffectiveServo::new(p)?))
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    registry.register(
        BehaviorDescriptor::new(
            EFFECTIVE_SERVO,
            "Effective bounded position servo",
            vec![
                acausal("shaft", ConnectorKind::Rotational),
                acausal("housing", ConnectorKind::Rotational),
                signal_in("target", QuantityKind::Angle),
                signal_out("torque", QuantityKind::Torque),
            ],
            make,
        )
        .with_parameters(vec![
            P::required("stiffness", "N·m/rad").positive(),
            P::required("damping", "N·m·s/rad").nonnegative(),
            P::required("stall_torque", "N·m").positive(),
            P::required("no_load_speed", "rad/s").positive(),
        ]),
    )
}

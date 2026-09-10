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

#[path = "servo_command.rs"]
mod command;
pub use command::{ServoCommandCheck, ServoCommandLimits};

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
        let available = self.torque_capacity(speed, request);
        request.clamp(-available, available)
    }

    /// Unclamped position command realizing torque feedforward plus the existing
    /// position/velocity feedback gains about a moving reference. The normal
    /// torque-speed envelope and external command bounds still apply.
    pub fn reference_target(&self, angle: f64, speed: f64, torque_nm: f64) -> Result<f64, String> {
        let target = angle + (self.damping * speed + torque_nm) / self.stiffness;
        if [angle, speed, torque_nm, target]
            .iter()
            .any(|v| !v.is_finite())
        {
            return Err("finite moving reference and feedforward torque required".into());
        }
        Ok(target)
    }

    /// Torque magnitude available at a finite signed speed, in the direction
    /// of a requested torque. Braking retains stall torque even above no-load
    /// speed. This is the same envelope used by the running servo model.
    pub fn torque_capacity(&self, speed: f64, requested_torque: f64) -> f64 {
        self.stall * (1.0 - (requested_torque.signum() * speed / self.speed).max(0.0)).max(0.0)
    }

    /// Nonnegative optimization residual in Nm with the same zero-violation
    /// set as the exact torque envelope. For nonzero motoring torque, extend
    /// the sloping boundary beyond no-load speed instead of clipping its
    /// gradient. This is not negative physical capacity or a backdrive limit:
    /// braking retains stall torque, and zero-torque coasting has no penalty.
    /// The runtime torque law remains `torque_capacity` above.
    pub fn optimization_torque_violation(&self, speed: f64, requested_torque: f64) -> f64 {
        if requested_torque == 0.0 {
            return 0.0;
        }
        let along = (requested_torque.signum() * speed / self.speed).max(0.0);
        (requested_torque.abs() - self.stall * (1.0 - along)).max(0.0)
    }

    /// Optimistic positive mechanical power, achieved at half no-load speed
    /// and half stall torque. Does not establish sustained thermal capacity.
    pub fn peak_motoring_power_w(&self) -> f64 {
        self.stall * self.speed / 4.0
    }
}

#[cfg(test)]
mod optimization_tests {
    use super::*;

    #[test]
    fn moving_reference_realizes_feedforward_and_feedback_without_bypassing_limits() {
        let motor = EffectiveServo {
            stiffness: 10.,
            damping: 0.1,
            stall: 3.,
            speed: 2.,
        };
        let target = motor.reference_target(0.2, 0.5, 0.4).unwrap();
        assert!((motor.torque(0.2, 0.5, target) - 0.4).abs() < 1e-12);
        assert!((motor.torque(0.19, 0.4, target) - (0.4 + 0.1 + 0.01)).abs() < 1e-12);
        let saturated = motor.reference_target(0.2, 0.5, 10.).unwrap();
        assert_eq!(
            motor.torque(0.2, 0.5, saturated),
            motor.torque_capacity(0.5, 10.)
        );
        assert!(motor.reference_target(f64::NAN, 0., 0.).is_err());
        assert!(motor.reference_target(0., f64::INFINITY, 0.).is_err());
    }

    #[test]
    fn extended_penalty_keeps_physical_feasibility_and_free_backdrive() {
        let motor = EffectiveServo {
            stiffness: 10.0,
            damping: 0.1,
            stall: 3.0,
            speed: 2.0,
        };
        for speed in [-20.0, -3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 20.0] {
            for torque in [-4.0, -3.0, -1.0, -0.01, 0.0, 0.01, 1.0, 3.0, 4.0] {
                assert_eq!(
                    motor.optimization_torque_violation(speed, torque) == 0.0,
                    torque.abs() <= motor.torque_capacity(speed, torque)
                );
            }
        }
        assert_eq!(motor.optimization_torque_violation(100.0, 0.0), 0.0);
        assert_eq!(motor.optimization_torque_violation(100.0, -1.0), 0.0);
        assert_eq!(motor.optimization_torque_violation(-100.0, 1.0), 0.0);
        // The physical capacity is flat at zero; the search still receives
        // the exact linear envelope slope and a useful direction of repair.
        assert_eq!(motor.torque_capacity(3.0, 0.2), 0.0);
        assert_eq!(motor.torque_capacity(4.0, 0.2), 0.0);
        assert!(
            (motor.optimization_torque_violation(4.0, 0.2)
                - motor.optimization_torque_violation(3.0, 0.2)
                - 1.5)
                .abs()
                < 1e-12
        );
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

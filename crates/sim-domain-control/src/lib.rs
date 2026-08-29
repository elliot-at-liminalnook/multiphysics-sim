//! Directed, sampled control behaviors.

use serde::{Deserialize, Serialize};
use sim_core::{
    BehaviorDescriptor, BehaviorRegistry, BehaviorTypeId, PortDeclaration, PortSchema,
    QuantityKind, RegistryError,
};

pub const POSITION_CONTROLLER: &str = "control.position_pi";
pub const POSITION_SENSOR: &str = "control.position_sensor";
pub const POSITION_SETPOINT: &str = "control.position_setpoint";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PositionControllerConfig {
    pub sample_period: f64,
    pub kp: f64,
    pub ki: f64,
    pub duty_limit: f64,
}

impl Default for PositionControllerConfig {
    fn default() -> Self {
        Self {
            sample_period: 1.0e-3,
            kp: 28.0,
            ki: 6.0,
            duty_limit: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct PositionControllerState {
    pub integral: f64,
    pub duty: f64,
    pub last_error: f64,
    pub sample_count: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PositionController {
    pub config: PositionControllerConfig,
}

impl PositionController {
    pub fn sample(&self, state: &mut PositionControllerState, target: f64, measured: f64) -> f64 {
        let error = target - measured;
        let p = self.config.kp * error;
        let candidate_integral =
            state.integral + self.config.ki * self.config.sample_period * error;
        let candidate = p + candidate_integral;
        let limit = self.config.duty_limit;

        // Conditional integration: do not accumulate error farther into saturation.
        let saturating_high = candidate > limit && error > 0.0;
        let saturating_low = candidate < -limit && error < 0.0;
        if !(saturating_high || saturating_low) {
            state.integral = candidate_integral;
        }

        state.duty = (p + state.integral).clamp(-limit, limit);
        state.last_error = error;
        state.sample_count += 1;
        state.duty
    }
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    registry.register(BehaviorDescriptor {
        type_id: BehaviorTypeId::from(POSITION_SETPOINT),
        display_name: "Position setpoint",
        ports: vec![PortDeclaration {
            name: "target",
            schema: PortSchema::SignalOut(QuantityKind::Length),
        }],
    })?;
    registry.register(BehaviorDescriptor {
        type_id: BehaviorTypeId::from(POSITION_CONTROLLER),
        display_name: "Sampled PI position controller",
        ports: vec![
            PortDeclaration {
                name: "target",
                schema: PortSchema::SignalIn(QuantityKind::Length),
            },
            PortDeclaration {
                name: "measured",
                schema: PortSchema::SignalIn(QuantityKind::Length),
            },
            PortDeclaration {
                name: "duty",
                schema: PortSchema::SignalOut(QuantityKind::Dimensionless),
            },
        ],
    })?;
    registry.register(BehaviorDescriptor {
        type_id: BehaviorTypeId::from(POSITION_SENSOR),
        display_name: "Ideal position sensor",
        ports: vec![
            PortDeclaration {
                name: "axis",
                schema: PortSchema::Acausal(sim_core::ConnectorKind::Translational),
            },
            PortDeclaration {
                name: "position",
                schema: PortSchema::SignalOut(QuantityKind::Length),
            },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anti_windup_freezes_integral_when_saturated_farther() {
        let controller = PositionController {
            config: PositionControllerConfig::default(),
        };
        let mut state = PositionControllerState::default();
        for _ in 0..100 {
            controller.sample(&mut state, 1.0, 0.0);
        }
        assert_eq!(state.duty, 1.0);
        assert_eq!(state.integral, 0.0);
    }
}

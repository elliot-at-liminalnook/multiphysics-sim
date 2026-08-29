//! Electrical boundaries and an averaged H-bridge.

use serde::{Deserialize, Serialize};
use sim_core::{
    BehaviorDescriptor, BehaviorRegistry, BehaviorTypeId, ConnectorKind, PortDeclaration,
    PortSchema, QuantityKind, RegistryError,
};

pub const POWER_SUPPLY: &str = "electrical.power_supply";
pub const AVERAGED_H_BRIDGE: &str = "electrical.averaged_h_bridge";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PowerSupply {
    pub voltage: f64,
}

impl Default for PowerSupply {
    fn default() -> Self {
        Self { voltage: 24.0 }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AveragedHBridge {
    pub current_limit: f64,
    pub current_limit_gain: f64,
    pub on_resistance: f64,
}

impl Default for AveragedHBridge {
    fn default() -> Self {
        Self {
            current_limit: 8.0,
            current_limit_gain: 120.0,
            on_resistance: 0.08,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DriverOutput {
    pub requested_voltage: f64,
    pub motor_voltage: f64,
    pub current_limited: bool,
    pub loss: f64,
}

impl AveragedHBridge {
    pub fn output(&self, bus_voltage: f64, duty: f64, current: f64) -> DriverOutput {
        let request = duty.clamp(-1.0, 1.0) * bus_voltage;
        // A fast inner current loop may oppose either motoring or regenerative current.
        let excess = (current.abs() - self.current_limit).max(0.0) * current.signum();
        let voltage = (request - self.current_limit_gain * excess - self.on_resistance * current)
            .clamp(-bus_voltage.abs(), bus_voltage.abs());
        DriverOutput {
            requested_voltage: request,
            motor_voltage: voltage,
            current_limited: excess != 0.0,
            loss: ((request - voltage) * current).max(0.0),
        }
    }
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    registry.register(BehaviorDescriptor {
        type_id: BehaviorTypeId::from(POWER_SUPPLY),
        display_name: "Ideal DC power supply",
        ports: vec![PortDeclaration {
            name: "dc",
            schema: PortSchema::Acausal(ConnectorKind::Electrical),
        }],
    })?;
    registry.register(BehaviorDescriptor {
        type_id: BehaviorTypeId::from(AVERAGED_H_BRIDGE),
        display_name: "Averaged H-bridge",
        ports: vec![
            PortDeclaration {
                name: "bus",
                schema: PortSchema::Acausal(ConnectorKind::Electrical),
            },
            PortDeclaration {
                name: "motor",
                schema: PortSchema::Acausal(ConnectorKind::Electrical),
            },
            PortDeclaration {
                name: "duty",
                schema: PortSchema::SignalIn(QuantityKind::Dimensionless),
            },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_limit_opposes_motoring_and_regenerative_current() {
        let bridge = AveragedHBridge::default();
        let motoring = bridge.output(24.0, 1.0, 9.0);
        let regenerative = bridge.output(24.0, 0.0, -9.0);
        assert!(motoring.current_limited);
        assert!(regenerative.current_limited);
        assert!(motoring.motor_voltage < motoring.requested_voltage);
        assert!(regenerative.motor_voltage > regenerative.requested_voltage);
    }
}

//! One-dimensional rotational mechanics.

use serde::{Deserialize, Serialize};
use sim_core::{
    BehaviorDescriptor, BehaviorRegistry, BehaviorTypeId, ConnectorKind, PortDeclaration,
    PortSchema, RegistryError,
};

pub const ROTOR_INERTIA: &str = "rotational.inertia";
pub const IDEAL_GEAR: &str = "rotational.ideal_gear";
pub const FIXED_MOUNT: &str = "rotational.fixed_mount";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Rotor {
    pub inertia: f64,
    pub viscous_drag: f64,
}

impl Default for Rotor {
    fn default() -> Self {
        Self {
            inertia: 2.0e-4,
            viscous_drag: 2.0e-4,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct IdealGear {
    /// Input turns divided by output turns.
    pub reduction: f64,
    pub efficiency: f64,
}

impl Default for IdealGear {
    fn default() -> Self {
        Self {
            reduction: 10.0,
            efficiency: 0.90,
        }
    }
}

impl IdealGear {
    pub fn output_angle(self, input_angle: f64) -> f64 {
        input_angle / self.reduction
    }

    pub fn output_speed(self, input_speed: f64) -> f64 {
        input_speed / self.reduction
    }
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    registry.register(BehaviorDescriptor {
        type_id: BehaviorTypeId::from(ROTOR_INERTIA),
        display_name: "Rotor inertia",
        ports: vec![PortDeclaration {
            name: "shaft",
            schema: PortSchema::Acausal(ConnectorKind::Rotational),
        }],
    })?;
    registry.register(BehaviorDescriptor {
        type_id: BehaviorTypeId::from(IDEAL_GEAR),
        display_name: "Ideal reduction gear",
        ports: vec![
            PortDeclaration {
                name: "input",
                schema: PortSchema::Acausal(ConnectorKind::Rotational),
            },
            PortDeclaration {
                name: "output",
                schema: PortSchema::Acausal(ConnectorKind::Rotational),
            },
        ],
    })?;
    registry.register(BehaviorDescriptor {
        type_id: BehaviorTypeId::from(FIXED_MOUNT),
        display_name: "Fixed rotational mount",
        ports: vec![PortDeclaration {
            name: "flange",
            schema: PortSchema::Acausal(ConnectorKind::Rotational),
        }],
    })
}

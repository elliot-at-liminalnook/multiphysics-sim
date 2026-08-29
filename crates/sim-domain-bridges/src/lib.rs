//! Behaviors that explicitly bridge otherwise independent physical domains.

use serde::{Deserialize, Serialize};
use sim_core::{
    BehaviorDescriptor, BehaviorRegistry, BehaviorTypeId, ConnectorKind, PortDeclaration,
    PortSchema, RegistryError,
};

pub const DC_MOTOR: &str = "bridge.dc_motor";
pub const LEAD_SCREW: &str = "bridge.lead_screw";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DcMotor {
    pub resistance: f64,
    pub inductance: f64,
    pub torque_constant: f64,
    pub back_emf_constant: f64,
}

impl Default for DcMotor {
    fn default() -> Self {
        Self {
            resistance: 0.6,
            inductance: 8.0e-4,
            torque_constant: 0.05,
            back_emf_constant: 0.05,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LeadScrew {
    pub lead: f64,
}

impl Default for LeadScrew {
    fn default() -> Self {
        Self { lead: 0.005 }
    }
}

impl LeadScrew {
    pub fn metres_per_screw_radian(self) -> f64 {
        self.lead / std::f64::consts::TAU
    }
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    registry.register(BehaviorDescriptor {
        type_id: BehaviorTypeId::from(DC_MOTOR),
        display_name: "Brushed DC motor",
        ports: vec![
            PortDeclaration {
                name: "winding",
                schema: PortSchema::Acausal(ConnectorKind::Electrical),
            },
            PortDeclaration {
                name: "shaft",
                schema: PortSchema::Acausal(ConnectorKind::Rotational),
            },
            PortDeclaration {
                name: "case",
                schema: PortSchema::Acausal(ConnectorKind::Rotational),
            },
        ],
    })?;
    registry.register(BehaviorDescriptor {
        type_id: BehaviorTypeId::from(LEAD_SCREW),
        display_name: "Ideal lead screw",
        ports: vec![
            PortDeclaration {
                name: "shaft",
                schema: PortSchema::Acausal(ConnectorKind::Rotational),
            },
            PortDeclaration {
                name: "carriage",
                schema: PortSchema::Acausal(ConnectorKind::Translational),
            },
        ],
    })
}

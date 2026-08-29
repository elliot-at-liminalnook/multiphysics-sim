//! One-dimensional translational mechanics.

use serde::{Deserialize, Serialize};
use sim_core::{
    BehaviorDescriptor, BehaviorRegistry, BehaviorTypeId, ConnectorKind, PortDeclaration,
    PortSchema, RegistryError,
};

pub const LINEAR_MASS: &str = "translational.mass";
pub const PRISMATIC_GUIDE: &str = "translational.prismatic_guide";
pub const COMPLIANT_END_STOP: &str = "translational.compliant_end_stop";
pub const FIXED_MOUNT: &str = "translational.fixed_mount";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LinearLoad {
    pub mass: f64,
    pub viscous_drag: f64,
    pub min_position: f64,
    pub max_position: f64,
}

impl Default for LinearLoad {
    fn default() -> Self {
        Self {
            mass: 2.0,
            viscous_drag: 20.0,
            min_position: 0.0,
            max_position: 0.150,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CompliantEndStop {
    pub stiffness: f64,
    pub damping: f64,
}

impl Default for CompliantEndStop {
    fn default() -> Self {
        Self {
            stiffness: 2_000_000.0,
            damping: 2_000.0,
        }
    }
}

impl CompliantEndStop {
    pub fn force(self, position: f64, velocity: f64, min_position: f64, max_position: f64) -> f64 {
        if position > max_position {
            -self.stiffness * (position - max_position) - self.damping * velocity.max(0.0)
        } else if position < min_position {
            self.stiffness * (min_position - position) - self.damping * velocity.min(0.0)
        } else {
            0.0
        }
    }

    pub fn potential(self, position: f64, min_position: f64, max_position: f64) -> f64 {
        let upper = (position - max_position).max(0.0);
        let lower = (min_position - position).max(0.0);
        0.5 * self.stiffness * (upper * upper + lower * lower)
    }

    /// Energy-consistent contact force for one accepted step.
    ///
    /// The conservative term is a discrete gradient, so its work over the step
    /// exactly equals the change in stop potential even when the step first
    /// crosses a limit. Damping acts only while moving farther into contact.
    pub fn discrete_force(
        self,
        old_position: f64,
        new_position: f64,
        midpoint_velocity: f64,
        min_position: f64,
        max_position: f64,
    ) -> f64 {
        let delta = new_position - old_position;
        let conservative = if delta.abs() > 1.0e-14 {
            -(self.potential(new_position, min_position, max_position)
                - self.potential(old_position, min_position, max_position))
                / delta
        } else {
            self.force(new_position, 0.0, min_position, max_position)
        };
        let entering_upper = new_position > max_position && midpoint_velocity > 0.0;
        let entering_lower = new_position < min_position && midpoint_velocity < 0.0;
        let damping = if entering_upper || entering_lower {
            -self.damping * midpoint_velocity
        } else {
            0.0
        };
        conservative + damping
    }
}

pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    registry.register(BehaviorDescriptor {
        type_id: BehaviorTypeId::from(LINEAR_MASS),
        display_name: "Linear mass",
        ports: vec![PortDeclaration {
            name: "axis",
            schema: PortSchema::Acausal(ConnectorKind::Translational),
        }],
    })?;
    registry.register(BehaviorDescriptor {
        type_id: BehaviorTypeId::from(PRISMATIC_GUIDE),
        display_name: "Prismatic guide",
        ports: vec![
            PortDeclaration {
                name: "axis",
                schema: PortSchema::Acausal(ConnectorKind::Translational),
            },
            PortDeclaration {
                name: "chassis",
                schema: PortSchema::Acausal(ConnectorKind::Frame),
            },
        ],
    })?;
    registry.register(BehaviorDescriptor {
        type_id: BehaviorTypeId::from(COMPLIANT_END_STOP),
        display_name: "Compliant end stop",
        ports: vec![PortDeclaration {
            name: "axis",
            schema: PortSchema::Acausal(ConnectorKind::Translational),
        }],
    })?;
    registry.register(BehaviorDescriptor {
        type_id: BehaviorTypeId::from(FIXED_MOUNT),
        display_name: "Fixed translational mount",
        ports: vec![PortDeclaration {
            name: "frame",
            schema: PortSchema::Acausal(ConnectorKind::Frame),
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discrete_stop_force_accounts_for_crossing_energy() {
        let stop = CompliantEndStop::default();
        let old = 0.079;
        let new = 0.081;
        let force = stop.discrete_force(old, new, 0.0, 0.0, 0.080);
        let work = force * (new - old);
        let potential_change = stop.potential(new, 0.0, 0.080) - stop.potential(old, 0.0, 0.080);
        assert!((work + potential_change).abs() < 1.0e-12);
    }
}

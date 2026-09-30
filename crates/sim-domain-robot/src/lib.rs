//! `sim-domain-robot`: a robot described by the CAD tool as a physical
//! assembly (`cad/PHYSICAL_MODEL.md`), simulated as coupled multiphysics —
//! the articulated body with geometry contact and modal flexibility, motors
//! with their electrical, gearbox and thermal behaviour, servo firmware,
//! drivers, a battery, inertial sensors and cables.

pub mod articulated;
pub mod actuator_audit;
pub mod actuator_profile;
pub mod cad_link;
pub mod power_profile;
pub mod contract;
pub mod math;
pub mod model;
mod checked_json;
pub mod motor;
pub mod switchable_bridge;
pub mod effective_servo;
pub mod motion_capability;
pub mod reduction;
pub mod actuator_envelope;
pub mod contact_feasibility;
pub mod world_load;
pub mod sdf;

pub use articulated::{Articulated, Generalized, Options, ARTICULATED};
pub use model::{model_by_handle, register_model, PhysicalModel};
pub use motor::{BATTERY, H_BRIDGE, MOTOR_UNIT, SERVO_FIRMWARE, THERMAL_PROBE};

use sim_core::{BehaviorRegistry, RegistryError};

/// Register every element of this crate.
pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    reduction::register(registry).map_err(RegistryError::Primitive)?;
    actuator_envelope::register(registry).map_err(RegistryError::Primitive)?;
    contact_feasibility::register(registry).map_err(RegistryError::Primitive)?;
    articulated::register(registry)?;
    effective_servo::register(registry)?;
    motor::register(registry)?;
    switchable_bridge::register(registry)
}
pub mod notes;

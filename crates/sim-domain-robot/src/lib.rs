//! `sim-domain-robot`: a robot described by the CAD tool as a physical
//! assembly (`cad/PHYSICAL_MODEL.md`), simulated as coupled multiphysics —
//! the articulated body with geometry contact and modal flexibility, motors
//! with their electrical, gearbox and thermal behaviour, servo firmware,
//! drivers, a battery, inertial sensors and cables.

pub mod articulated;
pub mod math;
pub mod model;
pub mod motor;
pub mod sdf;

pub use articulated::{Articulated, Generalized, Options, ARTICULATED};
pub use model::{model_by_handle, register_model, PhysicalModel};
pub use motor::{BATTERY, H_BRIDGE, MOTOR_UNIT, SERVO_FIRMWARE, THERMAL_PROBE};

use sim_core::{BehaviorRegistry, RegistryError};

/// Register every element of this crate.
pub fn register(registry: &mut BehaviorRegistry) -> Result<(), RegistryError> {
    articulated::register(registry)?;
    motor::register(registry)
}

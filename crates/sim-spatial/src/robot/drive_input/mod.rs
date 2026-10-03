//! Drive input (teleoperation's first layer): device bindings turn keys,
//! sticks and buttons into normalized axes and a drive profile's named
//! actions, written as `RobotAction::Drive` for robot mode's one apply
//! system, which scales them by the robot's `sim.drive/1` profile and hands
//! the twist to the run thread (`RunController::drive`).
//!
//! - [`bindings`]: the `sim.drive-bindings/1` bindings (per device, shared
//!   across robots, persisted through `app::settings`) and [`DriveBindings`],
//!   the active validated set.
//! - [`input`]: the keyboard and gamepad input system and [`DriveInput`],
//!   what it last did (both read by the inspector).
//! - [`plugin`]: registration ([`build`], called by `RobotPlugin`); the input
//!   system itself runs in robot mode's input chain (`robot/mod.rs`).
pub mod bindings;
pub mod input;
mod plugin;
#[cfg(test)]
mod tests;

pub use bindings::{BindingsFile, DriveBindings};
pub use input::DriveInput;
pub(in crate::robot) use input::devices;
pub(crate) use plugin::build;

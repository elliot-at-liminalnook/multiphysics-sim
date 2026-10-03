//! Drive input (teleoperation's first layer), mode-neutral: device bindings
//! turn keys, sticks and buttons into normalized axes and a drive profile's
//! named actions for whichever mode offers a drivable run. One poller reads
//! the devices for every mode ([`input::devices`]); each mode drains its own
//! [`DriveDevice`] messages into its one drive apply: Robot mode as
//! `RobotAction::Drive` (`robot::actions::apply` → `RunController::drive`),
//! Build mode through `Builder::drive_request` (`RunControl::Twist` → the robot
//! system's `DriveHost`). Both scale the axes by the robot's `sim.drive/1`
//! profile (`DriveRequest::interpret`).
//!
//! - [`bindings`]: the Bevy mapping of the shared `sim.drive-bindings/1`
//!   bindings (`sim_runtime::drive_bindings`: per device, shared across
//!   robots and with the browser, persisted through `app::settings`) and
//!   [`DriveBindings`], the active validated set.
//! - [`input`]: the poller and [`DriveInput`], what it last did.
//! - [`DriveTarget`]: what the active mode offers to drive (one writer per
//!   mode, set_if_neq), [`DriveDevice`]: what the poller asks of it.
//! - [`plugin`]: [`DriveInputPlugin`].
pub mod bindings;
pub mod input;
mod plugin;
#[cfg(test)]
mod tests;

use crate::app::ViewerMode;
use bevy::prelude::*;
use sim_runtime::drive_host::DriveRequest;

pub use bindings::{BindingsFile, DriveBindings};
pub use input::DriveInput;
pub use plugin::DriveInputPlugin;

/// What the active mode offers to drive. Written by one system per mode
/// while that mode runs (`robot::controls::drive_target`,
/// `builder::robot_run::drive_target`; each with `set_if_neq`) and reset by
/// [`leave_mode`] on the mode's exit. The poller acts only on a live target
/// whose `mode` is the current mode ([`input::live_target`]).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct DriveTarget {
    /// A run the devices may drive now; None: nothing is read.
    pub live: Option<LiveTarget>,
    /// Keys the mode owns while the target is live (Robot: Q/A/Z while the
    /// Leg calibration panel is shown; Build: its editing keys): never read
    /// for driving.
    pub owned_keys: Vec<KeyCode>,
}

/// A drivable run.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveTarget {
    /// The mode that wrote it; its messages carry it.
    pub mode: ViewerMode,
    /// The profile's supported axes (`resolved.limits.supported`, in
    /// `kinematics::AXIS_NAMES` order): the others are zeroed per device.
    pub supported: [bool; 3],
    /// What is driven (Robot: the model file; Build: the system file and
    /// its run): a change disarms held inputs. A Reset of the same run keeps it.
    pub run: String,
}

/// A drive request from the devices for `mode` (the target's), written as
/// `Act<DriveDevice>` by the poller (`Act::ui` for stops and named actions
/// that are shown, `Act::quiet` for repeated axes) and drained by that
/// mode's apply only: a mode's reader skips another mode's requests, so a
/// request still buffered across a mode switch never reaches the new mode.
#[derive(Clone, Debug, PartialEq)]
pub struct DriveDevice {
    pub mode: ViewerMode,
    pub request: DriveRequest,
}

/// OnExit of a mode that writes a [`DriveTarget`]: nothing is driven from
/// the devices any more, and their status (with a refusal that belonged to
/// the mode's run) is cleared. A mode whose run outlives the exit (Build's
/// is kept, paused) first stops what the devices were driving
/// (`DriveInput::axes` nonzero) and then calls this as a function
/// (`builder::actions::leave_drive`), so no ordering edge is needed.
pub fn leave_mode(mut target: ResMut<DriveTarget>, mut input: ResMut<DriveInput>) {
    target.set_if_neq(DriveTarget::default());
    input.set_if_neq(DriveInput::default());
}

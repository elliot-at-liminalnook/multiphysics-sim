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
//! - [`Disarm`]: a mode's one apply telling the poller that it accepted a
//!   stop, halt, named action, Pause or Reset from any origin ([`DISARM_RULE`]).
//! - [`insert_state`]: the one serializer of `bindings` and `drive_input` in
//!   Robot mode's `robot_state` and Build mode's `system_state`.
//! - [`plugin`]: [`DriveInputPlugin`].
pub mod bindings;
pub mod input;
mod plugin;
#[cfg(test)]
mod tests;

use crate::app::ViewerMode;
use bevy::prelude::*;
use serde_json::{Value, json};
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
    /// What is driven, with the run's identity: a change disarms held
    /// inputs. Robot: the model file, the run controller's generation and
    /// whether a replay is in progress (`robot::controls::robot_target`), so
    /// another file, a reload, a Reset, a replay start and a replay end each
    /// change it. Build: the system file, the run thread's run id (content
    /// hash and revision), the run's start number and its generation
    /// (`Builder::drive_target`), so an edited file, a new run (even of
    /// unchanged content) and a Reset each change it.
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

/// What a stop, halt or Pause from any origin does to held drive inputs
/// (shown as `drive_input.disarm_rule`).
pub const DISARM_RULE: &str = "an accepted stop, halt, named profile action, Pause (one that paused a running run: Robot refuses a Pause when not running, Build accepts it as a no-op that disarms nothing) or Reset from any origin (a panel button, a key, system_ui, REST robot_drive / robot_run / system_drive / system_run, a device) disarms the held drive inputs, as Escape or a bound stop key does: a key, stick or button held when the stop was applied is ignored until it is released and pressed again (a gamepad until every bound stick is in its deadzone and every bound button released), while one first pressed after the stop drives (the poller reads the stop in the next frame and disarms only what was held in the frame it was applied; Robot's apply writes no disarm for the devices' own requests, which disarmed themselves), and if the devices were driving and it was a drive stop or action they send one stop first, so a held input applied in the same frame after the click does not leave the robot driving (a Pause or Reset sends none itself: held axes are refused while paused, and a fresh zero would replace the paused request's on-loss stop; a Reset rebuilds the run as a new drive target, so devices still sending then send one stop to the rebuilt run through the target change); paths that stop or replace a run by removing or changing the drive target (leaving the mode, opening a lesson, a hot swap, another file, a reload, a replay's start or end, a new Build run) disarm through the target change instead; together with the drive host's pause rule (sim_runtime::drive_host::PAUSE_RULE) nothing requested before a Pause moves the robot after Run without fresh input";

/// A native stop, halt, pause or drive action applied in `mode` ([`DISARM_RULE`]):
/// the drive inputs held when it was applied are disarmed, as Escape or a
/// bound stop key disarms them, so a key, stick or button still held after
/// the click does not drive again until it is released and pressed again.
/// The poller reads it one frame after it was applied, so it blocks only a
/// key held since before that frame and the gamepad if it was held then: a
/// key or stick first pressed after the stop drives.
///
/// One public path into the poller's latch: written by each mode's one
/// drive apply when it accepts a stop, halt, Pause or named action from any
/// origin (a panel button, `system_ui`, REST, a device), in
/// `ViewerSet::Actions`; read by the one poller ([`input::devices`], every
/// frame in `InputSet::Window`, never gated by `run_if`, so a message written
/// in Actions is read in the next frame's Input, well inside its two-update
/// lifetime). A message for a mode that is not the live target's is ignored.
///
/// Writers, one per mode: Robot mode's `robot::actions::apply` (every
/// `RobotAction::Drive` stop or action, `system_ui` drive:* activation and
/// `RobotAction::Run` Pause or Reset it accepts) and Build mode's
/// `builder::system_actions::apply` (the run panel's and keys'
/// `BuildAction::Drive` stop or action, Pause and Reset, their `system_ui`
/// activations, REST `system_drive` and `system_run` pause / reset). Build's
/// device requests go to `Builder::drive_request` without it: the poller has
/// disarmed itself for those already.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct Disarm {
    pub mode: ViewerMode,
    /// What was applied, for `DriveInput::last_action` (e.g. "Stop", "Pause from REST").
    pub reason: String,
    /// Send one Stop first if the devices were driving: true for a drive
    /// stop or named action (so held axes applied after the click in the
    /// same frame do not leave the robot driving). False for Pause and
    /// Reset: a fresh zero request there would replace the paused
    /// request's on-loss rule (`sim_runtime::drive_host::PAUSE_RULE`) with
    /// the acceleration limit, and Reset rebuilds the drive state; held
    /// axes are refused while paused anyway.
    pub stop: bool,
}

/// `bindings` and `drive_input` in a mode's state answer (Robot mode's
/// `robot_state` through `RobotView::with_drive_input`, Build mode's
/// `system_state` through `Builder::with_drive_input`): the device layer is
/// resources rather than view state, so each mode's answer adds them here,
/// with one shape. `drivable`: the mode's run takes drive requests now
/// (Robot: a controlled run; Build: a robot system's run); both are null
/// otherwise, as are absent resources. A `state` that is not an object is
/// left unchanged.
pub fn insert_state(state: &mut Value, drivable: bool, bindings: Option<&DriveBindings>, input: Option<&DriveInput>) {
    let bindings = bindings.filter(|_| drivable).map_or(Value::Null, |b| Value::Array(b.describe().into_iter().map(|(input, action)| json!({"input": input, "action": action})).collect()));
    let input = input.filter(|_| drivable).map_or(Value::Null, DriveInput::json);
    if let Some(o) = state.as_object_mut() {
        o.insert("bindings".into(), bindings);
        o.insert("drive_input".into(), input);
    }
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

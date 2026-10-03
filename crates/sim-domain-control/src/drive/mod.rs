//! Teleoperation drive layers shared by every host: the body twist, a
//! robot's drive profile (`sim.drive/1`, owned with its model and validated
//! through the registry), the shared acceleration limit and deadman rule,
//! and the differential-drive and mecanum mixers that a robot's controller
//! uses as its kinematic adapter. See docs/architecture/native-viewer.md
//! "Teleoperation".
//!
//! - [`kinematics`]: pure math (standard library only; the golden generator
//!   compiles it standalone).
//! - [`profile`]: the `sim.drive/1` file, its registry description and the
//!   resolved form handed to a controller.
//! - [`geometry`]: drive geometry with provenance (derived from the model or
//!   declared with its source).
//! - [`steered`]: the same twist driving `SteeredGait`.
pub mod kinematics;
pub mod geometry;
pub mod profile;
pub mod steered;

pub use kinematics::{Axes, BodyTwist, Commanded, Deadman, DifferentialDrive, KinematicsError, Limits, Mecanum, OnLoss};
pub use geometry::{DriveGeometry, Provenance, Valued, WheelJoint};
pub use profile::{DriveProfile, DriveProfileError, KinematicsSpec, ResolvedDrive};

//! Drive geometry with provenance: the track width, wheelbase, wheel radius
//! and per-wheel joint signs a mixer needs, each carrying where it came from.
//!
//! Values come either from the robot's CAD model (`Derived`, computed by
//! `sim_domain_robot::drive_geometry::derive` from joint origins, joint axes
//! and wheel collision geometry) or from a drive profile's declared geometry
//! (`Declared`, with the source the profile names). Nothing here invents a
//! value: a mixer is built only from a complete geometry.
use super::kinematics::{DifferentialDrive, Mecanum};
use serde::{Deserialize, Serialize};

/// Where a value came from (AGENTS.md: measured, derived and estimated
/// values are kept apart).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Provenance {
    /// Computed from the model; `from` names the joints/links and the method.
    Derived { from: String },
    /// Stated in a file by its author; `source` says where the number comes from.
    Declared { source: String },
    /// A judgement, not a measurement.
    Estimated { source: String },
    /// Measured on hardware; `source` names the measurement.
    Measured { source: String },
}

/// A number with its provenance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Valued {
    pub value: f64,
    pub provenance: Provenance,
}

/// One driven wheel joint in mixer order: joint rate = `sign` × rolling rate
/// (rolling rate positive rolls the body forward).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WheelJoint {
    pub joint: String,
    pub sign: f64,
    pub provenance: Provenance,
}

/// The geometry a mixer needs. `wheels` are in mixer order: `[left, right]`
/// for a differential drive, `[front_left, front_right, rear_left,
/// rear_right]` for mecanum. `wheelbase_m` is `None` for a differential drive.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriveGeometry {
    pub track_width_m: Valued,
    pub wheelbase_m: Option<Valued>,
    pub wheel_radius_m: Valued,
    pub wheels: Vec<WheelJoint>,
}

impl DriveGeometry {
    /// Joint names in mixer order.
    pub fn joints(&self) -> Vec<&str> {
        self.wheels.iter().map(|w| w.joint.as_str()).collect()
    }

    /// The differential-drive mixer, refused by name unless there are
    /// exactly two wheels and the values pass the mixer's checks.
    pub fn differential(&self) -> Result<DifferentialDrive, String> {
        let [left, right] = self.wheels.as_slice() else {
            return Err(format!("a differential drive needs 2 wheels [left, right]; the geometry lists {}", self.wheels.len()));
        };
        DifferentialDrive::new(self.track_width_m.value, self.wheel_radius_m.value, [left.sign, right.sign])
            .map_err(|e| format!("differential drive geometry: {e}"))
    }

    /// The mecanum mixer, refused by name unless there are exactly four
    /// wheels and a wheelbase.
    pub fn mecanum(&self) -> Result<Mecanum, String> {
        let [fl, fr, rl, rr] = self.wheels.as_slice() else {
            return Err(format!(
                "a mecanum drive needs 4 wheels [front_left, front_right, rear_left, rear_right]; the geometry lists {}",
                self.wheels.len()
            ));
        };
        let Some(wheelbase) = &self.wheelbase_m else {
            return Err("a mecanum drive needs wheelbase_m; the geometry has none".into());
        };
        Mecanum::new(self.track_width_m.value, wheelbase.value, self.wheel_radius_m.value, [fl.sign, fr.sign, rl.sign, rr.sign])
            .map_err(|e| format!("mecanum drive geometry: {e}"))
    }
}

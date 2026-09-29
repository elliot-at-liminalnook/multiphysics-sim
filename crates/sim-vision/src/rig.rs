//! A camera carried by a turntable: its pose is a function of the disc angle.
//!
//! The rig geometry (`sim.vision-rig/1`) is exported from CAD: the turning
//! axis, and the camera's optical centre, optical axis and up direction at
//! disc angle 0. Nothing here is guessed: a missing field is an error.
use crate::{Pose, V3, add, cross, dot, mul, rotation, scale, sub, unit};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rig {
    pub schema: String,
    /// A point on the turning axis, m (world frame, +Z up).
    pub axis_point: V3,
    /// Turning direction for positive disc angle (right-handed), unit.
    pub axis: V3,
    /// Optical centre at disc angle 0, m.
    pub optical_center: V3,
    /// Viewing direction at disc angle 0, unit.
    pub optical_axis: V3,
    /// Image "up" at disc angle 0 (projected perpendicular to the optical axis).
    pub up: V3,
    /// Where the geometry came from (CAD file and hash).
    #[serde(default)]
    pub source: serde_json::Value,
}

impl Rig {
    pub const SCHEMA: &'static str = "sim.vision-rig/1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!("rig schema `{}` is not {}", self.schema, Self::SCHEMA));
        }
        for (name, v) in [("axis", self.axis), ("optical_axis", self.optical_axis), ("up", self.up)] {
            if !v.iter().all(|x| x.is_finite()) || crate::norm(v) < 1e-9 {
                return Err(format!("rig.{name} must be a finite nonzero vector"));
            }
        }
        if crate::norm(cross(self.optical_axis, self.up)) < 1e-6 {
            return Err("rig.up must not be parallel to rig.optical_axis".into());
        }
        Ok(())
    }

    /// Camera pose at disc angle `angle` (rad).
    pub fn pose(&self, angle: f64) -> Pose {
        let z = unit(self.optical_axis);
        let up = unit(sub(self.up, scale(z, dot(self.up, z))));
        let y = scale(up, -1.0);
        let x = cross(y, z);
        let turn = rotation(self.axis, angle);
        let (x, y, z) = (mul(&turn, x), mul(&turn, y), mul(&turn, z));
        let center = add(self.axis_point, mul(&turn, sub(self.optical_center, self.axis_point)));
        Pose { r: [[x[0], y[0], z[0]], [x[1], y[1], z[1]], [x[2], y[2], z[2]]], center }
    }

    /// Distance from the axis to the optical centre, m.
    pub fn radius(&self) -> f64 {
        let d = sub(self.optical_center, self.axis_point);
        crate::norm(sub(d, scale(unit(self.axis), dot(d, unit(self.axis)))))
    }
}

/// Where a rig stands: its frame (turning axis base) placed in a parent
/// frame by a translation and a turn about +Z. Scans are reconstructed in
/// their own station frame; stations are then related by registration.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Station {
    pub position: V3,
    #[serde(default)]
    pub yaw_deg: f64,
}

impl Station {
    pub fn rotation(&self) -> crate::M3 {
        rotation([0.0, 0.0, 1.0], self.yaw_deg.to_radians())
    }
    pub fn point(&self, p: V3) -> V3 {
        add(mul(&self.rotation(), p), self.position)
    }
    pub fn pose(&self, p: &Pose) -> Pose {
        Pose { r: crate::matmul(&self.rotation(), &p.r), center: self.point(p.center) }
    }
    /// The inverse placement.
    pub fn inverse(&self) -> Station {
        let back = rotation([0.0, 0.0, 1.0], -self.yaw_deg.to_radians());
        Station { position: scale(mul(&back, self.position), -1.0), yaw_deg: -self.yaw_deg }
    }
    /// `self` then `other` (other ∘ self).
    pub fn then(&self, other: &Station) -> Station {
        Station { position: other.point(self.position), yaw_deg: self.yaw_deg + other.yaw_deg }
    }
}

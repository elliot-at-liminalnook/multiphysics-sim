//! Pinhole camera models with rolling-shutter timing.
//!
//! Images are ideal pinhole images, i.e. what a real camera gives *after*
//! lens undistortion with its calibration. The Camera Module 3 Wide's real
//! lens has strong barrel distortion (120° diagonal vs 109.5° for a pinhole
//! with the same 102° horizontal field). Real frames must be undistorted with
//! a measured calibration before [`crate::mvs`] or [`crate::refine`] use them.
use crate::V3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraModel {
    pub name: String,
    pub width: usize,
    pub height: usize,
    /// Focal lengths and principal point in pixels (pixel centres at +0.5).
    pub fx: f64,
    pub fy: f64,
    pub cx: f64,
    pub cy: f64,
    /// Time from reading the first row to reading the last (rolling shutter), s.
    pub readout_s: f64,
    /// Exposure of each row, s.
    pub exposure_s: f64,
    /// Where the numbers come from.
    pub provenance: String,
}

impl CameraModel {
    /// Raspberry Pi Camera Module 3 Wide (Sony IMX708): 4608 × 2592 active
    /// pixels, 102° horizontal field of view (datasheet), scaled to `width`.
    /// Readout ≈ 1/15 s at full resolution (estimate from the 14 fps
    /// full-resolution mode); exposure 1/100 s (a design choice for indoor light).
    pub fn camera_module_3_wide(width: usize) -> Self {
        let full = (4608.0, 2592.0);
        let k = width as f64 / full.0;
        let height = (full.1 * k).round() as usize;
        let fx = 0.5 * width as f64 / (51.0f64).to_radians().tan();
        Self {
            name: "Raspberry Pi Camera Module 3 Wide (ideal pinhole after undistortion)".into(),
            width,
            height,
            fx,
            fy: fx,
            cx: 0.5 * width as f64,
            cy: 0.5 * height as f64,
            readout_s: 1.0 / 15.0,
            exposure_s: 0.01,
            provenance: "datasheet 102° H FoV, 4608×2592; readout estimated from 14.35 fps full-res mode; exposure a design choice".into(),
        }
    }

    /// The same camera at another resolution (intrinsics scale with it).
    pub fn scaled(&self, width: usize) -> Self {
        let k = width as f64 / self.width as f64;
        Self { width, height: (self.height as f64 * k).round() as usize, fx: self.fx * k, fy: self.fy * k, cx: self.cx * k, cy: self.cy * k, ..self.clone() }
    }

    /// Direction (camera frame, z = 1) through pixel coordinate (u, v).
    pub fn ray(&self, u: f64, v: f64) -> V3 {
        [(u - self.cx) / self.fx, (v - self.cy) / self.fy, 1.0]
    }

    /// Pixel coordinate of a camera-frame point; `None` behind the camera.
    pub fn project(&self, p: V3) -> Option<(f64, f64)> {
        (p[2] > 1e-6).then(|| (self.cx + self.fx * p[0] / p[2], self.cy + self.fy * p[1] / p[2]))
    }

    pub fn contains(&self, u: f64, v: f64) -> bool {
        u >= 0.0 && v >= 0.0 && u < self.width as f64 && v < self.height as f64
    }

    pub fn horizontal_fov_deg(&self) -> f64 {
        2.0 * (0.5 * self.width as f64 / self.fx).atan().to_degrees()
    }
}

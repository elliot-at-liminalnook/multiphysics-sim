//! Deterministic ray-cast frames.
//!
//! Each image row is exposed over `[t_row, t_row + exposure]`, where
//! `t_row = start + (row + ½)/height · readout` (rolling shutter), and the
//! pose is taken from the mechanism's simulated trajectory at each sample
//! time. So a frame shot while turning shows the real smear and skew. Depth
//! is the camera-frame z of the first hit at mid-exposure, the ground truth
//! for the reconstruction.
use crate::camera::CameraModel;
use crate::scene::Scene;
use crate::{Gray, Pose, V3, map_rows, unit};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderSettings {
    /// Rays per pixel along each axis (2 → 4 rays): anti-aliasing.
    pub supersample: u32,
    /// Pose samples across each row's exposure when the camera moves.
    pub time_samples: u32,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self { supersample: 2, time_samples: 4 }
    }
}

pub struct Frame {
    pub width: usize,
    pub height: usize,
    /// Linear radiance, row-major.
    pub rgb: Vec<V3>,
    /// Camera-frame depth (z) in metres; infinite where nothing was hit.
    pub depth: Vec<f32>,
}

fn srgb(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.003_130_8 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
}

impl Frame {
    /// 8-bit sRGB, as a camera would store it.
    pub fn rgb8(&self) -> Vec<u8> {
        self.rgb.iter().flat_map(|c| c.map(|x| (srgb(x) * 255.0).round() as u8)).collect()
    }
    /// Luma of the 8-bit sRGB image (what matching sees on real photos too).
    pub fn gray(&self) -> Gray {
        gray_from_rgb8(self.width, self.height, &self.rgb8())
    }
}

pub fn gray_from_rgb8(width: usize, height: usize, rgb: &[u8]) -> Gray {
    let data = rgb.chunks_exact(3).map(|c| (0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32) / 255.0).collect();
    Gray { width, height, data }
}

/// Render one frame whose first row starts exposing at `start` (s).
pub fn render(scene: &Scene, camera: &CameraModel, pose_at: &(dyn Fn(f64) -> Pose + Sync), start: f64, settings: RenderSettings) -> Frame {
    let (w, h) = (camera.width, camera.height);
    let ss = settings.supersample.max(1) as usize;
    let first = pose_at(start);
    let last = pose_at(start + camera.readout_s + camera.exposure_s);
    let moving = crate::norm(crate::sub(first.center, last.center)) > 1e-9 || (0..3).any(|i| (0..3).any(|j| (first.r[i][j] - last.r[i][j]).abs() > 1e-9));
    let nt = if moving { settings.time_samples.max(1) as usize } else { 1 };
    let rows = map_rows(h, |y| {
        let t_row = start + (y as f64 + 0.5) / h as f64 * camera.readout_s;
        let poses: Vec<Pose> = (0..nt).map(|k| pose_at(t_row + camera.exposure_s * (k as f64 + 0.5) / nt as f64)).collect();
        let mid = pose_at(t_row + 0.5 * camera.exposure_s);
        let mut colors = Vec::with_capacity(w);
        let mut depths = Vec::with_capacity(w);
        for x in 0..w {
            let mut c = [0.0; 3];
            for pose in &poses {
                for sy in 0..ss {
                    for sx in 0..ss {
                        let u = x as f64 + (sx as f64 + 0.5) / ss as f64;
                        let v = y as f64 + (sy as f64 + 0.5) / ss as f64;
                        let dir = unit(pose.direction_to_world(camera.ray(u, v)));
                        if let Some(hit) = scene.hit(pose.center, dir) {
                            c = crate::add(c, scene.shade(&hit));
                        }
                    }
                }
            }
            colors.push(crate::scale(c, 1.0 / (ss * ss * nt) as f64));
            let ray = camera.ray(x as f64 + 0.5, y as f64 + 0.5);
            let dir = mid.direction_to_world(ray);
            depths.push(scene.hit(mid.center, dir).map_or(f32::INFINITY, |hit| hit.t as f32)); // ray has z = 1, so t is the depth
        }
        (colors, depths)
    });
    let mut frame = Frame { width: w, height: h, rgb: Vec::with_capacity(w * h), depth: Vec::with_capacity(w * h) };
    for (c, d) in rows {
        frame.rgb.extend(c);
        frame.depth.extend(d);
    }
    frame
}

/// Equirectangular panorama seen from `center`: azimuth −180°…180° across
/// (0° = +X turned by `yaw_deg`, increasing towards +Y), elevation
/// `+half_height_deg` at the top to `−half_height_deg` at the bottom.
pub fn panorama(scene: &Scene, center: V3, width: usize, half_height_deg: f64, yaw_deg: f64) -> Frame {
    let height = (width as f64 * (2.0 * half_height_deg) / 360.0).round() as usize;
    let rows = map_rows(height, |y| {
        let el = (half_height_deg - (y as f64 + 0.5) / height as f64 * 2.0 * half_height_deg).to_radians();
        let mut colors = Vec::with_capacity(width);
        let mut depths = Vec::with_capacity(width);
        for x in 0..width {
            let az = (-180.0 + (x as f64 + 0.5) / width as f64 * 360.0 + yaw_deg).to_radians();
            let dir = [el.cos() * az.cos(), el.cos() * az.sin(), el.sin()];
            match scene.hit(center, dir) {
                Some(hit) => {
                    colors.push(scene.shade(&hit));
                    depths.push(hit.t as f32);
                }
                None => {
                    colors.push([0.0; 3]);
                    depths.push(f32::INFINITY);
                }
            }
        }
        (colors, depths)
    });
    let mut frame = Frame { width, height, rgb: Vec::new(), depth: Vec::new() };
    for (c, d) in rows {
        frame.rgb.extend(c);
        frame.depth.extend(d);
    }
    frame
}

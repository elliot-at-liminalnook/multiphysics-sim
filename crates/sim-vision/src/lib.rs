//! Virtual cameras and reconstruction for rigs whose pose comes from the
//! simulated mechanism.
//!
//! * [`camera`]: pinhole camera models (Camera Module 3 Wide preset), with
//!   rolling-shutter readout and exposure time.
//! * [`rig`]: a camera on a turntable. The pose is a function of one angle,
//!   and the geometry comes from CAD (`sim.vision-rig/1`).
//! * [`scene`]: an environment file (`sim.vision-scene/1`): rooms, boxes,
//!   spheres and cylinders with procedural textures and fixed lighting.
//! * [`render`]: deterministic ray casting of a frame. Each row is exposed at
//!   its own time (rolling shutter) and averaged over the exposure (motion
//!   blur). It also gives ground-truth depth.
//! * [`mvs`]: known-pose plane-sweep stereo (NCC) turning frames into depth maps.
//! * [`refine`]: rig-constrained pose refinement, one angle correction per
//!   shot, found from multi-view photometric consistency.
//! * [`output`]: PNG, PLY, equirectangular panoramas and COLMAP text export.
//! * [`fusion`]: depth maps fused into a signed-distance volume, which gives a
//!   coloured mesh, ray casts and clearance.
//! * [`register`]: aligning scan stations (level, so 4-DoF ICP).
//! * [`place`]: the place model (`sim.place/1`): layout planes, height and
//!   free-space maps, photos with poses, rendering, path planning.
//!
//! Units are SI (metres, radians, seconds). World frame: +Z up. Camera frame:
//! x right, y down, z forward (OpenCV/COLMAP convention).
pub mod camera;
pub mod fusion;
pub mod mvs;
pub mod output;
pub mod place;
pub mod refine;
pub mod register;
pub mod render;
pub mod rig;
pub mod scene;

pub type V3 = [f64; 3];
/// Row-major 3×3 matrix.
pub type M3 = [[f64; 3]; 3];

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn scale(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
pub fn unit(a: V3) -> V3 {
    scale(a, 1.0 / norm(a))
}
pub fn mul(m: &M3, v: V3) -> V3 {
    [dot(m[0], v), dot(m[1], v), dot(m[2], v)]
}
pub fn transpose(m: &M3) -> M3 {
    [[m[0][0], m[1][0], m[2][0]], [m[0][1], m[1][1], m[2][1]], [m[0][2], m[1][2], m[2][2]]]
}
pub fn matmul(a: &M3, b: &M3) -> M3 {
    let bt = transpose(b);
    [[dot(a[0], bt[0]), dot(a[0], bt[1]), dot(a[0], bt[2])], [dot(a[1], bt[0]), dot(a[1], bt[1]), dot(a[1], bt[2])], [dot(a[2], bt[0]), dot(a[2], bt[1]), dot(a[2], bt[2])]]
}
/// Rotation by `angle` about the unit `axis` (Rodrigues).
pub fn rotation(axis: V3, angle: f64) -> M3 {
    let [x, y, z] = unit(axis);
    let (s, c) = angle.sin_cos();
    let t = 1.0 - c;
    [[t * x * x + c, t * x * y - s * z, t * x * z + s * y], [t * x * y + s * z, t * y * y + c, t * y * z - s * x], [t * x * z - s * y, t * y * z + s * x, t * z * z + c]]
}

/// A camera pose: `r` maps camera-frame directions to world (its columns are
/// the camera axes in world coordinates); `center` is the optical centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub r: M3,
    pub center: V3,
}

impl Pose {
    pub fn to_camera(&self, p: V3) -> V3 {
        mul(&transpose(&self.r), sub(p, self.center))
    }
    pub fn to_world(&self, p: V3) -> V3 {
        add(mul(&self.r, p), self.center)
    }
    pub fn direction_to_world(&self, d: V3) -> V3 {
        mul(&self.r, d)
    }
}

/// Map rows through `f` in parallel when the `parallel` feature is on.
pub(crate) fn map_rows<T: Send>(rows: usize, f: impl Fn(usize) -> T + Sync + Send) -> Vec<T> {
    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        (0..rows).into_par_iter().map(f).collect()
    }
    #[cfg(not(feature = "parallel"))]
    {
        (0..rows).map(f).collect()
    }
}

/// A single-channel image (values 0..1), for matching.
#[derive(Clone, Debug)]
pub struct Gray {
    pub width: usize,
    pub height: usize,
    pub data: Vec<f32>,
}

impl Gray {
    pub fn at(&self, x: usize, y: usize) -> f32 {
        self.data[y * self.width + x]
    }
    /// Bilinear sample at a pixel-centre coordinate (pixel (0,0) spans 0..1,
    /// centre 0.5). `None` outside the image.
    pub fn sample(&self, u: f64, v: f64) -> Option<f32> {
        let x = u - 0.5;
        let y = v - 0.5;
        if !(x >= 0.0 && y >= 0.0 && x <= (self.width - 1) as f64 && y <= (self.height - 1) as f64) {
            return None;
        }
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.width - 1), (y0 + 1).min(self.height - 1));
        let (fx, fy) = ((x - x0 as f64) as f32, (y - y0 as f64) as f32);
        let a = self.at(x0, y0) * (1.0 - fx) + self.at(x1, y0) * fx;
        let b = self.at(x0, y1) * (1.0 - fx) + self.at(x1, y1) * fx;
        Some(a * (1.0 - fy) + b * fy)
    }
    /// Half resolution by 2×2 box averaging.
    pub fn half(&self) -> Gray {
        let (w, h) = (self.width / 2, self.height / 2);
        let mut data = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                data.push(0.25 * (self.at(2 * x, 2 * y) + self.at(2 * x + 1, 2 * y) + self.at(2 * x, 2 * y + 1) + self.at(2 * x + 1, 2 * y + 1)));
            }
        }
        Gray { width: w, height: h, data }
    }
}

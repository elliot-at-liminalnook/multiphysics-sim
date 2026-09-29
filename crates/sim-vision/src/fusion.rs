//! Depth-map fusion into a truncated signed-distance volume (TSDF), and
//! what it gives: a coloured surface mesh (surface nets), ray casting,
//! clearance and free-space queries.
//!
//! Each voxel keeps a weighted running mean of the signed distance to the
//! nearest surface along camera rays, truncated to ±`truncation`, plus colour.
//! A voxel never seen has weight 0 ("unknown"): queries report unknown space
//! as unknown, never as free.
use crate::camera::CameraModel;
use crate::mvs::DepthMap;
use crate::{Pose, V3, add, scale, sub, unit};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grid {
    /// Centre of voxel (0, 0, 0), m.
    pub origin: V3,
    pub voxel: f64,
    pub dims: [usize; 3],
    pub truncation: f64,
}

pub struct Tsdf {
    pub grid: Grid,
    /// Signed distance / truncation, in −1…1 (1 where unobserved).
    pub sdf: Vec<f32>,
    pub weight: Vec<f32>,
    pub color: Vec<[f32; 3]>,
}

/// A triangle mesh with per-vertex normals and sRGB colours.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub vertices: Vec<V3>,
    pub normals: Vec<V3>,
    pub colors: Vec<[u8; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

pub struct Hit {
    pub point: V3,
    pub distance: f64,
    pub normal: V3,
}

impl Tsdf {
    pub fn new(min: V3, max: V3, voxel: f64, truncation: f64) -> Self {
        let dims = [0, 1, 2].map(|k| (((max[k] - min[k]) / voxel).ceil() as usize + 1).max(2));
        let n = dims[0] * dims[1] * dims[2];
        Self { grid: Grid { origin: min, voxel, dims, truncation }, sdf: vec![1.0; n], weight: vec![0.0; n], color: vec![[0.0; 3]; n] }
    }

    pub fn index(&self, i: usize, j: usize, k: usize) -> usize {
        (k * self.grid.dims[1] + j) * self.grid.dims[0] + i
    }

    pub fn center(&self, i: usize, j: usize, k: usize) -> V3 {
        add(self.grid.origin, scale([i as f64, j as f64, k as f64], self.grid.voxel))
    }

    /// Fuse one depth map (on `camera`'s pixel grid) seen from `pose`, with
    /// colours from `rgb8` (same resolution as `camera`).
    pub fn integrate(&mut self, map: &DepthMap, camera: &CameraModel, pose: &Pose, rgb8: &[u8]) {
        let [nx, ny, _] = self.grid.dims;
        let slice = nx * ny;
        let grid = self.grid.clone();
        let mu = grid.truncation;
        let rt = crate::transpose(&pose.r);
        let work = |k: usize, sdf: &mut [f32], weight: &mut [f32], color: &mut [[f32; 3]]| {
            for j in 0..ny {
                for i in 0..nx {
                    let p = add(grid.origin, scale([i as f64, j as f64, k as f64], grid.voxel));
                    let q = crate::mul(&rt, sub(p, pose.center));
                    if q[2] <= 1e-3 {
                        continue;
                    }
                    let Some((u, v)) = camera.project(q) else { continue };
                    if !camera.contains(u, v) {
                        continue;
                    }
                    let (gi, gj) = ((u / map.step as f64) as usize, (v / map.step as f64) as usize);
                    if gi >= map.columns || gj >= map.rows {
                        continue;
                    }
                    let d = map.depth[gj * map.columns + gi] as f64;
                    if !d.is_finite() {
                        continue;
                    }
                    let signed = d - q[2];
                    if signed < -mu {
                        continue; // behind the surface: occluded, no information
                    }
                    let t = (signed / mu).min(1.0) as f32;
                    let c = j * nx + i;
                    let (w0, w1) = (weight[c], weight[c] + 1.0);
                    sdf[c] = (sdf[c] * w0 + t) / w1;
                    if signed.abs() < mu {
                        let (x, y) = (u as usize, v as usize);
                        let o = 3 * (y * camera.width + x);
                        for ch in 0..3 {
                            color[c][ch] = (color[c][ch] * w0 + rgb8[o + ch] as f32) / w1;
                        }
                    }
                    weight[c] = w1;
                }
            }
        };
        #[cfg(feature = "parallel")]
        {
            use rayon::prelude::*;
            self.sdf.par_chunks_mut(slice).zip(self.weight.par_chunks_mut(slice)).zip(self.color.par_chunks_mut(slice)).enumerate().for_each(|(k, ((s, w), c))| work(k, s, w, c));
        }
        #[cfg(not(feature = "parallel"))]
        for (k, ((s, w), c)) in self.sdf.chunks_mut(slice).zip(self.weight.chunks_mut(slice)).zip(self.color.chunks_mut(slice)).enumerate() {
            work(k, s, w, c);
        }
    }

    /// Trilinear (signed distance in metres, weight) at a point; `None` outside the grid.
    pub fn sample(&self, p: V3) -> Option<(f64, f64)> {
        let g = &self.grid;
        let f = [0, 1, 2].map(|k| (p[k] - g.origin[k]) / g.voxel);
        if (0..3).any(|k| f[k] < 0.0 || f[k] > (g.dims[k] - 1) as f64) {
            return None;
        }
        let b = f.map(|x| x.floor() as usize);
        let b = [0, 1, 2].map(|k| b[k].min(g.dims[k] - 2));
        let t = [0, 1, 2].map(|k| f[k] - b[k] as f64);
        let (mut s, mut w, mut total) = (0.0, 0.0, 0.0);
        for dz in 0..2 {
            for dy in 0..2 {
                for dx in 0..2 {
                    let k = [(dx, 0), (dy, 1), (dz, 2)].iter().map(|(d, a)| if *d == 1 { t[*a] } else { 1.0 - t[*a] }).product::<f64>();
                    let idx = self.index(b[0] + dx, b[1] + dy, b[2] + dz);
                    if self.weight[idx] > 0.0 {
                        s += k * self.sdf[idx] as f64;
                        total += k;
                    }
                    w += k * self.weight[idx] as f64;
                }
            }
        }
        (total > 1e-9).then(|| (s / total * g.truncation, w))
    }

    /// Surface normal (normalised signed-distance gradient) at a point.
    pub fn normal(&self, p: V3) -> Option<V3> {
        let h = self.grid.voxel;
        let mut g = [0.0; 3];
        for k in 0..3 {
            let mut a = p;
            let mut b = p;
            a[k] += h;
            b[k] -= h;
            g[k] = self.sample(a)?.0 - self.sample(b)?.0;
        }
        (crate::norm(g) > 1e-12).then(|| unit(g))
    }

    /// First surface crossing (+ to −) along a ray, within `max_distance`.
    pub fn raycast(&self, origin: V3, direction: V3, max_distance: f64) -> Option<Hit> {
        let d = unit(direction);
        let step_min = 0.5 * self.grid.voxel;
        let mut t = 0.0;
        let mut previous: Option<(f64, f64)> = None;
        // A surface seen only in patches: bridge unknown gaps up to this long.
        let bridge = 3.0 * self.grid.voxel;
        while t < max_distance {
            let p = add(origin, scale(d, t));
            match self.sample(p) {
                Some((s, w)) if w > 0.0 => {
                    if let Some((pt, ps)) = previous {
                        if ps > 0.0 && s <= 0.0 && t - pt <= bridge + step_min {
                            let hit_t = pt + (t - pt) * ps / (ps - s);
                            let point = add(origin, scale(d, hit_t));
                            let normal = self.normal(point).unwrap_or(scale(d, -1.0));
                            return Some(Hit { point, distance: hit_t, normal });
                        }
                    }
                    previous = Some((t, s));
                    t += (0.8 * s.abs()).max(step_min);
                }
                _ => {
                    if previous.is_some_and(|(pt, _)| t - pt > bridge) {
                        previous = None;
                    }
                    t += step_min;
                }
            }
        }
        None
    }

    /// Colour (0–255) near a surface point.
    pub fn color_at(&self, p: V3) -> Option<[u8; 3]> {
        let g = &self.grid;
        let i = [0, 1, 2].map(|k| ((p[k] - g.origin[k]) / g.voxel).round() as i64);
        let mut best: Option<(f32, [f32; 3])> = None;
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (x, y, z) = (i[0] + dx, i[1] + dy, i[2] + dz);
                    if x < 0 || y < 0 || z < 0 || x >= g.dims[0] as i64 || y >= g.dims[1] as i64 || z >= g.dims[2] as i64 {
                        continue;
                    }
                    let idx = self.index(x as usize, y as usize, z as usize);
                    if self.weight[idx] > 0.0 && self.sdf[idx].abs() < 0.999 && best.is_none_or(|b| self.sdf[idx].abs() < b.0) {
                        best = Some((self.sdf[idx].abs(), self.color[idx]));
                    }
                }
            }
        }
        best.map(|b| b.1.map(|c| c.round().clamp(0.0, 255.0) as u8))
    }

    /// Surface nets: one vertex per cell the surface crosses (the mean of its
    /// edge crossings), one quad per crossed edge. Only edges whose both
    /// ends were observed and lie within the truncation band are used, so
    /// the boundary between seen and unseen space makes no false walls.
    pub fn mesh(&self) -> Mesh {
        let [nx, ny, nz] = self.grid.dims;
        let valid = |idx: usize| self.weight[idx] > 0.0 && self.sdf[idx].abs() < 0.999;
        let corner = |i: usize, j: usize, k: usize| self.index(i, j, k);
        let mut cell_vertex = vec![u32::MAX; (nx - 1) * (ny - 1) * (nz - 1)];
        let cell = |i: usize, j: usize, k: usize| (k * (ny - 1) + j) * (nx - 1) + i;
        let mut mesh = Mesh::default();
        const EDGES: [([usize; 3], [usize; 3]); 12] = [
            ([0, 0, 0], [1, 0, 0]), ([0, 1, 0], [1, 1, 0]), ([0, 0, 1], [1, 0, 1]), ([0, 1, 1], [1, 1, 1]),
            ([0, 0, 0], [0, 1, 0]), ([1, 0, 0], [1, 1, 0]), ([0, 0, 1], [0, 1, 1]), ([1, 0, 1], [1, 1, 1]),
            ([0, 0, 0], [0, 0, 1]), ([1, 0, 0], [1, 0, 1]), ([0, 1, 0], [0, 1, 1]), ([1, 1, 0], [1, 1, 1]),
        ];
        for k in 0..nz - 1 {
            for j in 0..ny - 1 {
                for i in 0..nx - 1 {
                    let mut sum = [0.0; 3];
                    let mut count = 0.0;
                    for (a, b) in EDGES {
                        let (ia, ib) = (corner(i + a[0], j + a[1], k + a[2]), corner(i + b[0], j + b[1], k + b[2]));
                        if !(valid(ia) && valid(ib)) {
                            continue;
                        }
                        let (sa, sb) = (self.sdf[ia] as f64, self.sdf[ib] as f64);
                        if (sa > 0.0) != (sb > 0.0) {
                            let t = sa / (sa - sb);
                            let pa = self.center(i + a[0], j + a[1], k + a[2]);
                            let pb = self.center(i + b[0], j + b[1], k + b[2]);
                            sum = add(sum, add(pa, scale(sub(pb, pa), t)));
                            count += 1.0;
                        }
                    }
                    if count > 0.0 {
                        let p = scale(sum, 1.0 / count);
                        cell_vertex[cell(i, j, k)] = mesh.vertices.len() as u32;
                        mesh.vertices.push(p);
                    }
                }
            }
        }
        // Quads around every crossed voxel edge (axis a), shared by four cells.
        for k in 1..nz - 1 {
            for j in 1..ny - 1 {
                for i in 1..nx - 1 {
                    let here = corner(i, j, k);
                    for axis in 0..3 {
                        let next = match axis {
                            0 if i + 1 < nx => corner(i + 1, j, k),
                            1 if j + 1 < ny => corner(i, j + 1, k),
                            2 if k + 1 < nz => corner(i, j, k + 1),
                            _ => continue,
                        };
                        if !(valid(here) && valid(next)) || (self.sdf[here] > 0.0) == (self.sdf[next] > 0.0) {
                            continue;
                        }
                        // The four cells around the edge from (i,j,k) along `axis`.
                        let cells = match axis {
                            0 => [cell(i, j - 1, k - 1), cell(i, j, k - 1), cell(i, j, k), cell(i, j - 1, k)],
                            1 => [cell(i - 1, j, k - 1), cell(i - 1, j, k), cell(i, j, k), cell(i, j, k - 1)],
                            _ => [cell(i - 1, j - 1, k), cell(i, j - 1, k), cell(i, j, k), cell(i - 1, j, k)],
                        };
                        let v = cells.map(|c| cell_vertex[c]);
                        if v.contains(&u32::MAX) {
                            continue;
                        }
                        // Outward (towards positive distance) winding.
                        let flip = self.sdf[here] > 0.0;
                        let (a, b) = if flip { ([v[0], v[2], v[1]], [v[0], v[3], v[2]]) } else { ([v[0], v[1], v[2]], [v[0], v[2], v[3]]) };
                        mesh.triangles.push(a);
                        mesh.triangles.push(b);
                    }
                }
            }
        }
        mesh.normals = mesh.vertices.iter().map(|p| self.normal(*p).unwrap_or([0.0, 0.0, 1.0])).collect();
        mesh.colors = mesh.vertices.iter().map(|p| self.color_at(*p).unwrap_or([128, 128, 128])).collect();
        mesh
    }

    /// Serialise as a small header plus i16 distance, u16 weight and u8 colour per voxel.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.sdf.len() * 7);
        for ((s, w), c) in self.sdf.iter().zip(&self.weight).zip(&self.color) {
            out.extend(((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
            out.extend((w.min(65535.0) as u16).to_le_bytes());
            out.extend(c.map(|x| x.round().clamp(0.0, 255.0) as u8));
        }
        out
    }

    pub fn from_bytes(grid: Grid, bytes: &[u8]) -> Result<Self, String> {
        let n = grid.dims.iter().product::<usize>();
        if bytes.len() != 7 * n {
            return Err(format!("volume has {} bytes, expected {} for {:?} voxels", bytes.len(), 7 * n, grid.dims));
        }
        let mut t = Self { grid, sdf: Vec::with_capacity(n), weight: Vec::with_capacity(n), color: Vec::with_capacity(n) };
        for c in bytes.chunks_exact(7) {
            t.sdf.push(i16::from_le_bytes([c[0], c[1]]) as f32 / 32767.0);
            t.weight.push(u16::from_le_bytes([c[2], c[3]]) as f32);
            t.color.push([c[4] as f32, c[5] as f32, c[6] as f32]);
        }
        Ok(t)
    }
}

impl Mesh {
    /// Binary little-endian PLY with normals and colours.
    pub fn write_ply(&self, path: &std::path::Path) -> Result<(), String> {
        let mut out = format!(
            "ply\nformat binary_little_endian 1.0\ncomment sim-vision fused mesh, metres, +Z up\nelement vertex {}\nproperty float x\nproperty float y\nproperty float z\nproperty float nx\nproperty float ny\nproperty float nz\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nelement face {}\nproperty list uchar int vertex_indices\nend_header\n",
            self.vertices.len(),
            self.triangles.len()
        )
        .into_bytes();
        for ((p, n), c) in self.vertices.iter().zip(&self.normals).zip(&self.colors) {
            for v in p.iter().chain(n) {
                out.extend((*v as f32).to_le_bytes());
            }
            out.extend(c);
        }
        for t in &self.triangles {
            out.push(3);
            for i in t {
                out.extend((*i as i32).to_le_bytes());
            }
        }
        std::fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// OBJ + MTL-free vertex colours (x y z r g b), for viewers; `up_y` maps +Z up to +Y up.
    pub fn write_obj(&self, path: &std::path::Path, up_y: bool) -> Result<(), String> {
        use std::fmt::Write as _;
        let mut s = String::from("# sim-vision fused mesh (metres)\n");
        for (p, c) in self.vertices.iter().zip(&self.colors) {
            let (x, y, z) = if up_y { (p[0], p[2], -p[1]) } else { (p[0], p[1], p[2]) };
            let _ = writeln!(s, "v {x:.4} {y:.4} {z:.4} {:.3} {:.3} {:.3}", c[0] as f64 / 255.0, c[1] as f64 / 255.0, c[2] as f64 / 255.0);
        }
        for n in &self.normals {
            let (x, y, z) = if up_y { (n[0], n[2], -n[1]) } else { (n[0], n[1], n[2]) };
            let _ = writeln!(s, "vn {x:.3} {y:.3} {z:.3}");
        }
        for t in &self.triangles {
            let _ = writeln!(s, "f {0}//{0} {1}//{1} {2}//{2}", t[0] + 1, t[1] + 1, t[2] + 1);
        }
        std::fs::write(path, s).map_err(|e| format!("{}: {e}", path.display()))
    }
}

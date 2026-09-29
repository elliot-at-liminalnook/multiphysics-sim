//! A place model (`sim.place/1`): what a scan of a place knows, in a form an
//! AI (or a simulator) can query.
//!
//! * a fused TSDF volume and the coloured mesh from it;
//! * the room layout: planes found by RANSAC, labelled floor, ceiling, wall,
//!   horizontal surface (tables, shelves) or vertical surface;
//! * a top-down height map (highest upward-facing surface per cell) and a
//!   free-space map for a height band (free / occupied / unknown);
//! * every photo with its pose, so answers can point to real evidence.
//!
//! Frame: the first scan station's frame (turntable base at the origin),
//! metres, +Z up. Unknown space stays unknown in every query.
use crate::camera::CameraModel;
use crate::fusion::{Grid, Mesh, Tsdf};
use crate::{Pose, V3, add, cross, dot, norm, scale, sub, unit};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SCHEMA: &str = "sim.place/1";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PoseRecord {
    pub center: V3,
    /// Camera axes in the place frame (columns: right, down, forward), row-major.
    pub rotation: [[f64; 3]; 3],
}
impl PoseRecord {
    pub fn pose(&self) -> Pose {
        Pose { r: self.rotation, center: self.center }
    }
    pub fn from(p: &Pose) -> Self {
        Self { center: p.center, rotation: p.r }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ViewRecord {
    pub name: String,
    /// Image path relative to the place directory.
    pub image: String,
    pub pose: PoseRecord,
    pub station: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plane {
    pub id: usize,
    /// floor | ceiling | wall | horizontal surface | vertical surface | sloped surface
    pub kind: String,
    /// Unit normal pointing into free space.
    pub normal: V3,
    pub centroid: V3,
    /// In-plane axes and the extent of the supporting points along them, m.
    pub axis_u: V3,
    pub axis_v: V3,
    pub extent_u: [f64; 2],
    pub extent_v: [f64; 2],
    pub area_m2: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MapGrid {
    /// Centre of cell (0, 0), m.
    pub origin: [f64; 2],
    pub cell: f64,
    pub dims: [usize; 2],
}
impl MapGrid {
    pub fn index_of(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        let (i, j) = (((x - self.origin[0]) / self.cell).round(), ((y - self.origin[1]) / self.cell).round());
        (i >= 0.0 && j >= 0.0 && (i as usize) < self.dims[0] && (j as usize) < self.dims[1]).then_some((i as usize, j as usize))
    }
    pub fn center(&self, i: usize, j: usize) -> [f64; 2] {
        [self.origin[0] + i as f64 * self.cell, self.origin[1] + j as f64 * self.cell]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Station {
    pub name: String,
    /// Placement of this station's frame in the place frame.
    pub placement: crate::rig::Station,
    /// How it was found (first station, registration result, ...).
    pub method: String,
    #[serde(default)]
    pub registration: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlaceFile {
    pub schema: String,
    pub description: String,
    pub frame: String,
    pub volume: Grid,
    pub volume_file: String,
    pub mesh_file: String,
    pub mesh_obj_file: String,
    pub bounds: [V3; 2],
    pub floor_z: Option<f64>,
    pub ceiling_z: Option<f64>,
    pub planes: Vec<Plane>,
    pub map: MapGrid,
    /// Height map file: f32 per cell, NaN where nothing was seen.
    pub height_file: String,
    pub camera: CameraModel,
    pub stations: Vec<Station>,
    pub views: Vec<ViewRecord>,
    pub quality: serde_json::Value,
    pub provenance: serde_json::Value,
}

pub struct Place {
    pub dir: PathBuf,
    pub file: PlaceFile,
    pub tsdf: Tsdf,
    pub heights: Vec<f32>,
    images: std::sync::OnceLock<Vec<(usize, usize, Vec<u8>)>>,
}

/// One view going into a build.
pub struct InputView {
    pub name: String,
    pub image: String,
    pub pose: Pose,
    pub station: usize,
    pub depth: crate::mvs::DepthMap,
    /// The camera the depth map's pixel grid belongs to, and matching colours.
    pub depth_camera: CameraModel,
    pub depth_rgb8: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BuildSettings {
    pub voxel: f64,
    pub truncation: f64,
    pub map_cell: f64,
    /// Planes need at least this area, m².
    pub min_plane_area: f64,
}
impl Default for BuildSettings {
    fn default() -> Self {
        Self { voxel: 0.03, truncation: 0.09, map_cell: 0.05, min_plane_area: 0.15 }
    }
}

fn percentile(mut v: Vec<f64>, p: f64) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[((v.len() - 1) as f64 * p).round() as usize]
}

/// Bounds of the observed points (1st–99th percentile, plus a margin).
pub fn observed_bounds(views: &[InputView], margin: f64) -> [V3; 2] {
    let mut pts: [Vec<f64>; 3] = Default::default();
    for v in views {
        for p in crate::output::points(&v.depth, &v.depth_camera, &v.pose, &v.depth_rgb8, 0).iter().step_by(7) {
            for k in 0..3 {
                pts[k].push(p.position[k]);
            }
        }
    }
    let lo = [0, 1, 2].map(|k| percentile(pts[k].clone(), 0.005) - margin);
    let hi = [0, 1, 2].map(|k| percentile(pts[k].clone(), 0.995) + margin);
    [lo, hi]
}

/// Fuse views into a volume.
pub fn fuse(views: &[InputView], bounds: [V3; 2], s: &BuildSettings) -> Tsdf {
    let mut tsdf = Tsdf::new(bounds[0], bounds[1], s.voxel, s.truncation);
    for v in views {
        tsdf.integrate(&v.depth, &v.depth_camera, &v.pose, &v.depth_rgb8);
    }
    tsdf
}

/// Dominant planes by normal-guided RANSAC over area-weighted triangle samples.
pub fn find_planes(mesh: &Mesh, s: &BuildSettings) -> Vec<Plane> {
    let mut samples: Vec<(V3, V3, f64)> = Vec::new();
    for t in &mesh.triangles {
        let [a, b, c] = t.map(|i| mesh.vertices[i as usize]);
        let n = cross(sub(b, a), sub(c, a));
        let area = 0.5 * norm(n);
        if area > 1e-9 {
            samples.push((scale(add(add(a, b), c), 1.0 / 3.0), unit(n), area));
        }
    }
    // Mesh winding follows the signed distance, so triangle normals point into free space.
    let stride = (samples.len() / 60_000).max(1);
    let mut pool: Vec<(V3, V3, f64)> = samples.iter().step_by(stride).map(|(p, n, a)| (*p, *n, a * stride as f64)).collect();
    let mut planes = Vec::new();
    let mut rng = 0x2545_F491_4F6C_DD1Du64;
    let mut next = |n: usize| {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng % n.max(1) as u64) as usize
    };
    let tol = 1.5 * s.voxel;
    for _ in 0..24 {
        if pool.len() < 50 {
            break;
        }
        let mut best: Option<(f64, V3, f64)> = None;
        for _ in 0..400 {
            let (p, n, _) = pool[next(pool.len())];
            let d = dot(n, p);
            let area: f64 = pool.iter().filter(|(q, m, _)| (dot(n, *q) - d).abs() < tol && dot(n, *m) > 0.9).map(|x| x.2).sum();
            if best.is_none_or(|b| area > b.0) {
                best = Some((area, n, d));
            }
        }
        let Some((area, n0, d0)) = best else { break };
        if area < s.min_plane_area {
            break;
        }
        let (inliers, rest): (Vec<&(V3, V3, f64)>, Vec<&(V3, V3, f64)>) = pool.iter().partition(|(q, m, _)| (dot(n0, *q) - d0).abs() < tol && dot(n0, *m) > 0.9);
        // Refit: area-weighted mean normal and centroid.
        let total: f64 = inliers.iter().map(|x| x.2).sum();
        let n = unit(inliers.iter().fold([0.0; 3], |acc, x| add(acc, scale(x.1, x.2))));
        let centroid = scale(inliers.iter().fold([0.0; 3], |acc, x| add(acc, scale(x.0, x.2))), 1.0 / total);
        let horizontal = n[2].abs() > 0.95;
        let vertical = n[2].abs() < 0.1;
        let axis_u = if horizontal { [1.0, 0.0, 0.0] } else { unit(cross([0.0, 0.0, 1.0], n)) };
        let axis_v = if horizontal { [0.0, 1.0, 0.0] } else { unit(cross(n, axis_u)) };
        let (mut eu, mut ev) = ([f64::INFINITY, f64::NEG_INFINITY], [f64::INFINITY, f64::NEG_INFINITY]);
        let mut us: Vec<f64> = inliers.iter().map(|x| dot(sub(x.0, centroid), axis_u)).collect();
        let mut vs: Vec<f64> = inliers.iter().map(|x| dot(sub(x.0, centroid), axis_v)).collect();
        us.sort_by(|a, b| a.total_cmp(b));
        vs.sort_by(|a, b| a.total_cmp(b));
        let q = |v: &[f64], p: f64| v[((v.len() - 1) as f64 * p) as usize];
        eu = [eu[0].min(q(&us, 0.01)), eu[1].max(q(&us, 0.99))];
        ev = [ev[0].min(q(&vs, 0.01)), ev[1].max(q(&vs, 0.99))];
        let kind = if horizontal { if n[2] > 0.0 { "horizontal surface" } else { "ceiling" } } else if vertical { "vertical surface" } else { "sloped surface" };
        planes.push(Plane { id: planes.len(), kind: kind.into(), normal: n, centroid, axis_u, axis_v, extent_u: eu, extent_v: ev, area_m2: total });
        pool = rest.into_iter().copied().collect();
    }
    // Labels: the lowest large upward surface is the floor; the highest
    // downward one the ceiling; large vertical planes are walls.
    if let Some(f) = planes.iter().filter(|p| p.kind == "horizontal surface" && p.area_m2 > 1.0).min_by(|a, b| a.centroid[2].total_cmp(&b.centroid[2])).map(|p| p.id) {
        planes[f].kind = "floor".into();
    }
    let ceilings: Vec<usize> = planes.iter().filter(|p| p.kind == "ceiling").map(|p| p.id).collect();
    if let Some(&top) = ceilings.iter().max_by(|a, b| planes[**a].centroid[2].total_cmp(&planes[**b].centroid[2])) {
        for id in ceilings {
            if id != top {
                planes[id].kind = "downward surface".into();
            }
        }
    }
    for p in planes.iter_mut().filter(|p| p.kind == "vertical surface") {
        if p.area_m2 > 1.0 && (p.extent_v[1] - p.extent_v[0]) > 1.0 {
            p.kind = "wall".into();
        }
    }
    planes
}

/// Highest upward-facing observed surface per column (NaN where none was seen).
pub fn height_map(tsdf: &Tsdf, map: &MapGrid, below: f64) -> Vec<f32> {
    let g = &tsdf.grid;
    let mut out = vec![f32::NAN; map.dims[0] * map.dims[1]];
    for j in 0..map.dims[1] {
        for i in 0..map.dims[0] {
            let [x, y] = map.center(i, j);
            let (vi, vj) = (((x - g.origin[0]) / g.voxel).round() as i64, ((y - g.origin[1]) / g.voxel).round() as i64);
            if vi < 0 || vj < 0 || vi >= g.dims[0] as i64 || vj >= g.dims[1] as i64 {
                continue;
            }
            // Walk down the column: an upward surface is + above, − below.
            let top = (((below - g.origin[2]) / g.voxel).floor() as i64).min(g.dims[2] as i64 - 1);
            let mut k = top;
            while k > 0 {
                let a = tsdf.index(vi as usize, vj as usize, k as usize);
                let b = tsdf.index(vi as usize, vj as usize, (k - 1) as usize);
                if tsdf.weight[a] > 0.0 && tsdf.weight[b] > 0.0 && tsdf.sdf[a] > 0.0 && tsdf.sdf[b] <= 0.0 && tsdf.sdf[a] < 0.999 {
                    let t = tsdf.sdf[a] as f64 / (tsdf.sdf[a] as f64 - tsdf.sdf[b] as f64);
                    out[j * map.dims[0] + i] = (g.origin[2] + (k as f64 - t) * g.voxel) as f32;
                    break;
                }
                k -= 1;
            }
        }
    }
    out
}

/// Free (0), occupied (1) or unknown (2) per map cell for bodies between
/// `z_low` and `z_high`.
pub fn free_space(tsdf: &Tsdf, map: &MapGrid, z_low: f64, z_high: f64) -> Vec<u8> {
    let g = &tsdf.grid;
    let mut out = vec![2u8; map.dims[0] * map.dims[1]];
    let k0 = (((z_low - g.origin[2]) / g.voxel).floor().max(0.0)) as usize;
    let k1 = (((z_high - g.origin[2]) / g.voxel).ceil() as usize).min(g.dims[2] - 1);
    for j in 0..map.dims[1] {
        for i in 0..map.dims[0] {
            let [x, y] = map.center(i, j);
            let (vi, vj) = (((x - g.origin[0]) / g.voxel).round() as i64, ((y - g.origin[1]) / g.voxel).round() as i64);
            if vi < 0 || vj < 0 || vi >= g.dims[0] as i64 || vj >= g.dims[1] as i64 || k0 > k1 {
                continue;
            }
            let (mut seen, mut blocked) = (0, false);
            for k in k0..=k1 {
                let idx = tsdf.index(vi as usize, vj as usize, k);
                if tsdf.weight[idx] > 0.0 {
                    seen += 1;
                    if tsdf.sdf[idx] < 0.3 {
                        blocked = true;
                    }
                }
            }
            out[j * map.dims[0] + i] = if blocked { 1 } else if seen * 2 > (k1 - k0 + 1) { 0 } else { 2 };
        }
    }
    out
}

impl Place {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = dir.join("place.json");
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let file: PlaceFile = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if file.schema != SCHEMA {
            return Err(format!("{}: schema `{}` is not {SCHEMA}", path.display(), file.schema));
        }
        let bytes = std::fs::read(dir.join(&file.volume_file)).map_err(|e| format!("{}: {e}", file.volume_file))?;
        let tsdf = Tsdf::from_bytes(file.volume.clone(), &bytes)?;
        let hb = std::fs::read(dir.join(&file.height_file)).map_err(|e| format!("{}: {e}", file.height_file))?;
        let heights = hb.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        Ok(Self { dir: dir.to_path_buf(), file, tsdf, heights, images: std::sync::OnceLock::new() })
    }

    pub fn save(dir: &Path, file: &PlaceFile, tsdf: &Tsdf, heights: &[f32]) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        std::fs::write(dir.join(&file.volume_file), tsdf.to_bytes()).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(&file.height_file), heights.iter().flat_map(|h| h.to_le_bytes()).collect::<Vec<u8>>()).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("place.json"), serde_json::to_string_pretty(file).unwrap()).map_err(|e| e.to_string())
    }

    /// Captured photos (width, height, sRGB bytes), loaded on first use.
    pub fn images(&self) -> &Vec<(usize, usize, Vec<u8>)> {
        self.images.get_or_init(|| {
            self.file.views.iter().map(|v| crate::output::read_png(&self.dir.join(&v.image)).unwrap_or((0, 0, Vec::new()))).collect()
        })
    }

    pub fn height_at(&self, x: f64, y: f64) -> Option<f64> {
        let (i, j) = self.file.map.index_of(x, y)?;
        let h = self.heights[j * self.file.map.dims[0] + i];
        h.is_finite().then_some(h as f64)
    }

    /// Surface crossings in the vertical column at (x, y), top to bottom:
    /// (height, "up" for floors/tabletops or "down" for undersides).
    pub fn column(&self, x: f64, y: f64) -> Vec<(f64, &'static str)> {
        let g = &self.tsdf.grid;
        let mut out = Vec::new();
        let mut previous: Option<(f64, f64)> = None;
        let mut z = g.origin[2] + (g.dims[2] - 1) as f64 * g.voxel;
        while z >= g.origin[2] {
            match self.tsdf.sample([x, y, z]) {
                Some((s, w)) if w > 0.0 => {
                    if let Some((pz, ps)) = previous {
                        if (ps > 0.0) != (s > 0.0) {
                            let t = ps / (ps - s);
                            out.push((pz + (z - pz) * t, if ps > 0.0 { "up" } else { "down" }));
                        }
                    }
                    previous = Some((z, s));
                }
                _ => previous = None,
            }
            z -= 0.5 * g.voxel;
        }
        out
    }

    /// Distance to the nearest surface (within the truncation band) or `None`
    /// when the point is unobserved; a negative value means inside a surface.
    pub fn clearance(&self, p: V3) -> Option<(f64, bool)> {
        let (s, w) = self.tsdf.sample(p)?;
        (w > 0.0).then(|| (s, s >= 0.999 * self.tsdf.grid.truncation))
    }

    /// Which photos see a point, with the pixel where it appears.
    pub fn photos_of(&self, p: V3) -> Vec<(usize, f64, f64, f64)> {
        let cam = &self.file.camera;
        let mut out = Vec::new();
        for (i, v) in self.file.views.iter().enumerate() {
            let pose = v.pose.pose();
            let q = pose.to_camera(p);
            let Some((u, vv)) = cam.project(q) else { continue };
            if !cam.contains(u, vv) {
                continue;
            }
            let d = sub(p, pose.center);
            let range = norm(d);
            let visible = self.tsdf.raycast(pose.center, d, range + 0.5).is_some_and(|h| (h.distance - range).abs() < 2.5 * self.tsdf.grid.voxel);
            if visible {
                out.push((i, u, vv, range));
            }
        }
        out
    }

    /// Render the place from `pose` with `camera`: `shaded` (fused colours,
    /// lit), `photo` (blended from the nearest real photos that see each
    /// point) or `depth`. Returns sRGB bytes and the centre-pixel hit.
    pub fn render(&self, pose: &Pose, camera: &CameraModel, mode: &str) -> (Vec<u8>, Option<crate::fusion::Hit>) {
        let far = norm(sub(self.file.bounds[1], self.file.bounds[0])) + 1.0;
        let images = if mode == "photo" { Some(self.images()) } else { None };
        let views: Vec<Pose> = self.file.views.iter().map(|v| v.pose.pose()).collect();
        let light = unit([0.3, -0.4, 0.85]);
        let rows = crate::map_rows(camera.height, |y| {
            let mut row = Vec::with_capacity(3 * camera.width);
            for x in 0..camera.width {
                let dir = pose.direction_to_world(camera.ray(x as f64 + 0.5, y as f64 + 0.5));
                let Some(hit) = self.tsdf.raycast(pose.center, dir, far) else {
                    row.extend([24, 26, 30]);
                    continue;
                };
                let c = match mode {
                    "depth" => {
                        let t = (hit.distance / 6.0).clamp(0.0, 1.0);
                        [(255.0 * (1.0 - t)) as u8, (180.0 * (1.0 - (2.0 * t - 1.0).abs())) as u8, (255.0 * t) as u8]
                    }
                    "photo" => photo_color(self, images.unwrap(), &views, pose.center, hit.point).or_else(|| self.tsdf.color_at(hit.point)).unwrap_or([128; 3]),
                    _ => {
                        let base = self.tsdf.color_at(hit.point).unwrap_or([150; 3]);
                        let k = 0.55 + 0.45 * dot(hit.normal, light).abs();
                        base.map(|c| (c as f64 * k).min(255.0) as u8)
                    }
                };
                row.extend(c);
            }
            (row, if y == camera.height / 2 { Some(()) } else { None })
        });
        let rgb: Vec<u8> = rows.into_iter().flat_map(|(r, _)| r).collect();
        let center_dir = pose.direction_to_world(camera.ray(camera.cx, camera.cy));
        (rgb, self.tsdf.raycast(pose.center, center_dir, far))
    }
}

/// Image-based colour: blend up to three photos that see `p` from the
/// directions closest to the viewer's, weighted by angular proximity.
fn photo_color(place: &Place, images: &[(usize, usize, Vec<u8>)], views: &[Pose], eye: V3, p: V3) -> Option<[u8; 3]> {
    let want = unit(sub(p, eye));
    let mut order: Vec<(f64, usize)> = views.iter().enumerate().map(|(i, v)| (dot(unit(sub(p, v.center)), want), i)).collect();
    order.sort_by(|a, b| b.0.total_cmp(&a.0));
    let cam = &place.file.camera;
    let (mut sum, mut total, mut used) = ([0.0; 3], 0.0, 0);
    for (cosang, i) in order.into_iter().take(6) {
        let (w, h, img) = &images[i];
        if img.is_empty() {
            continue;
        }
        let q = views[i].to_camera(p);
        let Some((u, v)) = cam.project(q) else { continue };
        if !cam.contains(u, v) {
            continue;
        }
        let range = norm(sub(p, views[i].center));
        let seen = place.tsdf.raycast(views[i].center, sub(p, views[i].center), range + 0.3).is_some_and(|hh| (hh.distance - range).abs() < 2.5 * place.tsdf.grid.voxel);
        if !seen {
            continue;
        }
        let (x, y) = (((u * *w as f64 / cam.width as f64) as usize).min(w - 1), ((v * *h as f64 / cam.height as f64) as usize).min(h - 1));
        let k = 3 * (y * w + x);
        let weight = 1.0 / (1.0 - cosang + 1e-4);
        for c in 0..3 {
            sum[c] += weight * img[k + c] as f64;
        }
        total += weight;
        used += 1;
        if used == 3 {
            break;
        }
    }
    (total > 0.0).then(|| sum.map(|s| (s / total).round() as u8))
}

/// A* over free cells of a free-space map (4-connected plus diagonals),
/// keeping `radius` m from occupied and (unless allowed) unknown cells.
pub fn plan_path(map: &MapGrid, cells: &[u8], start: [f64; 2], goal: [f64; 2], radius: f64, allow_unknown: bool) -> Result<Vec<[f64; 2]>, String> {
    let [nx, ny] = map.dims;
    let r = (radius / map.cell).ceil() as i64;
    let blocked_raw = |i: usize, j: usize| {
        let c = cells[j * nx + i];
        c == 1 || (c == 2 && !allow_unknown)
    };
    let mut blocked = vec![false; nx * ny];
    for j in 0..ny {
        for i in 0..nx {
            if blocked_raw(i, j) {
                for dj in -r..=r {
                    for di in -r..=r {
                        if di * di + dj * dj <= r * r {
                            let (a, b) = (i as i64 + di, j as i64 + dj);
                            if a >= 0 && b >= 0 && (a as usize) < nx && (b as usize) < ny {
                                blocked[b as usize * nx + a as usize] = true;
                            }
                        }
                    }
                }
            }
        }
    }
    let s = map.index_of(start[0], start[1]).ok_or("start is outside the mapped area")?;
    let g = map.index_of(goal[0], goal[1]).ok_or("goal is outside the mapped area")?;
    // Nearest usable cell, so a caller can retry with it.
    let nearest = |c: (usize, usize)| -> String {
        let best = (0..nx * ny).filter(|k| !blocked[*k]).min_by(|a, b| {
            let d = |k: usize| ((k % nx) as f64 - c.0 as f64).hypot((k / nx) as f64 - c.1 as f64);
            d(*a).total_cmp(&d(*b))
        });
        match best {
            Some(k) => {
                let p = map.center(k % nx, k / nx);
                format!("; the nearest usable point is [{:.2}, {:.2}]", p[0], p[1])
            }
            None => "; no cell is usable at this radius".into(),
        }
    };
    if blocked[s.1 * nx + s.0] {
        return Err(format!("start is not free (occupied, unknown or within the robot radius of an obstacle){}", nearest(s)));
    }
    if blocked[g.1 * nx + g.0] {
        return Err(format!("goal is not free (occupied, unknown or within the robot radius of an obstacle){}", nearest(g)));
    }
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let idx = |i: usize, j: usize| j * nx + i;
    let mut cost = vec![f64::INFINITY; nx * ny];
    let mut from = vec![usize::MAX; nx * ny];
    let mut heap = BinaryHeap::new();
    let h = |i: usize, j: usize| ((i as f64 - g.0 as f64).hypot(j as f64 - g.1 as f64)) * map.cell;
    cost[idx(s.0, s.1)] = 0.0;
    heap.push(Reverse(((h(s.0, s.1) * 1e6) as u64, idx(s.0, s.1))));
    while let Some(Reverse((_, c))) = heap.pop() {
        if c == idx(g.0, g.1) {
            break;
        }
        let (i, j) = (c % nx, c / nx);
        for (di, dj) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
            let (a, b) = (i as i64 + di, j as i64 + dj);
            if a < 0 || b < 0 || a as usize >= nx || b as usize >= ny || blocked[idx(a as usize, b as usize)] {
                continue;
            }
            let n = idx(a as usize, b as usize);
            let step = map.cell * if di != 0 && dj != 0 { std::f64::consts::SQRT_2 } else { 1.0 };
            if cost[c] + step < cost[n] {
                cost[n] = cost[c] + step;
                from[n] = c;
                heap.push(Reverse((((cost[n] + h(a as usize, b as usize)) * 1e6) as u64, n)));
            }
        }
    }
    let end = idx(g.0, g.1);
    if !cost[end].is_finite() {
        return Err("no free path between start and goal at this robot radius".into());
    }
    let mut path = vec![end];
    while path.last() != Some(&idx(s.0, s.1)) {
        path.push(from[*path.last().unwrap()]);
    }
    path.reverse();
    // Keep only turning points.
    let pts: Vec<[f64; 2]> = path.iter().map(|c| map.center(c % nx, c / nx)).collect();
    let mut out = vec![pts[0]];
    for w in pts.windows(3) {
        let (d1, d2) = ([w[1][0] - w[0][0], w[1][1] - w[0][1]], [w[2][0] - w[1][0], w[2][1] - w[1][1]]);
        if (d1[0] * d2[1] - d1[1] * d2[0]).abs() > 1e-9 {
            out.push(w[1]);
        }
    }
    out.push(*pts.last().unwrap());
    Ok(out)
}

/// A look-at pose: camera at `eye` looking at `target`, image up ≈ +Z.
pub fn look_at(eye: V3, target: V3) -> Pose {
    let z = unit(sub(target, eye));
    let up = if z[2].abs() > 0.99 { [0.0, 1.0, 0.0] } else { [0.0, 0.0, 1.0] };
    let x = unit(cross(z, up));
    let y = cross(z, x);
    Pose { r: [[x[0], y[0], z[0]], [x[1], y[1], z[1]], [x[2], y[2], z[2]]], center: eye }
}

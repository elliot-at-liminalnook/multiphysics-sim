//! Known-pose multi-view stereo by plane sweeping.
//!
//! For a reference pixel and a candidate depth z, the (fronto-parallel)
//! window around the pixel is placed at z, projected into each neighbour
//! view and compared by normalised cross-correlation (NCC). The depth whose
//! windows agree best wins, refined to sub-step by a parabola in inverse
//! depth. A pixel needs `min_views` neighbours agreeing, so a pose error in
//! one view shows up as disagreement between views instead of a quietly
//! shifted depth.
use crate::camera::CameraModel;
use crate::{Gray, M3, Pose, V3, map_rows, matmul, mul, sub, transpose};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MvsSettings {
    /// Grid spacing of reference samples, pixels.
    pub step: usize,
    /// Window half-size, pixels (2 → 5 × 5).
    pub half_window: usize,
    /// Candidate depths, uniform in inverse depth between z_min and z_max.
    pub depths: usize,
    pub z_min: f64,
    pub z_max: f64,
    /// Accept a depth when the mean NCC of the best `min_views` views reaches this.
    pub min_score: f64,
    /// Reject flat windows: minimum standard deviation of the window (0..1 units).
    pub min_texture: f64,
    pub min_views: usize,
}

impl Default for MvsSettings {
    fn default() -> Self {
        Self { step: 2, half_window: 2, depths: 128, z_min: 0.25, z_max: 8.0, min_score: 0.85, min_texture: 0.012, min_views: 2 }
    }
}

pub struct View<'a> {
    pub image: &'a Gray,
    pub camera: &'a CameraModel,
    pub pose: Pose,
}

struct Neighbor<'a> {
    image: &'a Gray,
    camera: &'a CameraModel,
    a: M3,
    b: V3,
}

/// Matches reference pixels against neighbour views at candidate depths.
pub struct Matcher<'a> {
    reference: &'a View<'a>,
    neighbors: Vec<Neighbor<'a>>,
    settings: MvsSettings,
    inverse: Vec<f64>,
    /// −1/+1 per neighbour: score with the best view on each side (both required).
    sides: Option<Vec<i8>>,
}

#[derive(Clone, Copy, Debug)]
pub struct Match {
    pub depth: f64,
    pub score: f64,
    pub views: usize,
}

impl<'a> Matcher<'a> {
    pub fn new(reference: &'a View<'a>, neighbors: &[View<'a>], settings: MvsSettings) -> Self {
        let rt = |p: &Pose| transpose(&p.r);
        let neighbors = neighbors
            .iter()
            .map(|n| Neighbor { image: n.image, camera: n.camera, a: matmul(&rt(&n.pose), &reference.pose.r), b: mul(&rt(&n.pose), sub(reference.pose.center, n.pose.center)) })
            .collect();
        let (lo, hi) = (1.0 / settings.z_max, 1.0 / settings.z_min);
        let inverse = (0..settings.depths).map(|k| lo + (hi - lo) * k as f64 / (settings.depths - 1).max(1) as f64).collect();
        Self { reference, neighbors, settings, inverse, sides: None }
    }

    /// Score each depth by the best neighbour on each side (−1/+1 per
    /// neighbour, both sides required) instead of the best `min_views`.
    pub fn with_sides(mut self, sides: Vec<i8>) -> Self {
        assert_eq!(sides.len(), self.neighbors.len(), "one side per neighbour");
        self.sides = Some(sides);
        self
    }

    /// The reference window at integer pixel (x, y), or `None` near the border or when flat.
    fn window(&self, x: usize, y: usize) -> Option<(Vec<f32>, Vec<V3>)> {
        let hw = self.settings.half_window;
        let img = self.reference.image;
        if x < hw || y < hw || x + hw >= img.width || y + hw >= img.height {
            return None;
        }
        let mut values = Vec::with_capacity((2 * hw + 1).pow(2));
        let mut rays = Vec::with_capacity(values.capacity());
        for dy in 0..=2 * hw {
            for dx in 0..=2 * hw {
                let (px, py) = (x + dx - hw, y + dy - hw);
                values.push(img.at(px, py));
                rays.push(self.reference.camera.ray(px as f64 + 0.5, py as f64 + 0.5));
            }
        }
        let n = values.len() as f32;
        let mean = values.iter().sum::<f32>() / n;
        let var = values.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / n;
        if (var as f64).sqrt() < self.settings.min_texture {
            return None;
        }
        let sd = var.sqrt();
        Some((values.iter().map(|v| (v - mean) / sd).collect(), rays))
    }

    /// Mean NCC of the best `min_views` neighbours at each candidate depth.
    fn scores(&self, x: usize, y: usize) -> Option<Vec<(f64, usize)>> {
        let (reference, rays) = self.window(x, y)?;
        let n = reference.len();
        let moved: Vec<Vec<V3>> = self.neighbors.iter().map(|nb| rays.iter().map(|r| mul(&nb.a, *r)).collect()).collect();
        let mut out = Vec::with_capacity(self.inverse.len());
        let mut sample = vec![0f32; n];
        let mut nccs = Vec::with_capacity(self.neighbors.len());
        let mut seen = Vec::with_capacity(self.neighbors.len());
        for inv in &self.inverse {
            let z = 1.0 / inv;
            nccs.clear();
            seen.clear();
            'neighbor: for (k, (nb, moved)) in self.neighbors.iter().zip(&moved).enumerate() {
                for (i, m) in moved.iter().enumerate() {
                    let q = [nb.b[0] + z * m[0], nb.b[1] + z * m[1], nb.b[2] + z * m[2]];
                    let Some((u, v)) = nb.camera.project(q) else { continue 'neighbor };
                    let Some(s) = nb.image.sample(u, v) else { continue 'neighbor };
                    sample[i] = s;
                }
                let mean = sample.iter().sum::<f32>() / n as f32;
                let var = sample.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / n as f32;
                if var <= 1e-12 {
                    continue;
                }
                let sd = var.sqrt();
                let ncc = sample.iter().zip(&reference).map(|(s, r)| (s - mean) / sd * r).sum::<f32>() / n as f32;
                nccs.push(ncc as f64);
                seen.push(k);
            }
            if let Some(sides) = &self.sides {
                let best = |side: i8| seen.iter().zip(&nccs).filter(|(k, _)| sides[**k] == side).map(|(_, c)| *c).fold(f64::NEG_INFINITY, f64::max);
                let (l, r) = (best(-1), best(1));
                out.push(if l.is_finite() && r.is_finite() { (0.5 * (l + r), nccs.len()) } else { (-1.0, nccs.len()) });
                continue;
            }
            if nccs.len() < self.settings.min_views {
                out.push((-1.0, nccs.len()));
                continue;
            }
            nccs.sort_by(|a, b| b.total_cmp(a));
            let k = self.settings.min_views.max(1);
            out.push((nccs[..k].iter().sum::<f64>() / k as f64, nccs.len()));
        }
        Some(out)
    }

    /// The best depth for reference pixel (x, y), accepted or not.
    pub fn best(&self, x: usize, y: usize) -> Option<Match> {
        let scores = self.scores(x, y)?;
        let (k, &(score, views)) = scores.iter().enumerate().max_by(|a, b| a.1.0.total_cmp(&b.1.0))?;
        if score <= -1.0 {
            return None;
        }
        // Sub-step: parabola through the neighbouring inverse depths; its
        // peak value is the score (smooth in the poses, which refinement needs).
        let mut inv = self.inverse[k];
        let mut peak = score;
        if k > 0 && k + 1 < scores.len() && scores[k - 1].0 > -1.0 && scores[k + 1].0 > -1.0 {
            let (a, b, c) = (scores[k - 1].0, score, scores[k + 1].0);
            let den = a - 2.0 * b + c;
            if den < -1e-12 {
                let off = (0.5 * (a - c) / den).clamp(-0.5, 0.5);
                inv += off * (self.inverse[1] - self.inverse[0]);
                peak = b - 0.125 * (a - c) * (a - c) / den;
            }
        }
        Some(Match { depth: 1.0 / inv, score: peak, views })
    }
}

/// Depths on a grid over the reference image (`NaN` where rejected).
pub struct DepthMap {
    pub step: usize,
    pub columns: usize,
    pub rows: usize,
    pub depth: Vec<f32>,
    pub score: Vec<f32>,
}

impl DepthMap {
    /// Pixel centre of grid sample (i, j).
    pub fn pixel(&self, i: usize, j: usize) -> (usize, usize) {
        (i * self.step + self.step / 2, j * self.step + self.step / 2)
    }
    pub fn accepted(&self) -> usize {
        self.depth.iter().filter(|d| d.is_finite()).count()
    }
}

pub fn depth_map(reference: &View, neighbors: &[View], settings: MvsSettings) -> DepthMap {
    let matcher = Matcher::new(reference, neighbors, settings);
    let step = settings.step.max(1);
    let (columns, rows) = (reference.image.width / step, reference.image.height / step);
    let lines = map_rows(rows, |j| {
        (0..columns)
            .map(|i| {
                let (x, y) = (i * step + step / 2, j * step + step / 2);
                match matcher.best(x, y) {
                    Some(m) if m.score >= settings.min_score && m.depth >= settings.z_min && m.depth <= settings.z_max => (m.depth as f32, m.score as f32),
                    Some(m) => (f32::NAN, m.score as f32),
                    None => (f32::NAN, f32::NAN),
                }
            })
            .collect::<Vec<_>>()
    });
    let mut map = DepthMap { step, columns, rows, depth: Vec::with_capacity(columns * rows), score: Vec::with_capacity(columns * rows) };
    for line in lines {
        for (d, s) in line {
            map.depth.push(d);
            map.score.push(s);
        }
    }
    map
}

/// Keep a depth sample only when at least `min_agree` other views whose
/// viewing direction is within `max_angle_deg` see the same point at the
/// same depth (within `tolerance`, relative). Stereo outliers rarely agree
/// across views; without this they become floating surfaces when fused.
pub fn consistency_filter(maps: &[DepthMap], poses: &[Pose], camera: &CameraModel, max_angle_deg: f64, tolerance: f64, min_agree: usize) -> Vec<DepthMap> {
    let forward: Vec<V3> = poses.iter().map(|p| p.direction_to_world([0.0, 0.0, 1.0])).collect();
    let cos = max_angle_deg.to_radians().cos();
    map_rows(maps.len(), |i| {
        let near: Vec<usize> = (0..maps.len()).filter(|&j| j != i && crate::dot(forward[i], forward[j]) >= cos).collect();
        let m = &maps[i];
        let mut depth = m.depth.clone();
        for jj in 0..m.rows {
            for ii in 0..m.columns {
                let k = jj * m.columns + ii;
                let z = m.depth[k] as f64;
                if !z.is_finite() {
                    continue;
                }
                let (x, y) = m.pixel(ii, jj);
                let world = poses[i].to_world(crate::scale(camera.ray(x as f64 + 0.5, y as f64 + 0.5), z));
                let agree = near
                    .iter()
                    .filter(|&&j| {
                        let q = poses[j].to_camera(world);
                        let Some((u, v)) = camera.project(q) else { return false };
                        let other = &maps[j];
                        let (a, b) = ((u / other.step as f64) as usize, (v / other.step as f64) as usize);
                        a < other.columns && b < other.rows && {
                            let zz = other.depth[b * other.columns + a] as f64;
                            zz.is_finite() && (zz - q[2]).abs() / q[2] < tolerance
                        }
                    })
                    .count();
                if agree < min_agree {
                    depth[k] = f32::NAN;
                }
            }
        }
        DepthMap { step: m.step, columns: m.columns, rows: m.rows, depth, score: m.score.clone() }
    })
}

/// Fill small holes: an empty sample with at least `min_neighbors` of its 8
/// neighbours valid, all within `tolerance` (relative) of their median, takes
/// that median. Repeated `passes` times, so it closes holes a few samples
/// wide on smooth surfaces and never bridges depth edges.
pub fn fill_holes(map: &DepthMap, passes: usize, min_neighbors: usize, tolerance: f32) -> DepthMap {
    let mut depth = map.depth.clone();
    for _ in 0..passes {
        let before = depth.clone();
        for j in 1..map.rows.saturating_sub(1) {
            for i in 1..map.columns.saturating_sub(1) {
                if before[j * map.columns + i].is_finite() {
                    continue;
                }
                let mut near: Vec<f32> = [(-1i64, -1i64), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)]
                    .iter()
                    .map(|(di, dj)| before[(j as i64 + dj) as usize * map.columns + (i as i64 + di) as usize])
                    .filter(|d| d.is_finite())
                    .collect();
                if near.len() < min_neighbors {
                    continue;
                }
                near.sort_by(|a, b| a.total_cmp(b));
                let median = near[near.len() / 2];
                if near.iter().all(|d| (d - median).abs() <= tolerance * median) {
                    depth[j * map.columns + i] = median;
                }
            }
        }
    }
    DepthMap { step: map.step, columns: map.columns, rows: map.rows, depth, score: map.score.clone() }
}

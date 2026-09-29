//! Registering one scan station to another: 4-DoF (x, y, z, yaw) point-to-plane
//! ICP. The turntable stands level (gravity is known), so roll and pitch are
//! not searched. It starts from a rough measured guess (tape measure and
//! compass), improves it with a coarse grid search over yaw and position, then
//! runs robust Gauss–Newton ICP.
use crate::rig::Station;
use crate::{V3, dot, rotation, sub};
use serde::Serialize;
use std::collections::HashMap;

/// Points with normals, hashed on a grid for nearest-neighbour lookup.
pub struct Target {
    points: Vec<V3>,
    normals: Vec<V3>,
    cell: f64,
    grid: HashMap<(i64, i64, i64), Vec<u32>>,
}

impl Target {
    pub fn new(points: Vec<V3>, normals: Vec<V3>, cell: f64) -> Self {
        let mut grid: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::new();
        for (i, p) in points.iter().enumerate() {
            grid.entry(Self::key(*p, cell)).or_default().push(i as u32);
        }
        Self { points, normals, cell, grid }
    }
    fn key(p: V3, cell: f64) -> (i64, i64, i64) {
        ((p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64, (p[2] / cell).floor() as i64)
    }
    /// Nearest target point within `max` (≤ the grid cell), with its normal.
    pub fn nearest(&self, p: V3, max: f64) -> Option<(V3, V3, f64)> {
        let (x, y, z) = Self::key(p, self.cell);
        let mut best: Option<(u32, f64)> = None;
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if let Some(ids) = self.grid.get(&(x + dx, y + dy, z + dz)) {
                        for &i in ids {
                            let d = crate::norm(sub(self.points[i as usize], p));
                            if d <= max && best.is_none_or(|b| d < b.1) {
                                best = Some((i, d));
                            }
                        }
                    }
                }
            }
        }
        best.map(|(i, d)| (self.points[i as usize], self.normals[i as usize], d))
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Registration {
    pub station: Station,
    /// Share of source points with a target within 5 cm after alignment.
    pub overlap: f64,
    /// RMS point-to-plane distance of the inliers, m.
    pub rms_m: f64,
    pub iterations: usize,
}

fn score(target: &Target, source: &[V3], s: &Station, max: f64) -> usize {
    source.iter().filter(|p| target.nearest(s.point(**p), max).is_some()).count()
}

/// Align `source` (in its station frame) to `target`, starting from `guess`.
/// `feature` is the scene's working length (a room: 0.1 m; a small object:
/// a few mm): the coarse grid step, inlier distance and ICP schedule scale
/// with it. The guess is kept unless alignment improves the overlap.
pub fn register(target: &Target, source: &[V3], guess: Station, search_m: f64, search_deg: f64, feature: f64) -> Registration {
    let k = feature / 0.1;
    let inlier = 0.1 * k;
    // Coarse: yaw then position, on a subsample, scored by points within `inlier`.
    let sub_source: Vec<V3> = source.iter().step_by((source.len() / 1500).max(1)).copied().collect();
    let guess_score = score(target, &sub_source, &guess, inlier);
    let mut best = (guess_score, guess);
    for _round in 0..2 {
        let center = best.1;
        let yaw_steps = (search_deg / 2.0).round() as i64;
        let pos_step = 0.05 * k;
        let pos_steps = (search_m / pos_step).round() as i64;
        for a in -yaw_steps..=yaw_steps {
            let s = Station { yaw_deg: center.yaw_deg + a as f64 * 2.0, ..center };
            let v = score(target, &sub_source, &s, inlier);
            if v > best.0 {
                best = (v, s);
            }
        }
        let center = best.1;
        for i in -pos_steps..=pos_steps {
            for j in -pos_steps..=pos_steps {
                let s = Station { position: [center.position[0] + i as f64 * pos_step, center.position[1] + j as f64 * pos_step, center.position[2]], ..center };
                let v = score(target, &sub_source, &s, inlier);
                if v > best.0 {
                    best = (v, s);
                }
            }
        }
    }
    // Fine: robust point-to-plane Gauss–Newton in (x, y, z, yaw).
    let mut s = best.1;
    let mut iterations = 0;
    for (it, max) in [0.2, 0.15, 0.1, 0.08, 0.06, 0.05, 0.04, 0.03, 0.03, 0.03, 0.02, 0.02].map(|m| m * k).iter().enumerate() {
        iterations = it + 1;
        let r = rotation([0.0, 0.0, 1.0], s.yaw_deg.to_radians());
        let mut h = [[0.0f64; 4]; 4];
        let mut g = [0.0f64; 4];
        for p in source {
            let rp = crate::mul(&r, *p);
            let q = crate::add(rp, s.position);
            let Some((t, n, _)) = target.nearest(q, *max) else { continue };
            let e = dot(n, sub(q, t));
            let w = if e.abs() < 0.5 * max { 1.0 } else { 0.5 * max / e.abs() }; // Huber
            // d q / d yaw = ẑ × (R p) = (−rp.y, rp.x, 0).
            let j = [n[0], n[1], n[2], n[0] * -rp[1] + n[1] * rp[0]];
            for a in 0..4 {
                g[a] += w * j[a] * e;
                for b in 0..4 {
                    h[a][b] += w * j[a] * j[b];
                }
            }
        }
        for (a, row) in h.iter_mut().enumerate() {
            row[a] += 1e-6;
        }
        let Some(dx) = solve4(h, g.map(|x| -x)) else { break };
        s.position = [s.position[0] + dx[0], s.position[1] + dx[1], s.position[2] + dx[2]];
        s.yaw_deg += dx[3].to_degrees();
    }
    let measure = |s: &Station| {
        let (mut inliers, mut sq) = (0usize, 0.0);
        for p in source {
            if let Some((t, n, _)) = target.nearest(s.point(*p), 0.5 * inlier) {
                inliers += 1;
                sq += dot(n, sub(s.point(*p), t)).powi(2);
            }
        }
        (inliers as f64 / source.len().max(1) as f64, (sq / inliers.max(1) as f64).sqrt())
    };
    let (overlap, rms) = measure(&s);
    let (guess_overlap, guess_rms) = measure(&guess);
    // Keep the guess when alignment did not make the scans agree better.
    if guess_overlap >= overlap && guess_rms <= rms {
        return Registration { station: guess, overlap: guess_overlap, rms_m: guess_rms, iterations: 0 };
    }
    Registration { station: s, overlap, rms_m: rms, iterations }
}

fn solve4(mut a: [[f64; 4]; 4], mut b: [f64; 4]) -> Option<[f64; 4]> {
    for c in 0..4 {
        let p = (c..4).max_by(|x, y| a[*x][c].abs().total_cmp(&a[*y][c].abs()))?;
        if a[p][c].abs() < 1e-12 {
            return None;
        }
        a.swap(c, p);
        b.swap(c, p);
        for r in c + 1..4 {
            let f = a[r][c] / a[c][c];
            for k in c..4 {
                a[r][k] -= f * a[c][k];
            }
            b[r] -= f * b[c];
        }
    }
    let mut x = [0.0; 4];
    for c in (0..4).rev() {
        x[c] = (b[c] - (c + 1..4).map(|k| a[c][k] * x[k]).sum::<f64>()) / a[c][c];
    }
    Some(x)
}

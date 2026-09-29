//! Rig-constrained pose refinement: one angle correction per shot.
//!
//! The turntable allows one degree of freedom per shot, so refinement
//! searches one angle per shot instead of a free 6-DoF pose. With a
//! stereo pair, an angle error looks like a constant disparity offset,
//! which a depth change can absorb. Here each shot is matched against
//! neighbours on *both* sides at once: an angle error shifts the disparity
//! against the next shot and the previous shot in opposite directions, so no
//! single depth satisfies both and the error becomes observable.
//!
//! The absolute angle is a gauge freedom (turning the whole world with the
//! rig changes no image), so corrections are reported with zero mean.
//! Each sweep searches every shot's angle in parallel against the current
//! neighbours (coarse grid, fine grid, parabola). The optima measure the
//! error field's neighbour differences, and one least-squares solve turns
//! them into corrections for all shots together.
use crate::camera::CameraModel;
use crate::mvs::{Matcher, MvsSettings, View};
use crate::rig::Rig;
use crate::{Gray, map_rows};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefineSettings {
    /// Search half-range around the current angle on the first sweep, degrees.
    pub range_deg: f64,
    pub coarse_step_deg: f64,
    pub fine_step_deg: f64,
    pub sweeps: usize,
    /// Textured reference pixels per shot.
    pub pixels: usize,
    /// Shots within this angle of each other are neighbours, degrees.
    pub neighbor_deg: f64,
    /// Share of each sweep's update applied.
    pub damping: f64,
    /// Turn the sweep's local optima into corrections with one global
    /// least-squares solve (true), or apply each shot's own optimum (false).
    pub global_solve: bool,
    /// Prior weight pulling the total correction towards the servo reading
    /// (relative to a typical shot's information): shots and error patterns
    /// the images barely constrain keep their reported angle.
    pub regularization: f64,
    /// Keep the reported angles unless the mean match score over all shots
    /// improves by at least this much.
    pub min_gain: f64,
    pub mvs: MvsSettings,
}

impl Default for RefineSettings {
    fn default() -> Self {
        Self {
            range_deg: 0.8,
            coarse_step_deg: 0.05,
            fine_step_deg: 0.005,
            sweeps: 4,
            pixels: 1000,
            neighbor_deg: 25.0,
            damping: 0.6,
            global_solve: true,
            regularization: 0.03,
            min_gain: 0.002,
            mvs: MvsSettings { depths: 64, min_score: -1.0, ..MvsSettings::default() },
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Refined {
    /// Added to each reported angle, rad, zero mean.
    pub corrections: Vec<f64>,
    /// Mean best NCC over each shot's pixels, before and after.
    pub score_before: Vec<f64>,
    pub score_after: Vec<f64>,
    /// Largest correction change in each sweep, rad.
    pub sweep_change: Vec<f64>,
    /// Relative information of each shot in the last sweep (score curvature).
    pub weights: Vec<f64>,
    /// False when the images did not support any change (corrections are zero).
    pub accepted: bool,
}

fn wrap(a: f64) -> f64 {
    (a + std::f64::consts::PI).rem_euclid(2.0 * std::f64::consts::PI) - std::f64::consts::PI
}

/// Textured pixels spread over the image: the most textured window in each grid cell.
fn pick_pixels(image: &Gray, count: usize, half_window: usize) -> Vec<(usize, usize)> {
    let cells = (count as f64 * 1.5).sqrt().ceil() as usize;
    let (cw, ch) = ((image.width - 2 * half_window - 2) / cells.max(1), (image.height - 2 * half_window - 2) / cells.max(1));
    let mut picked = Vec::new();
    for cy in 0..cells {
        for cx in 0..cells {
            let mut best = (0.0f32, (0, 0));
            for y in (half_window + 1 + cy * ch..half_window + 1 + (cy + 1) * ch).step_by(2) {
                for x in (half_window + 1 + cx * cw..half_window + 1 + (cx + 1) * cw).step_by(2) {
                    let gx = image.at(x + 1, y) - image.at(x - 1, y);
                    let gy = image.at(x, y + 1) - image.at(x, y - 1);
                    let g = gx * gx + gy * gy;
                    if g > best.0 {
                        best = (g, (x, y));
                    }
                }
            }
            if best.0 > 0.0 {
                picked.push((best.0, best.1));
            }
        }
    }
    picked.sort_by(|a, b| b.0.total_cmp(&a.0));
    picked.into_iter().take(count).map(|p| p.1).collect()
}

/// `u` minimising Σ wᵢ((I − W)·u − d)ᵢ² + λ Σ (uᵢ + cᵢ)² with Σu = 0, where W
/// averages each shot's neighbours, w weighs each shot's measurement and c
/// is the correction so far (the prior keeps the total near the servo
/// reading). Conjugate gradients on the normal equations.
fn solve_laplacian(neighbors: &[Vec<usize>], d: &[f64], w: &[f64], lambda: f64, c: &[f64]) -> Vec<f64> {
    let n = d.len();
    let apply = |u: &[f64]| -> Vec<f64> {
        (0..n).map(|i| u[i] - if neighbors[i].is_empty() { 0.0 } else { neighbors[i].iter().map(|&j| u[j]).sum::<f64>() / neighbors[i].len() as f64 }).collect()
    };
    let apply_t = |v: &[f64]| -> Vec<f64> {
        let mut out = v.to_vec();
        for i in 0..n {
            for &j in &neighbors[i] {
                out[j] -= v[i] / neighbors[i].len() as f64;
            }
        }
        out
    };
    let op = |u: &[f64]| -> Vec<f64> {
        let mean = u.iter().sum::<f64>() / n as f64;
        let lu: Vec<f64> = apply(u).iter().zip(w).map(|(x, wi)| x * wi).collect();
        apply_t(&lu).iter().zip(u).map(|(x, ui)| x + lambda * ui + mean).collect()
    };
    let wd: Vec<f64> = d.iter().zip(w).map(|(x, wi)| x * wi).collect();
    let b: Vec<f64> = apply_t(&wd).iter().zip(c).map(|(x, ci)| x - lambda * ci).collect();
    let mut u = vec![0.0; n];
    let mut r = b.clone();
    let mut p = r.clone();
    let mut rr: f64 = r.iter().map(|x| x * x).sum();
    for _ in 0..4 * n {
        if rr < 1e-30 {
            break;
        }
        let ap = op(&p);
        let alpha = rr / p.iter().zip(&ap).map(|(a, b)| a * b).sum::<f64>();
        for i in 0..n {
            u[i] += alpha * p[i];
            r[i] -= alpha * ap[i];
        }
        let next: f64 = r.iter().map(|x| x * x).sum();
        for i in 0..n {
            p[i] = r[i] + next / rr * p[i];
        }
        rr = next;
    }
    let mean = u.iter().sum::<f64>() / n as f64;
    u.iter().map(|x| x - mean).collect()
}

/// Refine the reported disc angles (rad) of `images` taken by `rig` with `camera`.
pub fn refine_angles(images: &[Gray], reported: &[f64], camera: &CameraModel, rig: &Rig, s: RefineSettings) -> Refined {
    let n = images.len();
    assert_eq!(n, reported.len(), "one reported angle per image");
    let pixels: Vec<Vec<(usize, usize)>> = images.iter().map(|g| pick_pixels(g, s.pixels, s.mvs.half_window)).collect();
    let neighbors: Vec<Vec<usize>> = (0..n)
        .map(|i| (0..n).filter(|&j| j != i && wrap(reported[j] - reported[i]).abs().to_degrees() <= s.neighbor_deg).collect())
        .collect();
    let score = |i: usize, angle_i: f64, angles: &[f64]| -> f64 {
        let reference = View { image: &images[i], camera, pose: rig.pose(angle_i) };
        let views: Vec<View> = neighbors[i].iter().map(|&j| View { image: &images[j], camera, pose: rig.pose(angles[j]) }).collect();
        if views.is_empty() {
            return 0.0;
        }
        let sides: Vec<i8> = neighbors[i].iter().map(|&j| if wrap(reported[j] - reported[i]) < 0.0 { -1 } else { 1 }).collect();
        let matcher = Matcher::new(&reference, &views, s.mvs).with_sides(sides);
        let total: f64 = pixels[i].iter().map(|&(x, y)| matcher.best(x, y).map_or(0.0, |m| m.score.max(0.0))).sum();
        total / pixels[i].len().max(1) as f64
    };
    let mut correction = vec![0.0; n];
    let current = |c: &[f64]| -> Vec<f64> { reported.iter().zip(c).map(|(a, d)| a + d).collect() };
    let before = {
        let a = current(&correction);
        map_rows(n, |i| score(i, a[i], &a))
    };
    let mut sweep_change = Vec::new();
    let mut weights = vec![1.0; n];
    for sweep in 0..s.sweeps {
        let angles = current(&correction);
        let range = if sweep == 0 { s.range_deg } else { (s.range_deg / 4.0).max(4.0 * s.coarse_step_deg) }.to_radians();
        // Each shot's best angle, and how sharply its score peaks there
        // (curvature over one coarse step): its information.
        let measured: Vec<(f64, f64)> = map_rows(n, |i| {
            let search = |center: f64, half: f64, step: f64| -> (f64, f64) {
                let k = (half / step).round() as i64;
                (-k..=k).map(|m| center + m as f64 * step).map(|d| (d, score(i, angles[i] + d, &angles))).max_by(|a, b| a.1.total_cmp(&b.1)).unwrap()
            };
            let coarse_step = s.coarse_step_deg.to_radians();
            let (coarse, _) = search(0.0, range, coarse_step);
            let fine = s.fine_step_deg.to_radians();
            let (d, f0) = search(coarse, coarse_step, fine);
            let (fm, fp) = (score(i, angles[i] + d - fine, &angles), score(i, angles[i] + d + fine, &angles));
            let den = fm - 2.0 * f0 + fp;
            let off = if den < -1e-12 { (0.5 * (fm - fp) / den).clamp(-0.5, 0.5) * fine } else { 0.0 };
            let (cm, cp) = (score(i, angles[i] + d - coarse_step, &angles), score(i, angles[i] + d + coarse_step, &angles));
            let curvature = (2.0 * f0 - cm - cp).max(0.0);
            (d + off, curvature)
        });
        let best: Vec<f64> = measured.iter().map(|m| m.0).collect();
        let mut sorted: Vec<f64> = measured.iter().map(|m| m.1).filter(|c| *c > 0.0).collect();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let typical = sorted.get(sorted.len() / 2).copied().unwrap_or(1.0).max(1e-12);
        weights = measured.iter().map(|m| m.1 / typical).collect();
        // Each local optimum measures the shot's error against its
        // neighbours' average: best_i ≈ −(e_i − mean_{j∈N(i)} e_j), i.e.
        // L·e = −best with L = I − W. Solve for the whole error field at
        // once (weighted by information, with the servo reading as prior).
        let update = if s.global_solve { solve_laplacian(&neighbors, &best, &weights, s.regularization, &correction) } else { best.clone() };
        let mut change: f64 = 0.0;
        for i in 0..n {
            let step = s.damping * update[i];
            correction[i] += step;
            change = change.max(step.abs());
        }
        let mean = correction.iter().sum::<f64>() / n as f64;
        correction.iter_mut().for_each(|c| *c -= mean);
        sweep_change.push(change);
    }
    let after = {
        let a = current(&correction);
        map_rows(n, |i| score(i, a[i], &a))
    };
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
    let accepted = mean(&after) - mean(&before) >= s.min_gain;
    if !accepted {
        correction.iter_mut().for_each(|c| *c = 0.0);
    }
    Refined { corrections: correction, score_before: before, score_after: after, sweep_change, weights, accepted }
}

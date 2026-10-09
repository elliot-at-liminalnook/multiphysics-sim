//! GT2 belt geometry (RoboCAD's `belt_derivation.py`, its geometry half):
//! pitch and tip radii, a printable toothed pulley outline, and the belt
//! path around circles (tangent spans, wraps, pitch length).
use std::f64::consts::{PI, TAU};

/// GT2 nominal values (mm): tooth pitch and the pitch line below the tips (Gates).
pub const PITCH: f64 = 2.0;
pub const PLD: f64 = 0.254;

/// Pitch radius (mm): circumference = teeth × pitch.
pub fn pitch_radius(teeth: f64) -> f64 {
    teeth * PITCH / (2.0 * PI)
}

pub fn tip_radius(teeth: f64) -> f64 {
    pitch_radius(teeth) - PLD
}

/// Closed outline (mm, centred at the origin) of a GT2 pulley: rounded
/// grooves `depth` deep and `groove` wide at the tip circle, an
/// approximation of the 2GT profile that prints well at 0.4 mm. Print a fit
/// coupon before trusting it with load.
pub fn toothed_outline(teeth: usize, depth: f64, groove: f64, samples: usize) -> Vec<[f64; 2]> {
    let ro = tip_radius(teeth as f64);
    let half = (groove / 2.0) / ro;
    let mut pts = Vec::with_capacity(teeth * samples);
    for k in 0..teeth {
        let c = TAU * k as f64 / teeth as f64;
        for i in 0..samples {
            let s = -1.0 + 2.0 * i as f64 / (samples - 1).max(1) as f64;
            let r = ro - depth * (1.0 - s * s).max(0.0).sqrt();
            let a = c + s * half;
            pts.push([r * a.cos(), r * a.sin()]);
        }
    }
    pts
}

/// One straight span of a belt path.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub from: usize,
    pub to: usize,
    pub start: [f64; 2],
    pub end: [f64; 2],
    pub direction: [f64; 2],
    pub length: f64,
}

/// A belt around circles in travel order (counter-clockwise from +Z).
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub spans: Vec<Span>,
    /// (pulley, wrap angle rad, arc mm).
    pub wraps: Vec<(usize, f64, f64)>,
    pub length: f64,
}

fn rot(v: [f64; 2], a: f64) -> [f64; 2] {
    let (s, c) = a.sin_cos();
    [c * v[0] - s * v[1], s * v[0] + c * v[1]]
}

/// The belt path around `(center, signed pitch radius)` circles: positive
/// inside the loop (the belt turns left around it), negative for a
/// back-side idler outside it.
pub fn belt_path(pulleys: &[([f64; 2], f64)]) -> Result<Path, String> {
    let n = pulleys.len();
    if n < 2 {
        return Err("a belt needs at least two pulleys".into());
    }
    let mut spans = Vec::with_capacity(n);
    for i in 0..n {
        let ((ca, ra), (cb, rb)) = (pulleys[i], pulleys[(i + 1) % n]);
        let (dx, dy) = (cb[0] - ca[0], cb[1] - ca[1]);
        let d = dx.hypot(dy);
        let s = rb - ra;
        if d <= s.abs() + 1e-9 {
            return Err(format!("pulleys {i} and {} overlap: no belt span between them", (i + 1) % n));
        }
        let length = (d * d - s * s).sqrt();
        let u = rot([dx / d, dy / d], -s.atan2(length));
        let nrm = [-u[1], u[0]];
        let p = [ca[0] - ra * nrm[0], ca[1] - ra * nrm[1]];
        let q = [cb[0] - rb * nrm[0], cb[1] - rb * nrm[1]];
        spans.push(Span { from: i, to: (i + 1) % n, start: p, end: q, direction: u, length });
    }
    let mut wraps = Vec::with_capacity(n);
    for i in 0..n {
        let (u_in, u_out) = (spans[(i + n - 1) % n].direction, spans[i].direction);
        let turn = (u_in[0] * u_out[1] - u_in[1] * u_out[0]).atan2(u_in[0] * u_out[0] + u_in[1] * u_out[1]);
        let r = pulleys[i].1;
        let wrap = (if r > 0.0 { turn } else { -turn }).rem_euclid(TAU);
        wraps.push((i, wrap, r.abs() * wrap));
    }
    let length = spans.iter().map(|s| s.length).sum::<f64>() + wraps.iter().map(|w| w.2).sum::<f64>();
    Ok(Path { spans, wraps, length })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_equal_pulleys_make_a_stadium() {
        let r = pitch_radius(20.0);
        let p = belt_path(&[([0.0, 0.0], r), ([100.0, 0.0], r)]).unwrap();
        assert!((p.length - (200.0 + TAU * r)).abs() < 1e-9);
        assert!((p.wraps[0].1 - PI).abs() < 1e-9);
        assert_eq!(toothed_outline(20, 0.75, 1.3, 7).len(), 140);
    }
}

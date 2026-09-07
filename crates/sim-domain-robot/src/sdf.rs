//! Signed-distance sampling on the grid the CAD tool exported, broad-phase
//! boxes, and the choice of contact vertices per link.

use crate::model::{Collision, Link, Sdf, V3};
use nalgebra::Vector3;

/// Strict interior of a joint's contact-exclusion sphere. Boundary samples stay
/// eligible for collision. World transforms and subtraction can move a sample
/// on an invariant radius a few ULPs either side of that radius; do not turn that
/// arithmetic noise into a discontinuous contact force. The guard scales with
/// coordinate roundoff, not a fixed physical clearance or a derivative step.
pub(crate) fn inside_exclusion_band(point: Vector3<f64>, center: Vector3<f64>, radius: f64) -> bool {
    let roundoff = 32.0 * f64::EPSILON * (point.amax() + center.amax() + radius);
    radius - (point - center).norm() > roundoff
}

#[cfg(test)]
mod band_tests {
    use super::*;
    use nalgebra::Rotation3;

    #[test]
    fn rotating_boundary_samples_remain_eligible_for_collision() {
        // The contact-enabled CAD fixture has +/-6 mm samples at a radius of
        // exactly 10 mm. A raw norm<radius predicate flickered while rotating.
        for scale in [1e-3, 1.0, 1e3] {
            let center = Vector3::new(0.0, 0.0, 0.2)*scale;
            for sign in [-1.0, 1.0] {
                let local = Vector3::new(-0.008, sign*0.006, 0.0)*scale;
                for i in 0..4096 {
                    let rotation = Rotation3::from_axis_angle(&Vector3::y_axis(),i as f64*0.002);
                    let point = center + rotation*local;
                    assert!(!inside_exclusion_band(point,center,0.01*scale));
                    assert!(inside_exclusion_band(center+(point-center)*(1.0-1e-9),center,0.01*scale));
                    assert!(!inside_exclusion_band(center+(point-center)*(1.0+1e-9),center,0.01*scale));
                }
            }
        }
    }
}

impl Sdf {
    pub fn is_valid(&self) -> bool {
        self.dims.iter().all(|&d| d >= 2) && self.values.len() >= self.dims[0] * self.dims[1] * self.dims[2] && self.cell > 0.0
    }
    fn at(&self, ix: usize, iy: usize, iz: usize) -> f64 {
        self.values[(ix * self.dims[1] + iy) * self.dims[2] + iz]
    }
    /// Signed distance (negative inside) and its gradient at a point in the
    /// grid's frame. Outside the grid the distance grows with the distance
    /// to the grid box, so far points never register as contact.
    pub fn sample(&self, p: Vector3<f64>) -> (f64, Vector3<f64>) {
        let n = [self.dims[0], self.dims[1], self.dims[2]];
        let f = [(p.x - self.origin[0]) / self.cell, (p.y - self.origin[1]) / self.cell, (p.z - self.origin[2]) / self.cell];
        let mut outside = 0.0;
        let mut c = [0.0; 3];
        for k in 0..3 {
            let max = (n[k] - 1) as f64;
            let clamped = f[k].clamp(0.0, max - 1e-9);
            let d = if f[k] < 0.0 { -f[k] } else if f[k] > max { f[k] - max } else { 0.0 };
            outside += d * d;
            c[k] = clamped;
        }
        let i = [c[0].floor() as usize, c[1].floor() as usize, c[2].floor() as usize];
        let t = [c[0] - i[0] as f64, c[1] - i[1] as f64, c[2] - i[2] as f64];
        let v = |dx: usize, dy: usize, dz: usize| self.at(i[0] + dx, i[1] + dy, i[2] + dz);
        let (v000, v100, v010, v110) = (v(0, 0, 0), v(1, 0, 0), v(0, 1, 0), v(1, 1, 0));
        let (v001, v101, v011, v111) = (v(0, 0, 1), v(1, 0, 1), v(0, 1, 1), v(1, 1, 1));
        let lerp = |a: f64, b: f64, s: f64| a + (b - a) * s;
        let c00 = lerp(v000, v100, t[0]);
        let c10 = lerp(v010, v110, t[0]);
        let c01 = lerp(v001, v101, t[0]);
        let c11 = lerp(v011, v111, t[0]);
        let c0 = lerp(c00, c10, t[1]);
        let c1 = lerp(c01, c11, t[1]);
        let value = lerp(c0, c1, t[2]);
        // Gradient of the trilinear interpolant.
        let gx = (lerp(lerp(v100 - v000, v110 - v010, t[1]), lerp(v101 - v001, v111 - v011, t[1]), t[2])) / self.cell;
        let gy = (lerp(lerp(v010 - v000, v110 - v100, t[0]), lerp(v011 - v001, v111 - v101, t[0]), t[2])) / self.cell;
        let gz = (lerp(lerp(v001 - v000, v101 - v100, t[0]), lerp(v011 - v010, v111 - v110, t[0]), t[1])) / self.cell;
        let mut g = Vector3::new(gx, gy, gz);
        if g.norm() < 1e-12 {
            g = Vector3::z();
        }
        (value + outside.sqrt() * self.cell, g.normalize())
    }
}

/// Axis-aligned box of a link's collision geometry in its own frame.
pub fn local_bounds(link: &Link) -> (Vector3<f64>, Vector3<f64>) {
    let pts: Vec<&V3> = if !link.collision.vertices.is_empty() { link.collision.vertices.iter().collect() } else { link.bbox.iter().collect() };
    if pts.is_empty() {
        return (Vector3::repeat(-1e-3), Vector3::repeat(1e-3));
    }
    let mut lo = Vector3::repeat(f64::INFINITY);
    let mut hi = Vector3::repeat(f64::NEG_INFINITY);
    for p in pts {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo, hi)
}

/// The vertices a link touches the world with: hull vertices first, then
/// farthest-point samples of the surface mesh, up to `max`.
pub fn contact_vertices(c: &Collision, max: usize) -> Vec<Vector3<f64>> {
    if max == 0 { return Vec::new(); }
    let all: Vec<Vector3<f64>> = c.hull.iter().chain(&c.vertices)
        .map(|v| Vector3::new(v[0], v[1], v[2])).collect();
    let mut chosen: Vec<Vector3<f64>> = Vec::new();
    // A mesh's hull order is not a contact priority. Preserve support extrema
    // before sampling: truncating the first hull vertices can omit a thin foot
    // entirely and let it pass through the floor. Z first covers gravity even
    // for deliberately tiny budgets; normal budgets retain all six extrema.
    for axis in [2, 0, 1] {
        for maximum in [false, true] {
            if chosen.len() >= max { break; }
            let extremum = all.iter().min_by(|a, b| {
                if maximum { b[axis].total_cmp(&a[axis]) } else { a[axis].total_cmp(&b[axis]) }
            });
            if let Some(p) = extremum {
                if !chosen.iter().any(|q| (q-p).norm() < 1e-9) { chosen.push(*p); }
            }
        }
    }
    // Farthest-point sampling over the remaining vertices.
    let mut dist: Vec<f64> = all.iter().map(|p| chosen.iter().map(|c| (p - c).norm()).fold(f64::INFINITY, f64::min)).collect();
    while chosen.len() < max && !all.is_empty() {
        let (k, d) = dist.iter().enumerate().fold((0, f64::NEG_INFINITY), |acc, (i, &d)| if d > acc.1 { (i, d) } else { acc });
        if d < 1e-9 {
            break;
        }
        let p = all[k];
        chosen.push(p);
        for (i, q) in all.iter().enumerate() {
            dist[i] = dist[i].min((q - p).norm());
        }
    }
    chosen
}

/// A tiny deterministic generator for sensor noise and Monte Carlo draws.
#[derive(Clone, Debug)]
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E3779B97F4A7C15) ^ 0xD1B54A32D192ED03)
    }
    pub fn next_u64(&mut self) -> u64 {
        // splitmix64
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn normal(&mut self) -> f64 {
        let u1 = self.uniform().max(1e-300);
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}

/// Gaussian draw keyed by (stream, sample index): the same sample always
/// gets the same noise, so a residual re-evaluated in a Newton iteration
/// sees one value.
pub fn keyed_normal(stream: u64, index: u64) -> f64 {
    Rng::new(stream.wrapping_mul(6364136223846793005).wrapping_add(index.wrapping_mul(1442695040888963407))).normal()
}

#[cfg(test)]
mod tests {

    #[test]
    fn contact_budget_retains_thin_foot_and_all_support_extrema() {
        let mut c = super::Collision::default();
        c.hull = (0..100).map(|i| {
            let a = i as f64 * std::f64::consts::TAU / 100.;
            [a.cos(), a.sin(), 0.]
        }).collect();
        c.hull.push([0., 0., -10.]);
        c.vertices = c.hull.clone();
        let contacts = super::contact_vertices(&c, 24);
        assert_eq!(contacts.len(), 24);
        for axis in 0..3 {
            let lo = c.hull.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min);
            let hi = c.hull.iter().map(|p| p[axis]).fold(f64::NEG_INFINITY, f64::max);
            assert!(contacts.iter().any(|p| p[axis] == lo));
            assert!(contacts.iter().any(|p| p[axis] == hi));
        }
        assert!(super::contact_vertices(&c, 0).is_empty());
    }
    use super::*;

    /// A grid of the signed distance to a box [-a, a]³.
    pub fn box_sdf(a: f64, cell: f64, pad: usize) -> Sdf {
        let n = ((2.0 * a / cell).round() as usize) + 1 + 2 * pad;
        let origin = -a - pad as f64 * cell;
        let mut values = Vec::with_capacity(n * n * n);
        for ix in 0..n {
            for iy in 0..n {
                for iz in 0..n {
                    let p = [origin + ix as f64 * cell, origin + iy as f64 * cell, origin + iz as f64 * cell];
                    let q = [p[0].abs() - a, p[1].abs() - a, p[2].abs() - a];
                    let outside = (q[0].max(0.0).powi(2) + q[1].max(0.0).powi(2) + q[2].max(0.0).powi(2)).sqrt();
                    let inside = q[0].max(q[1]).max(q[2]).min(0.0);
                    values.push(outside + inside);
                }
            }
        }
        Sdf { origin: [origin; 3], cell, dims: [n; 3], values }
    }

    #[test]
    fn box_distance_and_gradient() {
        let s = box_sdf(0.05, 0.005, 2);
        let (d, g) = s.sample(Vector3::new(0.0, 0.0, 0.0));
        assert!((d + 0.05).abs() < 1e-9, "centre {d}");
        let (d, g2) = s.sample(Vector3::new(0.0, 0.0, 0.04));
        assert!((d + 0.01).abs() < 1e-9, "inside {d}");
        assert!(g2.z > 0.99, "gradient points out: {g2}");
        let (d, _) = s.sample(Vector3::new(0.0, 0.0, 0.07));
        assert!((d - 0.02).abs() < 1e-9, "outside {d}");
        let (d, _) = s.sample(Vector3::new(0.0, 0.0, 1.0));
        assert!(d > 0.5, "far {d}");
        let _ = g;
    }
}

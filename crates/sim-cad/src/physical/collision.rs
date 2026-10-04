//! A link's collision surface in its frame (metres, origin at the link's
//! centre of mass): the members' exact-geometry tessellations joined and
//! decimated by vertex clustering (RoboCAD's `_decimate`: each cluster keeps
//! the real surface vertex nearest its mean, so corners and edges survive as
//! contact samples), and an approximate convex hull. RoboCAD's signed
//! distance grid (link-to-link contact) is not built: `sdf` is null and the
//! export says so.
use crate::geometry::BodyGeometry;
use serde_json::{Value, json};
use std::collections::HashMap;

/// RoboCAD's `_MAX_COLLISION_VERTICES`.
pub const MAX_VERTICES: usize = 3000;
/// How the hull is found (directions sampled on the sphere).
pub const HULL_DIRECTIONS: usize = 162;
pub const HULL_RULE: &str = "approximate convex hull: the surface vertices extreme along 162 directions spread over the sphere (a subset of the exact hull's vertices)";

/// The collision block of `members` (their geometry) about `com_m`.
pub fn block(members: &[&BodyGeometry], com_m: [f64; 3]) -> Value {
    let mut verts: Vec<[f64; 3]> = Vec::new();
    let mut tris: Vec<[usize; 3]> = Vec::new();
    for g in members {
        let off = verts.len();
        verts.extend(g.vertices_mm.iter().map(|p| [p[0] * 1e-3 - com_m[0], p[1] * 1e-3 - com_m[1], p[2] * 1e-3 - com_m[2]]));
        tris.extend(g.triangles.iter().map(|t| [off + t[0] as usize, off + t[1] as usize, off + t[2] as usize]));
    }
    if verts.is_empty() {
        return json!({"vertices": [], "triangles": [], "hull": [], "sdf": null});
    }
    let (dv, dt) = decimate(&verts, &tris, MAX_VERTICES);
    let hull = hull(&verts);
    let round = |p: &[f64; 3]| p.map(|c| (c * 1e6).round() / 1e6);
    json!({
        "vertices": dv.iter().map(round).collect::<Vec<_>>(),
        "triangles": dt,
        "hull": hull.iter().map(round).collect::<Vec<_>>(),
        "sdf": null,
        "sign_derivation": {"algorithm": "none: no signed distance grid", "hull": HULL_RULE, "surface": "exact B-rep tessellation (the CAD display mesh), vertex-clustered to at most 3000 vertices"},
    })
}

/// Vertex clustering to at most `target` vertices (see the module doc):
/// the cell grows by 1.25× until the clusters fit.
pub fn decimate(verts: &[[f64; 3]], tris: &[[usize; 3]], target: usize) -> (Vec<[f64; 3]>, Vec<[usize; 3]>) {
    if verts.len() <= target {
        return (verts.to_vec(), tris.to_vec());
    }
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in verts {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let extent: Vec<f64> = (0..3).map(|k| (hi[k] - lo[k]).max(1e-9)).collect();
    let mut cell = (extent.iter().product::<f64>() / target as f64).cbrt() * 0.8;
    let mut inverse: Vec<usize> = Vec::new();
    let mut count = 0;
    // RoboCAD stops after 12 growths; a flat part (one extent ~0) starts
    // far too fine, so growth continues until the cap holds.
    for _ in 0..200 {
        let mut ids: HashMap<[i64; 3], usize> = HashMap::new();
        inverse = verts.iter().map(|p| {
            let key = [0, 1, 2].map(|k| ((p[k] - lo[k]) / cell).floor() as i64);
            let n = ids.len();
            *ids.entry(key).or_insert(n)
        }).collect();
        count = ids.len();
        if count <= target {
            break;
        }
        cell *= 1.25;
    }
    let mut sums = vec![[0.0; 3]; count];
    let mut counts = vec![0.0; count];
    for (p, &c) in verts.iter().zip(&inverse) {
        for k in 0..3 {
            sums[c][k] += p[k];
        }
        counts[c] += 1.0;
    }
    let mut rep = vec![usize::MAX; count];
    let mut best = vec![f64::INFINITY; count];
    for (i, (p, &c)) in verts.iter().zip(&inverse).enumerate() {
        let m = sums[c].map(|s| s / counts[c]);
        let d = ((p[0] - m[0]).powi(2) + (p[1] - m[1]).powi(2) + (p[2] - m[2]).powi(2)).sqrt();
        if d < best[c] {
            best[c] = d;
            rep[c] = i;
        }
    }
    let new_verts = rep.iter().map(|&i| verts[i]).collect();
    let new_tris = tris.iter().map(|t| t.map(|i| inverse[i])).filter(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2]).collect();
    (new_verts, new_tris)
}

/// The vertices extreme along [`HULL_DIRECTIONS`] directions (a Fibonacci
/// sphere), deduplicated, in first-found order.
pub fn hull(verts: &[[f64; 3]]) -> Vec<[f64; 3]> {
    let golden = std::f64::consts::PI * (3.0 - 5f64.sqrt());
    let mut chosen: Vec<usize> = Vec::new();
    for k in 0..HULL_DIRECTIONS {
        let y = 1.0 - 2.0 * (k as f64 + 0.5) / HULL_DIRECTIONS as f64;
        let r = (1.0 - y * y).sqrt();
        let d = [r * (golden * k as f64).cos(), y, r * (golden * k as f64).sin()];
        let best = verts.iter().enumerate().max_by(|a, b| {
            let da = a.1[0] * d[0] + a.1[1] * d[1] + a.1[2] * d[2];
            let db = b.1[0] * d[0] + b.1[1] * d[1] + b.1[2] * d[2];
            da.total_cmp(&db)
        });
        if let Some((i, _)) = best
            && !chosen.contains(&i)
        {
            chosen.push(i);
        }
    }
    chosen.into_iter().map(|i| verts[i]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimation_caps_vertices_keeps_real_surface_points_and_drops_degenerate_triangles() {
        // A 40 × 40 grid of points on z = 0 with two triangles per cell.
        let n = 40;
        let verts: Vec<[f64; 3]> = (0..n * n).map(|i| [(i % n) as f64 * 0.001, (i / n) as f64 * 0.001, 0.0]).collect();
        let mut tris = Vec::new();
        for y in 0..n - 1 {
            for x in 0..n - 1 {
                let a = y * n + x;
                tris.push([a, a + 1, a + n]);
                tris.push([a + 1, a + n + 1, a + n]);
            }
        }
        let (dv, dt) = decimate(&verts, &tris, 400);
        assert!(dv.len() <= 400 && dv.len() > 100, "{}", dv.len());
        assert!(dv.iter().all(|p| verts.contains(p)), "representatives are real vertices");
        assert!(dt.iter().all(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2] && t.iter().all(|&i| i < dv.len())));
    }

    #[test]
    fn the_hull_holds_a_cubes_corners() {
        let mut verts = Vec::new();
        for x in [-1.0, 1.0] {
            for y in [-1.0, 1.0] {
                for z in [-1.0, 1.0] {
                    verts.push([x, y, z]);
                }
            }
        }
        verts.push([0.0, 0.0, 0.0]);
        let h = hull(&verts);
        assert_eq!(h.len(), 8);
        assert!(!h.contains(&[0.0, 0.0, 0.0]));
    }
}

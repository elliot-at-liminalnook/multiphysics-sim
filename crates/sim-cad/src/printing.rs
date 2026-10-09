//! Print checks on one body (RoboCAD's `printing.py`): thin walls by rays
//! cast inward from sampled face points, and validation for export (a valid
//! B-rep, a closed tessellation, a positive volume). Reads only: nothing
//! here changes the document.
use crate::archive::ArchiveDocument;
use crate::geometry::{BodyGeometry, resolved_brep, tessellate_node};
use crate::kernel::{Measure, measure};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

/// A thin spot: where the wall behind a face point is thinner than asked.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ThinRegion {
    pub point: [f64; 3],
    pub thickness: f64,
    pub face: i64,
}

/// One validation issue (RoboCAD's `ValidationIssue`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Issue {
    /// error | warning.
    pub severity: String,
    pub message: String,
    pub location: Option<[f64; 3]>,
    pub fix: Option<String>,
}

/// A body's validation report (RoboCAD's `ValidationReport`, with the
/// tessellation's open edges its desktop also checked).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Validation {
    pub valid: bool,
    pub watertight: bool,
    pub issues: Vec<Issue>,
    pub summary: String,
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn triangle(mesh: &BodyGeometry, t: usize) -> [[f64; 3]; 3] {
    mesh.triangles[t].map(|i| mesh.vertices_mm[i as usize])
}

/// Thin regions of body `id` thinner than `threshold` mm (RoboCAD's
/// `wall_thickness`): the largest `samples_per_face` triangles of each face
/// of a 0.2 mm tessellation, a ray cast inward from each centroid, and the
/// first wall it meets (hits on the same face within the tessellation's
/// own error skipped).
pub fn wall_thickness(doc: &ArchiveDocument, id: &str, threshold: f64, samples_per_face: usize, cancelled: &dyn Fn() -> bool) -> Result<Vec<ThinRegion>, String> {
    if !(threshold.is_finite() && threshold > 0.0) {
        return Err("the threshold must be a positive length (mm)".into());
    }
    let brep = resolved_brep(doc, id)?;
    let mesh = tessellate_node(doc, id, 0.2, cancelled)?;
    let mut by_face: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (t, f) in mesh.triangle_faces.iter().enumerate() {
        by_face.entry(*f).or_default().push(t);
    }
    let area = |t: usize| {
        let [a, b, c] = triangle(&mesh, t);
        0.5 * norm(cross(sub(b, a), sub(c, a)))
    };
    let mut thin = Vec::new();
    for (face, mut tris) in by_face {
        if cancelled() {
            return Err("cancelled".into());
        }
        tris.sort_by(|a, b| area(*b).total_cmp(&area(*a)));
        for t in tris.into_iter().take(samples_per_face) {
            let [a, b, c] = triangle(&mesh, t);
            let centroid = [0, 1, 2].map(|k| (a[k] + b[k] + c[k]) / 3.0);
            let n = cross(sub(b, a), sub(c, a));
            let l = norm(n);
            if l == 0.0 {
                continue;
            }
            let n = n.map(|x| x / l);
            let origin = [0, 1, 2].map(|k| centroid[k] - 1e-3 * n[k]);
            let raw = measure(Measure::RayHits, &[&brep], &[origin[0], origin[1], origin[2], -n[0], -n[1], -n[2]], &[])?;
            let mut hits: Vec<(f64, i64)> = raw.chunks_exact(5).map(|h| (h[0], h[4] as i64)).filter(|(w, f)| !(*f == face as i64 && *w < 0.25)).collect();
            hits.sort_by(|x, y| x.0.total_cmp(&y.0));
            if let Some((d, _)) = hits.first()
                && 0.02 < *d
                && *d < threshold
            {
                thin.push(ThinRegion { point: centroid, thickness: *d, face: face as i64 });
            }
        }
    }
    Ok(thin)
}

/// Open edges of a tessellation once coincident vertices are merged
/// (RoboCAD's `weld` and `mesh_open_edges`): edges used by one triangle.
pub fn open_edges(mesh: &BodyGeometry, tolerance: f64) -> usize {
    let q = 1.0 / tolerance;
    let mut ids: HashMap<[i64; 3], usize> = HashMap::new();
    let welded: Vec<usize> = mesh
        .vertices_mm
        .iter()
        .map(|v| {
            let key = v.map(|x| (x * q).round() as i64);
            let next = ids.len();
            *ids.entry(key).or_insert(next)
        })
        .collect();
    let mut uses: HashMap<(usize, usize), usize> = HashMap::new();
    for t in &mesh.triangles {
        let v = t.map(|i| welded[i as usize]);
        if v[0] == v[1] || v[1] == v[2] || v[0] == v[2] {
            continue;
        }
        for (a, b) in [(v[0], v[1]), (v[1], v[2]), (v[2], v[0])] {
            *uses.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    uses.values().filter(|n| **n == 1).count()
}

/// Validate body `id` for export (RoboCAD's `validate` and
/// `validate_for_export`): the kernel's B-rep check; for a solid, a
/// positive volume and a closed 0.05 mm tessellation.
pub fn validate(doc: &ArchiveDocument, id: &str, cancelled: &dyn Fn() -> bool) -> Result<Validation, String> {
    let node = doc.node(id).ok_or_else(|| format!("no node {id}"))?;
    let solid = node["body_kind"].as_str().unwrap_or("solid") == "solid";
    let brep = resolved_brep(doc, id)?;
    let mut issues = Vec::new();
    let valid = measure(Measure::Valid, &[&brep], &[], &[])?.first().copied() == Some(1.0);
    if !valid {
        issues.push(Issue { severity: "error".into(), message: "the B-rep is invalid (a face or edge fails the kernel's checks)".into(), location: None, fix: Some("run Heal, or undo the last operation".into()) });
    }
    let mut watertight = true;
    if solid {
        let mesh = tessellate_node(doc, id, 0.05, cancelled)?;
        if mesh.properties.volume_mm3 <= 0.0 {
            watertight = false;
            issues.push(Issue { severity: "error".into(), message: "zero or negative volume: the solid is inside out".into(), location: None, fix: Some("Reverse the body".into()) });
        }
        let open = open_edges(&mesh, 1e-5);
        if open > 0 {
            watertight = false;
            issues.push(Issue { severity: "error".into(), message: format!("tessellation has {open} open edge(s): the mesh will not be watertight"), location: None, fix: Some("increase the tolerance or heal the body".into()) });
        }
    }
    let summary = if valid && watertight { "valid, watertight".to_string() } else { format!("{} issue(s)", issues.len()) };
    Ok(Validation { valid, watertight, issues, summary })
}

/// `validate_for_export`'s verdict and messages over named reports.
pub fn export_messages(reports: &[(String, Validation)]) -> (bool, Vec<String>) {
    let mut ok = true;
    let mut messages = Vec::new();
    for (name, rep) in reports {
        if !(rep.valid && rep.watertight) {
            ok = false;
            for issue in &rep.issues {
                let at = issue.location.map_or_else(String::new, |p| format!(" near ({:.1}, {:.1}, {:.1})", p[0], p[1], p[2]));
                let fix = issue.fix.as_ref().map_or_else(String::new, |f| format!(" — {f}"));
                messages.push(format!("{name}: {}{at}{fix}", issue.message));
            }
        }
    }
    (ok, messages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_cube_has_no_open_edges_and_an_open_one_does() {
        // A unit cube as six faces of two triangles each, vertices duplicated per face.
        let c = |x: f64, y: f64, z: f64| [x, y, z];
        let faces = [
            [c(0., 0., 0.), c(1., 0., 0.), c(1., 1., 0.), c(0., 1., 0.)],
            [c(0., 0., 1.), c(0., 1., 1.), c(1., 1., 1.), c(1., 0., 1.)],
            [c(0., 0., 0.), c(0., 0., 1.), c(1., 0., 1.), c(1., 0., 0.)],
            [c(0., 1., 0.), c(1., 1., 0.), c(1., 1., 1.), c(0., 1., 1.)],
            [c(0., 0., 0.), c(0., 1., 0.), c(0., 1., 1.), c(0., 0., 1.)],
            [c(1., 0., 0.), c(1., 0., 1.), c(1., 1., 1.), c(1., 1., 0.)],
        ];
        let mut mesh = BodyGeometry::default();
        for f in faces {
            let base = mesh.vertices_mm.len() as u32;
            mesh.vertices_mm.extend(f);
            mesh.triangles.push([base, base + 1, base + 2]);
            mesh.triangles.push([base, base + 2, base + 3]);
        }
        assert_eq!(open_edges(&mesh, 1e-5), 0);
        mesh.triangles.truncate(10);
        assert_eq!(open_edges(&mesh, 1e-5), 4);
    }
}

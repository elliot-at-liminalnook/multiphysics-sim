//! Print studies from CAD (RoboCAD's `print_study.py`): parts written as
//! binary STL with the regions where they are held and loaded
//! (`sim.print-study/1`, read by `sim_print::study`). Regions are in the
//! part's CAD frame (mm), best built from geometry: faces of a body, the
//! faces touching another body (a load path through a contact), or plain
//! spheres, boxes and cylinders.
use super::{K, V3, norm, slug, sub};
use crate::archive::ArchiveDocument;
use crate::geometry::{BodyGeometry, resolved_brep, tessellate_node};
use crate::printing::open_edges;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

/// A welded copy of a tessellation: coincident vertices merged, so the
/// mesh is closed for the voxeliser (RoboCAD's `weld`).
pub fn weld(mesh: &BodyGeometry, tolerance: f64) -> BodyGeometry {
    let q = 1.0 / tolerance;
    let mut ids: std::collections::HashMap<[i64; 3], u32> = std::collections::HashMap::new();
    let mut vertices = Vec::new();
    let map: Vec<u32> = mesh
        .vertices_mm
        .iter()
        .map(|v| {
            let key = v.map(|x| (x * q).round() as i64);
            *ids.entry(key).or_insert_with(|| {
                vertices.push(*v);
                (vertices.len() - 1) as u32
            })
        })
        .collect();
    let mut triangles = Vec::new();
    let mut faces = Vec::new();
    for (t, f) in mesh.triangles.iter().zip(mesh.triangle_faces.iter().chain(std::iter::repeat(&0))) {
        let w = t.map(|i| map[i as usize]);
        if w[0] != w[1] && w[1] != w[2] && w[0] != w[2] {
            triangles.push(w);
            faces.push(*f);
        }
    }
    BodyGeometry { node_id: mesh.node_id.clone(), vertices_mm: vertices, normals: Vec::new(), triangles, triangle_faces: faces, properties: mesh.properties.clone(), solids: Vec::new() }
}

/// A node's welded mesh (RoboCAD's `body_mesh`).
pub fn body_mesh(doc: &ArchiveDocument, id: &str, cancelled: &dyn Fn() -> bool) -> Result<BodyGeometry, String> {
    Ok(weld(&tessellate_node(doc, id, 0.1, cancelled)?, 1e-5))
}

/// A body that is not a node (a split piece), welded.
pub fn body_mesh_of(k: &K, body: &[u8]) -> Result<BodyGeometry, String> {
    Ok(weld(&k.geometry(body, 0.1)?, 1e-5))
}

/// Binary STL in millimetres.
pub fn write_stl(mesh: &BodyGeometry, path: &Path) -> Result<(), String> {
    let mut out = Vec::with_capacity(84 + 50 * mesh.triangles.len());
    let mut header = b"robocad print study".to_vec();
    header.resize(80, b' ');
    out.extend_from_slice(&header);
    out.extend_from_slice(&(mesh.triangles.len() as u32).to_le_bytes());
    for t in &mesh.triangles {
        let [a, b, c] = t.map(|i| mesh.vertices_mm[i as usize]);
        let n = super::cross(sub(b, a), sub(c, a));
        let l = norm(n);
        let n = if l > 0.0 { n.map(|x| x / l) } else { [0.0, 0.0, 0.0] };
        for v in [n, a, b, c] {
            for x in v {
                out.extend_from_slice(&(x as f32).to_le_bytes());
            }
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    let mut f = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(&out).map_err(|e| format!("{}: {e}", path.display()))
}

// ---- regions -------------------------------------------------------------

pub fn sphere(center: V3, radius: f64) -> Value {
    json!({"sphere": {"center": center, "radius": radius}})
}
pub fn cuboid(lo: V3, hi: V3) -> Value {
    json!({"box": {"min": lo, "max": hi}})
}
pub fn below(axis: V3, height: f64) -> Value {
    json!({"below": {"axis": axis, "height": height}})
}

/// Triangle centroids plus vertices of the selected faces, thinned to about `spacing` mm.
fn sample_points(mesh: &BodyGeometry, faces: Option<&BTreeSet<i64>>, spacing: f64) -> Vec<V3> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (t, tri) in mesh.triangles.iter().enumerate() {
        if let Some(faces) = faces
            && !mesh.triangle_faces.get(t).is_some_and(|f| faces.contains(&(*f as i64)))
        {
            continue;
        }
        let [a, b, c] = tri.map(|i| mesh.vertices_mm[i as usize]);
        let centroid = [0, 1, 2].map(|k| (a[k] + b[k] + c[k]) / 3.0);
        for p in [a, b, c, centroid] {
            let key = p.map(|x| (x / spacing).floor() as i64);
            if seen.insert(key) {
                out.push(p);
            }
        }
    }
    out
}

/// The surface of these faces of a body (RoboCAD's `faces_region`).
pub fn faces_region(doc: &ArchiveDocument, id: &str, faces: &[i64], radius: f64, cancelled: &dyn Fn() -> bool) -> Result<Value, String> {
    let mesh = tessellate_node(doc, id, 0.1, cancelled)?;
    let set: BTreeSet<i64> = faces.iter().copied().collect();
    let pts = sample_points(&mesh, Some(&set), (radius * 0.8).max(0.3));
    if pts.is_empty() {
        return Err(format!("{}: faces {faces:?} have no tessellation", name_of(doc, id)));
    }
    Ok(json!({"points": {"points": pts, "radius": radius}}))
}

/// The surface of one body that touches (within `gap` mm) another: where a
/// load passes between them (RoboCAD's `contact_region`).
pub fn contact_region(doc: &ArchiveDocument, id: &str, other: &str, gap: f64, radius: f64, cancelled: &dyn Fn() -> bool) -> Result<Value, String> {
    let k = K { cancelled };
    let mesh = tessellate_node(doc, id, 0.1, cancelled)?;
    let other_brep = resolved_brep(doc, other)?;
    let (lo, hi) = k.bounds(&other_brep)?;
    let mut pts = Vec::new();
    for p in sample_points(&mesh, None, (radius * 0.8).max(0.3)) {
        if (0..3).all(|i| lo[i] - gap <= p[i] && p[i] <= hi[i] + gap) && k.contains(&other_brep, p, gap)? {
            pts.push(p);
        }
    }
    if pts.is_empty() {
        return Err(format!("{} does not touch {} within {gap} mm", name_of(doc, id), name_of(doc, other)));
    }
    Ok(json!({"points": {"points": pts, "radius": radius}}))
}

/// Everything within `depth` mm of the body's lowest point along `up` (RoboCAD's `bottom_region`).
pub fn bottom_region(doc: &ArchiveDocument, id: &str, up: V3, depth: f64, cancelled: &dyn Fn() -> bool) -> Result<Value, String> {
    let mesh = tessellate_node(doc, id, doc.node(id).and_then(|n| n["tessellation_tolerance"].as_f64()).unwrap_or(0.05), cancelled)?;
    let u = super::unit(up);
    let h = mesh.vertices_mm.iter().map(|v| super::dot(*v, u)).fold(f64::INFINITY, f64::min);
    Ok(below(u, h + depth))
}

fn name_of(doc: &ArchiveDocument, id: &str) -> String {
    doc.node(id).and_then(|n| n["name"].as_str()).unwrap_or(id).to_string()
}

/// A study region from a REST/UI description (RoboCAD's `region_from`):
/// plain regions pass through; `contact` (another node), `faces` (face
/// indices) and `bottom` are found from the geometry.
pub fn region_from(doc: &ArchiveDocument, id: &str, spec: &Value, cancelled: &dyn Fn() -> bool) -> Result<Value, String> {
    let m = spec.as_object().filter(|m| !m.is_empty()).ok_or_else(|| format!("region must be an object, got {spec}"))?;
    let f = |k: &str, d: f64| m.get(k).and_then(Value::as_f64).unwrap_or(d);
    if let Some(other) = m.get("contact") {
        return contact_region(doc, id, other.as_str().ok_or("region contact: give the other node's id")?, f("gap", 0.3), 1.0, cancelled);
    }
    if let Some(faces) = m.get("faces") {
        let faces: Vec<i64> = faces.as_array().ok_or("region faces: give face indices")?.iter().filter_map(Value::as_i64).collect();
        return faces_region(doc, id, &faces, f("radius", 1.0), cancelled);
    }
    if let Some(b) = m.get("bottom") {
        let up = b.as_array().filter(|a| a.len() == 3).map(|a| [0, 1, 2].map(|i| a[i].as_f64().unwrap_or(0.0))).unwrap_or([0.0, 0.0, 1.0]);
        return bottom_region(doc, id, up, f("depth", 0.3), cancelled);
    }
    const KNOWN: [&str; 5] = ["sphere", "box", "cylinder", "points", "below"];
    if m.len() != 1 || !KNOWN.contains(&m.keys().next().map(String::as_str).unwrap_or("")) {
        return Err("region: use one of below, bottom, box, contact, cylinder, faces, points, sphere".into());
    }
    Ok(spec.clone())
}

// ---- the study -------------------------------------------------------------

/// One part of a study (RoboCAD's `PartSpec`).
#[derive(Clone, Debug)]
pub struct PartSpec {
    pub node: String,
    pub name: Option<String>,
    pub build_direction: V3,
    pub settings: Value,
    /// `[{name, region}]`.
    pub fixtures: Vec<Value>,
    /// `[{name, region, direction, magnitude, …}]`.
    pub loads: Vec<Value>,
    pub acceleration: Option<V3>,
    pub sections: Vec<Value>,
    pub seams: Vec<Value>,
    pub directions: Option<Vec<V3>>,
    /// Instead of the node's own body (a split piece).
    pub mesh: Option<BodyGeometry>,
}

/// RoboCAD's default print settings for a part.
pub fn default_settings() -> Value {
    json!({"walls": 3, "infill": 0.15, "pattern": "gyroid", "layer_height": 0.2, "top_bottom_layers": 5})
}

impl PartSpec {
    pub fn new(node: &str) -> PartSpec {
        PartSpec { node: node.into(), name: None, build_direction: [0.0, 0.0, 1.0], settings: default_settings(), fixtures: Vec::new(), loads: Vec::new(), acceleration: Some([0.0, 0.0, -9.81]), sections: Vec::new(), seams: Vec::new(), directions: None, mesh: None }
    }
}

/// The study's options (RoboCAD's `write_study` keywords).
#[derive(Clone, Debug)]
pub struct StudyOptions {
    pub printer: String,
    pub material: String,
    pub simulation: Option<Value>,
    pub safety_target: f64,
    pub voxels: u64,
    pub voxel_mm: Option<f64>,
    pub plan: Option<Value>,
    pub provenance: Value,
}

impl Default for StudyOptions {
    fn default() -> Self {
        StudyOptions { printer: "bambu-h2c".into(), material: "pla-basic".into(), simulation: None, safety_target: 2.0, voxels: 40_000, voxel_mm: None, plan: None, provenance: json!({}) }
    }
}

fn v3(v: &Value) -> Option<V3> {
    let a = v.as_array().filter(|a| a.len() == 3)?;
    Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?])
}

/// The parts of a REST body (`/print/analyze`'s `parts`; RoboCAD's
/// `PrintJobs._parts`): regions resolved against the geometry.
pub fn parts_of(doc: &ArchiveDocument, body: &Value, cancelled: &dyn Fn() -> bool) -> Result<Vec<PartSpec>, String> {
    let mut parts = Vec::new();
    for (i, p) in body["parts"].as_array().map_or(&[][..], Vec::as_slice).iter().enumerate() {
        let node = p["node"].as_str().unwrap_or_default();
        if !doc.node(node).is_some_and(|n| matches!(n["kind"].as_str(), Some("body" | "instance"))) {
            return Err(format!("parts[{i}].node {:?} is not a body", p["node"]));
        }
        let fixtures = p["fixtures"].as_array().map_or(&[][..], Vec::as_slice).iter().enumerate().map(|(k, f)| {
            let region = f.get("region").ok_or_else(|| format!("parts[{i}]: missing 'region'"))?;
            Ok(json!({"name": f.get("name").cloned().unwrap_or_else(|| json!(format!("fixture {}", k + 1))), "region": region_from(doc, node, region, cancelled)?}))
        }).collect::<Result<Vec<Value>, String>>()?;
        let loads = p["loads"].as_array().map_or(&[][..], Vec::as_slice).iter().enumerate().map(|(k, l)| {
            let missing = |key: &str| format!("parts[{i}]: missing '{key}'");
            let region = l.get("region").ok_or_else(|| missing("region"))?;
            let direction = l.get("direction").ok_or_else(|| missing("direction"))?;
            let magnitude = l.get("magnitude").ok_or_else(|| missing("magnitude"))?;
            let mut out = Map::new();
            out.insert("name".into(), l.get("name").cloned().unwrap_or_else(|| json!(format!("load {}", k + 1))));
            out.insert("region".into(), region_from(doc, node, region, cancelled)?);
            out.insert("direction".into(), direction.clone());
            out.insert("magnitude".into(), magnitude.clone());
            Ok(Value::Object(out))
        }).collect::<Result<Vec<Value>, String>>()?;
        let mut spec = PartSpec::new(node);
        spec.name = p["name"].as_str().map(str::to_string);
        spec.build_direction = v3(&p["build_direction"]).unwrap_or([0.0, 0.0, 1.0]);
        if p["settings"].is_object() {
            spec.settings = p["settings"].clone();
        }
        spec.fixtures = fixtures;
        spec.loads = loads;
        spec.acceleration = Some(v3(&p["acceleration"]).unwrap_or([0.0, 0.0, -9.81]));
        spec.sections = p["sections"].as_array().cloned().unwrap_or_default();
        spec.seams = p["seams"].as_array().cloned().unwrap_or_default();
        spec.directions = p["directions"].as_array().map(|a| a.iter().filter_map(v3).collect());
        parts.push(spec);
    }
    if parts.is_empty() {
        return Err("give `parts`: [{node, fixtures, loads, …}]".into());
    }
    Ok(parts)
}

/// Write the STLs and `study.json` into `out_dir` (paths in it relative to
/// it); the study's path (RoboCAD's `write_study`).
pub fn write_study(doc: &ArchiveDocument, parts: &[PartSpec], out_dir: &Path, o: &StudyOptions, cancelled: &dyn Fn() -> bool) -> Result<std::path::PathBuf, String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let mut entries = Vec::new();
    for p in parts {
        let name = p.name.clone().unwrap_or_else(|| name_of(doc, &p.node));
        let stl = format!("{}.stl", slug(&name));
        let mesh = match &p.mesh {
            Some(m) => m.clone(),
            None => body_mesh(doc, &p.node, cancelled)?,
        };
        if open_edges(&mesh, 1e-5) > 0 {
            return Err(format!("{name}: its tessellation is not closed, so it cannot be voxelised; heal the body first"));
        }
        write_stl(&mesh, &out_dir.join(&stl))?;
        let mut e = json!({"name": name, "mesh": stl, "build_direction": p.build_direction, "settings": p.settings, "fixtures": p.fixtures, "loads": p.loads, "sections": p.sections, "seams": p.seams});
        if let Some(a) = p.acceleration {
            e["acceleration"] = json!(a);
        }
        if let Some(d) = p.directions.as_ref().filter(|d| !d.is_empty()) {
            e["directions"] = json!(d);
        }
        entries.push(e);
    }
    let mut study = json!({"schema": "sim.print-study/1", "printer": o.printer, "material": o.material, "voxels": o.voxels, "safety_target": o.safety_target, "parts": entries, "provenance": o.provenance});
    if let Some(v) = o.voxel_mm {
        study["voxel_mm"] = json!(v);
    }
    if let Some(sim) = &o.simulation {
        let mut sim = sim.clone();
        // The simulation's system path, relative to the study's folder.
        if let Some(system) = sim["system"].as_str() {
            let absolute = std::fs::canonicalize(system).unwrap_or_else(|_| Path::new(system).to_path_buf());
            sim["system"] = json!(relative_path(&absolute, out_dir));
        }
        study["simulation"] = sim;
    }
    if let Some(plan) = &o.plan {
        study["plan"] = plan.clone();
    }
    let path = out_dir.join("study.json");
    std::fs::write(&path, serde_json::to_string_pretty(&study).map_err(|e| e.to_string())? + "\n").map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// `path` relative to `base` (Python's `os.path.relpath`): `..` steps up to
/// their common ancestor.
pub fn relative_path(path: &Path, base: &Path) -> String {
    let base = std::fs::canonicalize(base).unwrap_or_else(|_| base.to_path_buf());
    let p: Vec<_> = path.components().collect();
    let b: Vec<_> = base.components().collect();
    let common = p.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut out: Vec<String> = std::iter::repeat_n("..".to_string(), b.len() - common).collect();
    out.extend(p[common..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned()));
    if out.is_empty() { ".".into() } else { out.join("/") }
}

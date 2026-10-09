//! Reference meshes (RoboCAD's mesh nodes, `io/importers.py`
//! `load_mesh_file` and `import_mesh`): STL, OBJ, PLY and 3MF read into a
//! triangle mesh in millimetres, the unit prompt's guess, and the archive's
//! `mesh/<id>.npz` entry (NumPy's `savez`: `vertices` and `normals` float32
//! N×3, `triangles` int32 M×3, `triangle_face` int32 M), read and written
//! byte-compatibly so RoboCAD opens what this writes.
//!
//! A mesh node is a reference: it is drawn and can be measured against,
//! but it has no exact solid, so it carries no mass and no physics.
use std::io::{Cursor, Read, Write};
use std::path::Path;

/// The extensions read as meshes.
pub const EXTENSIONS: [&str; 4] = ["stl", "obj", "ply", "3mf"];
/// Mesh formats RoboCAD read through trimesh that this editor refuses by name.
pub const REFUSED: [&str; 3] = ["fbx", "glb", "gltf"];
/// The unit prompt's choices (RoboCAD's `LENGTH_UNITS` the prompt offers): name → mm.
pub const UNITS: [(&str, f64); 5] = [("mm", 1.0), ("cm", 10.0), ("m", 1000.0), ("in", 25.4), ("ft", 304.8)];

/// A triangle mesh as RoboCAD keeps it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TriMesh {
    pub vertices: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub triangles: Vec<[i32; 3]>,
    /// The face each triangle belongs to (an imported mesh: its own index).
    pub triangle_face: Vec<i32>,
}

impl TriMesh {
    /// Area-weighted vertex normals (unit; zero where no triangle touches).
    fn compute_normals(&mut self) {
        let mut sums = vec![[0f64; 3]; self.vertices.len()];
        for t in &self.triangles {
            let [a, b, c] = t.map(|i| self.vertices[i as usize].map(f64::from));
            let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
            let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            for i in t {
                for k in 0..3 {
                    sums[*i as usize][k] += n[k];
                }
            }
        }
        self.normals = sums
            .into_iter()
            .map(|n| {
                let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                if l > 0.0 { n.map(|x| (x / l) as f32) } else { [0.0; 3] }
            })
            .collect();
    }

    /// The largest extent of the bounding box, in the mesh's own units.
    pub fn extent(&self) -> f64 {
        if self.vertices.is_empty() {
            return 0.0;
        }
        (0..3)
            .map(|k| {
                let (lo, hi) = self.vertices.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v[k] as f64), hi.max(v[k] as f64)));
                hi - lo
            })
            .fold(0.0, f64::max)
    }

    fn check(&self, what: &str) -> Result<(), String> {
        if self.triangles.is_empty() {
            return Err(format!("{what}: the mesh has no triangles"));
        }
        let n = self.vertices.len() as i32;
        if let Some(t) = self.triangles.iter().find(|t| t.iter().any(|i| *i < 0 || *i >= n)) {
            return Err(format!("{what}: a triangle {t:?} names a vertex outside 0..{n}"));
        }
        if self.vertices.iter().flatten().any(|x| !x.is_finite()) {
            return Err(format!("{what}: a vertex is not finite"));
        }
        Ok(())
    }
}

/// RoboCAD's guess for the unit prompt from the largest raw extent.
pub fn units_guess(extent: f64) -> &'static str {
    if extent < 1.0 {
        "m"
    } else if extent < 20.0 {
        "in"
    } else {
        "mm"
    }
}

/// Millimetres per `unit` (one of [`UNITS`]).
pub fn unit_scale(unit: &str) -> Result<f64, String> {
    UNITS.iter().find(|(u, _)| *u == unit).map(|(_, s)| *s).ok_or_else(|| format!("unit `{unit}` is not one of {}", UNITS.map(|(u, _)| u).join(", ")))
}

/// The mesh at `path` in its own units, and its largest extent there.
pub fn read_raw(path: &Path) -> Result<(TriMesh, f64), String> {
    let shown = path.display();
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if REFUSED.contains(&ext.as_str()) {
        return Err(format!("{shown}: .{ext} meshes are not read by the in-process editor (STL, OBJ, PLY and 3MF are); convert it to one of those"));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{shown}: {e}"))?;
    let mesh = match ext.as_str() {
        "stl" => stl(&bytes),
        "obj" => obj(&String::from_utf8_lossy(&bytes)),
        "ply" => ply(&bytes),
        "3mf" => three_mf(&bytes),
        other => Err(format!("not a mesh file (.{other}; meshes are {})", EXTENSIONS.join(", "))),
    }
    .map_err(|e| format!("{shown}: {e}"))?;
    mesh.check(&shown.to_string())?;
    let extent = mesh.extent();
    Ok((mesh, extent))
}

/// The mesh at `path` in millimetres (`unit`: the file's unit), with vertex
/// normals, each triangle its own face (RoboCAD's `load_mesh_file`).
pub fn read(path: &Path, unit: &str) -> Result<TriMesh, String> {
    let scale = unit_scale(unit)? as f32;
    let (mut mesh, _) = read_raw(path)?;
    for v in &mut mesh.vertices {
        *v = v.map(|x| x * scale);
    }
    mesh.compute_normals();
    mesh.triangle_face = (0..mesh.triangles.len() as i32).collect();
    Ok(mesh)
}

fn stl(bytes: &[u8]) -> Result<TriMesh, String> {
    // Binary: an 80-byte header, a triangle count, 50 bytes per triangle.
    if bytes.len() >= 84 {
        let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        if bytes.len() == 84 + 50 * n {
            let mut mesh = TriMesh::default();
            for k in 0..n {
                let at = 84 + 50 * k + 12;
                let f = |i: usize| f32::from_le_bytes(bytes[at + 4 * i..at + 4 * i + 4].try_into().expect("4 bytes"));
                let base = mesh.vertices.len() as i32;
                for v in 0..3 {
                    mesh.vertices.push([f(3 * v), f(3 * v + 1), f(3 * v + 2)]);
                }
                mesh.triangles.push([base, base + 1, base + 2]);
            }
            return Ok(mesh);
        }
    }
    // ASCII: `vertex x y z` lines, three per facet.
    let text = String::from_utf8_lossy(bytes);
    if !text.trim_start().starts_with("solid") {
        return Err("neither a binary STL (its size does not match its triangle count) nor an ASCII one".into());
    }
    let mut mesh = TriMesh::default();
    let mut facet = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("vertex") => {
                let v: Vec<f32> = words.map(|w| w.parse::<f32>().map_err(|_| format!("line {}: `{w}` is not a number", n + 1))).collect::<Result<_, _>>()?;
                if v.len() != 3 {
                    return Err(format!("line {}: a vertex has {} coordinates", n + 1, v.len()));
                }
                facet.push([v[0], v[1], v[2]]);
            }
            Some("endfacet") => {
                if facet.len() != 3 {
                    return Err(format!("line {}: a facet has {} vertices (STL facets are triangles)", n + 1, facet.len()));
                }
                let base = mesh.vertices.len() as i32;
                mesh.vertices.append(&mut facet);
                mesh.triangles.push([base, base + 1, base + 2]);
            }
            _ => {}
        }
    }
    Ok(mesh)
}

fn obj(text: &str) -> Result<TriMesh, String> {
    let mut mesh = TriMesh::default();
    for (n, line) in text.lines().enumerate() {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("v") => {
                let v: Vec<f32> = words.take(3).map(|w| w.parse::<f32>().map_err(|_| format!("line {}: `{w}` is not a number", n + 1))).collect::<Result<_, _>>()?;
                if v.len() != 3 {
                    return Err(format!("line {}: a vertex needs x y z", n + 1));
                }
                mesh.vertices.push([v[0], v[1], v[2]]);
            }
            Some("f") => {
                let count = mesh.vertices.len() as i64;
                let corners: Vec<i32> = words
                    .map(|w| {
                        let i: i64 = w.split('/').next().unwrap_or("").parse().map_err(|_| format!("line {}: `{w}` is not a vertex index", n + 1))?;
                        // 1-based; negative counts back from the last vertex so far.
                        let i = if i < 0 { count + i } else { i - 1 };
                        if i < 0 || i >= count {
                            return Err(format!("line {}: vertex index {w} is out of range", n + 1));
                        }
                        Ok(i as i32)
                    })
                    .collect::<Result<_, String>>()?;
                if corners.len() < 3 {
                    return Err(format!("line {}: a face needs three vertices", n + 1));
                }
                for k in 1..corners.len() - 1 {
                    mesh.triangles.push([corners[0], corners[k], corners[k + 1]]);
                }
            }
            _ => {}
        }
    }
    Ok(mesh)
}

/// The scalar types a PLY property may have, with their sizes.
fn ply_size(kind: &str) -> Result<usize, String> {
    Ok(match kind {
        "char" | "uchar" | "int8" | "uint8" => 1,
        "short" | "ushort" | "int16" | "uint16" => 2,
        "int" | "uint" | "float" | "int32" | "uint32" | "float32" => 4,
        "double" | "float64" => 8,
        other => return Err(format!("unknown PLY property type `{other}`")),
    })
}

fn ply_value(kind: &str, b: &[u8]) -> f64 {
    match kind {
        "char" | "int8" => b[0] as i8 as f64,
        "uchar" | "uint8" => b[0] as f64,
        "short" | "int16" => i16::from_le_bytes([b[0], b[1]]) as f64,
        "ushort" | "uint16" => u16::from_le_bytes([b[0], b[1]]) as f64,
        "int" | "int32" => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
        "uint" | "uint32" => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
        "float" | "float32" => f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
        _ => f64::from_le_bytes(b[..8].try_into().expect("8 bytes")),
    }
}

/// One PLY element's properties: (name, scalar type, list count type).
struct PlyElement {
    name: String,
    count: usize,
    properties: Vec<(String, String, Option<String>)>,
}

fn ply(bytes: &[u8]) -> Result<TriMesh, String> {
    let end = bytes.windows(10).position(|w| w == b"end_header").ok_or("no end_header")?;
    let header = String::from_utf8_lossy(&bytes[..end]);
    // Exactly one line ending follows end_header (binary data may itself start with 0x0A).
    let after = &bytes[end + 10..];
    let body = after.strip_prefix(b"\r\n").or_else(|| after.strip_prefix(b"\n")).unwrap_or(after);
    let mut format = String::new();
    let mut elements: Vec<PlyElement> = Vec::new();
    for line in header.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["format", f, ..] => format = f.to_string(),
            ["element", name, count] => elements.push(PlyElement { name: name.to_string(), count: count.parse().map_err(|_| format!("element {name}: bad count"))?, properties: Vec::new() }),
            ["property", "list", count, item, name] => elements.last_mut().ok_or("a property before any element")?.properties.push((name.to_string(), item.to_string(), Some(count.to_string()))),
            ["property", kind, name] => elements.last_mut().ok_or("a property before any element")?.properties.push((name.to_string(), kind.to_string(), None)),
            _ => {}
        }
    }
    let mut mesh = TriMesh::default();
    let ascii = match format.as_str() {
        "ascii" => true,
        "binary_little_endian" => false,
        other => return Err(format!("PLY format `{other}` is not read (ascii and binary_little_endian are)")),
    };
    let text = if ascii { String::from_utf8_lossy(body).into_owned() } else { String::new() };
    let mut words = text.split_whitespace();
    let mut at = 0usize;
    for element in &elements {
        for _ in 0..element.count {
            let mut xyz = [0f32; 3];
            for (name, kind, list) in &element.properties {
                let mut read = |kind: &str| -> Result<f64, String> {
                    if ascii {
                        words.next().ok_or("the data ends early")?.parse::<f64>().map_err(|e| e.to_string())
                    } else {
                        let size = ply_size(kind)?;
                        let slice = body.get(at..at + size).ok_or("the data ends early")?;
                        at += size;
                        Ok(ply_value(kind, slice))
                    }
                };
                match list {
                    Some(count_kind) => {
                        let count = read(count_kind)? as usize;
                        let mut items = Vec::with_capacity(count);
                        for _ in 0..count {
                            items.push(read(kind)? as i32);
                        }
                        if element.name == "face" && (name == "vertex_indices" || name == "vertex_index") {
                            for k in 1..items.len().saturating_sub(1) {
                                mesh.triangles.push([items[0], items[k], items[k + 1]]);
                            }
                        }
                    }
                    None => {
                        let value = read(kind)?;
                        if element.name == "vertex" {
                            match name.as_str() {
                                "x" => xyz[0] = value as f32,
                                "y" => xyz[1] = value as f32,
                                "z" => xyz[2] = value as f32,
                                _ => {}
                            }
                        }
                    }
                }
            }
            if element.name == "vertex" {
                mesh.vertices.push(xyz);
            }
        }
    }
    Ok(mesh)
}

fn three_mf(bytes: &[u8]) -> Result<TriMesh, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("not a 3MF (zip) archive: {e}"))?;
    let name = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_owned()))
        .find(|n| n.to_lowercase().ends_with(".model"))
        .ok_or("the 3MF holds no .model part")?;
    let mut xml = String::new();
    zip.by_name(&name).map_err(|e| format!("{name}: {e}"))?.read_to_string(&mut xml).map_err(|e| format!("{name}: {e}"))?;
    let doc = roxmltree::Document::parse(&xml).map_err(|e| format!("{name}: {e}"))?;
    let mut mesh = TriMesh::default();
    // Every object's mesh, in document order (build items and their
    // transforms are not applied: the objects as modelled).
    for m in doc.descendants().filter(|n| n.has_tag_name("mesh")) {
        let base = mesh.vertices.len() as i32;
        let number = |n: &roxmltree::Node, a: &str| -> Result<f64, String> { n.attribute(a).ok_or_else(|| format!("{name}: a <{}> has no {a}", n.tag_name().name()))?.parse::<f64>().map_err(|e| format!("{name}: {a}: {e}")) };
        for v in m.descendants().filter(|n| n.has_tag_name("vertex")) {
            mesh.vertices.push([number(&v, "x")? as f32, number(&v, "y")? as f32, number(&v, "z")? as f32]);
        }
        for t in m.descendants().filter(|n| n.has_tag_name("triangle")) {
            mesh.triangles.push([base + number(&t, "v1")? as i32, base + number(&t, "v2")? as i32, base + number(&t, "v3")? as i32]);
        }
    }
    Ok(mesh)
}

// ---- NumPy .npy / .npz ------------------------------------------------------

/// One `.npy` array: little-endian float32 or int32, C order.
fn npy(descr: &str, shape: &[usize], data: &[u8]) -> Vec<u8> {
    let shape_text = match shape {
        [n] => format!("({n},)"),
        _ => format!("({})", shape.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", ")),
    };
    let mut header = format!("{{'descr': '{descr}', 'fortran_order': False, 'shape': {shape_text}, }}");
    // Pad with spaces so the data starts on a 64-byte boundary, ending in '\n'.
    let total = 10 + header.len() + 1;
    header.push_str(&" ".repeat((64 - total % 64) % 64));
    header.push('\n');
    let mut out = Vec::with_capacity(10 + header.len() + data.len());
    out.extend_from_slice(b"\x93NUMPY\x01\x00");
    out.extend_from_slice(&(header.len() as u16).to_le_bytes());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(data);
    out
}

/// A parsed `.npy`: its type, shape and data.
fn parse_npy(bytes: &[u8]) -> Result<(String, Vec<usize>, &[u8]), String> {
    if bytes.len() < 10 || &bytes[..6] != b"\x93NUMPY" {
        return Err("not a .npy array".into());
    }
    let (len, start) = match bytes[6] {
        1 => (u16::from_le_bytes([bytes[8], bytes[9]]) as usize, 10),
        2 | 3 => (u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize, 12),
        v => return Err(format!(".npy version {v} is not read")),
    };
    let header = std::str::from_utf8(bytes.get(start..start + len).ok_or("a .npy header runs past the end")?).map_err(|e| e.to_string())?;
    let field = |key: &str| -> Option<&str> {
        let at = header.find(&format!("'{key}':"))? + key.len() + 3;
        Some(header[at..].trim_start())
    };
    let descr = field("descr").and_then(|v| v.strip_prefix('\'')).and_then(|v| v.split('\'').next()).ok_or("a .npy header has no descr")?.to_string();
    if field("fortran_order").is_some_and(|v| v.starts_with("True")) {
        return Err("a Fortran-order .npy is not read".into());
    }
    let shape_text = field("shape").and_then(|v| v.strip_prefix('(')).and_then(|v| v.split(')').next()).ok_or("a .npy header has no shape")?;
    let shape = shape_text.split(',').map(str::trim).filter(|s| !s.is_empty()).map(|s| s.parse::<usize>().map_err(|e| e.to_string())).collect::<Result<Vec<_>, _>>()?;
    Ok((descr, shape, &bytes[start + len..]))
}

/// Values of a parsed `.npy` as f64 (float32/float64/int32/int64, little-endian).
fn values(descr: &str, data: &[u8], count: usize) -> Result<Vec<f64>, String> {
    let size = match descr {
        "<f4" | "<i4" => 4,
        "<f8" | "<i8" => 8,
        other => return Err(format!("a .npy of type {other} is not read")),
    };
    if data.len() < count * size {
        return Err("a .npy array is shorter than its shape".into());
    }
    Ok(data
        .chunks_exact(size)
        .take(count)
        .map(|b| match descr {
            "<f4" => f32::from_le_bytes(b.try_into().expect("4")) as f64,
            "<i4" => i32::from_le_bytes(b.try_into().expect("4")) as f64,
            "<f8" => f64::from_le_bytes(b.try_into().expect("8")),
            _ => i64::from_le_bytes(b.try_into().expect("8")) as f64,
        })
        .collect())
}

/// The `mesh/<id>.npz` bytes of `mesh` (as NumPy's `savez` writes them: stored, not compressed).
pub fn to_npz(mesh: &TriMesh) -> Result<Vec<u8>, String> {
    let floats = |rows: &[[f32; 3]]| rows.iter().flatten().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
    let ints = |rows: &mut dyn Iterator<Item = i32>| rows.flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
    let arrays = [
        ("vertices.npy", npy("<f4", &[mesh.vertices.len(), 3], &floats(&mesh.vertices))),
        ("normals.npy", npy("<f4", &[mesh.normals.len(), 3], &floats(&mesh.normals))),
        ("triangles.npy", npy("<i4", &[mesh.triangles.len(), 3], &ints(&mut mesh.triangles.iter().flatten().copied()))),
        ("triangle_face.npy", npy("<i4", &[mesh.triangle_face.len()], &ints(&mut mesh.triangle_face.iter().copied()))),
    ];
    let mut written = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut written);
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, data) in arrays {
            zip.start_file(name, options).map_err(|e| e.to_string())?;
            zip.write_all(&data).map_err(|e| e.to_string())?;
        }
        zip.finish().map_err(|e| e.to_string())?;
    }
    Ok(written.into_inner())
}

/// A mesh node's `mesh/<id>.npz` entry read back.
pub fn from_npz(bytes: &[u8]) -> Result<TriMesh, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("mesh entry: {e}"))?;
    let mut array = |name: &str, columns: usize| -> Result<Vec<f64>, String> {
        let mut data = Vec::new();
        zip.by_name(&format!("{name}.npy")).map_err(|e| format!("mesh entry: {name}: {e}"))?.read_to_end(&mut data).map_err(|e| e.to_string())?;
        let (descr, shape, raw) = parse_npy(&data).map_err(|e| format!("mesh entry: {name}: {e}"))?;
        let count: usize = shape.iter().product();
        if (columns > 1 && (shape.len() != 2 || shape[1] != columns)) || (columns == 1 && shape.len() != 1) {
            return Err(format!("mesh entry: {name} has shape {shape:?}"));
        }
        values(&descr, raw, count).map_err(|e| format!("mesh entry: {name}: {e}"))
    };
    let triples = |v: Vec<f64>| v.chunks_exact(3).map(|c| [c[0] as f32, c[1] as f32, c[2] as f32]).collect::<Vec<_>>();
    let vertices = triples(array("vertices", 3)?);
    let normals = triples(array("normals", 3)?);
    let triangles = array("triangles", 3)?.chunks_exact(3).map(|c| [c[0] as i32, c[1] as i32, c[2] as i32]).collect();
    let triangle_face = array("triangle_face", 1)?.into_iter().map(|x| x as i32).collect();
    let mesh = TriMesh { vertices, normals, triangles, triangle_face };
    mesh.check("mesh entry")?;
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> TriMesh {
        let mut m = TriMesh { vertices: vec![[0., 0., 0.], [10., 0., 0.], [10., 10., 0.], [0., 10., 0.]], triangles: vec![[0, 1, 2], [0, 2, 3]], ..Default::default() };
        m.compute_normals();
        m.triangle_face = vec![0, 1];
        m
    }

    #[test]
    fn npz_round_trips_with_numpys_layout() {
        let m = square();
        let bytes = to_npz(&m).unwrap();
        assert_eq!(from_npz(&bytes).unwrap(), m);
        // The header pads the data to a 64-byte boundary, as NumPy writes it.
        let a = npy("<f4", &[4, 3], &[0; 48]);
        let len = u16::from_le_bytes([a[8], a[9]]) as usize;
        assert_eq!((10 + len) % 64, 0);
        assert!(std::str::from_utf8(&a[10..10 + len]).unwrap().ends_with('\n'));
        assert_eq!(m.normals[0], [0., 0., 1.]);
    }

    #[test]
    fn the_formats_read_one_square() {
        let dir = std::env::temp_dir().join(format!("sim-cad-mesh-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let obj = dir.join("s.obj");
        std::fs::write(&obj, "v 0 0 0\nv 10 0 0\nv 10 10 0\nv 0 10 0\nf 1 2 3 4\n").unwrap();
        let ply = dir.join("s.ply");
        std::fs::write(&ply, "ply\nformat ascii 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n10 0 0\n10 10 0\n0 10 0\n4 0 1 2 3\n").unwrap();
        let stl = dir.join("s.stl");
        std::fs::write(&stl, "solid s\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 10 0 0\nvertex 10 10 0\nendloop\nendfacet\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 10 10 0\nvertex 0 10 0\nendloop\nendfacet\nendsolid s\n").unwrap();
        for path in [&obj, &ply, &stl] {
            let (m, extent) = read_raw(path).unwrap();
            assert_eq!(m.triangles.len(), 2, "{}", path.display());
            assert_eq!(extent, 10.0);
        }
        let inches = read(&obj, "in").unwrap();
        assert_eq!(inches.vertices[2], [254., 254., 0.]);
        assert_eq!(units_guess(10.0), "in");
        assert_eq!(units_guess(0.2), "m");
        assert_eq!(units_guess(120.0), "mm");
        assert!(read_raw(&dir.join("s.glb")).unwrap_err().contains("glb"));
    }
}

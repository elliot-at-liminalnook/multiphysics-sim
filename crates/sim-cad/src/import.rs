//! File > Import (RoboCAD's `api.import_file` and `io/importers.py`): STEP
//! and IGES solids become bodies named after the file, an SVG drawing becomes
//! one sketch on the XY plane, and PNG/JPEG files become reference images.
//! Meshes (STL, OBJ, PLY, 3MF) become reference mesh nodes in the unit the
//! caller states (`crate::mesh`; RoboCAD's unit prompt), stored as
//! `mesh/<id>.npz`; FBX and glTF are refused by name.
//!
//! STEP is read by plain transfer (RoboCAD's fallback path when the file has
//! no XDE structure): every solid is a body named after the file stem. XDE
//! names, colours and assembly groups are not read.
use crate::kernel::{self, Exchange};
use crate::ops::Ctx;
use crate::sketch::{Curve, Plane, Sketch, V2};
use serde_json::Map;
use std::path::Path;

/// The extensions RoboCAD read as meshes (those in `crate::mesh::REFUSED` are refused here).
pub const MESH: [&str; 7] = ["stl", "obj", "3mf", "fbx", "ply", "glb", "gltf"];

/// Import `path`: the new node ids, as `{"imported": [...]}` answers them.
/// A mesh needs its file's unit ([`import_file_in`]).
pub fn import_file(cx: &mut Ctx, path: &str) -> Result<Vec<String>, String> {
    import_file_in(cx, path, None)
}

/// [`import_file`] with the unit a mesh file is in (mm, cm, m, in, ft).
pub fn import_file_in(cx: &mut Ctx, path: &str, unit: Option<&str>) -> Result<Vec<String>, String> {
    let p = Path::new(path);
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let stem = p.file_stem().map_or_else(|| path.to_string(), |s| s.to_string_lossy().into_owned());
    let base = p.file_name().map_or_else(|| path.to_string(), |s| s.to_string_lossy().into_owned());
    match ext.as_str() {
        "step" | "stp" | "iges" | "igs" => {
            let format = if ext.starts_with("st") { Exchange::Step } else { Exchange::Iges };
            let shapes = kernel::import(format, p, 1.0).map_err(|e| format!("could not read {path}: {e}"))?;
            if shapes.is_empty() {
                return Err(format!("{path}: the file holds no shapes"));
            }
            shapes.into_iter().map(|s| cx.add_built(s, &stem, None, None)).collect()
        }
        "svg" => {
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            let sketch = svg_sketch(&text, Plane::xy(0.), 1.0, &base)?;
            let mut extra = Map::new();
            extra.insert("sketch".into(), sketch.json());
            Ok(vec![cx.add_node("sketch", &base, None, extra)?])
        }
        "png" | "jpg" | "jpeg" => crate::references::import(cx.edit, &[path.to_string()], None),
        e if MESH.contains(&e) => {
            let unit = unit.ok_or_else(|| format!("Import {base}: a mesh needs the unit its file is in ({})", crate::mesh::UNITS.map(|(u, _)| u).join(", ")))?;
            Ok(vec![import_mesh(cx, p, unit, &base)?])
        }
        _ => Err(format!("Import {base}: not a file the editor imports (step, stp, iges, igs, svg, png, jpg)")),
    }
}

/// A reference mesh node named `name` from the mesh file at `path` in `unit`
/// (RoboCAD's `import_mesh`): its triangles in millimetres in a
/// `mesh/<id>.npz` entry. A mesh has no exact solid: no mass, no physics.
pub fn import_mesh(cx: &mut Ctx, path: &Path, unit: &str, name: &str) -> Result<String, String> {
    let mesh = crate::mesh::read(path, unit)?;
    let id = cx.add_node("mesh", name, None, Map::new())?;
    cx.edit.entries.insert(format!("mesh/{id}.npz"), Some(crate::mesh::to_npz(&mesh)?));
    Ok(id)
}

/// The attribute `name="…"` of one SVG element's opening tag, as a number.
fn attr(tag: &str, name: &str) -> f64 {
    attr_text(tag, name).and_then(|v| v.trim().parse().ok()).unwrap_or(0.)
}
fn attr_text<'t>(tag: &'t str, name: &str) -> Option<&'t str> {
    let mut rest = tag;
    while let Some(at) = rest.find(name) {
        let before = rest[..at].chars().last();
        let after = &rest[at + name.len()..];
        let after_trim = after.trim_start();
        if before.is_some_and(char::is_whitespace) && after_trim.starts_with('=') {
            let v = after_trim[1..].trim_start();
            let quote = v.chars().next()?;
            if quote == '"' || quote == '\'' {
                let body = &v[1..];
                return body.find(quote).map(|end| &body[..end]);
            }
        }
        rest = &rest[at + name.len()..];
    }
    None
}

/// The numbers in `s` (RoboCAD's `_NUM` pattern: signs, decimals, exponents).
fn numbers(s: &str) -> Vec<f64> {
    tokens(s).into_iter().filter_map(|t| t.parse().ok()).collect()
}

/// `re.findall(r"[MmLlHhVvCcSsQqTtAaZz]|" + _NUM, d)`.
fn tokens(d: &str) -> Vec<String> {
    let b = d.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if "MmLlHhVvCcSsQqTtAaZz".contains(c) {
            out.push(c.to_string());
            i += 1;
            continue;
        }
        // [-+]?(?:\d+\.\d*|\.\d+|\d+)(?:[eE][-+]?\d+)?
        let start = i;
        let mut j = i;
        if j < b.len() && (b[j] == b'-' || b[j] == b'+') {
            j += 1;
        }
        let digits = |k: &mut usize| {
            let s = *k;
            while *k < b.len() && b[*k].is_ascii_digit() {
                *k += 1;
            }
            *k > s
        };
        let mut k = j;
        let int = digits(&mut k);
        let mut ok = int;
        if k < b.len() && b[k] == b'.' {
            let mut m = k + 1;
            let frac = digits(&mut m);
            if int || frac {
                k = m;
                ok = true;
            }
        }
        if !ok {
            i = start + 1;
            continue;
        }
        if k < b.len() && (b[k] == b'e' || b[k] == b'E') {
            let mut m = k + 1;
            if m < b.len() && (b[m] == b'-' || b[m] == b'+') {
                m += 1;
            }
            if digits(&mut m) {
                k = m;
            }
        }
        out.push(d[start..k].to_string());
        i = k;
    }
    out
}

/// RoboCAD's `import_svg`: lines, polylines, polygons, rects, circles,
/// ellipses and paths (curves flattened, 8 steps per Bézier, arcs to their
/// chord) as sketch curves, y flipped so drawings read upright.
pub fn svg_sketch(text: &str, plane: Plane, scale: f64, name: &str) -> Result<Sketch, String> {
    if !text.contains("<svg") {
        return Err(format!("{name}: not an SVG drawing"));
    }
    let flip = |x: f64, y: f64| -> V2 { [x * scale, -y * scale] };
    let mut sk = Sketch::new(plane, name);
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        let after = &rest[open + 1..];
        let end = after.find('>').unwrap_or(after.len());
        let tag = &after[..end];
        rest = &after[end.min(after.len())..];
        if tag.starts_with('/') || tag.starts_with('!') || tag.starts_with('?') {
            continue;
        }
        let elem = tag.split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        let elem = elem.rsplit(':').next().unwrap_or(elem);
        let a = |n: &str| attr(tag, n);
        match elem {
            "line" => sk.curves.push(Curve::line(flip(a("x1"), a("y1")), flip(a("x2"), a("y2")))),
            "polyline" | "polygon" => {
                let nums = numbers(attr_text(tag, "points").unwrap_or(""));
                let pts: Vec<V2> = nums.chunks_exact(2).map(|p| flip(p[0], p[1])).collect();
                if pts.len() >= 2 {
                    sk.curves.push(Curve::polyline(pts, elem == "polygon"));
                }
            }
            "rect" => {
                let (x, y, w, h) = (a("x"), a("y"), a("width"), a("height"));
                sk.curves.push(Curve::polyline(vec![flip(x, y), flip(x + w, y), flip(x + w, y + h), flip(x, y + h)], true));
            }
            "circle" => sk.curves.push(Curve::circle(flip(a("cx"), a("cy")), a("r") * scale)),
            "ellipse" => sk.curves.push(Curve { center: Some(flip(a("cx"), a("cy"))), radius: a("rx") * scale, radius2: a("ry") * scale, ..ellipse() }),
            "path" => {
                for (sub, closed) in path_polylines(attr_text(tag, "d").unwrap_or("")) {
                    if sub.len() >= 2 {
                        sk.curves.push(Curve::polyline(sub.iter().map(|p| flip(p[0], p[1])).collect(), closed));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(sk)
}

fn ellipse() -> Curve {
    Curve { kind: "ellipse".into(), ..Curve::default() }
}

/// RoboCAD's `_svg_path_polylines`.
fn path_polylines(d: &str) -> Vec<(Vec<V2>, bool)> {
    let t = tokens(d);
    let mut out = Vec::new();
    let mut cur: Vec<V2> = Vec::new();
    let (mut pos, mut start): (V2, V2) = ([0., 0.], [0., 0.]);
    let mut i = 0;
    let mut cmd: Option<char> = None;
    let mut last_ctrl: Option<V2> = None;
    let letter = |s: &str| s.len() == 1 && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
    while i < t.len() {
        if letter(&t[i]) {
            let c = t[i].chars().next().expect("one letter");
            cmd = Some(c);
            i += 1;
            if c == 'Z' || c == 'z' {
                if !cur.is_empty() {
                    out.push((std::mem::take(&mut cur), true));
                }
                pos = start;
                continue;
            }
        }
        let Some(c0) = cmd else {
            i += 1;
            continue;
        };
        let rel = c0.is_ascii_lowercase();
        let c = c0.to_ascii_uppercase();
        let mut nums = |n: usize| -> Option<Vec<f64>> {
            let v: Option<Vec<f64>> = (0..n).map(|k| t.get(i + k).and_then(|s| s.parse().ok())).collect();
            i += n;
            v
        };
        match c {
            'M' => {
                let Some(v) = nums(2) else { break };
                pos = if rel { [pos[0] + v[0], pos[1] + v[1]] } else { [v[0], v[1]] };
                if !cur.is_empty() {
                    out.push((std::mem::take(&mut cur), false));
                }
                cur = vec![pos];
                start = pos;
                cmd = Some(if rel { 'l' } else { 'L' });
            }
            'L' => {
                let Some(v) = nums(2) else { break };
                pos = if rel { [pos[0] + v[0], pos[1] + v[1]] } else { [v[0], v[1]] };
                cur.push(pos);
            }
            'H' => {
                let Some(v) = nums(1) else { break };
                pos = [if rel { pos[0] + v[0] } else { v[0] }, pos[1]];
                cur.push(pos);
            }
            'V' => {
                let Some(v) = nums(1) else { break };
                pos = [pos[0], if rel { pos[1] + v[0] } else { v[0] }];
                cur.push(pos);
            }
            'C' | 'S' | 'Q' | 'T' => {
                let n = match c {
                    'C' => 6,
                    'S' | 'Q' => 4,
                    _ => 2,
                };
                let Some(v) = nums(n) else { break };
                let mut pts: Vec<V2> = v.chunks_exact(2).map(|p| if rel { [pos[0] + p[0], pos[1] + p[1]] } else { [p[0], p[1]] }).collect();
                if c == 'S' || c == 'T' {
                    let refl = last_ctrl.map_or(pos, |l| [2. * pos[0] - l[0], 2. * pos[1] - l[1]]);
                    pts.insert(0, refl);
                }
                let mut ctrl = vec![pos];
                ctrl.extend(pts);
                for k in 1..=8 {
                    let s = k as f64 / 8.;
                    let mut tmp = ctrl.clone();
                    while tmp.len() > 1 {
                        tmp = tmp.windows(2).map(|w| [(1. - s) * w[0][0] + s * w[1][0], (1. - s) * w[0][1] + s * w[1][1]]).collect();
                    }
                    cur.push(tmp[0]);
                }
                last_ctrl = Some(ctrl[ctrl.len() - 2]);
                pos = ctrl[ctrl.len() - 1];
            }
            'A' => {
                let Some(v) = nums(7) else { break };
                pos = if rel { [pos[0] + v[5], pos[1] + v[6]] } else { [v[5], v[6]] };
                cur.push(pos); // arcs flattened to their chord
            }
            _ => i += 1,
        }
    }
    if !cur.is_empty() {
        out.push((cur, false));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_shapes_and_paths_become_curves_upright() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect x="0" y="0" width="10" height="5"/><circle cx="3" cy="4" r="2"/><path d="M0,0 L10,0 l0,-10 z M1 1 C1 2, 3 2, 3 1"/><polyline points="0,0 1,1 2,0"/></svg>"#;
        let sk = svg_sketch(svg, Plane::xy(0.), 1.0, "t.svg").unwrap();
        let kinds: Vec<&str> = sk.curves.iter().map(|c| c.kind.as_str()).collect();
        assert_eq!(kinds, ["polyline", "circle", "polyline", "polyline", "polyline"]);
        assert_eq!(sk.curves[0].points[2], [10., -5.], "y is flipped");
        assert_eq!(sk.curves[1].center, Some([3., -4.]));
        assert!(sk.curves[2].closed && sk.curves[2].points == vec![[0., 0.], [10., 0.], [10., 10.]]);
        assert_eq!(sk.curves[3].points.len(), 9, "a cubic is 8 steps after its start");
        assert!(!sk.curves[4].closed);
    }

    #[test]
    fn numbers_follow_robocads_pattern() {
        assert_eq!(tokens("M-1.5e2.5,3L.5-2z"), ["M", "-1.5e2", ".5", "3", "L", ".5", "-2", "z"]);
    }
}

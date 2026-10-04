//! Reference images (`image` nodes whose bytes are the archive entry
//! `image/<id>`) and the linked system file (`robot_settings.system`).
//! Reference: RoboCAD's references.py (`ReferenceOps`) and system_link.py.
use crate::edit::{Edit, new_id};
use crate::sketch::Plane;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// An image's pixel size from its header (PNG, JPEG, GIF, BMP, WebP).
pub fn image_size(data: &[u8]) -> Result<(u32, u32), String> {
    let be32 = |b: &[u8]| u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    if data.len() > 24 && &data[..8] == b"\x89PNG\r\n\x1a\n" {
        return Ok((be32(&data[16..20]), be32(&data[20..24])));
    }
    if data.len() > 10 && &data[..6] == b"GIF89a" || data.len() > 10 && &data[..6] == b"GIF87a" {
        return Ok((u32::from(u16::from_le_bytes([data[6], data[7]])), u32::from(u16::from_le_bytes([data[8], data[9]]))));
    }
    if data.len() > 26 && &data[..2] == b"BM" {
        let w = i32::from_le_bytes([data[18], data[19], data[20], data[21]]);
        let h = i32::from_le_bytes([data[22], data[23], data[24], data[25]]);
        return Ok((w.unsigned_abs(), h.unsigned_abs()));
    }
    if data.len() > 30 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        match &data[12..16] {
            b"VP8X" => return Ok((1 + u32::from_le_bytes([data[24], data[25], data[26], 0]), 1 + u32::from_le_bytes([data[27], data[28], data[29], 0]))),
            b"VP8 " => return Ok((u32::from(u16::from_le_bytes([data[26], data[27]]) & 0x3fff), u32::from(u16::from_le_bytes([data[28], data[29]]) & 0x3fff))),
            b"VP8L" => {
                let b = u32::from_le_bytes([data[21], data[22], data[23], data[24]]);
                return Ok(((b & 0x3fff) + 1, ((b >> 14) & 0x3fff) + 1));
            }
            _ => {}
        }
    }
    if data.len() > 4 && data[0] == 0xff && data[1] == 0xd8 {
        let mut i = 2;
        while i + 9 < data.len() {
            if data[i] != 0xff {
                i += 1;
                continue;
            }
            let marker = data[i + 1];
            let len = usize::from(u16::from_be_bytes([data[i + 2], data[i + 3]]));
            if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
                return Ok((u32::from(u16::from_be_bytes([data[i + 7], data[i + 8]])), u32::from(u16::from_be_bytes([data[i + 5], data[i + 6]]))));
            }
            i += 2 + len;
        }
    }
    Err("not an image this editor can read (PNG, JPEG, GIF, BMP or WebP)".into())
}

/// `import_references`: one locked image node per file (100 mm wide, 0.6
/// opaque) on `plane`; their ids.
pub fn import(edit: &mut Edit, paths: &[String], plane: Option<Plane>) -> Result<Vec<String>, String> {
    let plane = plane.unwrap_or(Plane::xy(0.));
    let mut ids = Vec::new();
    let mut names: Vec<String> = edit.manifest["nodes"].as_array().into_iter().flatten().filter_map(|n| n["name"].as_str().map(str::to_string)).collect();
    for path in paths {
        let data = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        let (w, h) = image_size(&data).map_err(|e| format!("{path}: {e}"))?;
        if w == 0 || h == 0 {
            return Err(format!("{path}: the image has no pixels"));
        }
        let base = Path::new(path).file_name().map_or_else(|| path.clone(), |f| f.to_string_lossy().into_owned());
        let mut name = base.clone();
        let mut n = 2;
        while names.contains(&name) {
            name = format!("{base} {n}");
            n += 1;
        }
        names.push(name.clone());
        let id = new_id();
        let node = json!({
            "id": id, "kind": "image", "name": name, "parent": null, "children": [], "visible": true, "locked": true, "disabled": false,
            "material": null, "color": null, "pivot": null,
            "transform": {"translation": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0], "angle_deg": 0.0, "scale": 1.0},
            "source": null, "tessellation_tolerance": 0.05,
            "image": {"path": path, "plane": plane.json(), "width": 100.0, "height": 100.0 * f64::from(h) / f64::from(w), "opacity": 0.6, "rotation_deg": 0.0},
        });
        edit.manifest["nodes"].as_array_mut().ok_or("manifest nodes must be an array")?.push(node);
        if !edit.manifest["roots"].is_array() {
            edit.manifest["roots"] = json!([]);
        }
        edit.manifest["roots"].as_array_mut().expect("array").push(json!(id));
        edit.entries.insert(format!("image/{id}"), Some(data));
        ids.push(id);
    }
    Ok(ids)
}

/// `update_reference`'s fields.
#[derive(Clone, Debug, Default)]
pub struct Update {
    pub width: Option<f64>,
    pub opacity: Option<f64>,
    pub origin: Option<[f64; 3]>,
    pub plane: Option<Plane>,
    pub rotation_deg: Option<f64>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    pub name: Option<String>,
}

/// `update_reference`.
pub fn update(edit: &mut Edit, id: &str, u: Update) -> Result<(), String> {
    let node = edit.node_mut(id)?;
    let image = node["image"].as_object_mut().ok_or("Select a reference image")?;
    let mut img = Value::Object(image.clone());
    if let Some(w) = u.width {
        if !(w.is_finite() && w > 0.) {
            return Err("Width must be positive".into());
        }
        let old = img["width"].as_f64().unwrap_or(100.);
        img["height"] = json!(img["height"].as_f64().unwrap_or(100.) * w / old);
        img["width"] = json!(w);
    }
    if let Some(o) = u.opacity {
        if !(o.is_finite() && (0. ..=1.).contains(&o)) {
            return Err("Opacity must be between 0 and 1".into());
        }
        img["opacity"] = json!(o);
    }
    let mut p = Plane::parse(&img["plane"]).unwrap_or(Plane::xy(0.));
    if let Some(plane) = u.plane {
        p = plane;
        img["rotation_deg"] = json!(0.0);
    }
    if let Some(o) = u.origin {
        if o.iter().any(|v| !v.is_finite()) {
            return Err("Origin needs three finite coordinates".into());
        }
        p.origin = o;
    }
    if let Some(r) = u.rotation_deg {
        if !r.is_finite() {
            return Err("Rotation must be finite".into());
        }
        let a = (r - img["rotation_deg"].as_f64().unwrap_or(0.)).to_radians();
        let y = p.y_axis();
        p.x_axis = [0, 1, 2].map(|i| a.cos() * p.x_axis[i] + a.sin() * y[i]);
        img["rotation_deg"] = json!(r);
    }
    img["plane"] = p.json();
    node["image"] = img;
    if let Some(v) = u.visible {
        node["visible"] = json!(v);
    }
    if let Some(l) = u.locked {
        node["locked"] = json!(l);
    }
    if let Some(n) = u.name {
        node["name"] = json!(n);
    }
    Ok(())
}

/// `calibrate_reference`: scale about the first pick so the picks are `distance` apart.
pub fn calibrate(edit: &mut Edit, id: &str, first: [f64; 3], second: [f64; 3], distance: f64) -> Result<(), String> {
    if first.iter().chain(&second).chain([&distance]).any(|v| !v.is_finite()) {
        return Err("Calibration coordinates and distance must be finite".into());
    }
    let current = ((first[0] - second[0]).powi(2) + (first[1] - second[1]).powi(2) + (first[2] - second[2]).powi(2)).sqrt();
    if current < 1e-9 || distance <= 0. {
        return Err("Pick two distinct points and enter a positive distance".into());
    }
    let image = edit.node(id).ok_or_else(|| format!("node {id} does not exist"))?["image"].clone();
    let factor = distance / current;
    let o = Plane::parse(&image["plane"]).map(|p| p.origin).unwrap_or([0.; 3]);
    let origin = [0, 1, 2].map(|i| first[i] + factor * (o[i] - first[i]));
    update(edit, id, Update { width: Some(image["width"].as_f64().unwrap_or(100.) * factor), origin: Some(origin), ..Update::default() })
}

/// system_link.py's `read`: a `sim.system/1` file's hash and summary.
pub fn read_system(path: &Path) -> Result<Value, String> {
    let data = std::fs::read(path).map_err(|e| format!("Cannot read system file: {e}"))?;
    let name = path.file_name().map_or_else(|| path.display().to_string(), |f| f.to_string_lossy().into_owned());
    let doc: Value = serde_json::from_slice(&data).map_err(|e| format!("{name} is not JSON: {e}"))?;
    if doc["schema"] != "sim.system/1" {
        return Err(format!("{name} is not a sim.system/1 system file"));
    }
    let defs = doc["definitions"].as_object().map_or(0, |d| d.len());
    let root = doc["root"].as_str().and_then(|r| doc["definitions"].get(r)).cloned().unwrap_or(Value::Null);
    Ok(json!({
        "sha256": format!("{:x}", Sha256::digest(&data)),
        "title": doc["title"].as_str().unwrap_or(""),
        "revision": doc["revision"].as_i64().unwrap_or(0),
        "definitions": defs,
        "instances": root["instances"].as_object().map_or(0, |i| i.len()),
    }))
}

/// The path stored for a linked file: relative to the CAD file when close, else absolute.
fn stored_path(doc_path: &Path, system: &Path) -> String {
    let system = std::path::absolute(system).unwrap_or_else(|_| system.to_path_buf());
    if let Some(dir) = std::path::absolute(doc_path).ok().as_deref().and_then(Path::parent)
        && let Some(rel) = relative(&system, dir)
        && !rel.starts_with("../../../")
    {
        return rel;
    }
    system.display().to_string()
}
fn relative(path: &Path, base: &Path) -> Option<String> {
    let (p, b): (Vec<_>, Vec<_>) = (path.components().collect(), base.components().collect());
    let common = p.iter().zip(&b).take_while(|(x, y)| x == y).count();
    if common == 0 {
        return None;
    }
    let mut out = PathBuf::new();
    for _ in common..b.len() {
        out.push("..");
    }
    for c in &p[common..] {
        out.push(c);
    }
    Some(out.display().to_string())
}
/// Where a stored link path points.
pub fn resolve(doc_path: &Path, stored: &str) -> PathBuf {
    let p = Path::new(stored);
    if p.is_absolute() { p.to_path_buf() } else { doc_path.parent().unwrap_or(Path::new(".")).join(p) }
}

/// `link_system`: a durable reference (path, hash, title, revision, when).
pub fn link(edit: &mut Edit, doc_path: &Path, system: &Path) -> Result<Value, String> {
    let mut link = read_system(system)?;
    link["path"] = json!(stored_path(doc_path, system));
    link["linked"] = json!(crate::edit::now_iso()[..19]);
    edit.object_mut("robot_settings").insert("system".into(), link.clone());
    Ok(link)
}

/// `status`: unlinked, current, changed or missing.
pub fn status(doc_path: &Path, link: &Value) -> Value {
    if link.is_null() {
        return json!({"state": "unlinked"});
    }
    let path = resolve(doc_path, link["path"].as_str().unwrap_or(""));
    match read_system(&path) {
        Err(e) => json!({"state": "missing", "path": path, "error": e, "link": link}),
        Ok(now) => json!({"state": if now["sha256"] == link["sha256"] { "current" } else { "changed" }, "path": path, "link": link, "now": now}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_headers_give_their_size() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(640u32.to_be_bytes());
        png.extend(480u32.to_be_bytes());
        png.extend([0u8; 8]);
        assert_eq!(image_size(&png).unwrap(), (640, 480));
        assert!(image_size(b"not an image at all").is_err());
        assert_eq!(relative(Path::new("/a/b/c/d.json"), Path::new("/a/b")).unwrap(), "c/d.json");
    }
}

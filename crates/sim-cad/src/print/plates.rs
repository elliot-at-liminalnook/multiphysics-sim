//! Plates from a print plan (RoboCAD's `print_plan.py`): each piece turned
//! so its chosen build direction points up, turned about z to its smallest
//! footprint, set on the bed and packed onto plates (grouped by layer
//! height), then written as one 3MF per plate with each object's settings.
//!
//! The 3MF carries the geometry (core 3MF) plus Bambu Studio's per-object
//! settings file (`Metadata/model_settings.config`: wall loops, sparse
//! infill density and pattern, top/bottom shell layers, layer height). Those
//! keys are Bambu Studio's names; check the object settings after opening,
//! and pick the printer, nozzle and filament profiles there. A manifest
//! lists every object's settings, estimated time and filament, so nothing
//! depends on the 3MF extras.
use super::study::weld;
use super::{K, Registry, V3, cross, dot, round, unit};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

/// mm between parts on a plate.
pub const SPACING: f64 = 6.0;

/// One piece to lay out.
#[derive(Clone, Debug)]
pub struct PlanPiece {
    pub name: String,
    pub body: Vec<u8>,
    pub build_direction: V3,
    pub settings: Value,
    pub estimate: Value,
    pub safety_factor: Option<f64>,
    pub source: Option<String>,
}

/// The body turned so `build_direction` points up, turned about z to the
/// smallest footprint and set on the bed at the origin: (body, width, depth).
pub fn orient(k: &K, body: &[u8], build_direction: V3) -> Result<(Vec<u8>, f64, f64), String> {
    let d = unit(build_direction);
    let up = [0.0, 0.0, 1.0];
    let c = dot(d, up);
    let mut b = body.to_vec();
    if c < 1.0 - 1e-9 {
        let axis = if c > -1.0 + 1e-9 { cross(d, up) } else { [1.0, 0.0, 0.0] };
        b = k.rotate(&b, unit(axis), c.clamp(-1.0, 1.0).acos().to_degrees(), [0.0; 3])?;
    }
    let pts: Vec<[f64; 2]> = k.geometry(&b, 0.5)?.vertices_mm.iter().map(|v| [v[0], v[1]]).collect();
    let mut best = (f64::INFINITY, 0);
    for deg in (0..90).step_by(3) {
        let (s, co) = (deg as f64).to_radians().sin_cos();
        let (mut xl, mut xh, mut yl, mut yh) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
        for p in &pts {
            let (x, y) = (co * p[0] - s * p[1], s * p[0] + co * p[1]);
            xl = xl.min(x);
            xh = xh.max(x);
            yl = yl.min(y);
            yh = yh.max(y);
        }
        let area = (xh - xl) * (yh - yl);
        if area < best.0 - 1e-6 {
            best = (area, deg);
        }
    }
    if best.1 != 0 {
        b = k.rotate(&b, [0.0, 0.0, 1.0], best.1 as f64, [0.0; 3])?;
    }
    let (lo, hi) = k.bounds(&b)?;
    b = k.translate(&b, [-lo[0], -lo[1], -lo[2]])?;
    Ok((b, hi[0] - lo[0], hi[1] - lo[1]))
}

/// A piece placed on a plate.
#[derive(Clone, Debug)]
pub struct Placed {
    pub piece: PlanPiece,
    pub body: Vec<u8>,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub d: f64,
}

#[derive(Clone, Debug)]
pub struct Plate {
    pub index: usize,
    pub layer_height: f64,
    pub items: Vec<Placed>,
}

impl Plate {
    pub fn hours(&self) -> f64 {
        self.items.iter().map(|p| p.piece.estimate["print_hours"].as_f64().unwrap_or(0.0)).sum()
    }
    pub fn grams(&self) -> f64 {
        self.items.iter().map(|p| p.piece.estimate["filament_g"].as_f64().unwrap_or(0.0) + p.piece.estimate["support_g"].as_f64().unwrap_or(0.0)).sum()
    }
}

/// Shelf-pack oriented pieces onto plates.
pub fn pack(k: &K, reg: &Registry, pieces: &[PlanPiece], printer: &str) -> Result<Vec<Plate>, String> {
    let [ux, uy, uz] = reg.usable_mm(printer)?;
    let mut oriented = Vec::new();
    for p in pieces {
        if (k.cancelled)() {
            return Err("cancelled".into());
        }
        let (mut b, mut w, mut d) = orient(k, &p.body, p.build_direction)?;
        let (lo, hi) = k.bounds(&b)?;
        let h = hi[2] - lo[2];
        if h > uz + 1e-6 || !((w <= ux && d <= uy) || (w <= uy && d <= ux)) {
            return Err(format!("{}: {w:.0}×{d:.0}×{h:.0} mm does not fit the {printer} standing this way: split it first", p.name));
        }
        if w > ux || d > uy {
            // Turn a quarter turn to fit.
            b = k.rotate(&b, [0.0, 0.0, 1.0], 90.0, [0.0; 3])?;
            let (lo, _) = k.bounds(&b)?;
            b = k.translate(&b, [-lo[0], -lo[1], -lo[2]])?;
            std::mem::swap(&mut w, &mut d);
        }
        oriented.push((p.clone(), b, w, d));
    }
    // Grouped by layer height (as text keys, sorted by value).
    let mut by_height: BTreeMap<i64, (f64, Vec<(PlanPiece, Vec<u8>, f64, f64)>)> = BTreeMap::new();
    for item in oriented {
        let lh = item.0.settings["layer_height"].as_f64().unwrap_or(0.2);
        by_height.entry((lh * 1e6).round() as i64).or_insert_with(|| (lh, Vec::new())).1.push(item);
    }
    let mut plates: Vec<Plate> = Vec::new();
    for (_, (lh, mut items)) in by_height {
        items.sort_by(|a, b| (b.2 * b.3).total_cmp(&(a.2 * a.3)));
        // (plate, shelves [(y, height, x used)])
        let mut open: Vec<(Plate, Vec<[f64; 3]>)> = Vec::new();
        for (p, b, w, d) in items {
            let mut placed = false;
            for (plate, shelves) in open.iter_mut() {
                for s in shelves.iter_mut() {
                    let [y, height, used] = *s;
                    if d <= height && used + w <= ux {
                        plate.items.push(Placed { piece: p.clone(), body: b.clone(), x: used, y, w, d });
                        s[2] = used + w + SPACING;
                        placed = true;
                        break;
                    }
                }
                if placed {
                    break;
                }
                let top = shelves.iter().map(|s| s[0] + s[1] + SPACING).fold(0.0, f64::max);
                if top + d <= uy && w <= ux {
                    plate.items.push(Placed { piece: p.clone(), body: b.clone(), x: 0.0, y: top, w, d });
                    shelves.push([top, d, w + SPACING]);
                    placed = true;
                    break;
                }
            }
            if !placed {
                let plate = Plate { index: plates.len() + open.len() + 1, layer_height: lh, items: vec![Placed { piece: p, body: b, x: 0.0, y: 0.0, w, d }] };
                open.push((plate, vec![[0.0, d, w + SPACING]]));
            }
        }
        plates.extend(open.into_iter().map(|(p, _)| p));
    }
    for (i, p) in plates.iter_mut().enumerate() {
        p.index = i + 1;
    }
    Ok(plates)
}

fn fmt_g(x: f64) -> String {
    // Python's `:g` for the values settings hold (short decimals).
    let s = format!("{x}");
    if s.contains('e') { format!("{x:e}") } else { s }
}

/// Bambu Studio's per-object setting names for a part's settings.
pub fn bambu_settings(s: &Value) -> Map<String, Value> {
    let n = |k: &str, d: f64| s[k].as_f64().unwrap_or(d);
    let mut m = Map::new();
    m.insert("wall_loops".into(), json!((n("walls", 3.0) as i64).to_string()));
    m.insert("sparse_infill_density".into(), json!(format!("{}%", fmt_g(round(n("infill", 0.15) * 100.0, 6)))));
    m.insert("sparse_infill_pattern".into(), json!(s["pattern"].as_str().unwrap_or("gyroid")));
    let tb = (n("top_bottom_layers", 5.0) as i64).to_string();
    m.insert("top_shell_layers".into(), json!(tb));
    m.insert("bottom_shell_layers".into(), json!(tb));
    m.insert("layer_height".into(), json!(fmt_g(n("layer_height", 0.2))));
    m
}

/// XML text and attribute escaping.
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/><Default Extension=\"config\" ContentType=\"text/xml\"/></Types>";
const RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/></Relationships>";

/// A 3MF from named files (`[Content_Types].xml` and `_rels/.rels` added).
pub fn write_3mf(path: &Path, files: &[(&str, String)]) -> Result<(), String> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, text) in [("[Content_Types].xml", CONTENT_TYPES.to_string()), ("_rels/.rels", RELS.to_string())].iter().chain(files.iter().map(|(n, t)| (*n, t.clone())).collect::<Vec<_>>().iter()) {
            z.start_file(*name, o).map_err(|e| e.to_string())?;
            z.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        }
        z.finish().map_err(|e| e.to_string())?;
    }
    std::fs::write(path, buf.into_inner()).map_err(|e| format!("{}: {e}", path.display()))
}

/// The core 3MF model of named meshes placed by translations.
pub fn model_xml(objects: &[(String, crate::geometry::BodyGeometry, [f64; 2])], application: &str) -> String {
    let mut res = String::new();
    let mut items = String::new();
    for (n, (name, m, at)) in objects.iter().enumerate() {
        let oid = n + 1;
        res += &format!("<object id=\"{oid}\" type=\"model\" name=\"{}\"><mesh><vertices>", escape(name));
        for v in &m.vertices_mm {
            res += &format!("<vertex x=\"{:.4}\" y=\"{:.4}\" z=\"{:.4}\"/>", v[0], v[1], v[2]);
        }
        res += "</vertices><triangles>";
        for t in &m.triangles {
            res += &format!("<triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>", t[0], t[1], t[2]);
        }
        res += "</triangles></mesh></object>";
        items += &format!("<item objectid=\"{oid}\" transform=\"1 0 0 0 1 0 0 0 1 {:.3} {:.3} 0\" printable=\"1\"/>", at[0], at[1]);
    }
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\"><metadata name=\"Application\">{}</metadata><resources>{res}</resources><build>{items}</build></model>", escape(application))
}

/// Bambu's `model_settings.config`: each object's settings and one plate.
pub fn settings_xml(objects: &[(String, Map<String, Value>)], plate: usize) -> String {
    let mut configs = String::new();
    for (n, (name, settings)) in objects.iter().enumerate() {
        let mut meta = format!("<metadata key=\"name\" value=\"{}\"/><metadata key=\"extruder\" value=\"1\"/>", escape(name));
        for (k, v) in settings {
            meta += &format!("<metadata key=\"{k}\" value=\"{}\"/>", escape(v.as_str().unwrap_or("")));
        }
        configs += &format!("<object id=\"{}\">{meta}<part id=\"1\" subtype=\"normal_part\"><metadata key=\"name\" value=\"{}\"/></part></object>", n + 1, escape(name));
    }
    let instances: String = (0..objects.len()).map(|n| format!("<model_instance><metadata key=\"object_id\" value=\"{}\"/><metadata key=\"instance_id\" value=\"0\"/></model_instance>", n + 1)).collect();
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<config>{configs}<plate><metadata key=\"plater_id\" value=\"1\"/><metadata key=\"plater_name\" value=\"Plate {plate}\"/>{instances}</plate></config>")
}

/// One 3MF: geometry, object names, placements on the bed, Bambu per-object settings.
pub fn write_plate(k: &K, reg: &Registry, plate: &Plate, path: &Path, printer: &str) -> Result<Value, String> {
    let margin = reg.margin_mm(printer)?;
    let mut objects = Vec::new();
    let mut configs = Vec::new();
    for p in &plate.items {
        let m = weld(&k.geometry(&p.body, 0.05)?, 1e-5);
        objects.push((p.piece.name.clone(), m, [p.x + margin, p.y + margin]));
        configs.push((p.piece.name.clone(), bambu_settings(&p.piece.settings)));
    }
    write_3mf(path, &[("3D/3dmodel.model", model_xml(&objects, "robocad print plan")), ("Metadata/model_settings.config", settings_xml(&configs, plate.index))])?;
    Ok(json!({
        "file": path.file_name().map(|f| f.to_string_lossy().into_owned()),
        "plate": plate.index, "layer_height_mm": plate.layer_height,
        "estimated_hours": round(plate.hours(), 2), "estimated_filament_g": round(plate.grams(), 1),
        "objects": plate.items.iter().map(|p| json!({
            "name": p.piece.name, "source": p.piece.source, "at_mm": [round(p.x + margin, 1), round(p.y + margin, 1)], "footprint_mm": [round(p.w, 1), round(p.d, 1)],
            "settings": p.piece.settings, "bambu": bambu_settings(&p.piece.settings), "estimate": p.piece.estimate, "safety_factor": p.piece.safety_factor,
        })).collect::<Vec<_>>(),
    }))
}

/// Pack the pieces, write one 3MF per plate and `plates.json`; the manifest.
pub fn write_plates(k: &K, reg: &Registry, pieces: &[PlanPiece], out_dir: &Path, printer: &str, material: &str, plan_path: Option<&Path>) -> Result<Value, String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let plates = pack(k, reg, pieces, printer)?;
    let mut files = Vec::new();
    for pl in &plates {
        if (k.cancelled)() {
            return Err("cancelled".into());
        }
        files.push(write_plate(k, reg, pl, &out_dir.join(format!("plate-{}.3mf", pl.index)), printer)?);
    }
    let hours: f64 = files.iter().map(|f| f["estimated_hours"].as_f64().unwrap_or(0.0)).sum();
    let grams: f64 = files.iter().map(|f| f["estimated_filament_g"].as_f64().unwrap_or(0.0)).sum();
    let manifest = json!({
        "schema": "sim.print-plates/1", "printer": printer, "material": material, "registry_sha256": reg.sha256(),
        "plan": plan_path.map(|p| p.display().to_string()), "plates": files,
        "total_hours": round(hours, 2), "total_filament_g": round(grams, 1),
        "note": "Open each plate in Bambu Studio, choose the printer, nozzle and filament, and check the per-object settings. Times and filament are estimates from the plan; the slicer is authoritative.",
    });
    let path = out_dir.join("plates.json");
    std::fs::write(&path, serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bambu_names_follow_the_settings() {
        let b = bambu_settings(&json!({"walls": 4, "infill": 0.25, "pattern": "grid", "layer_height": 0.16, "top_bottom_layers": 6}));
        assert_eq!(b["wall_loops"], "4");
        assert_eq!(b["sparse_infill_density"], "25%");
        assert_eq!(b["layer_height"], "0.16");
        assert_eq!(b["top_shell_layers"], "6");
    }

    #[test]
    fn settings_name_every_object_and_the_plate() {
        let x = settings_xml(&[("a & b".into(), Map::new())], 3);
        assert!(x.contains("a &amp; b"));
        assert!(x.contains("Plate 3"));
    }
}

//! Exports of the open archive, in process. Reference: RoboCAD's
//! io/exporters.py (`export_stl`, `export_3mf`, `export_obj`, `export_step`,
//! `export_iges`, `export_sketch_svg`) and io/drawing.py
//! (`export_drawing_svg`, `project_view`, `chain_loops`). Settings are the
//! viewer's checked `POST /export` settings (files::formats). What a format
//! cannot carry here is reported in `warnings`, not silently dropped.
use crate::geometry::BodyGeometry;
use crate::kernel::{self, Exchange, Op};
use crate::sketch::{Plane, Sketch};
use crate::ArchiveDocument;
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;

/// The nodes an export writes: `ids` (and what is under them), else every
/// shown body, sheet and instance, in the tree's order.
pub fn bodies(doc: &ArchiveDocument, ids: Option<&[String]>) -> Vec<String> {
    let nodes = doc.manifest["nodes"].as_array().cloned().unwrap_or_default();
    let mut wanted: Option<Vec<String>> = ids.map(<[String]>::to_vec);
    if let Some(w) = wanted.as_mut() {
        let mut i = 0;
        while i < w.len() {
            let kids: Vec<String> = doc.node(&w[i]).and_then(|n| n["children"].as_array().cloned()).into_iter().flatten().filter_map(|c| c.as_str().map(str::to_string)).collect();
            w.extend(kids);
            i += 1;
        }
    }
    nodes
        .iter()
        .filter(|n| matches!(n["kind"].as_str(), Some("body" | "sheet" | "instance")))
        .filter_map(|n| n["id"].as_str().map(str::to_string))
        .filter(|id| match &wanted { Some(w) => w.contains(id), None => doc.visible(id) })
        .collect()
}

fn color(doc: &ArchiveDocument, id: &str) -> [f32; 3] {
    let n = doc.node(id);
    let c = n.and_then(|n| crate::ops::v3(&n["color"])).or_else(|| {
        let m = n.and_then(|n| n["material"].as_str())?;
        crate::ops::v3(&crate::edit::material(&doc.manifest, m)?["color"])
    });
    c.map_or([0.70, 0.72, 0.76], |c| c.map(|v| v as f32))
}
fn name(doc: &ArchiveDocument, id: &str) -> String {
    doc.node(id).and_then(|n| n["name"].as_str()).unwrap_or(id).to_string()
}
fn unit_scale(unit: &str) -> Result<f64, String> {
    Ok(match unit {
        "mm" => 1.,
        "cm" => 0.1,
        "m" => 0.001,
        "in" => 1. / 25.4,
        "ft" => 1. / 304.8,
        other => return Err(format!("unknown unit {other}")),
    })
}

/// The bodies' tessellations: `geometry` where its tolerance matches, else tessellated again.
fn meshes(doc: &ArchiveDocument, geometry: &[BodyGeometry], ids: &[String], tolerance: Option<f64>, cancelled: &dyn Fn() -> bool) -> Result<Vec<BodyGeometry>, String> {
    ids.iter()
        .map(|id| {
            let own = doc.node(id).and_then(|n| n["tessellation_tolerance"].as_f64()).unwrap_or(0.05);
            match (tolerance.filter(|t| (t - own).abs() > 1e-12), geometry.iter().find(|b| &b.node_id == id)) {
                (None, Some(g)) => Ok(g.clone()),
                (t, _) => crate::geometry::tessellate_node(doc, id, t.unwrap_or(own), cancelled),
            }
        })
        .collect()
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension(format!("{}.part", path.extension().and_then(|e| e.to_str()).unwrap_or("out")));
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Export `format` to `path` with `settings`: `{exported, warnings}`.
pub fn export(doc: &ArchiveDocument, geometry: &[BodyGeometry], format: &str, path: &Path, settings: &Value, ids: Option<&[String]>, cancelled: &dyn Fn() -> bool) -> Result<Value, String> {
    let mut warnings: Vec<String> = Vec::new();
    let ids_all = bodies(doc, ids);
    let need = |list: &Vec<String>| if list.is_empty() { Err("nothing to export: no shown bodies".to_string()) } else { Ok(()) };
    match format {
        "stl" => {
            need(&ids_all)?;
            let s = unit_scale(settings["unit"].as_str().unwrap_or("mm"))?;
            let ms = meshes(doc, geometry, &ids_all, settings["tolerance"].as_f64(), cancelled)?;
            if settings["angular_deg"].as_f64().is_some_and(|a| (a - 20.).abs() > 1e-9) {
                warnings.push("angular tolerance: the in-process tessellation uses 20°".into());
            }
            let tris: Vec<[[f64; 3]; 3]> = ms.iter().flat_map(|m| m.triangles.iter().map(move |t| t.map(|i| m.vertices_mm[i as usize].map(|v| v * s)))).collect();
            let bytes = if settings["binary"].as_bool().unwrap_or(true) {
                let mut b = vec![0u8; 80];
                b[..20].copy_from_slice(b"sim-cad binary STL  ");
                b.extend((tris.len() as u32).to_le_bytes());
                for t in &tris {
                    let n = normal(t);
                    for v in std::iter::once(n).chain(t.iter().copied()) {
                        for x in v {
                            b.extend((x as f32).to_le_bytes());
                        }
                    }
                    b.extend([0u8, 0]);
                }
                b
            } else {
                let mut out = String::from("solid sim-cad\n");
                for t in &tris {
                    let n = normal(t);
                    let _ = writeln!(out, "  facet normal {} {} {}\n    outer loop", n[0], n[1], n[2]);
                    for v in t {
                        let _ = writeln!(out, "      vertex {} {} {}", v[0], v[1], v[2]);
                    }
                    out.push_str("    endloop\n  endfacet\n");
                }
                out.push_str("endsolid sim-cad\n");
                out.into_bytes()
            };
            write_file(path, &bytes)?;
        }
        "obj" => {
            need(&ids_all)?;
            let ms = meshes(doc, geometry, &ids_all, settings["tolerance"].as_f64(), cancelled)?;
            let s = settings["scale"].as_f64().unwrap_or(1.);
            let y_up = settings["up_axis"].as_str() == Some("Y");
            let mtl = settings["mtl"].as_bool().unwrap_or(true);
            if settings["quads"].as_bool() == Some(true) || settings["ngons"].as_bool() == Some(true) {
                warnings.push("quads and n-gons: written as triangles".into());
            }
            if settings["uvs"].as_bool().unwrap_or(true) {
                warnings.push("UVs: not written (the tessellation has none)".into());
            }
            let stem = path.file_stem().map_or("model".into(), |s| s.to_string_lossy().into_owned());
            let mut obj = String::from("# sim-cad OBJ export\n");
            if mtl {
                let _ = writeln!(obj, "mtllib {stem}.mtl");
            }
            let mut base = 1usize;
            let mut materials = String::new();
            for (m, id) in ms.iter().zip(&ids_all) {
                let nm = name(doc, id).replace(char::is_whitespace, "_");
                let _ = writeln!(obj, "o {nm}");
                if mtl {
                    let _ = writeln!(obj, "usemtl m_{nm}");
                    let c = color(doc, id);
                    let _ = writeln!(materials, "newmtl m_{nm}\nKd {} {} {}\n", c[0], c[1], c[2]);
                }
                for v in &m.vertices_mm {
                    let (x, y, z) = (v[0] * s, v[1] * s, v[2] * s);
                    let _ = if y_up { writeln!(obj, "v {x} {z} {}", -y) } else { writeln!(obj, "v {x} {y} {z}") };
                }
                for t in &m.triangles {
                    let _ = writeln!(obj, "f {} {} {}", base + t[0] as usize, base + t[1] as usize, base + t[2] as usize);
                }
                base += m.vertices_mm.len();
            }
            write_file(path, obj.as_bytes())?;
            if mtl {
                write_file(&path.with_extension("mtl"), materials.as_bytes())?;
            }
        }
        "3mf" => {
            need(&ids_all)?;
            let ms = meshes(doc, geometry, &ids_all, settings["tolerance"].as_f64(), cancelled)?;
            let (colors, names) = (settings["colors"].as_bool().unwrap_or(true), settings["names"].as_bool().unwrap_or(true));
            let mut model = String::from(r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xml:lang="en-US" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><resources>"#);
            if colors {
                model.push_str(r#"<basematerials id="1">"#);
                for id in &ids_all {
                    let c = color(doc, id).map(|v| (v.clamp(0., 1.) * 255.).round() as u8);
                    let _ = write!(model, r##"<base name="{}" displaycolor="#{:02X}{:02X}{:02X}"/>"##, xml(&name(doc, id)), c[0], c[1], c[2]);
                }
                model.push_str("</basematerials>");
            }
            for (k, (m, id)) in ms.iter().zip(&ids_all).enumerate() {
                let oid = k + 2;
                let nm = if names { format!(r#" name="{}""#, xml(&name(doc, id))) } else { String::new() };
                let mat = if colors { format!(r#" pid="1" pindex="{k}""#) } else { String::new() };
                let _ = write!(model, r#"<object id="{oid}" type="model"{nm}{mat}><mesh><vertices>"#);
                for v in &m.vertices_mm {
                    let _ = write!(model, r#"<vertex x="{}" y="{}" z="{}"/>"#, v[0], v[1], v[2]);
                }
                model.push_str("</vertices><triangles>");
                for t in &m.triangles {
                    let _ = write!(model, r#"<triangle v1="{}" v2="{}" v3="{}"/>"#, t[0], t[1], t[2]);
                }
                model.push_str("</triangles></mesh></object>");
            }
            model.push_str("</resources><build>");
            for k in 0..ms.len() {
                let _ = write!(model, r#"<item objectid="{}"/>"#, k + 2);
            }
            model.push_str("</build></model>");
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            let files = [
                ("[Content_Types].xml", r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/></Types>"#.to_string()),
                ("_rels/.rels", r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/></Relationships>"#.to_string()),
                ("3D/3dmodel.model", model),
            ];
            for (n, body) in files {
                zip.start_file(n, o).map_err(|e| e.to_string())?;
                zip.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
            }
            let bytes = zip.finish().map_err(|e| e.to_string())?.into_inner();
            write_file(path, &bytes)?;
        }
        "step" | "iges" => {
            need(&ids_all)?;
            let breps: Vec<Vec<u8>> = ids_all.iter().map(|id| crate::geometry::resolved_brep(doc, id)).collect::<Result<_, _>>()?;
            let refs: Vec<&[u8]> = breps.iter().map(Vec::as_slice).collect();
            if format == "step" {
                if settings["names"].as_bool().unwrap_or(true) || settings["colors"].as_bool().unwrap_or(true) {
                    warnings.push("STEP names and colours: not written (geometry only in process)".into());
                }
                kernel::export(Exchange::Step, &refs, path, settings["schema"].as_str().unwrap_or("AP214"))?;
            } else {
                kernel::export(Exchange::Iges, &refs, path, "")?;
            }
        }
        "svg" => {
            let id = settings["sketch"].as_str().ok_or("the sketch SVG needs settings.sketch")?;
            let n = doc.node(id).ok_or_else(|| format!("no node {id}"))?;
            let s = Sketch::from_json(n.get("sketch").filter(|s| !s.is_null()).ok_or_else(|| format!("{} is not a sketch", name(doc, id)))?)?;
            write_file(path, sketch_svg(&s).as_bytes())?;
        }
        "drawing" => {
            need(&ids_all)?;
            let svg = drawing(doc, &ids_all, settings, cancelled, &mut warnings)?;
            write_file(path, svg.as_bytes())?;
        }
        other => return Err(format!("unknown export format {other}")),
    }
    Ok(json!({"exported": path, "warnings": warnings, "bodies": ids_all.len()}))
}

fn normal(t: &[[f64; 3]; 3]) -> [f64; 3] {
    let (a, b, c) = (t[0], t[1], t[2]);
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l < 1e-30 { [0., 0., 0.] } else { n.map(|x| x / l) }
}
fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// `export_sketch_svg`: the sketch's curves in plane coordinates (v up).
pub fn sketch_svg(s: &Sketch) -> String {
    let polys: Vec<Vec<[f64; 2]>> = s.curves.iter().map(|c| c.sample(64)).collect();
    let pts: Vec<[f64; 2]> = polys.iter().flatten().copied().collect();
    let (x0, x1) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[0]), b.max(p[0])));
    let (y0, y1) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[1]), b.max(p[1])));
    let (x0, y0, w, h) = if pts.is_empty() { (0., 0., 1., 1.) } else { (x0 - 2., y0 - 2., (x1 - x0 + 4.).max(1.), (y1 - y0 + 4.).max(1.)) };
    let mut out = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}mm" height="{h}mm" viewBox="{x0} {} {w} {h}">"#, -(y0 + h));
    for p in polys.iter().filter(|p| p.len() >= 2) {
        let d: Vec<String> = p.iter().map(|q| format!("{:.4} {:.4}", q[0], -q[1])).collect();
        let _ = write!(out, r##"<path d="M {}" fill="none" stroke="#111" stroke-width="0.25"/>"##, d.join(" L "));
    }
    out.push_str("</svg>");
    out
}

/// The drawing's views: (name, eye direction, up), RoboCAD's `STANDARD_VIEWS`
/// (the camera looks along `direction`; the eye is on the opposite side).
fn standard(view: &str) -> Option<(&'static str, [f64; 3], [f64; 3])> {
    let r3 = 1. / 3f64.sqrt();
    Some(match view {
        "front" => ("Front", [0., 1., 0.], [0., 0., 1.]),
        "top" => ("Top", [0., 0., -1.], [0., 1., 0.]),
        "right" => ("Right", [-1., 0., 0.], [0., 0., 1.]),
        "iso" => ("Isometric", [-r3, r3, -r3], [0., 0., 1.]),
        _ => return None,
    })
}

fn polylines2(brep: &[u8]) -> Result<Vec<Vec<[f64; 2]>>, String> {
    let t = kernel::full_topology(brep, 24)?;
    Ok(t["edges"].as_array().into_iter().flatten().map(|e| e["points"].as_array().into_iter().flatten().filter_map(|p| Some([p[0].as_f64()?, p[1].as_f64()?])).collect::<Vec<_>>()).filter(|p| p.len() >= 2).collect())
}

/// `export_drawing_svg` on an A4-landscape sheet: two columns of views with
/// visible and hidden edges, an optional section A-A (hatched) and the title.
fn drawing(doc: &ArchiveDocument, ids: &[String], settings: &Value, cancelled: &dyn Fn() -> bool, warnings: &mut Vec<String>) -> Result<String, String> {
    let views: Vec<String> = settings["views"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_else(|| ["front", "top", "right", "iso"].map(String::from).to_vec());
    let title = settings["title"].as_str().unwrap_or("");
    let section = settings.get("section").filter(|s| !s.is_null()).map(Plane::parse).transpose()?;
    let breps: Vec<Vec<u8>> = ids.iter().map(|id| crate::geometry::resolved_brep(doc, id)).collect::<Result<_, _>>()?;
    let refs: Vec<&[u8]> = breps.iter().map(Vec::as_slice).collect();
    let mut panels: Vec<(String, Vec<Vec<[f64; 2]>>, Vec<Vec<[f64; 2]>>, Vec<Vec<[f64; 2]>>)> = Vec::new();
    for v in &views {
        let (nm, d, up) = standard(v).ok_or_else(|| format!("unknown drawing view {v}"))?;
        let (vis, hid) = project(&refs, d, up, cancelled)?;
        panels.push((nm.to_string(), vis, hid, Vec::new()));
    }
    if let Some(p) = section {
        // Section A-A: the bodies cut (kept below the plane), seen along the plane normal.
        let mut cut = Vec::new();
        let mut hatch3 = Vec::new();
        for b in &refs {
            if let Ok(parts) = kernel::op(Op::SplitPlane, &[b], &[&p.origin[..], &p.normal, &[2.]].concat(), &[], cancelled) {
                cut.extend(parts.into_iter().map(|x| x.brep));
            }
            hatch3.extend(kernel::section(b, p.origin, p.normal, cancelled).unwrap_or_default());
        }
        if cut.is_empty() {
            warnings.push("section A-A: the plane does not cut the bodies".into());
        } else {
            let crefs: Vec<&[u8]> = cut.iter().map(Vec::as_slice).collect();
            let d = p.normal.map(|x| -x);
            let up = p.y_axis();
            let (vis, hid) = project(&crefs, d, up, cancelled)?;
            let (x, y) = frame(d, up);
            let hatch = chain(hatch3).into_iter().map(|l| l.iter().map(|q| [dot(*q, x), dot(*q, y)]).collect()).collect();
            panels.push(("Section A-A".into(), vis, hid, hatch));
        }
    }
    let (sw, sh, margin) = (297.0f64, 210.0f64, 12.0f64);
    let cols = if panels.len() > 1 { 2 } else { 1 };
    let rows = panels.len().div_ceil(cols);
    let (cw, ch) = ((sw - 2. * margin) / cols as f64, (sh - 2. * margin - 10.) / rows as f64);
    let mut out = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{sw}mm" height="{sh}mm" viewBox="0 0 {sw} {sh}">"#);
    let _ = write!(out, r##"<rect x="0" y="0" width="{sw}" height="{sh}" fill="white"/><rect x="{}" y="{}" width="{}" height="{}" fill="none" stroke="#333" stroke-width="0.4"/>"##, margin / 2., margin / 2., sw - margin, sh - margin);
    if !title.is_empty() {
        let _ = write!(out, r#"<text x="{}" y="{}" text-anchor="end" font-family="sans-serif" font-size="4">{}</text>"#, sw - margin, sh - margin / 2. - 2., xml(title));
    }
    out.push_str(r##"<defs><pattern id="hatch" patternUnits="userSpaceOnUse" width="2" height="2" patternTransform="rotate(45)"><line x1="0" y1="0" x2="0" y2="2" stroke="#8b2323" stroke-width="0.25"/></pattern></defs>"##);
    for (i, (nm, vis, hid, hatch)) in panels.iter().enumerate() {
        let pts: Vec<[f64; 2]> = vis.iter().chain(hid).chain(hatch).flatten().copied().collect();
        if pts.is_empty() {
            continue;
        }
        let (x0, x1) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[0]), b.max(p[0])));
        let (y0, y1) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[1]), b.max(p[1])));
        let s = ((cw - 10.) / (x1 - x0).max(1e-6)).min((ch - 12.) / (y1 - y0).max(1e-6));
        let cx = margin + (i % cols) as f64 * cw + cw / 2.;
        let cy = margin + (i / cols) as f64 * ch + ch / 2. + 4.;
        let (ox, oy) = (cx - s * (x0 + x1) / 2., cy + s * (y0 + y1) / 2.);
        let tr = |p: &[f64; 2]| format!("{:.3} {:.3}", ox + s * p[0], oy - s * p[1]);
        let _ = write!(out, r#"<g id="view-{i}"><text x="{cx:.2}" y="{:.2}" text-anchor="middle" font-family="sans-serif" font-size="3">{}  1:{:.2}</text>"#, margin + (i / cols) as f64 * ch + 4., xml(nm), 1. / s);
        for p in hatch.iter().filter(|p| p.len() >= 3) {
            let _ = write!(out, r##"<path d="M {} Z" fill="url(#hatch)" stroke="#8b2323" stroke-width="0.5"/>"##, p.iter().map(tr).collect::<Vec<_>>().join(" L "));
        }
        for p in hid {
            let _ = write!(out, r##"<path d="M {}" fill="none" stroke="#666666" stroke-width="0.25" stroke-dasharray="1.5,1"/>"##, p.iter().map(tr).collect::<Vec<_>>().join(" L "));
        }
        for p in vis {
            let _ = write!(out, r##"<path d="M {}" fill="none" stroke="#111111" stroke-width="0.5" stroke-linecap="round"/>"##, p.iter().map(tr).collect::<Vec<_>>().join(" L "));
        }
        out.push_str("</g>");
    }
    out.push_str("</svg>");
    Ok(out)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn frame(d: [f64; 3], up: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let l = |v: [f64; 3]| dot(v, v).sqrt().max(1e-12);
    let d = d.map(|x| x / l(d));
    let up = if dot(up.map(|x| x / l(up)), d).abs() < 0.99 { up } else { [0., 1., 0.] };
    let c = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    // The HLR projector looks along d with its frame Z = -d: x = d × up puts
    // "up" on +y and the viewer's right on +x.
    let x = c(d, up);
    let x = x.map(|v| v / l(x));
    let y = c(x, d);
    (x, y.map(|v| v / l(y)))
}
/// Visible and hidden 2D polylines of `bodies` seen along `d` (`project_view`).
fn project(bodies: &[&[u8]], d: [f64; 3], up: [f64; 3], cancelled: &dyn Fn() -> bool) -> Result<(Vec<Vec<[f64; 2]>>, Vec<Vec<[f64; 2]>>), String> {
    let (x, _) = frame(d, up);
    let out = kernel::op(Op::Hlr, bodies, &[d, x].concat(), &[], cancelled)?;
    Ok((polylines2(&out[0].brep)?, polylines2(&out.get(1).ok_or("no hidden-line result")?.brep)?))
}
/// `chain_loops`: section pieces joined end to end into loops.
fn chain(polylines: Vec<Vec<[f64; 3]>>) -> Vec<Vec<[f64; 3]>> {
    let close = |a: [f64; 3], b: [f64; 3]| (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4 && (a[2] - b[2]).abs() < 1e-4;
    let mut pieces: Vec<Vec<[f64; 3]>> = polylines.into_iter().filter(|p| p.len() >= 2).collect();
    let mut loops = Vec::new();
    while !pieces.is_empty() {
        let mut cur = pieces.remove(0);
        loop {
            let found = pieces.iter().position(|q| close(cur[cur.len() - 1], q[0]) || close(cur[cur.len() - 1], q[q.len() - 1]) || close(cur[0], q[q.len() - 1]) || close(cur[0], q[0]));
            let Some(i) = found else { break };
            let mut q = pieces.remove(i);
            if close(cur[cur.len() - 1], q[0]) {
                cur.extend(q.into_iter().skip(1));
            } else if close(cur[cur.len() - 1], q[q.len() - 1]) {
                q.reverse();
                cur.extend(q.into_iter().skip(1));
            } else if close(cur[0], q[q.len() - 1]) {
                q.pop();
                q.extend(cur);
                cur = q;
            } else {
                q.reverse();
                q.pop();
                q.extend(cur);
                cur = q;
            }
        }
        loops.push(cur);
    }
    loops
}

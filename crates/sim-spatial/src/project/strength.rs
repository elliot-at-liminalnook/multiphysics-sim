//! Printed-part strength for a project test (`part_strength` criterion):
//! after the run, each printed body of the design is checked by the print
//! registry's layer-aware stress check (`sim_runtime::part_strength`) under
//! loads the run measured.
//!
//! The load cases (stated in the outcome):
//! - **A body carried by a driven joint** (in the joint's child link) is
//!   held around the joint's pivot (the horn: the surface within reach of
//!   the pivot) and loaded at its far end, perpendicular to the arm in the
//!   joint's plane of motion, by the force that gives the run's peak motor
//!   torque about the pivot: the same root bending moment as the measured
//!   load, concentrated at the tip (conservative for shear near the tip).
//! - **A body of the joint's parent link** (the base) is held on its
//!   underside and loaded at the motor's mount by the joint's peak reaction
//!   force (downward) and the peak motor torque about the joint's axis.
//!
//! Bodies that are not printed (metal, bought parts) are not checked;
//! printed bodies of links no driven joint touches are reported as not
//! loaded by the test.
use serde_json::{Value, json};
use sim_print::study::{Load, Magnitude, Region};
use sim_print::voxel::Settings;
use sim_runtime::part_strength::Part;
use std::path::Path;

fn v3(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array().filter(|a| a.len() == 3)?;
    Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?])
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    a.map(|c| c * s)
}
fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: [f64; 3]) -> [f64; 3] {
    let n = norm(a);
    if n < 1e-12 { [0.0, 0.0, -1.0] } else { scale(a, 1.0 / n) }
}

pub(crate) const LOAD_RULE: &str = "a body carried by a driven joint is held at the joint's pivot and loaded at its far end by the force giving the run's peak motor torque about the pivot (the measured root bending moment, concentrated at the tip); a base body is held on its underside and loaded at the motor mount by the joint's peak reaction force and the peak motor torque";

/// Check the design's printed parts against the run in `report`; the
/// criterion's outcome (status, measured, detail).
pub(crate) fn assess(cad: &Path, model: &Value, report: &Value, min: f64, cancelled: &dyn Fn() -> bool) -> (String, Value, String) {
    let not_assessed = |why: String| ("not_assessed".to_string(), Value::Null, why);
    let registry_path = match crate::workspace::path("library/printing/registry.json") {
        Ok(p) => p,
        Err(e) => return not_assessed(format!("no print registry to read layer strengths from: {e}")),
    };
    let registry = match sim_print::registry::load(&registry_path) {
        Ok(r) => r,
        Err(e) => return not_assessed(format!("the print registry could not be read: {e}")),
    };
    let archive = match sim_cad::ArchiveDocument::open(cad) {
        Ok(a) => a,
        Err(e) => return not_assessed(format!("the CAD file could not be read: {e}")),
    };
    let geometry = match sim_cad::geometry::load_geometry(&archive, cancelled, &|_| {}) {
        Ok(g) => g,
        Err(e) => return not_assessed(format!("the CAD geometry could not be read: {e}")),
    };
    let printed = |id: &str| -> Option<String> {
        let n = archive.node(id)?;
        if n["robot"]["kind"] == "motor" {
            return None;
        }
        let mid = n["material"].as_str()?;
        let m = sim_cad::edit::material(&archive.manifest, mid)?;
        m["tags"].as_array()?.iter().any(|t| t == "print").then(|| mid.to_string())
    };
    let links: Vec<&Value> = model["links"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    let link = |name: &str| links.iter().copied().find(|l| l["name"] == name);
    let mut parts: Vec<Part> = Vec::new();
    let mut checked_ids: Vec<String> = Vec::new();
    for j in model["joints"].as_array().into_iter().flatten() {
        let Some(motor_name) = j["motor"].as_str() else { continue };
        let tau = report["results"]["motors"][motor_name]["peak_torque_nm"].as_f64().unwrap_or(0.0);
        let reaction = report["results"]["joints"][j["name"].as_str().unwrap_or("")]["peak_reaction_force_n"].as_f64().unwrap_or(0.0);
        let (Some(origin), Some(axis)) = (v3(&j["origin"]), v3(&j["axis"])) else { continue };
        let pivot = scale(origin, 1000.0);
        let axis = unit(axis);
        // The child link's printed bodies: held at the pivot, loaded at the far end.
        if let Some(child) = j["child"].as_str().and_then(link) {
            for id in child["members"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                let (Some(material), Some(g)) = (printed(id), geometry.iter().find(|g| g.node_id == id)) else { continue };
                let radial = |p: [f64; 3]| {
                    let d = sub(p, pivot);
                    sub(d, scale(axis, dot(d, axis)))
                };
                let Some(far) = g.vertices_mm.iter().copied().max_by(|a, b| norm(radial(*a)).total_cmp(&norm(radial(*b)))) else { continue };
                let lever_mm = norm(radial(far));
                if lever_mm < 1.0 {
                    continue;
                }
                let near = g.vertices_mm.iter().map(|p| norm(sub(*p, pivot))).fold(f64::INFINITY, f64::min);
                let mut dir = unit(cross(axis, unit(radial(far))));
                if dot(dir, [0.0, 0.0, -1.0]) < 0.0 {
                    dir = scale(dir, -1.0);
                }
                let force = tau / (lever_mm * 1e-3);
                let name = archive.node(id).and_then(|n| n["name"].as_str()).unwrap_or(id).to_string();
                parts.push(Part {
                    name: name.clone(),
                    material,
                    vertices_mm: g.vertices_mm.clone(),
                    triangles: g.triangles.clone(),
                    build_direction: v3(&child["print"]["orientation"]).unwrap_or([0.0, 0.0, 1.0]),
                    settings: settings(child),
                    fixture: (format!("{} horn", j["name"].as_str().unwrap_or("joint")), Region::Sphere { center: pivot, radius: (near + 3.0).max(8.0) }),
                    loads: vec![(Load { name: "tip load".into(), region: Region::Sphere { center: far, radius: 6.0 }, direction: dir, magnitude: Magnitude::Newtons(force), moment: None, about: None }, force, format!("{motor_name} peak torque {tau:.4} N·m over a {lever_mm:.1} mm lever"))],
                });
                checked_ids.push(id.to_string());
            }
        }
        // The parent link's printed bodies: held underneath, loaded at the motor mount.
        let mount = model["motors"].as_array().into_iter().flatten().find(|m| m["name"] == motor_name).and_then(|m| v3(&m["mount_point"])).map(|p| scale(p, 1000.0)).unwrap_or(pivot);
        if let Some(parent) = j["parent"].as_str().and_then(link) {
            for id in parent["members"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                if checked_ids.iter().any(|c| c == id) {
                    continue;
                }
                let (Some(material), Some(g)) = (printed(id), geometry.iter().find(|g| g.node_id == id)) else { continue };
                let zmin = g.vertices_mm.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
                let near = g.vertices_mm.iter().map(|p| norm(sub(*p, mount))).fold(f64::INFINITY, f64::min);
                let name = archive.node(id).and_then(|n| n["name"].as_str()).unwrap_or(id).to_string();
                let force = reaction.max(1e-3);
                parts.push(Part {
                    name,
                    material,
                    vertices_mm: g.vertices_mm.clone(),
                    triangles: g.triangles.clone(),
                    build_direction: v3(&parent["print"]["orientation"]).unwrap_or([0.0, 0.0, 1.0]),
                    settings: settings(parent),
                    fixture: ("underside".into(), Region::Below { axis: [0.0, 0.0, 1.0], height: zmin + 1.0 }),
                    loads: vec![(Load { name: "motor mount".into(), region: Region::Sphere { center: mount, radius: (near + 3.0).max(12.0) }, direction: [0.0, 0.0, -1.0], magnitude: Magnitude::Newtons(force), moment: Some(scale(axis, tau)), about: Some(mount) }, force, format!("{} peak reaction {reaction:.3} N and {motor_name} peak torque {tau:.4} N·m", j["name"].as_str().unwrap_or("joint")))],
                });
                checked_ids.push(id.to_string());
            }
        }
    }
    // Printed bodies no driven joint loads.
    let unloaded: Vec<String> = geometry.iter().filter(|g| printed(&g.node_id).is_some() && !checked_ids.contains(&g.node_id)).filter_map(|g| archive.node(&g.node_id).and_then(|n| n["name"].as_str()).map(str::to_string)).collect();
    if parts.is_empty() {
        return not_assessed(if unloaded.is_empty() { "the design has no printed part (a material tagged print) to check".into() } else { format!("no driven joint loads the printed parts ({})", unloaded.join(", ")) });
    }
    let results = sim_runtime::part_strength::check(&registry, &parts, cancelled);
    let assessed: Vec<&Value> = results.iter().filter(|r| r["assessed"] == json!(true)).collect();
    let missing: Vec<String> = results.iter().filter(|r| r["assessed"] != json!(true)).map(|r| format!("{}: {}", r["part"].as_str().unwrap_or(""), r["why"].as_str().unwrap_or(""))).collect();
    let worst = assessed.iter().filter_map(|r| r["safety_factor"].as_f64().map(|sf| (r["part"].as_str().unwrap_or("").to_string(), sf, r["mode"].as_str().unwrap_or("").to_string()))).min_by(|a, b| a.1.total_cmp(&b.1));
    let lines: Vec<String> = assessed.iter().map(|r| format!("{} {:.1} ({})", r["part"].as_str().unwrap_or(""), r["safety_factor"].as_f64().unwrap_or(f64::NAN), r["mode"].as_str().unwrap_or(""))).collect();
    let measured = json!({"parts": results, "min_safety_factor": min, "load_rule": LOAD_RULE, "not_loaded_by_the_test": unloaded});
    let tail = format!("{}{}. Loads: {LOAD_RULE}; filament strengths are the registry's (estimated unless measured).", if missing.is_empty() { String::new() } else { format!("; not assessed: {}", missing.join("; ")) }, if unloaded.is_empty() { String::new() } else { format!("; not loaded by this test: {}", unloaded.join(", ")) });
    match worst {
        Some((part, sf, mode)) if sf < min => ("fail".into(), measured, format!("{part} has a safety factor of {sf:.2} ({mode}), below {min}: {}{tail}", lines.join(", "))),
        Some(_) if !missing.is_empty() => ("not_assessed".into(), measured, format!("safety factors {}{tail}", lines.join(", "))),
        Some(_) => ("pass".into(), measured, format!("safety factors {}{tail}", lines.join(", "))),
        None => ("not_assessed".into(), measured, format!("no part could be checked{tail}")),
    }
}

fn settings(link: &Value) -> Settings {
    let mut s = Settings::default();
    if let Some(i) = link["print"]["infill"].as_f64() {
        s.infill = i.clamp(0.0, 1.0);
    }
    if let Some(w) = link["print"]["walls"].as_u64() {
        s.walls = w as u32;
    }
    s
}

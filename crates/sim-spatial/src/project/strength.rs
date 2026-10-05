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
//! - **A body of a joint's parent link** is loaded at that joint's motor
//!   mount by the joint's peak reaction force (downward) and the peak motor
//!   torque about the joint's axis: once for every driven joint it carries.
//!   A body no joint drives (the base) is held on its underside; a link
//!   that is driven and also carries a further joint keeps its horn
//!   fixture and gets both loads (which overstates: the tip load already
//!   stands for everything beyond the pivot).
//!
//! A load the run did not record (no peak torque or reaction for the joint)
//! leaves its body not assessed; a body whose loads are all zero is
//! reported as not loaded by the test. Neither is ever a pass.
//!
//! Each body is checked with its own print settings from the CAD file
//! (build direction, infill, walls: `make::print_settings`, the same the
//! part list states); a setting the design does not state is named as
//! assumed in the outcome.
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

pub(crate) const LOAD_RULE: &str = "a body carried by a driven joint is held at the joint's pivot and loaded at its far end by the force giving the run's peak motor torque about the pivot (the measured root bending moment, concentrated at the tip); a body is also loaded at the motor mount of every driven joint it carries by that joint's peak reaction force and peak motor torque (a body no joint drives is held on its underside)";

/// Loads below these are no load at all (N, N·m).
const NO_FORCE: f64 = 1e-6;
const NO_MOMENT: f64 = 1e-9;

/// One printed body being loaded.
struct Loaded<'a> {
    id: String,
    name: String,
    material: String,
    geometry: &'a sim_cad::geometry::BodyGeometry,
    fixture: Option<(String, Region)>,
    loads: Vec<(Load, f64, String)>,
    /// Loads the run did not record.
    missing: Vec<String>,
}

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
    let node_name = |id: &str| archive.node(id).and_then(|n| n["name"].as_str()).unwrap_or(id).to_string();
    let mut bodies: Vec<Loaded> = Vec::new();
    fn body<'b, 'a>(bodies: &'b mut Vec<Loaded<'a>>, id: &str, name: String, material: String, geometry: &'a sim_cad::geometry::BodyGeometry) -> &'b mut Loaded<'a> {
        let at = bodies.iter().position(|b| b.id == id).unwrap_or_else(|| {
            bodies.push(Loaded { id: id.to_string(), name, material, geometry, fixture: None, loads: Vec::new(), missing: Vec::new() });
            bodies.len() - 1
        });
        &mut bodies[at]
    }
    struct Driven<'v> {
        joint: &'v Value,
        name: String,
        motor: String,
        tau: Option<f64>,
        reaction: Option<f64>,
        pivot: [f64; 3],
        axis: [f64; 3],
    }
    let driven: Vec<Driven> = model["joints"].as_array().into_iter().flatten().filter_map(|j| {
        let motor = j["motor"].as_str()?.to_string();
        let name = j["name"].as_str().unwrap_or("joint").to_string();
        let (origin, axis) = (v3(&j["origin"])?, v3(&j["axis"])?);
        Some(Driven { joint: j, tau: report["results"]["motors"][&motor]["peak_torque_nm"].as_f64(), reaction: report["results"]["joints"][&name]["peak_reaction_force_n"].as_f64(), name, motor, pivot: scale(origin, 1000.0), axis: unit(axis) })
    }).collect();
    // Bodies a joint drives: held at its pivot, loaded at the far end.
    for d in &driven {
        let Some(child) = d.joint["child"].as_str().and_then(link) else { continue };
        for id in child["members"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            let (Some(material), Some(g)) = (printed(id), geometry.iter().find(|g| g.node_id == id)) else { continue };
            let radial = |p: [f64; 3]| {
                let r = sub(p, d.pivot);
                sub(r, scale(d.axis, dot(r, d.axis)))
            };
            let Some(far) = g.vertices_mm.iter().copied().max_by(|a, b| norm(radial(*a)).total_cmp(&norm(radial(*b)))) else { continue };
            let lever_mm = norm(radial(far));
            if lever_mm < 1.0 {
                continue;
            }
            let near = g.vertices_mm.iter().map(|p| norm(sub(*p, d.pivot))).fold(f64::INFINITY, f64::min);
            let mut dir = unit(cross(d.axis, unit(radial(far))));
            if dot(dir, [0.0, 0.0, -1.0]) < 0.0 {
                dir = scale(dir, -1.0);
            }
            let b = body(&mut bodies, id, node_name(id), material, g);
            if b.fixture.is_none() {
                b.fixture = Some((format!("{} horn", d.name), Region::Sphere { center: d.pivot, radius: (near + 3.0).max(8.0) }));
            }
            match d.tau {
                Some(tau) => {
                    let force = tau / (lever_mm * 1e-3);
                    b.loads.push((Load { name: format!("{} tip load", d.name), region: Region::Sphere { center: far, radius: 6.0 }, direction: dir, magnitude: Magnitude::Newtons(force), moment: None, about: None }, force, format!("{} peak torque {tau:.4} N·m over a {lever_mm:.1} mm lever", d.motor)));
                }
                None => b.missing.push(format!("the run recorded no peak torque for {}", d.motor)),
            }
        }
    }
    // Bodies that carry a joint: loaded at its motor mount.
    for d in &driven {
        let Some(parent) = d.joint["parent"].as_str().and_then(link) else { continue };
        let mount = model["motors"].as_array().into_iter().flatten().find(|m| m["name"] == d.motor.as_str()).and_then(|m| v3(&m["mount_point"])).map(|p| scale(p, 1000.0)).unwrap_or(d.pivot);
        for id in parent["members"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            let (Some(material), Some(g)) = (printed(id), geometry.iter().find(|g| g.node_id == id)) else { continue };
            let zmin = g.vertices_mm.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
            let near = g.vertices_mm.iter().map(|p| norm(sub(*p, mount))).fold(f64::INFINITY, f64::min);
            let b = body(&mut bodies, id, node_name(id), material, g);
            if b.fixture.is_none() {
                b.fixture = Some(("underside".into(), Region::Below { axis: [0.0, 0.0, 1.0], height: zmin + 1.0 }));
            }
            match (d.reaction, d.tau) {
                (Some(reaction), Some(tau)) => b.loads.push((Load { name: format!("{} motor mount", d.name), region: Region::Sphere { center: mount, radius: (near + 3.0).max(12.0) }, direction: [0.0, 0.0, -1.0], magnitude: Magnitude::Newtons(reaction), moment: Some(scale(d.axis, tau)), about: Some(mount) }, reaction, format!("{} peak reaction {reaction:.3} N and {} peak torque {tau:.4} N·m", d.name, d.motor))),
                _ => b.missing.push(format!("the run recorded no peak reaction force or torque for {}", d.name)),
            }
        }
    }
    let touched: Vec<String> = bodies.iter().map(|b| b.id.clone()).collect();
    // Printed bodies no driven joint loads, or whose loads were all zero in this run.
    let mut unloaded: Vec<String> = geometry.iter().filter(|g| printed(&g.node_id).is_some() && !touched.contains(&g.node_id)).map(|g| node_name(&g.node_id)).collect();
    let mut unrecorded: Vec<String> = Vec::new();
    let mut assumed: Vec<String> = Vec::new();
    let mut parts: Vec<Part> = Vec::new();
    for b in bodies {
        if !b.missing.is_empty() {
            unrecorded.push(format!("{}: {}", b.name, b.missing.join("; ")));
            continue;
        }
        let loaded = b.loads.iter().any(|(l, n, _)| n.abs() > NO_FORCE || l.moment.is_some_and(|m| norm(m) > NO_MOMENT));
        let (Some(fixture), true) = (b.fixture, loaded) else {
            unloaded.push(b.name);
            continue;
        };
        let print = super::make::print_settings(archive.node(&b.id).unwrap_or(&Value::Null));
        if !print.assumed.is_empty() {
            assumed.push(format!("{}: {}", b.name, print.assumed.join(", ")));
        }
        parts.push(Part { name: b.name, material: b.material, vertices_mm: b.geometry.vertices_mm.clone(), triangles: b.geometry.triangles.clone(), build_direction: print.orientation, settings: Settings { infill: print.infill, walls: print.walls, ..Settings::default() }, fixture, loads: b.loads });
    }
    if parts.is_empty() {
        let why = if !unrecorded.is_empty() {
            format!("the loads could not be read from the run ({})", unrecorded.join("; "))
        } else if unloaded.is_empty() {
            "the design has no printed part (a material tagged print) to check".into()
        } else {
            format!("this test puts no load on the printed parts ({})", unloaded.join(", "))
        };
        return not_assessed(why);
    }
    let results = sim_runtime::part_strength::check(&registry, &parts, cancelled);
    let assessed: Vec<&Value> = results.iter().filter(|r| r["assessed"] == json!(true)).collect();
    let mut missing: Vec<String> = results.iter().filter(|r| r["assessed"] != json!(true)).map(|r| format!("{}: {}", r["part"].as_str().unwrap_or(""), r["why"].as_str().unwrap_or(""))).collect();
    missing.extend(unrecorded);
    // A check that was cancelled part-way judged only some of the parts.
    if results.len() < parts.len() {
        missing.push(format!("{} part(s) were not reached before the check stopped", parts.len() - results.len()));
    }
    let worst = assessed.iter().filter_map(|r| r["safety_factor"].as_f64().map(|sf| (r["part"].as_str().unwrap_or("").to_string(), sf, r["mode"].as_str().unwrap_or("").to_string()))).min_by(|a, b| a.1.total_cmp(&b.1));
    let lines: Vec<String> = assessed.iter().map(|r| format!("{} {:.1} ({})", r["part"].as_str().unwrap_or(""), r["safety_factor"].as_f64().unwrap_or(f64::NAN), r["mode"].as_str().unwrap_or(""))).collect();
    let measured = json!({"parts": results, "min_safety_factor": min, "load_rule": LOAD_RULE, "not_loaded_by_the_test": unloaded, "not_assessed": missing, "assumed_print_settings": assumed, "print_registry_sha256": registry.sha256});
    let tail = format!("{}{}{}. Loads: {LOAD_RULE}; filament strengths are the registry's (estimated unless measured).",
        if missing.is_empty() { String::new() } else { format!("; not assessed: {}", missing.join("; ")) },
        if unloaded.is_empty() { String::new() } else { format!("; not loaded by this test: {}", unloaded.join(", ")) },
        if assumed.is_empty() { String::new() } else { format!("; print settings the design does not state were assumed ({})", assumed.join("; ")) });
    match worst {
        Some((part, sf, mode)) if sf < min => ("fail".into(), measured, format!("{part} has a safety factor of {sf:.2} ({mode}), below {min}: {}{tail}", lines.join(", "))),
        Some(_) if !missing.is_empty() => ("not_assessed".into(), measured, format!("safety factors {}{tail}", lines.join(", "))),
        Some(_) => ("pass".into(), measured, format!("safety factors {}{tail}", lines.join(", "))),
        None => ("not_assessed".into(), measured, format!("no part could be checked{tail}")),
    }
}

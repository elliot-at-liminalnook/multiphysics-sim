//! The physical model of a CAD document (`*.simrobot.json`, schema 4), made
//! in process: RoboCAD's `export_physical_model` (physical.py) on the
//! archive, its exact geometry and its exact mass results. One rule set
//! with explicit derivations:
//!
//! - **Links.** Every body is a link unless a fixed joint or a motor's
//!   `mounted_on` merges it into another (`link_groups`). Mass, centre of
//!   mass and inertia are the members' exact B-rep results
//!   ([`crate::mass`]) combined by the parallel-axis theorem; the collision
//!   surface is the members' tessellation ([`collision`]).
//! - **Joints.** Each joint node becomes a joint between the links of its
//!   bodies. Its bearing physics (pin and hole radii, clearance, friction
//!   under the outboard weight, wall stiffness) is estimated from the motor
//!   shaft or a 2 mm printed pin, as RoboCAD does when it finds no
//!   cylinder pair; the CAD's `robot.physics` overrides
//!   (`set_joint_physics`) replace it. Drive backlash is unmeasured until
//!   authored. Merged fixed joints are listed in `source.merged`.
//! - **Motors.** Library actuators with their datasheet or derived
//!   electrical, gearbox, thermal, firmware and driver blocks ([`motors`]).
//! - **Everything else** RoboCAD writes: sensors, cables, battery, control,
//!   world, uncertainty, identification, transmissions, materials with
//!   engineering properties ([`crate::materials`]).
//!
//! Every value that is a guess rather than a CAD derivation or a
//! measurement is listed in `source.assumptions`, each with where it is,
//! what was assumed, whether it blocks evidence, and how to resolve it.
//! Not ported (listed in `source.not_modelled`): flexible links (every link
//! is rigid), the signed distance grid for link-to-link contact, and bearing
//! pin/hole inference from cylinder pairs.
pub mod collision;
pub mod motors;

use crate::archive::ArchiveDocument;
use crate::geometry::BodyGeometry;
use crate::mass::MassResults;
use crate::materials;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub const SCHEMA_VERSION: u32 = 4;
const MM: f64 = 1e-3;
const G: f64 = 9.81;
/// RoboCAD's `_DEFAULT_WALL` (m: three perimeters at 0.4 mm plus infill).
const DEFAULT_WALL: f64 = 2.4e-3;
/// RoboCAD's `_DEFAULT_SERVO_HORN_CLEARANCE` (m).
const SERVO_HORN_CLEARANCE: f64 = 0.05e-3;
pub const EXPORTER: &str = "sim_cad::physical (in-process Rust port of RoboCAD export_physical_model)";
pub const NOT_MODELLED: [&str; 3] = [
    "flexible links: every link is rigid (RoboCAD's modal reduction is not ported)",
    "link-to-link contact: no signed distance grid is built (floor contact uses the collision surface)",
    "bearing inference from cylinder pairs: joint bearings are estimated from the motor shaft or a 2 mm printed pin unless set in CAD",
];

/// Export choices.
#[derive(Default)]
pub struct Options<'a> {
    /// RoboCAD's planar hint (normal, origin mm), for the simulation model.
    pub planar: Option<([f64; 3], [f64; 3])>,
    /// The print registry for filament engineering values.
    pub registry: Option<materials::Registry<'a>>,
    /// When the export was made (written as given).
    pub exported_at: String,
}

/// One guess in the model (see the module doc).
#[derive(Clone, Debug, PartialEq)]
pub struct Assumption {
    /// Where in the model, e.g. `joints[shoulder].physics.drive_backlash`.
    pub at: String,
    pub what: String,
    /// `unmeasured` (no value; blocks evidence), `estimated` (a stated
    /// estimate), `default` (a catalogue or rule-of-thumb value).
    pub status: &'static str,
    pub blocking: bool,
    /// How to resolve it (in CAD).
    pub fix: String,
}
impl Assumption {
    fn json(&self) -> Value {
        json!({"at": self.at, "what": self.what, "status": self.status, "blocking": self.blocking, "fix": self.fix})
    }
}

fn v3(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array().filter(|a| a.len() == 3)?;
    Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?])
}
fn unit(a: [f64; 3]) -> [f64; 3] {
    let n = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if n < 1e-12 { [0.0, 0.0, 1.0] } else { a.map(|c| c / n) }
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn norm(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
fn to_m(p: [f64; 3]) -> [f64; 3] {
    p.map(|c| c * MM)
}

/// A joint record (a `joint` node, by body ids).
struct JointRec<'a> {
    id: String,
    name: String,
    ty: String,
    parent: Option<String>,
    child: String,
    pivot_mm: [f64; 3],
    axis: [f64; 3],
    lower: Option<f64>,
    upper: Option<f64>,
    motor: Option<String>,
    gear_ratio: f64,
    home: f64,
    node: &'a Value,
}

/// A link being made.
struct LinkRec {
    id: String,
    name: String,
    material: String,
    mass: f64,
    com: [f64; 3],
}

/// RoboCAD's `default_settings`, with the document's `robot_settings` over them.
fn settings(doc: &ArchiveDocument) -> Map<String, Value> {
    let mut d = json!({
        "battery": null,
        "control": {"period_s": 0.02, "latency_s": 0.004, "targets": {}, "mode": "hold", "trajectory": []},
        "uncertainty": {"dimension_m": {"sigma": 0.15e-3}, "mass": {"sigma_fraction": 0.05}, "friction": {"sigma_fraction": 0.2}, "stiffness": {"sigma_fraction": 0.15}, "backlash": {"sigma_fraction": 0.3}, "motor_torque": {"sigma_fraction": 0.1}, "com_m": {"sigma": 0.5e-3}, "seed": 0},
        "world": {"floor_z": null, "floor_material": "world", "floor_stiffness": 2.0e5, "floor_damping": 2.0e3, "terrain": null, "ambient_c": 20.0},
        "identification": {},
    });
    let d = d.as_object_mut().expect("an object");
    if let Some(s) = doc.manifest["robot_settings"].as_object() {
        for (k, v) in s {
            match (d.get_mut(k).and_then(Value::as_object_mut), v.as_object()) {
                (Some(old), Some(new)) => old.extend(new.clone()),
                _ => {
                    d.insert(k.clone(), v.clone());
                }
            }
        }
    }
    d.clone()
}

/// The joint nodes reachable from the roots (RoboCAD's `doc.walk()` order).
fn walk(doc: &ArchiveDocument) -> Vec<&Value> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut stack: Vec<String> = doc.manifest["roots"].as_array().into_iter().flatten().rev().filter_map(|v| v.as_str().map(str::to_string)).collect();
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        if let Some(n) = doc.node(&id) {
            out.push(n);
            stack.extend(n["children"].as_array().into_iter().flatten().rev().filter_map(|v| v.as_str().map(str::to_string)));
        }
    }
    out
}

/// The physical model of `doc` (see the module doc). `geometry` and
/// `masses` are the document's current exact geometry and mass results.
/// Errors name what in the CAD prevents a model (a joint on a missing
/// body, two links of one name, an unknown motor).
pub fn export(doc: &ArchiveDocument, geometry: &[BodyGeometry], masses: &MassResults, opts: &Options) -> Result<Value, String> {
    let mut assumptions: Vec<Assumption> = Vec::new();
    let nodes = walk(doc);
    let name_of = |id: &str| doc.node(id).and_then(|n| n["name"].as_str()).unwrap_or(id).to_string();
    let geo: HashMap<&str, &BodyGeometry> = geometry.iter().map(|g| (g.node_id.as_str(), g)).collect();
    // Bodies: reachable `body` nodes with mass results (hidden and disabled included, as RoboCAD's doc.bodies()).
    let bodies: Vec<&Value> = nodes.iter().copied().filter(|n| n["kind"] == "body" && n["id"].as_str().is_some_and(|id| masses.bodies.contains_key(id))).collect();
    if bodies.is_empty() {
        return Err("the document has no solid bodies to simulate".into());
    }
    let body_ids: BTreeSet<&str> = bodies.iter().filter_map(|n| n["id"].as_str()).collect();
    let doc_materials: Vec<&Value> = doc.manifest["materials"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    let material_ids: BTreeSet<&str> = doc_materials.iter().filter_map(|m| m["id"].as_str()).collect();
    let library: BTreeMap<String, crate::robotics::MotorSpec> = crate::robotics::library().into_iter().map(|m| (m.id.clone(), m)).collect();

    // Joints.
    let mut joints: Vec<JointRec> = Vec::new();
    for n in nodes.iter().filter(|n| n["kind"] == "joint" && n["joint"].is_object()) {
        let j = &n["joint"];
        let id = n["id"].as_str().unwrap_or("").to_string();
        let name = n["name"].as_str().unwrap_or("joint").to_string();
        let child = j["child"].as_str().unwrap_or("");
        if !body_ids.contains(child) {
            return Err(format!("joint `{name}`: its child body is missing (reconnect it to a body)"));
        }
        let parent = j["parent"].as_str().map(str::to_string);
        if let Some(p) = &parent
            && !body_ids.contains(p.as_str())
        {
            return Err(format!("joint `{name}`: its parent body is missing (reconnect it to a body)"));
        }
        let axis = v3(&j["axis"]).unwrap_or([0.0, 0.0, 1.0]);
        if norm(axis) < 1e-12 {
            return Err(format!("joint `{name}`: zero axis"));
        }
        joints.push(JointRec {
            id,
            name,
            ty: j["type"].as_str().unwrap_or("revolute").to_string(),
            parent,
            child: child.to_string(),
            pivot_mm: v3(&j["pivot"]).ok_or_else(|| format!("joint `{}`: no pivot point", n["name"].as_str().unwrap_or("joint")))?,
            axis: unit(axis),
            lower: j["lower"].as_f64(),
            upper: j["upper"].as_f64(),
            motor: j["motor"].as_str().map(str::to_string),
            gear_ratio: j["gear_ratio"].as_f64().unwrap_or(1.0),
            home: j["home"].as_f64().unwrap_or(0.0),
            node: n,
        });
    }

    // Link groups: fixed-joint children and mounted motors collapse into their parent.
    let mut into: HashMap<String, String> = HashMap::new();
    for j in &joints {
        if j.ty == "fixed"
            && let Some(p) = &j.parent
        {
            into.insert(j.child.clone(), p.clone());
        }
    }
    for b in &bodies {
        let id = b["id"].as_str().unwrap_or("");
        if b["robot"]["kind"] == "motor"
            && let Some(m) = b["robot"]["mounted_on"].as_str()
            && body_ids.contains(m)
            && !into.contains_key(id)
        {
            into.insert(id.to_string(), m.to_string());
        }
    }
    let resolve = |mut i: String| {
        let mut seen = BTreeSet::new();
        while let Some(next) = into.get(&i) {
            if !seen.insert(i.clone()) {
                break;
            }
            i = next.clone();
        }
        i
    };
    let group: BTreeMap<String, String> = body_ids.iter().map(|b| (b.to_string(), resolve(b.to_string()))).collect();

    // Links (in body order), with exact mass properties.
    let mut links: Vec<LinkRec> = Vec::new();
    let mut out_links: Vec<Value> = Vec::new();
    let mut used_materials: BTreeSet<String> = BTreeSet::new();
    for root in &bodies {
        let lid = root["id"].as_str().unwrap_or("").to_string();
        if group[&lid] != lid {
            continue;
        }
        let members: Vec<&Value> = bodies.iter().copied().filter(|b| b["id"].as_str().is_some_and(|i| group[i] == lid)).collect();
        let ids: Vec<String> = members.iter().map(|m| m["id"].as_str().unwrap_or("").to_string()).collect();
        let results: Vec<&crate::mass::MassResult> = ids.iter().map(|i| &masses.bodies[i]).collect();
        let mass: f64 = results.iter().map(|r| r.mass_kg).sum();
        let com = if mass > 0.0 { [0, 1, 2].map(|k| results.iter().map(|r| r.mass_kg * r.centroid_m[k]).sum::<f64>() / mass) } else { results[0].centroid_m };
        let mut inertia = [[0.0; 3]; 3];
        for r in &results {
            let d = sub(r.centroid_m, com);
            let dd = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
            for a in 0..3 {
                for b in 0..3 {
                    inertia[a][b] += r.inertia_kg_m2[a][b] + r.mass_kg * (if a == b { dd } else { 0.0 } - d[a] * d[b]);
                }
            }
        }
        let name = root["name"].as_str().unwrap_or("link").to_string();
        if links.iter().any(|l| l.name == name) {
            return Err(format!("two links are both named `{name}`: joints refer to links by name, so rename one in CAD"));
        }
        let root_material = root["material"].as_str().filter(|m| material_ids.contains(m));
        let material = root_material.unwrap_or("pla").to_string();
        if root_material.is_none() {
            assumptions.push(Assumption { at: format!("links[{name}].material"), what: "the link's body has no material: its friction and strength are PLA's".into(), status: "default", blocking: false, fix: format!("set a material on `{name}` in CAD (Materials)") });
        }
        for (m, r) in members.iter().zip(&results) {
            if r.provenance["density_default"] == json!(true) {
                let body = m["name"].as_str().unwrap_or("body");
                assumptions.push(Assumption { at: format!("links[{name}].mass"), what: format!("`{body}` has no material, so its mass uses a default density"), status: "default", blocking: true, fix: format!("set a material on `{body}` in CAD, or declare its measured mass") });
            }
            if let Some(mid) = m["material"].as_str() {
                used_materials.insert(mid.to_string());
            }
            used_materials.extend(m["robot"]["solid_materials"].as_object().into_iter().flatten().filter_map(|(_, v)| v.as_str().map(str::to_string)));
        }
        used_materials.insert(material.clone());
        // Bounds of the members' surfaces about the COM.
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        let member_geo: Vec<&BodyGeometry> = ids.iter().filter_map(|i| geo.get(i.as_str()).copied()).collect();
        for g in &member_geo {
            for p in &g.vertices_mm {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k] * MM - com[k]);
                    hi[k] = hi[k].max(p[k] * MM - com[k]);
                }
            }
        }
        if !lo[0].is_finite() {
            lo = [0.0; 3];
            hi = [0.0; 3];
        }
        let robot = &root["robot"];
        let ground = members.iter().any(|m| {
            let n = m["name"].as_str().unwrap_or("").to_lowercase();
            n == "ground" || n.split_whitespace().any(|w| w == "ground") || m["robot"]["ground"].as_bool() == Some(true)
        });
        out_links.push(json!({
            "name": name, "id": lid, "members": ids, "member_names": members.iter().map(|m| m["name"].clone()).collect::<Vec<_>>(),
            "material": material,
            "mass_sources": ids.iter().zip(&results).map(|(i, r)| (i.clone(), r.source.clone())).collect::<Map<String, Value>>(),
            "ground": ground, "mass": mass, "com": com, "inertia": inertia, "bbox": [lo, hi],
            "collision": collision::block(&member_geo, com),
            "print": {"orientation": v3(&robot["print_orientation"]).unwrap_or([0.0, 0.0, 1.0]), "infill": robot["infill"].as_f64().unwrap_or(0.3), "walls": robot["walls"].as_u64().unwrap_or(3), "layer_height": 0.2e-3},
            "flex": null,
        }));
        links.push(LinkRec { id: lid, name, material, mass, com });
    }
    let link_of = |body: &str| -> Option<usize> { links.iter().position(|l| l.id == group[body]) };

    // Engineering properties of every material the model uses.
    let mut eng: Map<String, Value> = Map::new();
    for m in &doc_materials {
        if let Some(id) = m["id"].as_str() {
            eng.insert(id.to_string(), materials::engineering(m, opts.registry.as_ref()));
        }
    }
    let st = settings(doc);
    let floor_material = st["world"]["floor_material"].as_str().unwrap_or("world").to_string();

    // Joints out.
    let mut out_joints: Vec<Value> = Vec::new();
    let mut merged: Vec<Value> = Vec::new();
    let motor_spec = |motor: &str| -> Result<(&Value, &crate::robotics::MotorSpec), String> {
        let n = doc.node(motor).filter(|n| n["robot"]["kind"] == "motor").ok_or_else(|| format!("motor {motor} is missing"))?;
        let spec_id = n["robot"]["spec"].as_str().unwrap_or("");
        let spec = library.get(spec_id).ok_or_else(|| format!("motor `{}`: `{spec_id}` is not in the motor library", n["name"].as_str().unwrap_or(motor)))?;
        Ok((n, spec))
    };
    for j in &joints {
        let child_link = link_of(&j.child).ok_or_else(|| format!("joint `{}`: its child is not a link", j.name))?;
        let parent_link = match &j.parent {
            Some(p) => Some(link_of(p).ok_or_else(|| format!("joint `{}`: its parent is not a link", j.name))?),
            None => None,
        };
        if j.ty == "fixed" {
            merged.push(json!({"joint": j.name, "id": j.id, "body": name_of(&j.child), "merged_into": links[child_link].name}));
            continue;
        }
        if Some(child_link) == parent_link {
            return Err(format!("joint `{}` joins two bodies that a fixed joint or motor mount already merged into `{}`", j.name, links[child_link].name));
        }
        // Everything outboard of the joint (non-fixed, non-loop tree edges).
        let mut out = vec![child_link];
        let mut frontier = vec![child_link];
        while let Some(b) = frontier.pop() {
            for k in &joints {
                if k.ty.starts_with("loop_") || k.ty == "fixed" {
                    continue;
                }
                let p = k.parent.as_deref().and_then(link_of);
                let c = link_of(&k.child);
                if p == Some(b)
                    && let Some(c) = c
                    && !out.contains(&c)
                {
                    out.push(c);
                    frontier.push(c);
                }
            }
        }
        let outboard_mass: f64 = out.iter().map(|&l| links[l].mass).sum();
        let outboard_com = [0, 1, 2].map(|k| out.iter().map(|&l| links[l].mass * links[l].com[k]).sum::<f64>() / outboard_mass.max(1e-12));
        let motor = match &j.motor {
            Some(m) => Some(motor_spec(m)?),
            None => None,
        };
        let pivot = to_m(j.pivot_mm);
        let (pin_r, kind, pin_mat) = match motor {
            Some((_, spec)) => (0.5 * spec.shaft_diameter * MM, if spec.kind == "servo" { "servo_horn" } else { "printed_pin" }, "steel".to_string()),
            None => (2.0e-3, "printed_pin", links[child_link].material.clone()),
        };
        let hole_mat = parent_link.map(|p| links[p].material.clone()).unwrap_or_else(|| "steel".into());
        let hole_r = pin_r + if kind == "servo_horn" { SERVO_HORN_CLEARANCE } else { 0.15e-3 };
        let contact = 6.0e-3;
        let clearance = (hole_r - pin_r).max(0.0);
        let lever = if outboard_mass > 0.0 { norm(sub(outboard_com, pivot)) } else { 0.0 };
        let (mu_s, mu_k) = materials::friction_pair(&eng, Some(&hole_mat), Some(&pin_mat));
        let n_load = outboard_mass * G;
        let hole_props = eng.get(&hole_mat).or_else(|| eng.get("pla"));
        let e_mod = hole_props.and_then(|p| p["youngs_modulus"].as_f64()).unwrap_or(3.5e9);
        let bearing_allow = hole_props.and_then(|p| p["bearing_pressure"].as_f64()).unwrap_or(15e6);
        let mut radial = e_mod * contact * DEFAULT_WALL / hole_r.max(1e-4);
        if kind == "servo_horn" {
            radial *= 4.0; // a steel spline in a plastic horn is stiffer than a printed pin
        }
        let mut friction = json!({"coulomb": mu_k * n_load * pin_r, "viscous": 2.0e-4 * (pin_r / 2.0e-3), "stribeck": ((mu_s - mu_k) * n_load * pin_r).max(0.0), "stribeck_speed": 0.1, "static_ratio": 1.0});
        if j.ty == "prismatic" {
            friction = json!({"coulomb": mu_k * n_load, "viscous": 5.0, "stribeck": ((mu_s - mu_k) * n_load).max(0.0), "stribeck_speed": 0.01, "static_ratio": 1.0});
        }
        let backlash = clearance / lever.max(5.0e-3);
        let mut phys = json!({
            "source": "estimated", "pin_radius": pin_r, "hole_radius": hole_r, "contact_length": contact,
            "clearance": clearance, "backlash": backlash, "bearing_clearance_angle_rad": backlash, "wobble": (2.0 * clearance).atan2(contact.max(1.0e-4)),
            "drive_backlash": {"width_rad": null, "provenance": "unmeasured", "reference": "Radial bearing clearance does not determine drive-connection rotational lost motion"},
            "friction": friction,
            "stiffness": {"radial": radial, "axial": 0.5 * radial, "bending": radial * contact * contact / 12.0}, "damping_ratio": 0.05,
            "bearing": {"kind": kind, "allowable_pressure": bearing_allow, "pressure": n_load / (2.0 * pin_r * contact).max(1e-9)},
            "materials": {"pin": pin_mat, "hole": hole_mat}, "outboard_mass": outboard_mass, "lever": lever,
        });
        // CAD overrides (RoboCAD's `robot.physics`; `joint_physics` as earlier in-process edits stored it).
        let mut overrides = j.node["joint_physics"].as_object().cloned().unwrap_or_default();
        overrides.extend(j.node["robot"]["physics"].as_object().cloned().unwrap_or_default());
        for (key, v) in &overrides {
            match (phys.get_mut(key).and_then(Value::as_object_mut), v.as_object()) {
                (Some(old), Some(new)) => old.extend(new.clone()),
                _ => phys[key] = v.clone(),
            }
        }
        if !overrides.is_empty() {
            phys["source"] = json!("declared");
        }
        if overrides.contains_key("backlash") && !overrides.contains_key("drive_backlash")
            && let Some(w) = overrides["backlash"].as_f64()
        {
            phys["drive_backlash"] = json!({"width_rad": w, "provenance": "estimated", "reference": "legacy scalar backlash override; interpreted as an estimated drive-connection gap"});
        }
        let st_ident = &st["identification"][&j.name];
        if st_ident.is_object() {
            phys["identified"] = st_ident.clone();
            if let Some(b) = st_ident["backlash"].as_f64() {
                phys["drive_backlash"] = json!({"width_rad": b, "provenance": "derived", "reference": format!("identified from {}; fitted at {}", st_ident["source_log"].as_str().unwrap_or("unspecified source log"), st_ident["fitted_at"].as_str().unwrap_or("unspecified time"))});
            }
        }
        let at = format!("joints[{}].physics", j.name);
        if phys["source"] == "estimated" {
            assumptions.push(Assumption { at: at.clone(), what: format!("bearing estimated from {} (pin {:.2} mm radius, 6 mm contact, {:.2} mm clearance); friction from the {} / {} pair under {:.3} kg outboard", if motor.is_some() { "the motor shaft" } else { "a 2 mm printed pin" }, pin_r / MM, clearance / MM, pin_mat, hole_mat, outboard_mass), status: "estimated", blocking: false, fix: format!("measure the joint and set its physics in CAD (set_joint_physics on `{}`)", j.name) });
        }
        let db = &phys["drive_backlash"];
        let unmeasured = db["provenance"] == "unmeasured" || db["width_rad"].is_null();
        if motor.is_some() && unmeasured {
            assumptions.push(Assumption { at: format!("{at}.drive_backlash"), what: format!("drive backlash of motorized joint `{}` is unmeasured; the simulator refuses a v4 model without it", j.name), status: "unmeasured", blocking: true, fix: format!("set an estimate or a measurement in CAD: set_joint_physics `{}` drive_backlash {{width_rad, provenance: estimated | measured, reference}}", j.name) });
        } else if !unmeasured && db["provenance"] == "estimated" {
            assumptions.push(Assumption { at: format!("{at}.drive_backlash"), what: format!("drive backlash {} rad is an estimate ({})", db["width_rad"], db["reference"].as_str().unwrap_or("")), status: "estimated", blocking: false, fix: "measure it on the bench and set the measured value".into() });
        }
        let prismatic = j.ty == "prismatic";
        let scale = if prismatic { MM } else { 1.0 };
        let limits = match (j.lower, j.upper) {
            (Some(lo), Some(hi)) => json!([lo * scale, hi * scale]),
            (None, None) => Value::Null,
            _ => {
                assumptions.push(Assumption { at: format!("joints[{}].limits", j.name), what: "only one travel limit is set, so the joint is exported without limits".into(), status: "default", blocking: true, fix: format!("set both lower and upper limits on `{}` in CAD", j.name) });
                Value::Null
            }
        };
        if limits.is_null() && matches!(j.ty.as_str(), "revolute" | "prismatic") && j.lower.is_none() && j.upper.is_none() {
            assumptions.push(Assumption { at: format!("joints[{}].limits", j.name), what: format!("{} joint `{}` has no travel limits", j.ty, j.name), status: "default", blocking: false, fix: format!("set lower and upper limits on `{}` in CAD", j.name) });
        }
        out_joints.push(json!({
            "name": j.name, "id": j.id, "type": j.ty, "parent": parent_link.map(|p| links[p].name.clone()), "child": links[child_link].name,
            "origin": pivot, "axis": j.axis, "limits": limits, "home": j.home * scale,
            "physics": phys, "fastened": null, "motor": motor.map(|(n, _)| n["name"].clone()),
            "declared": {"damping": j.node["joint"]["damping"].as_f64().unwrap_or(0.0), "friction": j.node["joint"]["friction"].as_f64().unwrap_or(0.0), "stroke": j.node["joint"]["stroke"].as_f64().unwrap_or(0.0) * MM},
        }));
    }

    // Motors.
    let mut out_motors: Vec<Value> = Vec::new();
    for n in bodies.iter().filter(|n| n["robot"]["kind"] == "motor") {
        let id = n["id"].as_str().unwrap_or("");
        let name = n["name"].as_str().unwrap_or("motor");
        let (_, spec) = motor_spec(id)?;
        let joint = joints.iter().find(|j| j.motor.as_deref() == Some(id));
        let gear_extra = joint.map(|j| j.gear_ratio).unwrap_or(1.0);
        let meta = &n["robot"];
        let mut block = json!({
            "name": name, "id": id, "spec": spec.id, "kind": spec.kind, "joint": joint.map(|j| j.name.clone()),
            "mounted_on": meta["mounted_on"].as_str().map(name_of),
            "mount_point": to_m(v3(&meta["mount_point"]).unwrap_or([0.0; 3])), "shaft_axis": unit(v3(&meta["shaft_axis"]).unwrap_or([0.0, 0.0, 1.0])),
            "gear_ratio": gear_extra, "mass": spec.mass_g * 1e-3, "stall_torque": spec.stall_torque * gear_extra, "no_load_speed": spec.no_load_speed / gear_extra.max(1e-9),
        });
        let phys = motors::physics(spec, gear_extra);
        if let (Some(b), Some(p)) = (block.as_object_mut(), phys.as_object()) {
            b.extend(p.clone());
        }
        if joint.is_none() {
            assumptions.push(Assumption { at: format!("motors[{name}]"), what: format!("motor `{name}` drives no joint, so the simulator leaves it out"), status: "default", blocking: false, fix: format!("attach `{name}` to a joint in CAD (attach_motor)") });
        }
        assumptions.push(Assumption { at: format!("motors[{name}].electrical"), what: format!("{} constants: {}; thermal network and servo gains are estimates", spec.name, phys["notes"].as_str().unwrap_or("")), status: "estimated", blocking: false, fix: format!("identify `{name}` on the bench (Leg calibration / characterization) and promote the fit to the actuator registry") });
        out_motors.push(block);
    }

    // Sensors and cables.
    let mut sensors: Vec<Value> = Vec::new();
    let mut cables: Vec<Value> = Vec::new();
    for n in &nodes {
        let r = &n["robot"];
        let name = n["name"].as_str().unwrap_or("");
        if n["kind"] == "sensor" && r.is_object() {
            let Some(l) = r["body"].as_str().filter(|b| body_ids.contains(b)).and_then(link_of) else { continue };
            let kind = r["kind"].as_str().unwrap_or("imu");
            let mut block = match kind {
                "encoder" => json!({"rate_hz": 1000.0, "noise": {"angle": 0.0}, "quantization": {"angle": std::f64::consts::TAU / 4096.0}}),
                "current" => json!({"rate_hz": 1000.0, "noise": {"current": 0.01}, "quantization": {"current": 0.001}}),
                "force" => json!({"rate_hz": 500.0, "noise": {"force": 0.05}, "quantization": {"force": 0.01}}),
                _ => json!({"rate_hz": 200.0, "noise": {"accel": 0.02, "gyro": 0.002}, "bias": {"accel": [0.05, -0.03, 0.04], "gyro": [0.003, -0.002, 0.001]}, "bias_walk": 1e-4, "quantization": {"accel": 0.0006, "gyro": 6e-5}, "range": {"accel": 16.0, "gyro": 34.9}}),
            };
            let point = sub(to_m(v3(&r["point"]).unwrap_or([0.0; 3])), links[l].com);
            let axes = if r["axes"].is_array() { r["axes"].clone() } else { json!([[1, 0, 0], [0, 1, 0], [0, 0, 1]]) };
            let mut declared = false;
            for key in ["rate_hz", "noise", "bias", "bias_walk", "quantization", "range"] {
                if !r[key].is_null() {
                    block[key] = r[key].clone();
                    declared = true;
                }
            }
            if !declared {
                assumptions.push(Assumption { at: format!("sensors[{name}]"), what: format!("{kind} rate, noise and quantization are RoboCAD's typical values"), status: "default", blocking: false, fix: format!("set the sensor's datasheet values on `{name}` in CAD") });
            }
            let mut out = json!({"name": name, "id": n["id"], "kind": kind, "link": links[l].name, "point": point, "axes": axes, "joint": r["joint_name"]});
            if let (Some(o), Some(b)) = (out.as_object_mut(), block.as_object()) {
                o.extend(b.clone());
            }
            sensors.push(out);
        } else if n["kind"] == "cable" && r.is_object() {
            let (Some(a), Some(b)) = (r["from_body"].as_str().filter(|b| body_ids.contains(b)).and_then(link_of), r["to_body"].as_str().filter(|b| body_ids.contains(b)).and_then(link_of)) else { continue };
            let pa = to_m(v3(&r["from_point"]).unwrap_or([0.0; 3]));
            let pb = to_m(v3(&r["to_point"]).unwrap_or([0.0; 3]));
            let length = r["length"].as_f64().filter(|x| *x > 0.0).unwrap_or(norm(sub(pb, pa)) * 1.1);
            let mass = r["mass"].as_f64().filter(|x| *x > 0.0).unwrap_or(0.004 * length / 0.1);
            if r["mass"].as_f64().is_none_or(|x| x <= 0.0) {
                assumptions.push(Assumption { at: format!("cables[{name}].mass"), what: "cable mass from 4 g per 100 mm of 3-wire servo lead".into(), status: "default", blocking: false, fix: format!("weigh the cable and set its mass on `{name}`") });
            }
            cables.push(json!({"name": name, "id": n["id"], "from": {"link": links[a].name, "point": sub(pa, links[a].com)}, "to": {"link": links[b].name, "point": sub(pb, links[b].com)}, "length": length, "mass": mass, "stiffness": r["stiffness"].as_f64().unwrap_or(2000.0), "damping": r["damping"].as_f64().unwrap_or(0.5), "segments": r["segments"].as_u64().unwrap_or(4)}));
        }
    }

    // Transmissions.
    let mut transmissions: Vec<Value> = Vec::new();
    for t in st.get("transmissions").and_then(Value::as_array).into_iter().flatten() {
        let pair: Vec<Option<&JointRec>> = ["driver_joint", "driven_joint"].iter().map(|k| t[*k].as_str().and_then(|id| joints.iter().find(|j| j.id == id))).collect();
        if pair.iter().any(|j| j.is_none_or(|j| !matches!(j.ty.as_str(), "revolute" | "continuous"))) {
            return Err("a transmission needs two rotational joints".into());
        }
        let ratio = t["ratio"].as_f64().filter(|r| r.is_finite() && r.abs() > 1e-12).ok_or("a transmission ratio must be finite and nonzero")?;
        transmissions.push(json!({"name": t["name"], "driver_joint": pair[0].unwrap().name, "driven_joint": pair[1].unwrap().name, "ratio": ratio}));
    }

    // Control, world.
    let mut control = st["control"].clone();
    for j in &out_joints {
        if matches!(j["type"].as_str(), Some("revolute" | "continuous" | "prismatic"))
            && let Some(name) = j["name"].as_str()
            && control["targets"].get(name).is_none()
        {
            control["targets"][name] = json!(0.0);
        }
    }
    let mut world = st["world"].clone();
    if world["floor_z"].is_null() {
        let floor = out_links.iter().map(|l| l["bbox"][0][2].as_f64().unwrap_or(0.0) + l["com"][2].as_f64().unwrap_or(0.0)).fold(f64::INFINITY, f64::min);
        let grounded = out_links.iter().any(|l| l["ground"] == json!(true));
        // A robot standing free rests its lowest point on the floor; one bolted
        // to the world gets the floor a little below.
        world["floor_z"] = json!(if floor.is_finite() { floor - if grounded { 0.02 } else { 0.0 } } else { 0.0 });
        assumptions.push(Assumption { at: "world.floor_z".into(), what: format!("the floor is placed at the lowest point of the robot{}", if grounded { " less 20 mm (it is bolted to the world)" } else { "" }), status: "default", blocking: false, fix: "set world.floor_z in CAD's robot settings to test another placement".into() });
    }
    if let Some(w) = world.as_object_mut() {
        w.remove("floor_material");
    }
    let mus: Vec<(f64, f64)> = links.iter().map(|l| materials::friction_pair(&eng, Some(&l.material), Some(&floor_material))).collect();
    world["floor_friction"] = json!(if mus.is_empty() { 0.6 } else { mus.iter().map(|m| m.1).sum::<f64>() / mus.len() as f64 });
    world["floor_friction_static"] = json!(if mus.is_empty() { 0.7 } else { mus.iter().map(|m| m.0).sum::<f64>() / mus.len() as f64 });

    // Materials (RoboCAD's material_block) for the ones the model uses.
    let mut out_materials = Map::new();
    for mid in &used_materials {
        let Some(m) = doc_materials.iter().find(|m| m["id"] == mid.as_str()) else { continue };
        let e = &eng[mid];
        let mut friction = Map::new();
        for other in material_ids.iter().copied().chain(["steel", "world"]) {
            let (s, k) = materials::friction_pair(&eng, Some(mid), Some(other));
            friction.insert(other.to_string(), json!({"static": s, "kinetic": k}));
        }
        let (s, k) = materials::friction_pair(&eng, Some(mid), Some(&floor_material));
        friction.insert("world".into(), json!({"static": s, "kinetic": k}));
        out_materials.insert(mid.clone(), json!({
            "id": mid, "name": m["name"], "density": m["density"].as_f64().unwrap_or(1.24) * 1000.0,
            "youngs_modulus": e["youngs_modulus"], "poisson": e["poisson"], "yield_strength": e["yield_strength"], "ultimate_strength": e["ultimate_strength"],
            "glass_transition_c": e["glass_transition_c"], "thermal_conductivity": e["thermal_conductivity"], "specific_heat": e["specific_heat"], "thermal_expansion": e["thermal_expansion"],
            "bearing_pressure": e["bearing_pressure"], "friction": friction, "print": e["print"], "sources": e["sources"],
        }));
        let estimated: Vec<&str> = e["sources"].as_object().into_iter().flatten().filter(|(_, v)| v.as_str().is_some_and(|s| s.starts_with(materials::CATALOGUE_ESTIMATE))).map(|(k, _)| k.as_str()).collect();
        if !estimated.is_empty() {
            assumptions.push(Assumption { at: format!("materials[{mid}]"), what: format!("{} of `{mid}` are catalogue estimates", estimated.join(", ")), status: "default", blocking: false, fix: format!("set measured values for `{mid}` in CAD (Materials → engineering properties), or back it with a print-registry filament") });
        }
    }

    let blocking = assumptions.iter().filter(|a| a.blocking).count();
    let mut model = json!({
        "format": "simrobot", "version": SCHEMA_VERSION,
        "source": {
            "file": doc.path, "exported": opts.exported_at, "exporter": EXPORTER,
            "cad_revision": doc.manifest["revision"], "document_id": doc.manifest["document_id"], "archive_identity": doc.identity(),
            "mass_model_identity": masses.model_identity, "mass_derivation_identity": masses.derivation_identity,
            "assumptions": assumptions.iter().map(Assumption::json).collect::<Vec<_>>(),
            "blocking_assumptions": blocking,
            "not_modelled": NOT_MODELLED, "merged": merged,
        },
        "gravity": [0.0, 0.0, -G],
        "world": world,
        "materials": out_materials,
        "links": out_links, "joints": out_joints, "motors": out_motors, "transmissions": transmissions,
        "battery": st["battery"], "sensors": sensors, "cables": cables,
        "control": control, "uncertainty": st["uncertainty"], "identification": st["identification"],
        "planar": opts.planar.map(|(n, o)| json!({"normal": unit(n), "origin": to_m(o)})),
    });
    if let Some(p) = st.get("actuator_profiles") {
        model["actuator_profiles"] = p.clone();
    }
    if let Some(s) = st.get("system") {
        model["system"] = s.clone();
    }
    Ok(model)
}

/// The assumptions an exported model lists (`source.assumptions`).
pub fn assumptions(model: &Value) -> Vec<Value> {
    model["source"]["assumptions"].as_array().cloned().unwrap_or_default()
}

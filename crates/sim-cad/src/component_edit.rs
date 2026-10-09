//! Editing reusable components (RoboCAD's `components.py` `ComponentOps`,
//! ported onto staged edits). Definitions own geometry (their bodies are
//! archive entries `components/<definition>/<node>.brep`, in the
//! definition's own frame); an occurrence is a group whose
//! `component_instance` names a definition, its placement, overrides,
//! port bindings and the identities of its members; members are ordinary
//! nodes (`component_member`) whose geometry and physics the loader derives
//! from the definition (`component::restore`). Every operation here
//! changes an [`Edit`] (one undo step for the host); a refusal leaves it
//! as it was found only in the sense that the host discards the edit.
//!
//! Parameters are typed quantities with units and provenance (measured,
//! derived or estimated); features (box, cylinder, placements, joint
//! ratio, home and frame) evaluate expressions over them
//! (`component::expression`), so a definition regenerates exactly the way
//! the loader rebuilds it.
use crate::component::{self, Library};
use crate::edit::{Edit, new_id};
use crate::geometry::{resolved_brep, transform};
use crate::kernel::{self, Shape};
use crate::ops::Ctx;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::{Read, Write};
use std::path::Path;

pub const SCHEMA_VERSION: u64 = 1;
const SUPPORTED_NODES: [&str; 7] = ["body", "sheet", "curve", "group", "joint", "sensor", "cable"];

/// The feature catalogue (RoboCAD's `FEATURES`): each kind's label and its
/// arguments with their units and counts.
pub fn features() -> Value {
    json!({
        "assembly_placement": {"label": "Assembly placement", "arguments": {"translation": ["mm", 3], "axis": ["1", 3], "angle_deg": ["deg", 1]}},
        "joint_ratio": {"label": "Actuator-to-joint ratio", "arguments": {"ratio": ["1", 1]}},
        "joint_home": {"label": "Joint home angle", "arguments": {"angle_deg": ["deg", 1]}},
        "box": {"label": "Box", "arguments": {"corner": ["mm", 3], "size": ["mm", 3]}},
        "cylinder": {"label": "Cylinder", "arguments": {"base": ["mm", 3], "axis": ["1", 3], "radius": ["mm", 1], "height": ["mm", 1]}},
        "placement": {"label": "Part placement", "arguments": {"translation": ["mm", 3], "axis": ["1", 3], "angle_deg": ["deg", 1]}},
        "joint_frame": {"label": "Joint frame", "arguments": {"pivot": ["mm", 3], "axis": ["1", 3]}},
    })
}

/// `GET /component-recipes`: geometry recipes, features and units.
pub fn recipes() -> Value {
    json!({
        "recipes": {
            "body_thermal_capacity": {"type": "thermal.capacitance", "outputs": {"heat_capacity": "J/K"},
                "inputs": {"specific_heat": {"label": "Specific heat", "unit": "J/(kg·K)", "required": false, "default": null, "minimum": 0, "exclusive_minimum": true}}},
            "circular_fluid_volume": {"type": "fluid.pipe_ph", "outputs": {"length": "m", "diameter": "m", "rise": "m"},
                "inputs": {"flow_direction": {"label": "Fluid direction", "unit": "1", "required": false, "default": 1, "choices": [1, -1]}}},
        },
        "features": features(),
        "units": ["1", "mm", "cm", "m", "deg", "rad", "kg", "g", "s"],
    })
}

/// The `.rcomp` files of a library folder (the default: `~/Documents/RoboCAD/Components`).
pub fn library(path: Option<&str>) -> Result<Value, String> {
    let folder = match path.filter(|p| !p.is_empty()) {
        Some(p) => std::path::PathBuf::from(p),
        None => std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default().join("Documents").join("RoboCAD").join("Components"),
    };
    if folder.exists() && !folder.is_dir() {
        return Err("component-library.path: expected a directory".into());
    }
    let mut files: Vec<Value> = std::fs::read_dir(&folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("rcomp"))
        .map(|p| json!({"path": p.display().to_string(), "name": p.file_stem().map(|s| s.to_string_lossy().into_owned())}))
        .collect();
    files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    Ok(json!({"path": folder.display().to_string(), "files": files}))
}

// ---- 3×4 matrices ------------------------------------------------------------

type M = [f64; 12];

fn mul(a: M, b: M) -> M {
    let mut c = [0.0; 12];
    for i in 0..3 {
        for j in 0..3 {
            c[i * 4 + j] = (0..3).map(|k| a[i * 4 + k] * b[k * 4 + j]).sum();
        }
        c[i * 4 + 3] = a[i * 4 + 3] + (0..3).map(|k| a[i * 4 + k] * b[k * 4 + 3]).sum::<f64>();
    }
    c
}

/// A rigid matrix as RoboCAD's `Transform` JSON (axis–angle, RoboCAD's `matrix_placement`).
pub fn placement_json(m: M) -> Value {
    let r = |i: usize, j: usize| m[i * 4 + j];
    let angle = (((r(0, 0) + r(1, 1) + r(2, 2)) - 1.0) / 2.0).clamp(-1.0, 1.0).acos();
    let axis = if angle.abs() < 1e-10 {
        [0.0, 0.0, 1.0]
    } else if (std::f64::consts::PI - angle).abs() < 1e-7 {
        // The rotation's symmetric part: its largest-diagonal column is the axis.
        let d = [r(0, 0), r(1, 1), r(2, 2)];
        let i = (0..3).max_by(|a, b| d[*a].total_cmp(&d[*b])).unwrap_or(2);
        let mut v = [0.0; 3];
        for j in 0..3 {
            v[j] = if i == j { ((d[i] + 1.0) / 2.0).max(0.0).sqrt() } else { (r(i, j) + r(j, i)) / 4.0 / ((d[i] + 1.0) / 2.0).max(1e-12).sqrt() };
        }
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        v.map(|x| x / l)
    } else {
        let s = 2.0 * angle.sin();
        [(r(2, 1) - r(1, 2)) / s, (r(0, 2) - r(2, 0)) / s, (r(1, 0) - r(0, 1)) / s]
    };
    json!({"translation": [m[3], m[7], m[11]], "axis": axis, "angle_deg": angle.to_degrees(), "scale": 1.0})
}

fn translation(t: [f64; 3]) -> M {
    [1.0, 0.0, 0.0, t[0], 0.0, 1.0, 0.0, t[1], 0.0, 0.0, 1.0, t[2]]
}

fn v3(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array().filter(|a| a.len() == 3)?;
    Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?])
}

fn point(m: M, p: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|i| (0..3).map(|j| m[i * 4 + j] * p[j]).sum::<f64>() + m[i * 4 + 3])
}

fn direction(m: M, d: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|i| (0..3).map(|j| m[i * 4 + j] * d[j]).sum::<f64>())
}

/// A node moved rigidly by `m` (RoboCAD's `transform_node`, without its body):
/// pivot, joint frame, physical metadata, a nested occurrence's placement.
fn transform_node(n: &mut Value, m: M) -> Result<(), String> {
    if let Some(p) = v3(&n["pivot"]) {
        n["pivot"] = json!(point(m, p));
    }
    if n["joint"].is_object() {
        if let Some(p) = v3(&n["joint"]["pivot"]) {
            n["joint"]["pivot"] = json!(point(m, p));
        }
        if let Some(a) = v3(&n["joint"]["axis"]) {
            n["joint"]["axis"] = json!(direction(m, a));
        }
    }
    if n["robot"].is_object() {
        component::physical_transform(&mut n["robot"], m)?;
    }
    if n["component_instance"].is_object() {
        let child = component::placement(&n["component_instance"]["placement"])?;
        n["component_instance"]["placement"] = placement_json(mul(m, child));
    }
    Ok(())
}

/// Every string in `v` that names a node in `known`.
fn referenced(v: &Value, known: &dyn Fn(&str) -> bool, out: &mut BTreeSet<String>) {
    match v {
        Value::String(s) if known(s) => {
            out.insert(s.clone());
        }
        Value::Array(a) => a.iter().for_each(|x| referenced(x, known, out)),
        Value::Object(o) => {
            for (k, x) in o {
                if known(k) {
                    out.insert(k.clone());
                }
                referenced(x, known, out);
            }
        }
        _ => {}
    }
}

// ---- the document's library ------------------------------------------------

fn defs(edit: &Edit) -> Value {
    edit.manifest.get("component_definitions").filter(|d| d.is_object()).cloned().unwrap_or_else(|| json!({}))
}

fn defs_mut(edit: &mut Edit) -> &mut Map<String, Value> {
    edit.object_mut("component_definitions")
}

fn def<'a>(defs: &'a Value, id: &str) -> Result<&'a Value, String> {
    defs.get(id).filter(|d| d.is_object()).ok_or_else(|| format!("no component definition {id}"))
}

/// The definitions a definition depends on (nested occurrences, family variants).
pub fn dependencies(d: &Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for n in d["nodes"].as_array().into_iter().flatten() {
        if let Some(id) = n["component_instance"]["definition_id"].as_str() {
            out.insert(id.to_string());
        }
    }
    for v in d["variants"].as_object().into_iter().flatten().map(|(_, v)| v) {
        if let Some(id) = v["definition_id"].as_str() {
            out.insert(id.to_string());
        }
    }
    out
}

/// A definition's public descriptor (RoboCAD's `ComponentDefinition.descriptor`).
pub fn descriptor(d: &Value) -> Value {
    let nested: Map<String, Value> = d["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|n| n["component_instance"].is_object() && n["component_member"].is_null())
        .map(|n| (n["id"].as_str().unwrap_or("").to_string(), json!({"definition_id": n["component_instance"]["definition_id"], "parameter_bindings": n["component_instance"].get("parameter_bindings").cloned().unwrap_or_else(|| json!({})), "overrides": n["component_instance"].get("overrides").cloned().unwrap_or_else(|| json!({}))})))
        .collect();
    json!({
        "id": d["id"], "name": d["name"], "revision": d["revision"], "description": d.get("description").cloned().unwrap_or_else(|| json!("")),
        "parameters": d.get("parameters").cloned().unwrap_or_else(|| json!({})), "variants": d.get("variants").cloned().unwrap_or_else(|| json!({})),
        "default_variant": d["default_variant"], "features": d.get("features").cloned().unwrap_or_else(|| json!([])), "ports": d.get("ports").cloned().unwrap_or_else(|| json!({})),
        "nested": nested, "provenance": d.get("provenance").cloned().unwrap_or_else(|| json!({})),
        "frame": {"length_unit": "mm", "angle_unit": "deg", "axes": "right-handed XYZ"},
        "node_count": d["nodes"].as_array().map_or(0, Vec::len), "dependencies": dependencies(d),
    })
}

/// `GET /components`: every definition's descriptor with the occurrences that place it.
pub fn catalogue(manifest: &Value) -> Value {
    let nodes = manifest["nodes"].as_array().map_or(&[][..], Vec::as_slice);
    let definitions: Vec<Value> = manifest["component_definitions"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(id, d)| {
            let mut v = descriptor(d);
            v["targets"] = json!(nodes.iter().filter(|n| n["component_instance"]["definition_id"].as_str() == Some(id)).map(|n| json!({"id": n["id"], "name": n["name"], "kind": n["kind"], "component_member": n["component_member"], "component_instance": n["component_instance"]})).collect::<Vec<_>>());
            v
        })
        .collect();
    json!({"version": SCHEMA_VERSION, "features": features(), "definitions": definitions})
}

/// A definition's structure checked (RoboCAD's `validate_definition`), and
/// it and everything it depends on evaluated for each variant (parameters,
/// features, nested occurrences: what the loader will do).
pub fn validate(defs: &Value, id: &str, node: &dyn Fn(&str) -> Option<Value>) -> Result<(), String> {
    let mut visiting = BTreeSet::new();
    validate_tree(defs, id, node, &mut visiting, &mut BTreeSet::new())
}

fn validate_tree(defs: &Value, id: &str, node: &dyn Fn(&str) -> Option<Value>, ancestors: &mut BTreeSet<String>, done: &mut BTreeSet<String>) -> Result<(), String> {
    if !ancestors.insert(id.to_string()) {
        return Err("Circular component dependency".into());
    }
    let d = defs.get(id).filter(|d| d.is_object()).ok_or_else(|| format!("Missing nested component definition: {id}"))?;
    if !done.contains(id) {
        structure(d)?;
        for child in dependencies(d) {
            validate_tree(defs, &child, node, ancestors, done)?;
        }
        let lib = Library { defs, node };
        let choices: Vec<Option<String>> = match d["variants"].as_object().filter(|v| !v.is_empty()) {
            Some(v) => v.keys().map(|k| Some(k.clone())).collect(),
            None => vec![None],
        };
        for choice in choices {
            component::variant(&lib, id, &json!({}), &json!({}), choice.as_deref(), &mut HashSet::new(), &|| false, &|_| {}).map_err(|e| format!("{}: {e}", d["name"].as_str().unwrap_or(id)))?;
        }
        done.insert(id.to_string());
    }
    ancestors.remove(id);
    Ok(())
}

fn structure(d: &Value) -> Result<(), String> {
    let id = d["id"].as_str().filter(|s| !s.is_empty()).ok_or("A component requires an ID and name")?;
    if d["name"].as_str().is_none_or(|s| s.trim().is_empty()) {
        return Err("A component requires an ID and name".into());
    }
    let nodes = d["nodes"].as_array().map_or(&[][..], Vec::as_slice);
    let variants = d["variants"].as_object().filter(|v| !v.is_empty());
    if let Some(v) = variants {
        if !nodes.is_empty() || d["roots"].as_array().is_some_and(|r| !r.is_empty()) || d["features"].as_array().is_some_and(|f| !f.is_empty()) {
            return Err("A component family owns parameter mappings, not duplicate geometry".into());
        }
        if !d["default_variant"].as_str().is_some_and(|k| v.contains_key(k)) {
            return Err("Family default variant is missing".into());
        }
        return Ok(());
    }
    let known: BTreeSet<&str> = nodes.iter().filter_map(|n| n["id"].as_str()).collect();
    let roots: Vec<&str> = d["roots"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let parentless: BTreeSet<&str> = nodes.iter().filter(|n| n["parent"].is_null()).filter_map(|n| n["id"].as_str()).collect();
    if roots.iter().copied().collect::<BTreeSet<_>>() != parentless || roots.len() != parentless.len() {
        return Err("Component roots do not match its hierarchy".into());
    }
    for n in nodes {
        let kind = n["kind"].as_str().unwrap_or("");
        if !SUPPORTED_NODES.contains(&kind) {
            return Err(format!("Unsupported component node: {}", n["name"].as_str().unwrap_or("")));
        }
        for c in n["children"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            if !known.contains(c) || nodes.iter().find(|x| x["id"] == c).is_none_or(|x| x["parent"].as_str() != n["id"].as_str()) {
                return Err("Component parent/child identity mismatch".into());
            }
        }
    }
    let ports = d["ports"].as_object().cloned().unwrap_or_default();
    let mut external = BTreeSet::new();
    for (name, port) in &ports {
        let p = port.as_object().filter(|p| p.len() == 3 && p.contains_key("source_id") && p.contains_key("kind") && p.contains_key("label")).ok_or("Invalid component external port")?;
        let source = p["source_id"].as_str().ok_or("Invalid component external port")?;
        if name.is_empty() || known.contains(source) {
            return Err("External ports must refer outside the component".into());
        }
        if !external.insert(source.to_string()) {
            return Err("Duplicate component external port".into());
        }
    }
    let materials: BTreeSet<&str> = d["materials"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str()).collect();
    for n in nodes {
        if let Some(m) = n["material"].as_str()
            && !materials.contains(m)
        {
            return Err(format!("{}: missing material", n["name"].as_str().unwrap_or("")));
        }
        for key in ["parent", "child", "motor"] {
            if let Some(r) = n["joint"][key].as_str()
                && !known.contains(r)
                && !external.contains(r)
            {
                return Err(format!("{}: missing joint reference {r}", n["name"].as_str().unwrap_or("")));
            }
        }
    }
    for f in d["features"].as_array().into_iter().flatten() {
        let target = f["node"].as_str().unwrap_or("");
        if let Some(n) = nodes.iter().find(|n| n["id"] == target)
            && !n["component_member"].is_null()
        {
            return Err("Map parameters to the nested component instead of editing its derived members".into());
        }
        if !known.contains(target) && !(f["kind"] == "assembly_placement" && target == "*") {
            return Err("Component feature targets a missing node".into());
        }
    }
    let _ = id;
    Ok(())
}

// ---- writing occurrences ------------------------------------------------------

fn put_node(edit: &mut Edit, n: Value) -> Result<(), String> {
    let nodes = edit.manifest["nodes"].as_array_mut().ok_or("manifest nodes must be an array")?;
    match nodes.iter_mut().find(|x| x["id"] == n["id"]) {
        Some(x) => *x = n,
        None => nodes.push(n),
    }
    Ok(())
}

fn node(edit: &Edit, id: &str) -> Result<Value, String> {
    edit.node(id).cloned().ok_or_else(|| format!("node {id} does not exist"))
}

/// Re-derive occurrence `root_id`'s members into the edit (replacing the
/// members it had; members no longer derived are removed).
fn rematerialize(edit: &mut Edit, root_id: &str, cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    let root = node(edit, root_id)?;
    let library = defs(edit);
    let snapshot = edit.manifest["nodes"].clone();
    let lookup = |id: &str| snapshot.as_array().and_then(|a| a.iter().find(|n| n["id"] == id).cloned());
    let lib = Library { defs: &library, node: &lookup };
    let members = component::materialize(&lib, &root, &lookup, cancelled)?;
    let fresh: BTreeSet<String> = members.iter().filter_map(|m| m["id"].as_str().map(str::to_string)).collect();
    // Members of this occurrence (directly or through nested ones) that are not derived any more.
    let mut stale: Vec<String> = Vec::new();
    let mut owners: BTreeSet<String> = BTreeSet::from([root_id.to_string()]);
    let mut grew = true;
    while grew {
        grew = false;
        for n in snapshot.as_array().into_iter().flatten() {
            let id = n["id"].as_str().unwrap_or("").to_string();
            if n["component_member"]["instance_id"].as_str().is_some_and(|o| owners.contains(o)) && owners.insert(id.clone()) {
                grew = true;
                if !fresh.contains(&id) {
                    stale.push(id);
                }
            }
        }
    }
    edit.manifest["nodes"].as_array_mut().ok_or("manifest nodes must be an array")?.retain(|n| !n["id"].as_str().is_some_and(|i| stale.iter().any(|s| s == i)));
    for m in members {
        put_node(edit, m)?;
    }
    Ok(())
}

/// The definition a placement uses (a family resolved to its variant's assembly).
fn template<'a>(defs: &'a Value, d: &'a Value, variant: Option<&str>) -> Result<&'a Value, String> {
    match d["variants"].as_object().filter(|v| !v.is_empty()) {
        None => {
            if variant.is_some() {
                return Err("This component has no named variants".into());
            }
            Ok(d)
        }
        Some(v) => {
            let choice = variant.or(d["default_variant"].as_str()).ok_or("Unknown component variant")?;
            let target = v.get(choice).and_then(|x| x["definition_id"].as_str()).ok_or_else(|| format!("Unknown component variant: {choice}"))?;
            def(defs, target)
        }
    }
}

fn unique_name(edit: &Edit, base: &str) -> String {
    let names: BTreeSet<&str> = edit.manifest["nodes"].as_array().into_iter().flatten().filter_map(|n| n["name"].as_str()).collect();
    if !names.contains(base) {
        return base.to_string();
    }
    (2..).map(|k| format!("{base} {k}")).find(|n| !names.contains(n.as_str())).expect("a free name")
}

fn definition_id() -> String {
    format!("{}{}{}", new_id(), new_id(), &new_id()[..8])
}

fn group_node(id: &str, name: &str) -> Value {
    json!({"id": id, "kind": "group", "name": name, "parent": null, "children": [], "visible": true, "locked": false, "disabled": false,
           "material": null, "color": null, "pivot": null, "transform": {"translation": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0], "angle_deg": 0.0, "scale": 1.0},
           "source": null, "tessellation_tolerance": 0.05})
}

// ---- operations ------------------------------------------------------------------

/// Capture nodes `ids` as a new definition (RoboCAD's `capture_definition`):
/// the definition (bodies moved by `-origin`) and its geometry entries.
fn capture(cx: &mut Ctx, ids: &[String], name: &str, origin: [f64; 3]) -> Result<(Value, Vec<(String, Vec<u8>)>), String> {
    if ids.is_empty() {
        return Err("Select parts or a group to create a component".into());
    }
    let mut selected: BTreeSet<String> = BTreeSet::new();
    fn include(edit: &Edit, id: &str, out: &mut BTreeSet<String>) -> Result<(), String> {
        let n = edit.node(id).ok_or_else(|| format!("Missing component selection: {id}"))?;
        out.insert(id.to_string());
        for c in n["children"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            include(edit, c, out)?;
        }
        Ok(())
    }
    for id in ids {
        let n = node(cx.edit, id)?;
        if ids.len() == 1 && n["kind"] == "group" && n["component_instance"].is_null() {
            for c in n["children"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                include(cx.edit, c, &mut selected)?;
            }
        } else {
            include(cx.edit, id, &mut selected)?;
        }
    }
    for id in &selected {
        if let Some(owner) = node(cx.edit, id)?["component_member"]["instance_id"].as_str()
            && !selected.contains(owner)
        {
            return Err("Capture the whole linked occurrence, not individual linked members".into());
        }
    }
    let all = cx.edit.manifest["nodes"].as_array().cloned().unwrap_or_default();
    let has_geometry = |n: &Value| matches!(n["kind"].as_str(), Some("body" | "sheet" | "curve")) && n["component_member"].is_null() || (!n["component_member"].is_null() && !n["body_kind"].is_null());
    let bodies: BTreeSet<String> = selected.iter().filter(|id| all.iter().any(|n| n["id"] == id.as_str() && has_geometry(n))).cloned().collect();
    for n in &all {
        if n["joint"]["child"].as_str().is_some_and(|c| bodies.contains(c))
            && let Some(id) = n["id"].as_str()
        {
            selected.insert(id.to_string());
        }
    }
    if cx.edit.manifest["component_graph"]["components"].as_object().into_iter().flatten().any(|(_, c)| c["body_id"].as_str().is_some_and(|b| selected.contains(b))) {
        return Err("This selection has native system graph bindings; component capture must include those bindings before it can be reused".into());
    }
    if bodies.is_empty() {
        return Err("A component requires at least one geometric body".into());
    }
    let id = definition_id();
    let shift = translation(origin.map(|v| -v));
    let mut nodes = Vec::new();
    let mut entries = Vec::new();
    let mut external = BTreeSet::new();
    let known_doc = |s: &str| all.iter().any(|n| n["id"] == s);
    for nid in &selected {
        let mut n = node(cx.edit, nid)?;
        transform_node(&mut n, shift)?;
        if !n["parent"].as_str().is_some_and(|p| selected.contains(p)) {
            n["parent"] = Value::Null;
        }
        let kept: Vec<Value> = n["children"].as_array().into_iter().flatten().filter(|c| c.as_str().is_some_and(|c| selected.contains(c))).cloned().collect();
        n["children"] = json!(kept);
        if let Some(o) = n.as_object_mut() {
            o.remove("results");
        }
        if n["component_member"].is_null() && bodies.contains(nid) {
            let brep = cx.body(nid)?;
            let moved = kernel::build(&Shape::Transform { body: &brep, matrix: shift }, cx.cancelled)?;
            entries.push((format!("components/{id}/{nid}.brep"), moved));
        } else if !n["component_member"].is_null() && let Some(o) = n.as_object_mut() {
            o.remove("body_kind");
        }
        let mut refs = BTreeSet::new();
        referenced(&n["robot"], &known_doc, &mut refs);
        external.extend(refs.into_iter().filter(|r| !selected.contains(r)));
        for key in ["parent", "child", "motor"] {
            if let Some(r) = n["joint"][key].as_str()
                && !selected.contains(r)
            {
                external.insert(r.to_string());
            }
        }
        nodes.push(n);
    }
    let mut ports = Map::new();
    for (i, nid) in external.iter().enumerate() {
        let n = all.iter().find(|n| n["id"] == nid.as_str()).ok_or_else(|| format!("Component has a broken external reference: {nid}"))?;
        ports.insert(format!("connection_{}", i + 1), json!({"source_id": nid, "kind": n["kind"], "label": n["name"]}));
    }
    let roots: Vec<Value> = nodes.iter().filter(|n| n["parent"].is_null()).map(|n| n["id"].clone()).collect();
    let mut d = json!({
        "id": id, "name": name, "revision": 1, "description": "", "parameters": {}, "variants": {}, "default_variant": null,
        "features": [], "ports": ports,
        "provenance": {"document_id": cx.edit.manifest["document_id"], "path": cx.doc.path.display().to_string(), "origin_mm": origin, "source": "captured CAD"},
        "version": SCHEMA_VERSION, "nodes": nodes, "roots": roots, "materials": cx.edit.manifest["materials"],
    });
    let desc = descriptor(&d);
    for k in ["nested", "frame", "node_count", "dependencies"] {
        d[k] = desc[k].clone();
    }
    structure(&d)?;
    Ok((d, entries))
}

fn add_definition(cx: &mut Ctx, d: Value, entries: Vec<(String, Vec<u8>)>) -> Result<String, String> {
    let id = d["id"].as_str().ok_or("definition id missing")?.to_string();
    defs_mut(cx.edit).insert(id.clone(), d);
    for (k, v) in entries {
        cx.edit.entries.insert(k, Some(v));
    }
    let library = defs(cx.edit);
    let snapshot = cx.edit.manifest["nodes"].clone();
    let lookup = |n: &str| snapshot.as_array().and_then(|a| a.iter().find(|x| x["id"] == n).cloned());
    validate(&library, &id, &lookup)?;
    Ok(id)
}

/// `create_component`: capture the selection as a definition, leaving the model as it is.
pub fn create_component(cx: &mut Ctx, ids: &[String], name: &str, origin: [f64; 3]) -> Result<String, String> {
    let (d, entries) = capture(cx, ids, name, origin)?;
    add_definition(cx, d, entries)
}

/// `make_component`: capture the selection and replace it with a linked
/// occurrence at `origin`, keeping the parts' ids.
pub fn make_component(cx: &mut Ctx, ids: &[String], name: &str, origin: [f64; 3]) -> Result<Value, String> {
    let (d, entries) = capture(cx, ids, name, origin)?;
    let original = (ids.len() == 1).then(|| node(cx.edit, &ids[0])).transpose()?.filter(|n| n["kind"] == "group" && n["component_instance"].is_null());
    if original.as_ref().is_some_and(|o| !o["component_member"].is_null()) {
        return Err("Selection is already linked; detach before capturing".into());
    }
    let captured: BTreeSet<String> = d["nodes"].as_array().into_iter().flatten().filter_map(|n| n["id"].as_str().map(str::to_string)).collect();
    let bodies: Vec<String> = d["nodes"].as_array().into_iter().flatten().filter(|n| n["component_member"].is_null() && !n["body_kind"].is_null()).filter_map(|n| n["id"].as_str().map(str::to_string)).collect();
    let def_id = add_definition(cx, d.clone(), entries)?;
    let mut root = match &original {
        Some(o) => o.clone(),
        None => group_node(&new_id(), &unique_name(cx.edit, name)),
    };
    let root_id = root["id"].as_str().unwrap_or("").to_string();
    let bindings: Map<String, Value> = d["ports"].as_object().into_iter().flatten().map(|(k, p)| (k.clone(), p["source_id"].clone())).collect();
    root["component_instance"] = json!({
        "definition_id": def_id, "revision": 1, "placement": {"translation": origin, "axis": [0.0, 0.0, 1.0], "angle_deg": 0.0, "scale": 1.0},
        "overrides": {}, "bindings": bindings, "node_map": captured.iter().map(|n| (n.clone(), json!(n))).collect::<Map<String, Value>>(),
    });
    root["children"] = d["roots"].clone();
    // Other nodes stop listing the captured parts as their children.
    for n in cx.edit.manifest["nodes"].as_array_mut().into_iter().flatten() {
        let id = n["id"].as_str().unwrap_or("").to_string();
        if !captured.contains(&id) && id != root_id
            && let Some(c) = n["children"].as_array_mut()
        {
            c.retain(|x| !x.as_str().is_some_and(|x| captured.contains(x)));
        }
    }
    put_node(cx.edit, root)?;
    // The captured bodies are members now: their geometry is the definition's.
    for b in &bodies {
        cx.edit.entries.insert(format!("brep/{b}.brep"), None);
    }
    let names: BTreeMap<String, Value> = captured.iter().filter_map(|id| cx.edit.node(id).map(|n| (id.clone(), n["name"].clone()))).collect();
    rematerialize(cx.edit, &root_id, cx.cancelled)?;
    for (id, name) in names {
        if let Ok(n) = cx.edit.node_mut(&id) {
            n["name"] = name;
        }
    }
    let roots = cx.edit.manifest["roots"].as_array().cloned().unwrap_or_default();
    let mut kept: Vec<Value> = roots.into_iter().filter(|r| !r.as_str().is_some_and(|r| captured.contains(r))).collect();
    if original.is_none() {
        kept.push(json!(root_id));
    }
    cx.edit.manifest["roots"] = json!(kept);
    Ok(json!({"definition_id": def_id, "instance_id": root_id}))
}

/// `new_parametric_component`: a reusable box or cylinder with explicit dimensions.
pub fn new_parametric_component(cx: &mut Ctx, name: &str, shape: &str) -> Result<String, String> {
    let (body, dims, arguments) = match shape {
        "box" => (Shape::Box { corner: [0.0; 3], size: [20.0, 10.0, 60.0] }, vec![("width", 20.0), ("depth", 10.0), ("length", 60.0)], json!({"corner": [0, 0, 0], "size": ["width", "depth", "length"]})),
        "cylinder" => (Shape::Cylinder { base: [0.0; 3], axis: [0.0, 0.0, 1.0], radius: 5.0, height: 60.0 }, vec![("radius", 5.0), ("length", 60.0)], json!({"base": [0, 0, 0], "axis": [0, 0, 1], "radius": "radius", "height": "length"})),
        _ => return Err("Choose box or cylinder".into()),
    };
    let brep = kernel::build(&body, cx.cancelled)?;
    let id = definition_id();
    let nid = new_id();
    let mut n = group_node(&nid, if shape == "box" { "Box" } else { "Cylinder" });
    n["kind"] = json!("body");
    n["body_kind"] = json!("solid");
    let parameters: Map<String, Value> = dims.iter().map(|(k, v)| {
        let mut label = k.to_string();
        label[..1].make_ascii_uppercase();
        (k.to_string(), json!({"value": v, "unit": "mm", "min": 0.01, "max": 10000, "provenance": "estimated", "description": label}))
    }).collect();
    let mut d = json!({
        "id": id, "name": name, "revision": 1, "description": "", "parameters": parameters, "variants": {}, "default_variant": null,
        "features": [{"node": nid, "kind": shape, "arguments": arguments}], "ports": {},
        "provenance": {"source": "explicit parametric primitive", "geometry_units": "mm"},
        "version": SCHEMA_VERSION, "nodes": [n], "roots": [nid], "materials": cx.edit.manifest["materials"],
    });
    let desc = descriptor(&d);
    for k in ["nested", "frame", "node_count", "dependencies"] {
        d[k] = desc[k].clone();
    }
    structure(&d)?;
    add_definition(cx, d, vec![(format!("components/{id}/{nid}.brep"), brep)])
}

/// `create_component_family`: named variants of assembly definitions behind one parameter set.
pub fn create_component_family(cx: &mut Ctx, name: &str, variants: &Value, parameters: &Value, default_variant: Option<&str>) -> Result<String, String> {
    let v = variants.as_object().filter(|v| !v.is_empty()).ok_or("A family requires at least one variant")?;
    let library = defs(cx.edit);
    let first = def(&library, v.values().next().and_then(|x| x["definition_id"].as_str()).unwrap_or(""))?;
    for (k, variant) in v {
        let target = def(&library, variant["definition_id"].as_str().unwrap_or(""))?;
        if target["variants"].as_object().is_some_and(|x| !x.is_empty()) {
            return Err("A family variant must be an assembly definition, which may contain nested families".into());
        }
        let (a, b) = (target["ports"].as_object().cloned().unwrap_or_default(), first["ports"].as_object().cloned().unwrap_or_default());
        if a.len() != b.len() || a.iter().any(|(p, x)| b.get(p).is_none_or(|y| y["kind"] != x["kind"])) {
            return Err(format!("Family variants must expose the same typed connection ports ({k} differs)"));
        }
    }
    let id = definition_id();
    let mut d = json!({
        "id": id, "name": name, "revision": 1, "description": "", "parameters": parameters, "variants": variants,
        "default_variant": default_variant.map(str::to_string).or_else(|| v.keys().next().cloned()), "features": [], "ports": first["ports"],
        "provenance": {"source": "explicit component family", "document_id": cx.edit.manifest["document_id"]},
        "version": SCHEMA_VERSION, "nodes": [], "roots": [], "materials": cx.edit.manifest["materials"],
    });
    let desc = descriptor(&d);
    for k in ["nested", "frame", "node_count", "dependencies"] {
        d[k] = desc[k].clone();
    }
    add_definition(cx, d, Vec::new())
}

/// `link_component_family`: a top-level occurrence of a variant's assembly becomes the family's.
pub fn link_component_family(cx: &mut Ctx, instance_id: &str, definition_id: &str, variant: &str, overrides: &Value) -> Result<String, String> {
    let mut root = node(cx.edit, instance_id)?;
    if root["component_instance"].is_null() || !root["component_member"].is_null() {
        return Err("Link a top-level occurrence before nesting it".into());
    }
    let library = defs(cx.edit);
    let family = def(&library, definition_id)?;
    if family["variants"].as_object().is_none_or(|v| v.is_empty()) {
        return Err("Select a component family".into());
    }
    let target = template(&library, family, Some(variant))?;
    if root["component_instance"]["definition_id"] != target["id"] {
        return Err("The occurrence must already use the selected variant definition".into());
    }
    root["component_instance"]["definition_id"] = json!(definition_id);
    root["component_instance"]["revision"] = family["revision"].clone();
    root["component_instance"]["variant"] = json!(variant);
    root["component_instance"]["overrides"] = if overrides.is_object() { overrides.clone() } else { json!({}) };
    put_node(cx.edit, root)?;
    rematerialize(cx.edit, instance_id, cx.cancelled)?;
    Ok(instance_id.to_string())
}

/// `place_component`: a new occurrence of a definition (or a family's variant).
#[allow(clippy::too_many_arguments)]
pub fn place_component(cx: &mut Ctx, definition_id: &str, placement: &Value, overrides: &Value, bindings: &Value, name: Option<&str>, variant: Option<&str>) -> Result<String, String> {
    let library = defs(cx.edit);
    let d = def(&library, definition_id)?;
    let t = template(&library, d, variant)?;
    // A definition's materials must be this document's (import them explicitly first).
    for m in d["materials"].as_array().into_iter().flatten() {
        let mine = cx.edit.manifest["materials"].as_array().and_then(|a| a.iter().find(|x| x["id"] == m["id"]));
        let used = t["nodes"].as_array().into_iter().flatten().any(|n| n["material"] == m["id"]);
        if used && mine != Some(m) {
            return Err("Component material differs from this document; import its material explicitly first".into());
        }
    }
    let root_id = new_id();
    let mut root = group_node(&root_id, &unique_name(cx.edit, name.unwrap_or(d["name"].as_str().unwrap_or("Component"))));
    let map: Map<String, Value> = t["nodes"].as_array().into_iter().flatten().filter_map(|n| n["id"].as_str()).map(|n| (n.to_string(), json!(new_id()))).collect();
    let children: Vec<Value> = t["roots"].as_array().into_iter().flatten().filter_map(|r| r.as_str().and_then(|r| map.get(r).cloned())).collect();
    root["component_instance"] = json!({
        "definition_id": definition_id, "revision": d["revision"],
        "placement": if placement.is_object() { placement.clone() } else { json!({"translation": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0], "angle_deg": 0.0, "scale": 1.0}) },
        "overrides": if overrides.is_object() { overrides.clone() } else { json!({}) },
        "bindings": if bindings.is_object() { bindings.clone() } else { json!({}) },
        "node_map": map,
    });
    if d["variants"].as_object().is_some_and(|v| !v.is_empty()) {
        root["component_instance"]["variant"] = json!(variant.map(str::to_string).or_else(|| d["default_variant"].as_str().map(str::to_string)));
    }
    root["children"] = json!(children);
    put_node(cx.edit, root)?;
    rematerialize(cx.edit, &root_id, cx.cancelled)?;
    let roots = cx.edit.manifest["roots"].as_array_mut().ok_or("manifest roots must be an array")?;
    roots.push(json!(root_id));
    Ok(root_id)
}

/// `set_component_parameters`: a definition's defaults (and features, nested
/// mappings, family variants); it and every definition depending on it move
/// to a new revision and their occurrences are re-derived.
pub fn set_component_parameters(cx: &mut Ctx, definition_id: &str, parameters: &Value, features: Option<&Value>, nested: Option<&Value>, family_variants: Option<&Value>) -> Result<Value, String> {
    let mut library = defs(cx.edit);
    let mut d = def(&library, definition_id)?.clone();
    d["parameters"] = parameters.clone();
    if let Some(f) = features {
        d["features"] = f.clone();
    }
    if let Some(v) = family_variants {
        d["variants"] = v.clone();
    }
    if let Some(nested) = nested.and_then(Value::as_object) {
        for (nid, settings) in nested {
            let s = settings.as_object().ok_or("Unsupported nested setting")?;
            if s.keys().any(|k| !matches!(k.as_str(), "parameter_bindings" | "overrides")) {
                return Err("Unsupported nested setting".into());
            }
            let n = d["nodes"].as_array_mut().and_then(|a| a.iter_mut().find(|n| n["id"] == nid.as_str())).filter(|n| n["component_instance"].is_object() && n["component_member"].is_null()).ok_or("Nested parameter mapping must target an immediate child occurrence")?;
            for (k, v) in s {
                n["component_instance"][k.as_str()] = v.clone();
            }
        }
    }
    d["revision"] = json!(d["revision"].as_u64().unwrap_or(1) + 1);
    library[definition_id] = d;
    let mut affected: BTreeSet<String> = BTreeSet::from([definition_id.to_string()]);
    loop {
        let parents: Vec<String> = library.as_object().into_iter().flatten().filter(|(k, v)| !affected.contains(*k) && dependencies(v).iter().any(|x| affected.contains(x))).map(|(k, _)| k.clone()).collect();
        if parents.is_empty() {
            break;
        }
        for k in parents {
            let r = library[&k]["revision"].as_u64().unwrap_or(1) + 1;
            library[&k]["revision"] = json!(r);
            affected.insert(k);
        }
    }
    cx.edit.manifest["component_definitions"] = library.clone();
    let snapshot = cx.edit.manifest["nodes"].clone();
    let lookup = |n: &str| snapshot.as_array().and_then(|a| a.iter().find(|x| x["id"] == n).cloned());
    for k in &affected {
        validate(&library, k, &lookup)?;
    }
    let roots: Vec<(String, String)> = snapshot.as_array().into_iter().flatten().filter(|n| n["component_member"].is_null()).filter_map(|n| Some((n["id"].as_str()?.to_string(), n["component_instance"]["definition_id"].as_str()?.to_string()))).filter(|(_, d)| affected.contains(d)).collect();
    for (rid, did) in roots {
        cx.edit.node_mut(&rid)?["component_instance"]["revision"] = library[&did]["revision"].clone();
        rematerialize(cx.edit, &rid, cx.cancelled)?;
    }
    Ok(descriptor(&library[definition_id]))
}

/// `set_component_overrides`: an occurrence's parameter values (a nested
/// one's through its top-level occurrence) and, top level only, its placement.
pub fn set_component_overrides(cx: &mut Ctx, instance_id: &str, overrides: &Value, placement: Option<&Value>) -> Result<Value, String> {
    let mut root = node(cx.edit, instance_id)?;
    if root["component_instance"].is_null() {
        return Err("Select a component occurrence".into());
    }
    if !root["component_member"].is_null() {
        if placement.is_some() {
            return Err("Move nested components through the parent definition".into());
        }
        let target = instance_id.to_string();
        while !root["component_member"].is_null() {
            let owner = root["component_member"]["instance_id"].as_str().ok_or("member without its occurrence")?.to_string();
            root = node(cx.edit, &owner)?;
        }
        if root["locked"] == true {
            return Err("Component occurrence is locked".into());
        }
        let source = root["component_instance"]["node_map"].as_object().and_then(|m| m.iter().find(|(_, v)| v.as_str() == Some(target.as_str())).map(|(k, _)| k.clone())).ok_or("the nested occurrence is not in its parent's node map")?;
        if !root["component_instance"]["nested_overrides"].is_object() {
            root["component_instance"]["nested_overrides"] = json!({});
        }
        let branch = root["component_instance"]["nested_overrides"].as_object_mut().expect("just made an object");
        if overrides.as_object().is_some_and(|o| !o.is_empty()) {
            branch.insert(source, overrides.clone());
        } else {
            branch.remove(&source);
        }
    } else {
        if root["locked"] == true {
            return Err("Component occurrence is locked".into());
        }
        root["component_instance"]["overrides"] = if overrides.is_object() { overrides.clone() } else { json!({}) };
    }
    if let Some(p) = placement {
        transform(p, true)?;
        root["component_instance"]["placement"] = p.clone();
    }
    let rid = root["id"].as_str().unwrap_or("").to_string();
    let spec = root["component_instance"].clone();
    put_node(cx.edit, root)?;
    rematerialize(cx.edit, &rid, cx.cancelled)?;
    Ok(spec)
}

/// `detach_component`: the occurrence and its members become ordinary parts
/// (members keep their current geometry as their own bodies).
pub fn detach_component(cx: &mut Ctx, instance_id: &str) -> Result<String, String> {
    let root = node(cx.edit, instance_id)?;
    if root["component_instance"].is_null() {
        return Err("Select a component occurrence".into());
    }
    if !root["component_member"].is_null() {
        return Err("Detach the outer occurrence first".into());
    }
    let mut ids = vec![instance_id.to_string()];
    ids.extend(root["component_instance"]["node_map"].as_object().into_iter().flatten().filter_map(|(_, v)| v.as_str().map(str::to_string)));
    for id in &ids {
        let has_geometry = cx.edit.node(id).is_some_and(|n| !n["body_kind"].is_null() && n["kind"] != "group");
        if has_geometry {
            let brep = resolved_brep(cx.doc, id)?;
            cx.edit.entries.insert(format!("brep/{id}.brep"), Some(brep));
        }
        let n = cx.edit.node_mut(id)?;
        n["component_instance"] = Value::Null;
        n["component_member"] = Value::Null;
        n["locked"] = json!(false);
    }
    Ok(instance_id.to_string())
}

/// `transform_components`: move top-level occurrences rigidly (about each
/// one's own origin, or `center`).
pub fn transform_components(cx: &mut Ctx, ids: &[String], translation: [f64; 3], axis: [f64; 3], angle_deg: f64, center: Option<[f64; 3]>, scale: f64) -> Result<Value, String> {
    if scale != 1.0 {
        return Err("placement requires finite angle and nonzero scale (components require scale 1)".into());
    }
    let delta = transform(&json!({"translation": [0.0, 0.0, 0.0], "axis": axis, "angle_deg": angle_deg, "scale": 1.0}), true)?;
    for id in ids {
        let mut root = node(cx.edit, id)?;
        if root["locked"] == true {
            return Err("Component occurrence is locked".into());
        }
        if root["component_instance"].is_null() || !root["component_member"].is_null() {
            return Err(format!("{id} is not a top-level component occurrence"));
        }
        let old = transform(&root["component_instance"]["placement"], true)?;
        let t_old = [old[3], old[7], old[11]];
        let pivot = center.unwrap_or(t_old);
        let moved = direction(delta, [0, 1, 2].map(|i| t_old[i] - pivot[i]));
        let position = [0, 1, 2].map(|i| moved[i] + pivot[i] + translation[i]);
        let mut combined = mul(delta, old);
        combined[3] = position[0];
        combined[7] = position[1];
        combined[11] = position[2];
        root["component_instance"]["placement"] = placement_json(combined);
        put_node(cx.edit, root)?;
        rematerialize(cx.edit, id, cx.cancelled)?;
    }
    Ok(json!(ids))
}

/// `export_component`: the definition and everything it depends on, as a
/// `.rcomp` file (`component.json` and `geometry/<definition>/<node>.brep`).
pub fn export_component(doc: &crate::ArchiveDocument, definition_id: &str, path: &Path) -> Result<Value, String> {
    let library = doc.manifest["component_definitions"].clone();
    let mut included = BTreeSet::new();
    fn include(defs: &Value, key: &str, out: &mut BTreeSet<String>) -> Result<(), String> {
        if !out.insert(key.to_string()) {
            return Ok(());
        }
        let d = defs.get(key).filter(|d| d.is_object()).ok_or_else(|| format!("no component definition {key}"))?;
        for child in dependencies(d) {
            include(defs, &child, out)?;
        }
        Ok(())
    }
    include(&library, definition_id, &mut included)?;
    let definitions: Map<String, Value> = included.iter().map(|k| (k.clone(), library[k].clone())).collect();
    let manifest = json!({"version": SCHEMA_VERSION, "root": definition_id, "definitions": definitions});
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        z.start_file("component.json", o).map_err(|e| e.to_string())?;
        z.write_all(&serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        for key in &included {
            let prefix = format!("components/{key}/");
            for (name, bytes) in doc.entries.iter().filter(|(n, _)| n.starts_with(&prefix)) {
                z.start_file(format!("geometry/{key}/{}", &name[prefix.len()..]), o).map_err(|e| e.to_string())?;
                z.write_all(bytes).map_err(|e| e.to_string())?;
            }
        }
        z.finish().map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("rcomp.tmp");
    std::fs::write(&tmp, buf.into_inner()).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(json!({"path": path.display().to_string(), "id": definition_id, "revision": library[definition_id]["revision"], "definitions": included.len()}))
}

/// `import_component`: a `.rcomp` library's definitions embedded (an
/// identical one already here is kept; a material that clashes is renamed
/// `<definition>:<material>`). The root definition's id.
pub fn import_component(cx: &mut Ctx, path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("{}: not a component library: {e}", path.display()))?;
    let mut text = String::new();
    z.by_name("component.json").map_err(|_| "component.json is missing")?.read_to_string(&mut text).map_err(|e| e.to_string())?;
    let manifest: Value = serde_json::from_str(&text).map_err(|e| format!("component.json: {e}"))?;
    if manifest["version"].as_u64() != Some(SCHEMA_VERSION) {
        return Err("Unsupported component library version".into());
    }
    let root = manifest["root"].as_str().ok_or("Library root definition is missing")?.to_string();
    let incoming = manifest["definitions"].as_object().cloned().unwrap_or_default();
    if !incoming.contains_key(&root) {
        return Err("Library root definition is missing".into());
    }
    let mut geometry: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).map_err(|e| e.to_string())?;
        if let Some(rest) = f.name().strip_prefix("geometry/") {
            let name = format!("components/{rest}");
            let mut b = Vec::new();
            f.read_to_end(&mut b).map_err(|e| e.to_string())?;
            geometry.insert(name, b);
        }
    }
    let mut library = defs(cx.edit);
    let mut materials = cx.edit.manifest["materials"].as_array().cloned().unwrap_or_default();
    for (key, mut d) in incoming {
        if d["id"].as_str() != Some(key.as_str()) {
            return Err("Component identity mismatch".into());
        }
        structure(&d)?;
        if let Some(existing) = library.get(&key).filter(|e| e.is_object()) {
            if *existing != d {
                return Err("An embedded component has the same ID but different contents".into());
            }
            continue;
        }
        let mut identities: BTreeMap<String, String> = BTreeMap::new();
        for m in d["materials"].as_array().cloned().unwrap_or_default() {
            let mid = m["id"].as_str().unwrap_or("").to_string();
            let mut target = mid.clone();
            if materials.iter().any(|x| x["id"] == mid.as_str() && *x != m) {
                target = format!("{key}:{mid}");
            }
            let mut renamed = m.clone();
            renamed["id"] = json!(target);
            match materials.iter().find(|x| x["id"] == target.as_str()) {
                Some(x) if *x != renamed => return Err("Conflicting component material".into()),
                Some(_) => {}
                None => materials.push(renamed.clone()),
            }
            identities.insert(mid, target);
        }
        d["materials"] = json!(d["materials"].as_array().into_iter().flatten().map(|m| {
            let mut m = m.clone();
            if let Some(t) = m["id"].as_str().and_then(|i| identities.get(i)) {
                m["id"] = json!(t);
            }
            m
        }).collect::<Vec<_>>());
        for n in d["nodes"].as_array_mut().into_iter().flatten() {
            if let Some(t) = n["material"].as_str().and_then(|m| identities.get(m)) {
                n["material"] = json!(t);
            }
            if let Some(sm) = n["robot"]["solid_materials"].as_object_mut() {
                for v in sm.values_mut() {
                    if let Some(t) = v.as_str().and_then(|m| identities.get(m)) {
                        *v = json!(t);
                    }
                }
            }
        }
        let prefix = format!("components/{key}/");
        for (name, b) in geometry.iter().filter(|(n, _)| n.starts_with(&prefix)) {
            cx.edit.entries.insert(name.clone(), Some(b.clone()));
        }
        library[&key] = d;
    }
    cx.edit.manifest["materials"] = json!(materials);
    cx.edit.manifest["component_definitions"] = library.clone();
    let snapshot = cx.edit.manifest["nodes"].clone();
    let lookup = |n: &str| snapshot.as_array().and_then(|a| a.iter().find(|x| x["id"] == n).cloned());
    validate(&library, &root, &lookup)?;
    Ok(root)
}

/// Run component operation `name` with RoboCAD's keyword arguments on the edit.
pub fn run(cx: &mut Ctx, name: &str, kwargs: &Value) -> Result<Value, String> {
    let s = |k: &str| kwargs[k].as_str();
    let ids = || -> Vec<String> { kwargs["ids"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect() };
    let origin = || v3(&kwargs["origin"]).unwrap_or([0.0; 3]);
    match name {
        "create_component" => create_component(cx, &ids(), s("name").unwrap_or("Component"), origin()).map(Value::from),
        "make_component" => make_component(cx, &ids(), s("name").unwrap_or("Component"), origin()),
        "new_parametric_component" => new_parametric_component(cx, s("name").unwrap_or("Parametric link"), s("shape").unwrap_or("box")).map(Value::from),
        "create_component_family" => create_component_family(cx, s("name").unwrap_or("Family"), &kwargs["variants"], &kwargs["parameters"], s("default_variant")).map(Value::from),
        "link_component_family" => link_component_family(cx, s("instance_id").unwrap_or(""), s("definition_id").unwrap_or(""), s("variant").unwrap_or(""), &kwargs["overrides"]).map(Value::from),
        "place_component" => place_component(cx, s("definition_id").unwrap_or(""), &kwargs["placement"], &kwargs["overrides"], &kwargs["bindings"], s("name").filter(|n| !n.is_empty()), s("variant")).map(Value::from),
        "set_component_parameters" => set_component_parameters(cx, s("definition_id").unwrap_or(""), &kwargs["parameters"], kwargs.get("features").filter(|v| !v.is_null()), kwargs.get("nested").filter(|v| !v.is_null()), kwargs.get("family_variants").filter(|v| !v.is_null())),
        "set_component_overrides" => set_component_overrides(cx, s("instance_id").unwrap_or(""), &kwargs["overrides"], kwargs.get("placement").filter(|v| !v.is_null())),
        "detach_component" => detach_component(cx, s("instance_id").unwrap_or("")).map(Value::from),
        "transform_components" => transform_components(cx, &ids(), v3(&kwargs["translation"]).unwrap_or([0.0; 3]), v3(&kwargs["axis"]).unwrap_or([0.0, 0.0, 1.0]), kwargs["angle_deg"].as_f64().unwrap_or(0.0), v3(&kwargs["center"]), kwargs["scale"].as_f64().unwrap_or(1.0)),
        "import_component" => import_component(cx, Path::new(s("path").ok_or("import_component: give path")?)).map(Value::from),
        "export_component" => export_component(cx.doc, s("definition_id").unwrap_or(""), Path::new(s("path").ok_or("export_component: give path")?)),
        other => Err(format!("{other} is not a component operation")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_placement_survives_its_matrix() {
        let p = json!({"translation": [1.0, 2.0, 3.0], "axis": [0.0, 0.0, 1.0], "angle_deg": 30.0, "scale": 1.0});
        let m = transform(&p, true).unwrap();
        let back = placement_json(m);
        let m2 = transform(&back, true).unwrap();
        for (a, b) in m.iter().zip(m2) {
            assert!((a - b).abs() < 1e-9);
        }
    }

    #[test]
    fn a_family_must_not_carry_geometry() {
        let d = json!({"id": "f", "name": "F", "variants": {"a": {"definition_id": "x"}}, "default_variant": "a", "nodes": [{"id": "n"}]});
        assert!(structure(&d).unwrap_err().contains("family"));
    }
}

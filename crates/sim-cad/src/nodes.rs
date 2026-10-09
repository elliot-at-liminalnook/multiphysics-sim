//! Part edits on the archive: attributes, delete, new bodies and groups,
//! replaced geometry and moves. Reference: RoboCAD's api.py `patch`,
//! commands.py `delete`, `rename`, `set_visible`…, `transform`.
//! A body's B-rep is world placed (RoboCAD bakes a move into it); its
//! `transform` stays metadata, as RoboCAD keeps it.
use crate::annotations::{self, Stamps};
use crate::edit::{Edit, new_id};
use crate::ArchiveDocument;
use serde_json::{Map, Value, json};
use std::collections::HashSet;

fn finite3(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array().filter(|a| a.len() == 3)?;
    Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?]).filter(|p| p.iter().all(|x| x.is_finite()))
}

fn component_guard(edit: &Edit, id: &str, keys: &[&str]) -> Result<(), String> {
    let n = edit.node(id).ok_or_else(|| format!("part {id} does not exist"))?;
    if !n["component_member"].is_null() && keys.iter().any(|k| !["visible", "color", "name"].contains(k)) {
        return Err("Edit component parameters or detach the occurrence first".into());
    }
    Ok(())
}

/// `PATCH /nodes/{id}`: name, visible, locked, disabled, material, color,
/// pivot, tessellation_tolerance, parent (with `index`). A material is
/// checked when the edit's mass properties are derived.
pub fn patch(edit: &mut Edit, id: &str, attrs: &Map<String, Value>) -> Result<Vec<String>, String> {
    let keys: Vec<&str> = attrs.keys().map(String::as_str).collect();
    component_guard(edit, id, &keys)?;
    let mut changed = Vec::new();
    for (k, v) in attrs {
        let value = match k.as_str() {
            "name" => json!(v.as_str().map(str::trim).filter(|s| !s.is_empty() && s.chars().count() <= 200).ok_or("name must be 1–200 characters")?),
            "visible" | "locked" | "disabled" => json!(v.as_bool().ok_or_else(|| format!("{k} must be true or false"))?),
            "material" => json!(v.as_str().filter(|s| !s.is_empty()).ok_or("material must be a material id")?),
            "color" if v.is_null() => Value::Null,
            "color" => json!(finite3(v).filter(|c| c.iter().all(|x| (0. ..=1.).contains(x))).ok_or("color must be [r, g, b] in 0–1, or null")?),
            "pivot" if v.is_null() => Value::Null,
            "pivot" => json!(finite3(v).ok_or("pivot must be three finite millimetre coordinates, or null")?),
            "tessellation_tolerance" => json!(v.as_f64().filter(|t| t.is_finite() && *t > 0.).ok_or("tessellation_tolerance must be a positive number of millimetres")?),
            "parent" => {
                move_node(edit, id, v.as_str(), attrs.get("index").and_then(Value::as_u64).map(|i| i as usize))?;
                changed.push("parent".into());
                continue;
            }
            "index" => continue,
            other => return Err(format!("cannot set {other}")),
        };
        edit.node_mut(id)?[k.as_str()] = value;
        changed.push(k.clone());
    }
    Ok(changed)
}

fn children_of<'e>(edit: &'e mut Edit, parent: Option<&str>) -> Result<&'e mut Vec<Value>, String> {
    match parent {
        None => {
            if !edit.manifest["roots"].is_array() {
                edit.manifest["roots"] = json!([]);
            }
            Ok(edit.manifest["roots"].as_array_mut().expect("array"))
        }
        Some(p) => {
            let n = edit.node_mut(p)?;
            if !n["children"].is_array() {
                n["children"] = json!([]);
            }
            Ok(n["children"].as_array_mut().expect("array"))
        }
    }
}

/// Move node `id` under `parent` (None: the top level) at `index` (the end when absent).
pub fn move_node(edit: &mut Edit, id: &str, parent: Option<&str>, index: Option<usize>) -> Result<(), String> {
    if let Some(p) = parent {
        let mut at = Some(p.to_string());
        while let Some(a) = at {
            if a == id {
                return Err("a part cannot be moved into itself".into());
            }
            at = edit.node(&a).ok_or_else(|| format!("part {a} does not exist"))?["parent"].as_str().map(str::to_string);
        }
        if !matches!(edit.node(p).and_then(|n| n["kind"].as_str()), Some("group" | "assembly")) {
            return Err(format!("{p} is not a group"));
        }
    }
    let old = edit.node(id).ok_or_else(|| format!("part {id} does not exist"))?["parent"].as_str().map(str::to_string);
    children_of(edit, old.as_deref())?.retain(|c| c != id);
    let list = children_of(edit, parent)?;
    let at = index.unwrap_or(list.len()).min(list.len());
    list.insert(at, json!(id));
    edit.node_mut(id)?["parent"] = json!(parent);
    Ok(())
}

/// RoboCAD's `delete`: the parts and everything under them, their B-reps,
/// and their places in the tree. The ids deleted.
pub fn delete(edit: &mut Edit, ids: &[String]) -> Result<Vec<String>, String> {
    let mut selected: Vec<String> = Vec::new();
    let mut pending: Vec<String> = ids.to_vec();
    while let Some(id) = pending.pop() {
        let n = edit.node(&id).ok_or_else(|| format!("part {id} does not exist"))?;
        if selected.contains(&id) {
            continue;
        }
        pending.extend(n["children"].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(str::to_string)));
        selected.push(id);
    }
    let set: HashSet<&str> = selected.iter().map(String::as_str).collect();
    for id in ids {
        if let Some(inst) = edit.node(id).and_then(|n| n["component_member"]["instance_id"].as_str())
            && !set.contains(inst)
        {
            return Err("Delete the whole component occurrence or detach it first".into());
        }
    }
    let nodes = edit.manifest["nodes"].as_array_mut().ok_or("manifest nodes must be an array")?;
    nodes.retain(|n| !n["id"].as_str().is_some_and(|i| set.contains(i)));
    for n in nodes.iter_mut() {
        if let Some(c) = n["children"].as_array_mut() {
            c.retain(|c| !c.as_str().is_some_and(|i| set.contains(i)));
        }
    }
    if let Some(r) = edit.manifest["roots"].as_array_mut() {
        r.retain(|c| !c.as_str().is_some_and(|i| set.contains(i)));
    }
    for id in &selected {
        edit.entries.insert(format!("brep/{id}.brep"), None);
        // A reference mesh node's triangles go with it.
        edit.entries.insert(format!("mesh/{id}.npz"), None);
    }
    Ok(selected)
}

/// The node record RoboCAD writes for a new part.
fn record(id: &str, kind: &str, name: &str, parent: Option<&str>, material: Option<&str>) -> Value {
    json!({
        "id": id, "kind": kind, "name": name, "parent": parent, "children": [],
        "visible": true, "locked": false, "disabled": false, "material": material,
        "color": null, "pivot": null,
        "transform": {"translation": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0], "angle_deg": 0.0, "scale": 1.0},
        "source": null, "tessellation_tolerance": 0.05,
    })
}

fn insert(edit: &mut Edit, node: Value, parent: Option<&str>) -> Result<String, String> {
    let id = node["id"].as_str().expect("id").to_string();
    if let Some(p) = parent {
        edit.node(p).ok_or_else(|| format!("part {p} does not exist"))?;
    }
    edit.manifest["nodes"].as_array_mut().ok_or("manifest nodes must be an array")?.push(node);
    children_of(edit, parent)?.push(json!(id));
    Ok(id)
}

/// A new solid body from B-rep text: its id.
pub fn add_body(edit: &mut Edit, name: &str, parent: Option<&str>, material: Option<&str>, brep: Vec<u8>) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a part needs a name".into());
    }
    let id = new_id();
    let mut node = record(&id, "body", name, parent, Some(material.unwrap_or("pla")));
    node["body_kind"] = json!("solid");
    insert(edit, node, parent)?;
    edit.entries.insert(format!("brep/{id}.brep"), Some(brep));
    Ok(id)
}

/// A new empty group: its id.
pub fn add_group(edit: &mut Edit, name: &str, parent: Option<&str>) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a group needs a name".into());
    }
    let id = new_id();
    insert(edit, record(&id, "group", name, parent, None), parent)
}

/// The B-rep text of plain body `id` (not an instance or component member,
/// whose geometry is derived).
pub fn body_bytes<'d>(doc: &'d ArchiveDocument, id: &str) -> Result<&'d [u8], String> {
    let n = doc.node(id).ok_or_else(|| format!("part {id} does not exist"))?;
    if n["kind"] != "body" {
        return Err(format!("{} is a {}, not a body", n["name"].as_str().unwrap_or(id), n["kind"].as_str().unwrap_or("node")));
    }
    if !n["component_member"].is_null() {
        return Err("Edit component parameters or detach the occurrence first".into());
    }
    doc.entry(&format!("brep/{id}.brep")).ok_or_else(|| format!("part {id} has no B-rep"))
}

/// Body `id`'s geometry replaced by `brep` (a fillet, a boolean's result):
/// its pins then read `needs_review` (their stamp no longer matches).
pub fn replace_body(edit: &mut Edit, _stamps: &Stamps, id: &str, brep: Vec<u8>) -> Result<(), String> {
    if edit.node(id).and_then(|n| n["locked"].as_bool()) == Some(true) {
        return Err(format!("{} is locked", edit.node(id).and_then(|n| n["name"].as_str()).unwrap_or(id)));
    }
    edit.entries.insert(format!("brep/{id}.brep"), Some(brep));
    Ok(())
}

/// RoboCAD's `Ops.transform` for plain bodies: each moved body's B-rep is
/// transformed about `center` (else its pivot, else its bounding centre is
/// not known here: the caller passes the centroid), its pivot follows the
/// translation, its attached pins move with it. Locked bodies are skipped.
/// The ids moved.
pub fn transform(edit: &mut Edit, doc: &ArchiveDocument, stamps: &Stamps, ids: &[String], centers: &[[f64; 3]], translation: [f64; 3], axis: Option<[f64; 3]>, angle_deg: f64, scale: f64) -> Result<Vec<String>, String> {
    let mut moved = Vec::new();
    for (id, center) in ids.iter().zip(centers) {
        // As this edit has the node (a part made earlier in the same edit included).
        let n = edit.node(id).cloned().ok_or_else(|| format!("part {id} does not exist"))?;
        if !n["component_member"].is_null() || !n["component_instance"].is_null() {
            return Err("Move the whole component occurrence or edit its parameters".into());
        }
        if n["locked"] == true {
            continue;
        }
        let m = crate::kernel::placement(translation, axis, angle_deg, *center, scale)?;
        if n["kind"] == "body" {
            let staged = edit.entries.get(&format!("brep/{id}.brep")).cloned().flatten();
            let bytes = match &staged {
                Some(b) => b.as_slice(),
                None => body_bytes(doc, id)?,
            };
            let brep = crate::kernel::build(&crate::kernel::Shape::Transform { body: bytes, matrix: m }, &|| false)?;
            // The new stamp is RoboCAD's of the moved body, as drawn.
            let solid = n["body_kind"].as_str().unwrap_or("solid") == "solid";
            let stamp = crate::stamp::stamp(&brep, solid, Some(n["tessellation_tolerance"].as_f64().unwrap_or(0.05)))?;
            annotations::move_pins(edit, doc, stamps, id, &m, &stamp);
            edit.entries.insert(format!("brep/{id}.brep"), Some(brep));
            if let Some(p) = finite3(&n["pivot"]) {
                edit.node_mut(id)?["pivot"] = json!([p[0] + translation[0], p[1] + translation[1], p[2] + translation[2]]);
            }
        } else {
            // Instances, meshes and images keep their placement in `transform`.
            let t = &n["transform"];
            let old = finite3(&t["translation"]).unwrap_or([0.; 3]);
            let old_axis = finite3(&t["axis"]).unwrap_or([0., 0., 1.]);
            let same_axis = axis.is_none_or(|a| a == old_axis);
            let new = json!({
                "translation": [old[0] + translation[0], old[1] + translation[1], old[2] + translation[2]],
                "axis": axis.unwrap_or(old_axis),
                "angle_deg": if same_axis { t["angle_deg"].as_f64().unwrap_or(0.) + angle_deg } else { angle_deg },
                "scale": t["scale"].as_f64().unwrap_or(1.) * scale,
            });
            edit.node_mut(id)?["transform"] = new;
        }
        moved.push(id.clone());
    }
    Ok(moved)
}

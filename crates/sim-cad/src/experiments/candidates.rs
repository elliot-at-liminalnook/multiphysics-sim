//! Staged CAD edit batches and persistent candidates (RoboCAD's
//! `candidates.py`). A batch runs the shared operations (`ops::run`) on a
//! staged edit of a snapshot: a failure leaves the document and its undo
//! untouched. A candidate keeps its base and staged archives on disk for
//! review and experiments; accepting it hands the host one edit that turns
//! the current archive into the candidate's state (refused when the document
//! moved since the candidate's base).
use super::capture::{self, Snapshot};
use super::{now, read_json, write_bytes, write_json};
use crate::annotations::Stamps;
use crate::archive::ArchiveDocument;
use crate::edit::Edit;
use crate::ops::Ctx;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// The deliberate edit surface a batch may use (no export, scripts or other effects).
pub const EDIT_OPS: &[&str] = &[
    "delete", "rename", "set_visible", "set_locked", "set_disabled", "set_material", "set_color", "set_pivot", "group", "move_node", "isolate", "show_all",
    "box", "box_center", "box_three_point", "cylinder", "sphere", "new_sketch", "extrude", "revolve", "sweep", "pipe", "loft", "fill", "bridge", "push_pull",
    "offset_faces", "offset_face_to", "move_faces", "rotate_faces", "set_radius", "set_diameter", "set_distance", "set_angle", "draft", "delete_faces", "untrim",
    "imprint", "split_face", "boolean", "region", "cut", "shell", "thicken", "fillet", "fillet_chordal", "fillet_all", "full_round", "remove_fillets", "chamfer",
    "transform", "mirror", "instance", "make_unique", "array_rect", "array_radial", "array_curve", "join", "unjoin", "extract_components", "dissolve",
    "project_curve", "silhouette", "set_control_points", "raise_degree", "rebuild_face", "plane_from_face", "plane_three_points", "plane_two_points_camera",
    "plane_midplane", "add_measurement", "clearance", "fastener_hole", "add_joint", "set_joint", "connect_fixed", "add_motor", "mount_motor", "attach_motor",
    "set_ground", "infer_joints", "add_sensor", "add_cable", "set_robot_setting", "set_battery", "set_control", "set_uncertainty", "set_material_props",
    "set_joint_physics", "set_component_graph",
];

/// The manifest keys a published state carries (RoboCAD's `STATE_FIELDS`).
pub const STATE_FIELDS: [&str; 7] = ["nodes", "roots", "materials", "robot_settings", "component_graph", "annotations", "active_group"];

/// `{"$ref": alias}` replaced by that earlier result, recursively.
fn resolve(v: &Value, outputs: &Map<String, Value>) -> Result<Value, String> {
    Ok(match v {
        Value::Object(o) if o.len() == 1 && o.contains_key("$ref") => {
            let k = o["$ref"].as_str().unwrap_or("");
            outputs.get(k).cloned().ok_or_else(|| format!("Unknown prior operation reference {k}"))?
        }
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| resolve(v, outputs).map(|v| (k.clone(), v))).collect::<Result<_, _>>()?),
        Value::Array(a) => Value::Array(a.iter().map(|v| resolve(v, outputs)).collect::<Result<_, _>>()?),
        other => other.clone(),
    })
}

/// Apply `operations` (`[{op, args, kwargs, as}]`) to a staged edit of
/// `doc`, all or nothing: the edit and each result by alias.
pub fn stage(doc: &ArchiveDocument, operations: &Value, cancelled: &dyn Fn() -> bool) -> Result<(Edit, Map<String, Value>), String> {
    let ops = operations.as_array().filter(|a| !a.is_empty()).ok_or("An edit batch requires a nonempty operations array")?;
    let stamps = Stamps::default();
    let mut edit = Edit::of(doc);
    let mut outputs = Map::new();
    for (i, operation) in ops.iter().enumerate() {
        let o = operation.as_object().filter(|o| o.keys().all(|k| matches!(k.as_str(), "op" | "args" | "kwargs" | "as"))).ok_or_else(|| format!("Operation {i}: expected op, args, kwargs and optional as"))?;
        let name = o.get("op").and_then(Value::as_str).unwrap_or("");
        if !EDIT_OPS.contains(&name) {
            return Err(format!("Operation {i}: {name} is not an atomic document edit"));
        }
        let alias = o.get("as").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| i.to_string());
        if outputs.contains_key(&alias) {
            return Err(format!("Operation {i}: duplicate or invalid result alias"));
        }
        let args = resolve(o.get("args").unwrap_or(&json!([])), &outputs)?;
        let kwargs = resolve(o.get("kwargs").unwrap_or(&json!({})), &outputs)?;
        let out = if name == "set_component_graph" {
            let graph = args.get(0).or_else(|| kwargs.get("graph")).cloned().ok_or("set_component_graph: give the graph")?;
            let checked = crate::component_graph::validate(&graph, &edit.manifest).map_err(|e| format!("Operation {i} ({name}): {e}"))?;
            edit.manifest["component_graph"] = checked;
            Value::Null
        } else {
            let mut cx = Ctx { doc, stamps: &stamps, edit: &mut edit, centroid: &|_| None, cancelled };
            crate::ops::run(&mut cx, name, args.as_array().map_or(&[][..], Vec::as_slice), kwargs.as_object().unwrap_or(&Map::new())).map_err(|e| format!("Operation {i} ({name}): {e}"))?
        };
        outputs.insert(alias, out);
    }
    Ok((edit, outputs))
}

/// What changed between two snapshots: nodes (added, removed, modified,
/// geometry), document keys, and whether the physical model did.
pub fn change_set(before: &Snapshot, after: &Snapshot) -> Result<Value, String> {
    let contents = |s: &Snapshot| -> Result<(Value, std::collections::BTreeMap<String, String>), String> {
        let doc = capture::open(Path::new("snapshot.rcad"), s.data.clone())?;
        let geometry = doc.entries.iter().filter(|(n, _)| n.starts_with("brep/") || n.starts_with("mesh/")).map(|(n, b)| (n.clone(), super::digest(b))).collect();
        Ok((doc.manifest.clone(), geometry))
    };
    let ((a, ga), (b, gb)) = (contents(before)?, contents(after)?);
    let by_id = |m: &Value| -> std::collections::BTreeMap<String, Value> { m["nodes"].as_array().into_iter().flatten().filter_map(|n| n["id"].as_str().map(|i| (i.to_string(), n.clone()))).collect() };
    let (na, nb) = (by_id(&a), by_id(&b));
    let mut ids: Vec<&String> = na.keys().chain(nb.keys()).collect();
    ids.sort();
    ids.dedup();
    let mut changes = Vec::new();
    for id in ids {
        let (x, y) = (na.get(id), nb.get(id));
        let geometry = [format!("brep/{id}.brep"), format!("mesh/{id}.npz")].iter().any(|k| ga.get(k) != gb.get(k));
        if x != y || geometry {
            let kind = if x.is_none() { "added" } else if y.is_none() { "removed" } else { "modified" };
            changes.push(json!({"id": id, "name": y.or(x).map(|n| n["name"].clone()), "kind": kind, "geometry_changed": geometry, "before": x, "after": y}));
        }
    }
    let mut document = Map::new();
    for key in ["materials", "robot_settings", "component_graph", "annotations", "roots", "active_group"] {
        if a.get(key) != b.get(key) {
            document.insert(key.into(), json!({"before": a.get(key), "after": b.get(key)}));
        }
    }
    Ok(json!({"nodes": changes, "document": document, "physical_changed": before.physical_hash != after.physical_hash}))
}

/// The edit that turns `current` into `target`'s state (RoboCAD's
/// `PublishState`): its state keys, and every geometry entry added,
/// changed or removed.
pub fn edit_to(current: &ArchiveDocument, target: &ArchiveDocument) -> Edit {
    let mut edit = Edit::of(current);
    for key in STATE_FIELDS {
        edit.manifest[key] = target.manifest.get(key).cloned().unwrap_or(Value::Null);
    }
    for (name, bytes) in &target.entries {
        if name != "manifest.json" && current.entry(name) != Some(bytes.as_slice()) {
            edit.entries.insert(name.clone(), Some(bytes.clone()));
        }
    }
    let geometry = |n: &str| n.starts_with("brep/") || n.starts_with("mesh/") || n.starts_with("image/") || n.starts_with("components/");
    for (name, _) in &current.entries {
        if geometry(name) && target.entry(name).is_none() {
            edit.entries.insert(name.clone(), None);
        }
    }
    edit
}

/// The candidates of one runs folder (`runs/experiments/candidates`).
pub struct Candidates {
    pub root: PathBuf,
}

fn checked(id: &str) -> Result<&str, String> {
    if id.len() == 32 && id.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) { Ok(id) } else { Err(format!("no candidate {id}")) }
}

impl Candidates {
    pub fn new(root: PathBuf) -> Candidates {
        Candidates { root }
    }

    /// Stage `request.operations` on `doc` (at `revision`) and keep the
    /// candidate: its record (state draft).
    pub fn create(&self, doc: &ArchiveDocument, revision: u64, request: &Value, cancelled: &dyn Fn() -> bool) -> Result<Value, String> {
        let document_id = doc.manifest["document_id"].as_str().unwrap_or("");
        if request["document_id"].as_str().is_some_and(|d| d != document_id) {
            return Err("Document replaced before candidate capture".into());
        }
        if request["expected_revision"].as_u64() != Some(revision) {
            return Err(format!("Expected document revision {}; current revision is {revision}. Fetch the current document and rebuild the candidate before applying it.", request["expected_revision"]));
        }
        let before = capture::capture(doc, revision)?;
        let (edit, outputs) = stage(doc, &request["operations"], cancelled)?;
        let staged = doc.apply(edit)?;
        let after = capture::capture(&staged, revision + 1)?;
        let id = format!("{}{}{}", crate::edit::new_id(), crate::edit::new_id(), &crate::edit::new_id()[..8]);
        let folder = self.root.join(&id);
        write_bytes(&folder.join("base.rcad"), &before.data)?;
        write_bytes(&folder.join("candidate.rcad"), &after.data)?;
        let record = json!({
            "id": id, "document_id": before.document_id, "base_revision": revision, "revision": after.revision,
            "label": request["label"].as_str().filter(|l| !l.is_empty()).map(str::to_string).unwrap_or_else(|| format!("Candidate {}", &id[..8])),
            "state": "draft", "created_at": now(), "operations": request["operations"], "results": outputs,
            "physical_hash": after.physical_hash, "cad_archive_hash": after.archive_hash,
            "changes": change_set(&before, &after)?,
        });
        write_json(&folder.join("candidate.json"), &record)?;
        Ok(record)
    }

    /// Candidate `id` of `document_id`.
    pub fn get(&self, id: &str, document_id: &str) -> Result<Value, String> {
        let path = self.root.join(checked(id)?).join("candidate.json");
        let record = read_json(&path).map_err(|_| format!("no candidate {id}"))?;
        if record["document_id"].as_str() != Some(document_id) {
            return Err(format!("no candidate {id}"));
        }
        Ok(record)
    }

    /// Every candidate of `document_id`, newest first.
    pub fn list(&self, document_id: &str) -> Vec<Value> {
        let mut out: Vec<Value> = std::fs::read_dir(&self.root).into_iter().flatten().flatten().filter_map(|e| self.get(&e.file_name().to_string_lossy(), document_id).ok()).collect();
        out.sort_by(|a, b| b["created_at"].as_f64().unwrap_or(0.0).total_cmp(&a["created_at"].as_f64().unwrap_or(0.0)));
        out
    }

    /// The staged archive.
    pub fn document(&self, id: &str, document_id: &str) -> Result<ArchiveDocument, String> {
        self.get(id, document_id)?;
        ArchiveDocument::open(&self.root.join(id).join("candidate.rcad"))
    }

    /// The staged archive's snapshot (for an experiment on the candidate).
    pub fn snapshot(&self, id: &str, document_id: &str) -> Result<Snapshot, String> {
        let record = self.get(id, document_id)?;
        let doc = self.document(id, document_id)?;
        capture::capture(&doc, record["revision"].as_u64().unwrap_or(0))
    }

    /// Accept a draft: the edit that publishes it onto `current` (at
    /// `revision`), refused when the document moved since its base.
    pub fn accept(&self, id: &str, current: &ArchiveDocument, revision: u64, expected_revision: u64) -> Result<(Value, Edit), String> {
        let document_id = current.manifest["document_id"].as_str().unwrap_or("");
        let record = self.get(id, document_id)?;
        if record["state"] != "draft" {
            return Err("Only a draft candidate can be accepted".into());
        }
        if expected_revision != revision || record["base_revision"].as_u64() != Some(revision) {
            return Err(format!("Expected document revision {}; current revision is {revision}. Fetch the current document and rebuild the candidate before applying it.", record["base_revision"]));
        }
        let staged = self.document(id, document_id)?;
        Ok((record, edit_to(current, &staged)))
    }

    /// Record that a draft was accepted at `revision` (its receipt).
    pub fn mark_accepted(&self, id: &str, document_id: &str, revision: u64) -> Result<Value, String> {
        let mut record = self.get(id, document_id)?;
        record["state"] = json!("accepted");
        record["accepted_revision"] = json!(revision);
        record["updated_at"] = json!(now());
        write_json(&self.root.join(id).join("candidate.json"), &record).map_err(|e| format!("the candidate was applied, but its receipt could not be written; refresh before retrying: {e}"))?;
        Ok(record)
    }

    /// Discard a draft.
    pub fn discard(&self, id: &str, document_id: &str) -> Result<Value, String> {
        let mut record = self.get(id, document_id)?;
        if record["state"] != "draft" {
            return Err("Only a draft candidate can be discarded".into());
        }
        record["state"] = json!("discarded");
        record["updated_at"] = json!(now());
        write_json(&self.root.join(id).join("candidate.json"), &record)?;
        Ok(record)
    }
}

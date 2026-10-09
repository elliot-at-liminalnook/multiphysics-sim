//! Read-only captured geometry and replay (RoboCAD's `captured_review.py`).
//! The caller supplies the captured archive (a run's `model.rcad` or a
//! candidate's); missing captured CAD is said, never filled from live CAD.
use super::results::{replay_flex, replay_matrices, sample_index, signals, value_at};
use crate::archive::ArchiveDocument;
use serde_json::{Map, Value, json};

/// Where a capture came from (`CaptureIdentity`).
pub fn identity(record: &Value, kind: &str) -> Value {
    let p = &record["provenance"];
    json!({
        "document_id": record["document_id"], "revision": record["revision"], "source_kind": kind, "source_id": record["id"],
        "physical_hash": record.get("physical_hash").filter(|v| !v.is_null()).unwrap_or(&p["physical_hash"]),
        "archive_hash": record.get("cad_archive_hash").filter(|v| !v.is_null()).unwrap_or(&p["cad_archive_hash"]),
    })
}

/// The captured document's visible nodes as meshes (mm), in tree order.
pub fn geometry(doc: Option<&ArchiveDocument>, record: &Value, kind: &str, cancelled: &dyn Fn() -> bool) -> Result<Value, String> {
    let mut nodes = Vec::new();
    if let Some(doc) = doc {
        let mut order: Vec<String> = Vec::new();
        let mut stack: Vec<String> = doc.manifest["roots"].as_array().into_iter().flatten().rev().filter_map(|v| v.as_str().map(str::to_string)).collect();
        while let Some(id) = stack.pop() {
            if order.contains(&id) {
                continue;
            }
            if let Some(n) = doc.node(&id) {
                stack.extend(n["children"].as_array().into_iter().flatten().rev().filter_map(|v| v.as_str().map(str::to_string)));
            }
            order.push(id);
        }
        for id in order {
            let Some(n) = doc.node(&id) else { continue };
            if !doc.visible(&id) || n["disabled"] == true {
                continue;
            }
            let has_geometry = matches!(n["kind"].as_str(), Some("body" | "sheet" | "instance" | "mesh"));
            let mesh = if has_geometry {
                let g = crate::geometry::tessellate_node(doc, &id, 0.1, cancelled).ok();
                g.map(|g| {
                    let faces = g.triangle_faces.iter().copied().max().map_or(0, |m| m + 1);
                    json!({"vertices": g.vertices_mm, "triangles": g.triangles, "triangle_face": g.triangle_faces, "face_count": faces})
                })
            } else {
                None
            };
            nodes.push(json!({"id": id, "name": n["name"], "source": n["source"], "mesh": mesh}));
        }
    }
    Ok(json!({
        "identity": identity(record, kind), "units": "mm", "nodes": nodes,
        "missing_reason": if doc.is_none() { json!("Captured run has no CAD geometry") } else { Value::Null },
        "provenance": record["provenance"],
    }))
}

/// The replay at `seconds`: the nearest sample's matrices, flex arrows and
/// every signal's value there.
pub fn sample(result: &Value, record: &Value, seconds: f64, scale: f64) -> Result<Value, String> {
    if !seconds.is_finite() {
        return Err("Replay time must be finite".into());
    }
    if !(scale.is_finite() && scale > 0.0 && scale <= 1000.0) {
        return Err("Flex scale must be finite in (0,1000]".into());
    }
    let times: Vec<f64> = result["trace"]["t"].as_array().into_iter().flatten().filter_map(Value::as_f64).collect();
    if times.is_empty() {
        return Err("Captured result has no replay samples".into());
    }
    let index = sample_index(&times, seconds)?;
    let catalogue = signals(result)?;
    let t = times[index];
    let values: Map<String, Value> = catalogue.iter().map(|(k, s)| (k.clone(), value_at(s, t).map_or(Value::Null, |v| json!(v)))).collect();
    Ok(json!({
        "identity": identity(record, "experiment"), "index": index, "time": t,
        "matrices": replay_matrices(result, index)?, "flex": replay_flex(result, index, scale)?,
        "signals": catalogue.iter().map(|(k, s)| (k.clone(), s.json())).collect::<Map<String, Value>>(), "values": values,
    }))
}

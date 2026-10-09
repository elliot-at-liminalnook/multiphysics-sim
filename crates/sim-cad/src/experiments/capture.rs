//! Captured CAD inputs and stable content identities (RoboCAD's
//! `snapshots.py`). A snapshot is the archive's manifest (without results
//! and the save time) and its geometry entries, zipped stored with fixed
//! timestamps in name order, so capturing the same state twice gives the
//! same bytes. Capturing never changes the document.
use super::{canonical, digest};
use crate::archive::ArchiveDocument;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{Cursor, Write};

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub document_id: String,
    pub revision: u64,
    /// Everything the physical model is derived from.
    pub physical_hash: String,
    /// The snapshot archive's bytes.
    pub archive_hash: String,
    pub data: Vec<u8>,
    /// The physical identity without the component graph.
    pub cad_derivation_hash: String,
}

/// The review-free part of a manifest the physical model depends on
/// (RoboCAD's `physical_manifest`; names kept, as the export uses them).
pub fn physical_manifest(manifest: &Value) -> Value {
    const KINDS: [&str; 8] = ["body", "sheet", "instance", "mesh", "joint", "sensor", "cable", "plane"];
    const DROPPED: [&str; 6] = ["results", "color", "locked", "visible", "children", "parent"];
    let nodes = manifest["nodes"].as_array().map_or(&[][..], Vec::as_slice);
    let mut physical: Vec<Value> = nodes
        .iter()
        .filter(|n| n["kind"].as_str().is_some_and(|k| KINDS.contains(&k)))
        .map(|n| Value::Object(n.as_object().into_iter().flatten().filter(|(k, _)| !DROPPED.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect()))
        .collect();
    physical.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    let mut materials: Vec<Value> = manifest["materials"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| Value::Object(m.as_object().into_iter().flatten().filter(|(k, _)| !matches!(k.as_str(), "color" | "roughness" | "metallic")).map(|(k, v)| (k.clone(), v.clone())).collect()))
        .collect();
    materials.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    let occurrences: Vec<Value> = nodes
        .iter()
        .filter(|n| n.get("component_instance").is_some_and(|c| !c.is_null() && c != false))
        .map(|n| json!({"id": n["id"], "component_instance": n["component_instance"]}))
        .collect();
    json!({
        "nodes": physical, "materials": materials, "robot_settings": manifest["robot_settings"],
        "component_graph": manifest.get("component_graph").cloned().unwrap_or_else(|| json!({"version": 1, "components": {}, "connections": {}})),
        "component_definitions": manifest.get("component_definitions").cloned().unwrap_or_else(|| json!({})),
        "component_occurrences": occurrences,
    })
}

/// Capture `doc` (at `revision`).
pub fn capture(doc: &ArchiveDocument, revision: u64) -> Result<Snapshot, String> {
    let mut manifest = doc.manifest.clone();
    if let Some(m) = manifest.as_object_mut() {
        m.remove("saved");
    }
    // Results are independent artifacts: a snapshot never carries them.
    manifest["results"] = Value::Null;
    if let Some(nodes) = manifest["nodes"].as_array_mut() {
        for n in nodes {
            if let Some(o) = n.as_object_mut() {
                o.remove("results");
            }
        }
    }
    let mut blobs: BTreeMap<String, Vec<u8>> = doc.entries.iter().filter(|(name, _)| name != "manifest.json").map(|(n, b)| (n.clone(), b.clone())).collect();
    let mut physical = physical_manifest(&manifest);
    let geometry: BTreeMap<String, Value> = blobs.iter().filter(|(n, _)| n.starts_with("brep/") || n.starts_with("mesh/") || n.starts_with("components/")).map(|(n, b)| (n.clone(), json!(digest(b)))).collect();
    physical["geometry"] = json!(geometry);
    let physical_hash = digest(&canonical(&physical));
    let without_graph = Value::Object(physical.as_object().into_iter().flatten().filter(|(k, _)| *k != "component_graph").map(|(k, v)| (k.clone(), v.clone())).collect());
    let cad_derivation_hash = digest(&canonical(&without_graph));
    blobs.insert("manifest.json".into(), canonical(&manifest));
    let mut buf = Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).last_modified_time(zip::DateTime::default());
        for (name, data) in &blobs {
            z.start_file(name.as_str(), o).map_err(|e| e.to_string())?;
            z.write_all(data).map_err(|e| e.to_string())?;
        }
        z.finish().map_err(|e| e.to_string())?;
    }
    let data = buf.into_inner();
    Ok(Snapshot {
        document_id: manifest["document_id"].as_str().unwrap_or_default().to_string(),
        revision,
        physical_hash,
        archive_hash: digest(&data),
        data,
        cad_derivation_hash,
    })
}

/// An archive from snapshot bytes (as if read from `path`).
pub fn open(path: &std::path::Path, data: Vec<u8>) -> Result<ArchiveDocument, String> {
    ArchiveDocument::from_bytes(path, data, &|| false, &|_| {})
}

//! Results and identification carried back into the document (RoboCAD's
//! `physical.load_results` and `apply_identification`), as staged edits.
//!
//! - [`load_results`]: a `*.simresult.json` hung on its nodes (each link,
//!   joint and motor block on the node its CAD mapping names) and kept whole
//!   as the document's `results`, marked stale when it describes another
//!   physical state.
//! - [`apply_identification`]: fitted joint parameters stored under
//!   `robot_settings.identification`. A joint whose actuator role the
//!   accepted actuator registry covers is not given copied motor numbers:
//!   it records the registry family it is measured by, and the fit is
//!   reported as one to promote there (measured values have one source).
use crate::edit::Edit;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::Path;

fn read(path: &Path) -> Result<Value, String> {
    super::read_json(path)
}

/// Hang a results file on its nodes; the whole file (with `path`, `loaded`
/// and `stale`). `physical_hash`: the document's physical identity now.
pub fn load_results(edit: &mut Edit, path: &Path, physical_hash: &str) -> Result<Value, String> {
    let mut res = read(path)?;
    let mapping: BTreeMap<(String, String), Value> = res["cad_mapping"].as_array().into_iter().flatten().map(|m| ((m["section"].as_str().unwrap_or("links").to_string(), m["name"].as_str().unwrap_or("").to_string()), m.clone())).collect();
    let nodes = edit.manifest["nodes"].as_array_mut().ok_or("manifest nodes must be an array")?;
    let by_name: BTreeMap<String, usize> = nodes.iter().enumerate().filter_map(|(i, n)| n["name"].as_str().map(|s| (s.to_string(), i))).collect();
    let by_id: BTreeMap<String, usize> = nodes.iter().enumerate().filter_map(|(i, n)| n["id"].as_str().map(|s| (s.to_string(), i))).collect();
    for n in nodes.iter_mut() {
        if let Some(o) = n.as_object_mut() {
            o.insert("results".into(), Value::Null);
        }
    }
    for section in ["links", "joints", "motors"] {
        for (name, block) in res[section].as_object().cloned().into_iter().flatten() {
            let targets: Vec<usize> = match mapping.get(&(section.to_string(), name.clone())) {
                Some(m) => {
                    let mut ids: Vec<String> = m["id"].as_str().map(str::to_string).into_iter().collect();
                    ids.extend(m["members"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)));
                    ids.dedup();
                    ids.iter().filter_map(|i| by_id.get(i).copied()).collect()
                }
                None => by_name.get(&name).copied().into_iter().collect(),
            };
            for i in targets {
                let mut r = Map::new();
                r.insert("section".into(), json!(section));
                for (k, v) in block.as_object().into_iter().flatten() {
                    r.insert(k.clone(), v.clone());
                }
                nodes[i]["results"] = Value::Object(r);
            }
        }
    }
    res["path"] = json!(path.display().to_string());
    res["loaded"] = json!(crate::edit::now_iso());
    let identity = res["provenance"]["physical_hash"].as_str();
    res["stale"] = json!(identity != Some(physical_hash));
    edit.manifest["results"] = res.clone();
    Ok(res)
}

/// Store fitted joint parameters (a fit file's `identification` block, or
/// the file itself). `covered(joint)`: the accepted actuator registry's
/// family for the joint's role, when it has one. The stored block, and the
/// joints whose fit belongs in the registry instead.
pub fn apply_identification(edit: &mut Edit, path: &Path, covered: &dyn Fn(&str) -> Option<(String, Value)>) -> Result<Value, String> {
    let data = read(path)?;
    let ident = data.get("identification").cloned().unwrap_or_else(|| data.clone());
    let blocks = ident.as_object().ok_or("no identification block in the file")?;
    let fitted_at = data["fitted_at"].as_str().map(str::to_string).unwrap_or_else(crate::edit::now_iso);
    let settings = edit.object_mut("robot_settings");
    if !settings.get("identification").is_some_and(Value::is_object) {
        settings.insert("identification".into(), json!({}));
    }
    let st = settings.get_mut("identification").and_then(Value::as_object_mut).expect("just made an object");
    let mut promote = Vec::new();
    for (joint, block) in blocks {
        let Some(block) = block.as_object() else { continue };
        let mut b = block.clone();
        b.entry("source_log").or_insert_with(|| data["source_log"].clone());
        b.entry("fitted_at").or_insert_with(|| json!(fitted_at));
        if let Some((family, registry)) = covered(joint) {
            // The registry measures this actuator: keep the fit as evidence and point at the family.
            b.insert("actuator_family".into(), json!(family));
            b.insert("actuator_registry".into(), registry);
            b.insert("applies".into(), json!("not applied: the accepted actuator registry supplies this joint's motor values; promote the fit there"));
            promote.push(json!({"joint": joint, "family": family}));
        }
        st.insert(joint.clone(), Value::Object(b));
    }
    Ok(json!({"identification": st.clone(), "promote": promote}))
}

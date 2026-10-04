//! Named inspection views stored in the archive (`manifest.saved_views`),
//! in RoboCAD's schema. Reference: RoboCAD's saved_views.py
//! (`validate_state`, `SavedViewOps`).
//!
//! A view's `state` is exactly RoboCAD's (camera, grid, comment pins, display
//! mode, one section plane), so RoboCAD restores views saved here. What a
//! view adds beside its state, which RoboCAD carries along untouched:
//! `description` (why the view matters, ≤ 1000 characters), `parts` (the
//! parts the view shows alone, ≤ 200 ids), `author`, `author_kind`
//! (`person` | `agent`), `created_at` and `updated_at`.
use crate::edit::{Edit, new_id, now_iso};
use crate::ArchiveDocument;
use serde_json::{Map, Value, json};
use std::collections::HashSet;

pub const DISPLAY_MODES: [&str; 6] = ["shaded", "shaded_edges", "wireframe", "xray", "matcap", "render"];

fn unit(v: &Value) -> Result<[f64; 3], String> {
    let a = v.as_array().filter(|a| a.len() == 3).ok_or("Section plane needs finite 3D vectors")?;
    let mut out = [0.; 3];
    for i in 0..3 {
        if a[i].is_boolean() {
            return Err("Section plane needs finite 3D vectors".into());
        }
        out[i] = a[i].as_f64().filter(|x| x.is_finite()).ok_or("Section plane needs finite 3D vectors")?;
    }
    Ok(out)
}

/// `validate_state`: RoboCAD's defaults filled in, every field checked.
pub fn validate_state(state: &Value) -> Result<Value, String> {
    let s = state.as_object().ok_or("View state must be an object")?;
    const CAMERA: [&str; 8] = ["target", "distance", "yaw", "pitch", "fov", "orthographic", "mode", "rot"];
    if s.keys().any(|k| !CAMERA.contains(&k.as_str()) && !["section", "grid", "display_mode", "comment_pins"].contains(&k.as_str())) {
        return Err("Unsupported saved view field".into());
    }
    let mut camera: Map<String, Value> = json!({"target": [0, 0, 0], "distance": 250, "yaw": -35, "pitch": 28, "fov": 40, "orthographic": false, "mode": "turntable", "rot": [[1, 0, 0], [0, 1, 0], [0, 0, 1]]}).as_object().cloned().expect("object");
    for (k, v) in s.iter().filter(|(k, _)| CAMERA.contains(&k.as_str())) {
        camera.insert(k.clone(), v.clone());
    }
    let mut out = crate::annotations::camera_view(Some(&Value::Object(camera)))?;
    let pitch = out["pitch"].as_f64().unwrap_or(0.);
    if !(-89.5..=89.5).contains(&pitch) {
        return Err("View pitch must be between -89.5 and 89.5 degrees".into());
    }
    for key in ["grid", "comment_pins"] {
        match s.get(key) {
            None => {
                out.insert(key.into(), json!(true));
            }
            Some(Value::Bool(b)) => {
                out.insert(key.into(), json!(b));
            }
            Some(_) => return Err(format!("{key} must be boolean")),
        }
    }
    let mode = s.get("display_mode").map_or(Some("shaded_edges"), Value::as_str);
    let mode = mode.filter(|m| DISPLAY_MODES.contains(m)).ok_or("Unknown display mode")?;
    out.insert("display_mode".into(), json!(mode));
    let section = s.get("section").cloned().unwrap_or(json!({"enabled": false, "plane": null}));
    let sec = section.as_object().filter(|o| o.keys().all(|k| k == "enabled" || k == "plane")).ok_or("Section requires enabled and an optional plane")?;
    let enabled = match sec.get("enabled") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("Section requires enabled and an optional plane".into()),
    };
    let plane = match sec.get("plane").filter(|p| !p.is_null()) {
        None => Value::Null,
        Some(p) => {
            let p = p.as_object().filter(|o| o.len() == 3 && ["origin", "normal", "x_axis"].iter().all(|k| o.contains_key(*k))).ok_or("Section plane requires origin, normal and x_axis")?;
            let origin = unit(&p["origin"])?;
            let norm = |v: [f64; 3]| -> Result<[f64; 3], String> {
                let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                if l < 1e-10 {
                    return Err("Section axes must be nonzero".into());
                }
                Ok(v.map(|x| x / l))
            };
            let normal = norm(unit(&p["normal"])?)?;
            let x_axis = norm(unit(&p["x_axis"])?)?;
            if (normal[0] * x_axis[0] + normal[1] * x_axis[1] + normal[2] * x_axis[2]).abs() > 1e-6 {
                return Err("Section axes must be perpendicular".into());
            }
            json!({"origin": origin, "normal": normal, "x_axis": x_axis})
        }
    };
    if enabled && plane.is_null() {
        return Err("An enabled section needs a plane".into());
    }
    out.insert("section".into(), json!({"enabled": enabled, "plane": plane}));
    Ok(Value::Object(out))
}

/// A section plane through `origin` with `normal`, its in-plane x axis chosen
/// perpendicular (world X projected, else world Y), as the state stores it.
pub fn section_plane(origin: [f64; 3], normal: [f64; 3]) -> Result<Value, String> {
    let l = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    if !(l > 1e-10) || origin.iter().any(|v| !v.is_finite()) {
        return Err("A section needs a finite origin and a nonzero normal".into());
    }
    let n = normal.map(|v| v / l);
    let pick = if n[0].abs() < 0.9 { [1., 0., 0.] } else { [0., 1., 0.] };
    let d = pick[0] * n[0] + pick[1] * n[1] + pick[2] * n[2];
    let x = [pick[0] - d * n[0], pick[1] - d * n[1], pick[2] - d * n[2]];
    let xl = (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt();
    Ok(json!({"origin": origin, "normal": n, "x_axis": x.map(|v| v / xl)}))
}

fn name(v: &str) -> Result<String, String> {
    let n = v.trim();
    if n.is_empty() || n.chars().count() > 120 {
        return Err("View name must contain 1–120 characters".into());
    }
    Ok(n.to_string())
}

fn description(v: &str) -> Result<String, String> {
    if v.chars().count() > 1000 {
        return Err("View description must be text of at most 1000 characters".into());
    }
    Ok(v.trim().to_string())
}

fn parts(doc: &ArchiveDocument, ids: &[String]) -> Result<Vec<String>, String> {
    if ids.len() > crate::annotations::MAX_PARTS {
        return Err("A view shows at most 200 parts".into());
    }
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(format!("view part {id} is listed twice"));
        }
        if doc.node(id).is_none() {
            return Err(format!("view part {id} does not exist"));
        }
    }
    Ok(ids.to_vec())
}

/// Every saved view, in the order they were saved.
pub fn list(doc: &ArchiveDocument) -> Vec<Value> {
    let mut out: Vec<Value> = doc.manifest["saved_views"].as_object().into_iter().flat_map(|m| m.values().cloned()).collect();
    out.sort_by(|a, b| a["created_at"].as_str().unwrap_or("").cmp(b["created_at"].as_str().unwrap_or("")));
    out
}

/// A view to save or the fields of one to change.
#[derive(Clone, Debug, Default)]
pub struct ViewFields {
    pub name: Option<String>,
    pub state: Option<Value>,
    pub description: Option<String>,
    pub parts: Option<Vec<String>>,
    pub author: Option<String>,
    pub author_kind: Option<String>,
}

/// `save_view`: the new view's id.
pub fn save(edit: &mut Edit, doc: &ArchiveDocument, f: ViewFields) -> Result<String, String> {
    let n = name(f.name.as_deref().unwrap_or(""))?;
    let state = validate_state(f.state.as_ref().ok_or("A saved view needs its state")?)?;
    let kind = crate::annotations::AuthorKind::parse(f.author_kind.as_deref())?;
    let id = new_id();
    let ts = now_iso();
    let mut view = json!({"id": id, "name": n, "state": state, "author_kind": kind.name(), "created_at": ts, "updated_at": ts});
    if let Some(d) = f.description.as_deref().map(description).transpose()?.filter(|d| !d.is_empty()) {
        view["description"] = json!(d);
    }
    if let Some(p) = f.parts.as_deref().map(|p| parts(doc, p)).transpose()?.filter(|p| !p.is_empty()) {
        view["parts"] = json!(p);
    }
    if let Some(a) = f.author.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
        view["author"] = json!(a);
    }
    edit.object_mut("saved_views").insert(id.clone(), view);
    Ok(id)
}

/// `update_saved_view`: only the given fields change (an empty description
/// or part list removes it).
pub fn update(edit: &mut Edit, doc: &ArchiveDocument, id: &str, f: ViewFields) -> Result<(), String> {
    let mut view = edit.object_mut("saved_views").get(id).cloned().ok_or_else(|| format!("no saved view {id}"))?;
    if let Some(n) = &f.name {
        view["name"] = json!(name(n)?);
    }
    if let Some(s) = &f.state {
        view["state"] = validate_state(s)?;
    }
    let o = view.as_object_mut().ok_or("saved view must be an object")?;
    match f.description.as_deref().map(description).transpose()? {
        Some(d) if d.is_empty() => {
            o.remove("description");
        }
        Some(d) => {
            o.insert("description".into(), json!(d));
        }
        None => {}
    }
    match f.parts.as_deref().map(|p| parts(doc, p)).transpose()? {
        Some(p) if p.is_empty() => {
            o.remove("parts");
        }
        Some(p) => {
            o.insert("parts".into(), json!(p));
        }
        None => {}
    }
    o.insert("updated_at".into(), json!(now_iso()));
    edit.object_mut("saved_views").insert(id.to_string(), view);
    Ok(())
}

/// `delete_saved_view`.
pub fn delete(edit: &mut Edit, id: &str) -> Result<(), String> {
    edit.object_mut("saved_views").remove(id).map(|_| ()).ok_or_else(|| format!("no saved view {id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_take_robocads_defaults_and_refuse_bad_sections() {
        let s = validate_state(&json!({"yaw": 10})).unwrap();
        assert_eq!(s["distance"], 250);
        assert_eq!(s["display_mode"], "shaded_edges");
        assert_eq!(s["section"], json!({"enabled": false, "plane": null}));
        assert!(validate_state(&json!({"section": {"enabled": true}})).is_err());
        assert!(validate_state(&json!({"section": {"enabled": true, "plane": {"origin": [0, 0, 0], "normal": [0, 0, 1], "x_axis": [0, 1, 1]}}})).is_err());
        let plane = section_plane([1., 2., 3.], [0., 0., 2.]).unwrap();
        let ok = validate_state(&json!({"section": {"enabled": true, "plane": plane}})).unwrap();
        assert_eq!(ok["section"]["plane"]["normal"], json!([0.0, 0.0, 1.0]));
        assert!(validate_state(&json!({"pitch": 95})).is_err());
        assert!(validate_state(&json!({"isolate": []})).is_err(), "extras live beside the state, not in it");
    }
}

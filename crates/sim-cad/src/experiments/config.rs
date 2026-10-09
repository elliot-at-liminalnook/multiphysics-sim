//! Version-one experiment inputs (RoboCAD's `experiment_config.py`): the
//! fidelity profiles, settings and request validation, and CAD overrides
//! applied to a derived model with the evidence of what they changed.
use serde_json::{Map, Value, json};

/// The fidelity profiles and their defaults.
pub fn profile(name: &str) -> Result<Value, String> {
    match name {
        "quick_check" => Ok(json!({"seconds": 3.2, "step": 0.0005, "sample": 0.01, "contact": false, "flex": false, "planar": false, "noise": false})),
        "validation" => Ok(json!({"seconds": 3.2, "step": 0.00025, "sample": 0.005, "contact": true, "flex": true, "planar": false, "noise": true})),
        other => Err(format!("Unknown profile {other}; choose quick_check or validation")),
    }
}

/// Exact in both Rhai integers and native numeric parameters.
pub const MAX_SEED: u64 = (1 << 53) - 1;

/// A captured `configure` declaration's place, prefixed to a failure.
pub fn located(location: &Value, e: String) -> String {
    match location.as_object() {
        Some(l) => format!("{}:{}:{}: {e}", l.get("source").and_then(Value::as_str).unwrap_or(""), l.get("line").and_then(Value::as_u64).unwrap_or(0), l.get("column").and_then(Value::as_u64).unwrap_or(0)),
        None => e,
    }
}

/// `value` is an object whose keys are all in `allowed`.
pub fn object_fields<'v>(value: &'v Value, allowed: &[&str], label: &str) -> Result<&'v Map<String, Value>, String> {
    let m = value.as_object().ok_or_else(|| format!("{label} must be an object"))?;
    let mut unknown: Vec<&String> = m.keys().filter(|k| !allowed.contains(&k.as_str())).collect();
    if !unknown.is_empty() {
        unknown.sort();
        return Err(format!("Unknown {label} fields: {unknown:?}"));
    }
    Ok(m)
}

/// The profile's settings with `overrides`, checked.
pub fn settings(overrides: &Value, profile_name: &str) -> Result<Value, String> {
    let defaults = profile(profile_name)?;
    let keys: Vec<&str> = defaults.as_object().expect("object").keys().map(String::as_str).collect();
    let o = object_fields(overrides, &keys, "settings")?;
    let mut value = defaults.clone();
    for (k, v) in o {
        value[k.as_str()] = v.clone();
    }
    for name in ["seconds", "step", "sample"] {
        if !value[name].as_f64().is_some_and(|v| v.is_finite() && v > 0.0) || value[name].is_boolean() {
            return Err(format!("settings.{name} must be positive and finite"));
        }
    }
    if value["sample"].as_f64() < value["step"].as_f64() {
        return Err("settings.sample must be at least settings.step".into());
    }
    for name in ["contact", "flex", "planar", "noise"] {
        if !value[name].is_boolean() {
            return Err(format!("settings.{name} must be a boolean"));
        }
    }
    Ok(value)
}

/// A checked experiment request with its defaults (preflight false,
/// profile quick_check, seed 0, the profile's settings).
pub fn request(value: &Value) -> Result<Value, String> {
    object_fields(value, &["expected_revision", "system", "parameters", "controller", "settings", "label", "parent_run", "candidate_id", "profile", "seed", "preflight", "document_id"], "experiment")?;
    let mut v = value.clone();
    if v.get("preflight").is_none_or(Value::is_null) {
        v["preflight"] = json!(false);
    }
    if !v["preflight"].is_boolean() {
        return Err("preflight must be a boolean".into());
    }
    if v.get("profile").is_none_or(|p| p.is_null() || p == "") {
        v["profile"] = json!("quick_check");
    }
    if v.get("seed").is_none_or(Value::is_null) {
        v["seed"] = json!(0);
    }
    if !v["seed"].as_u64().is_some_and(|s| s <= MAX_SEED) {
        return Err(format!("seed must be an integer in [0, {MAX_SEED}]"));
    }
    let profile_name = v["profile"].as_str().ok_or("profile must be text")?.to_string();
    let overrides = v.get("settings").filter(|s| !s.is_null()).cloned().unwrap_or_else(|| json!({}));
    v["settings"] = settings(&overrides, &profile_name)?;
    if v.get("parameters").is_some_and(|p| !p.is_null() && !p.is_object()) {
        return Err("parameters must be an object".into());
    }
    if let Some(controller) = v.get("controller").filter(|c| !c.is_null()) {
        object_fields(controller, &["language", "sources", "parameters", "command", "process", "seam", "interface"], "controller")?;
        match controller["language"].as_str() {
            Some("rhai") => {}
            Some("process") => {
                return Err("controller.language process: an external process controller runs through the controller seam (cad_controllers, sim_controller), not a captured CAD experiment; use a Rhai controller here".into());
            }
            _ => return Err("controller.language must be rhai or process".into()),
        }
        if controller.get("parameters").is_some_and(|p| !p.is_null() && !p.is_object()) {
            return Err("controller.parameters must be an object".into());
        }
        if !matches!(controller.get("interface").and_then(Value::as_str).unwrap_or("position_target"), "position_target" | "driver_duty") {
            return Err("controller.interface must be position_target or driver_duty".into());
        }
        if controller.get("command").is_some() {
            return Err("Rhai controllers use sources, not a process command".into());
        }
        if controller.get("process").is_some() {
            return Err("Rhai controllers use sources, not a process bundle".into());
        }
    }
    Ok(v)
}

/// One JSON pointer segment unescaped (`~1` → `/`, `~0` → `~`).
fn segment(part: &str) -> Result<String, String> {
    if part.replace("~0", "").replace("~1", "").contains('~') {
        return Err("invalid JSON pointer escape".into());
    }
    Ok(part.replace("~1", "/").replace("~0", "~"))
}

/// Apply `overrides` (`[{section, id, field, value}]`) to a derived model:
/// each changes one existing numeric field of one links/joints/motors entry.
/// The evidence (with each value before).
pub fn cad_overrides(model: &mut Value, overrides: &Value, location: &Value) -> Result<Vec<Value>, String> {
    let list = overrides.as_array().ok_or("cad_overrides must be an array")?;
    let mut evidence = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for o in list {
        let m = object_fields(o, &["section", "id", "field", "value"], "CAD override")?;
        if m.len() != 4 {
            return Err("CAD overrides require section, id, field and value".into());
        }
        let section = m["section"].as_str().unwrap_or("");
        if !matches!(section, "links" | "joints" | "motors") {
            return Err(format!("Invalid CAD override section {section}"));
        }
        let identity = m["id"].as_str().unwrap_or("").to_string();
        let path = m["field"].as_str().filter(|p| p.starts_with('/')).ok_or("CAD override field must be a JSON pointer")?.to_string();
        if (section == "joints" && path == "/physics/flex_patch_radius") || (section == "links" && path.starts_with("/flex/boundary_frames/") && path.contains("/patch/")) {
            return Err("Flex patches must be set before CAD derivation: use set_joint_physics(flex_patch_radius=...) or the joint Properties editor, then capture a new run".into());
        }
        let value = m["value"].as_f64().filter(|v| v.is_finite() && !m["value"].is_boolean());
        let entries = model[section].as_array_mut().ok_or_else(|| format!("the model has no {section}"))?;
        let matches: Vec<usize> = entries.iter().enumerate().filter(|(_, e)| e["id"].as_str() == Some(&identity) || e["name"].as_str() == Some(&identity)).map(|(i, _)| i).collect();
        if matches.len() != 1 {
            return Err(format!("CAD override {identity} must identify one {section} entry"));
        }
        let item = &mut entries[matches[0]];
        let key = (section.to_string(), item["id"].as_str().or(item["name"].as_str()).unwrap_or("").to_string(), path.clone());
        if !seen.insert(key) {
            return Err(format!("Conflicting repeated CAD override for {identity}{path}"));
        }
        let (id, name) = (item["id"].clone(), item["name"].clone());
        let parts: Vec<String> = path[1..].split('/').map(segment).collect::<Result<_, _>>().map_err(|e| format!("CAD override {identity}{path}: field does not exist ({e})"))?;
        let target = item.pointer_mut(&path).ok_or_else(|| format!("CAD override {identity}{path}: field does not exist ({})", parts.last().cloned().unwrap_or_default()))?;
        let before = target.clone();
        let Some(value) = value.filter(|_| before.is_number()) else {
            return Err(format!("CAD override {identity}{path} must change an existing numeric field to a finite number"));
        };
        *target = json!(value);
        let mut e = o.clone();
        e["id"] = id;
        e["name"] = name;
        e["before"] = before;
        e["source"] = location.clone();
        evidence.push(e);
    }
    Ok(evidence)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_takes_its_profiles_defaults_and_refuses_unknown_fields() {
        let r = request(&json!({"label": "x"})).unwrap();
        assert_eq!(r["settings"]["step"], 0.0005);
        assert_eq!(r["seed"], 0);
        assert!(request(&json!({"bogus": 1})).unwrap_err().contains("Unknown experiment fields"));
        assert!(request(&json!({"settings": {"sample": 0.0001}})).unwrap_err().contains("sample must be at least"));
        assert!(request(&json!({"controller": {"language": "process"}})).unwrap_err().contains("controller seam"));
    }

    #[test]
    fn an_override_changes_one_numeric_field_and_records_it() {
        let mut model = json!({"links": [{"id": "a", "name": "arm", "mass": 1.0}], "joints": [], "motors": []});
        let ev = cad_overrides(&mut model, &json!([{"section": "links", "id": "arm", "field": "/mass", "value": 2.5}]), &Value::Null).unwrap();
        assert_eq!(model["links"][0]["mass"], 2.5);
        assert_eq!(ev[0]["before"], 1.0);
        assert!(cad_overrides(&mut model, &json!([{"section": "links", "id": "arm", "field": "/name", "value": 1}]), &Value::Null).is_err());
    }
}

//! The document's system composition (RoboCAD's `component_graph.py`):
//! registry components with parameters, optionally attached to CAD bodies,
//! and connections between their ports. Validated before any edit
//! publishes it; lowered into Rhai by `experiments::service::compose_sources`.
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub fn empty() -> Value {
    json!({"version": 1, "components": {}, "connections": {}})
}

/// The geometry recipes a component may derive parameters from, with the
/// component type each applies to and the parameters it supplies.
pub const RECIPES: [(&str, &str, &[&str]); 2] = [("body_thermal_capacity", "thermal.capacitance", &["heat_capacity"]), ("circular_fluid_volume", "fluid.pipe_ph", &["length", "diameter", "rise"])];

/// A component's derivation recipe checked: the parameters it supplies.
pub fn recipe_outputs(component: &Value) -> Result<Vec<&'static str>, String> {
    let Some(recipe) = component.get("derivation").filter(|d| !d.is_null()) else { return Ok(Vec::new()) };
    let kind = recipe["kind"].as_str().unwrap_or("");
    let (_, ty, outputs) = RECIPES.iter().find(|(k, _, _)| *k == kind).ok_or("Unknown component derivation recipe")?;
    if component["type"].as_str() != Some(ty) {
        return Err(format!("{kind} applies to {ty}"));
    }
    if component["body_id"].as_str().is_none_or(str::is_empty) {
        return Err("A geometry derivation requires an attached CAD body".into());
    }
    let fields: Vec<&str> = recipe.as_object().into_iter().flatten().map(|(k, _)| k.as_str()).collect();
    if kind == "body_thermal_capacity" {
        if fields.iter().any(|f| !matches!(*f, "kind" | "specific_heat")) {
            return Err("Unknown thermal-capacity recipe field".into());
        }
        if let Some(cp) = recipe.get("specific_heat").filter(|v| !v.is_null())
            && !cp.as_f64().is_some_and(|c| c.is_finite() && c > 0.0)
        {
            return Err("specific_heat must be positive and finite [J/(kg·K)]".into());
        }
    } else {
        if fields.iter().any(|f| !matches!(*f, "kind" | "flow_direction")) {
            return Err("Unknown circular-fluid-volume recipe field".into());
        }
        let d = recipe.get("flow_direction").and_then(Value::as_f64).unwrap_or(1.0);
        if d != 1.0 && d != -1.0 {
            return Err("flow_direction must be +1 or -1 along the CAD cylinder axis".into());
        }
    }
    let explicit: Vec<&&str> = outputs.iter().filter(|o| component["parameters"].get(**o).is_some()).collect();
    if !explicit.is_empty() {
        return Err(format!("Derived parameters also have explicit values: {explicit:?}"));
    }
    Ok(outputs.to_vec())
}

/// `graph` checked against the manifest's nodes: ids, names, bindings,
/// bodies, parameters, recipes and connections (each port in one connection).
pub fn validate(graph: &Value, manifest: &Value) -> Result<Value, String> {
    let g = graph.as_object().filter(|g| g.len() == 3 && g.contains_key("version") && g.contains_key("components") && g.contains_key("connections")).ok_or("System graph requires version, components and connections")?;
    if g["version"].as_u64() != Some(1) || g["version"].is_f64() {
        return Err("Unsupported system graph version".into());
    }
    for section in ["components", "connections"] {
        let m = g[section].as_object().ok_or_else(|| format!("{section} must be an object keyed by stable ID"))?;
        if m.keys().any(|k| k.trim().is_empty()) {
            return Err(format!("Invalid {section} ID"));
        }
    }
    let body = |id: &str| manifest["nodes"].as_array().into_iter().flatten().any(|n| n["id"] == id && matches!(n["kind"].as_str(), Some("body" | "instance")));
    let (mut names, mut bindings) = (BTreeSet::new(), BTreeSet::new());
    for (identity, c) in g["components"].as_object().expect("checked") {
        let m = c.as_object().filter(|m| m.keys().all(|k| matches!(k.as_str(), "id" | "name" | "type" | "body_id" | "parameters" | "derivation" | "binding"))).ok_or_else(|| format!("Invalid component {identity}"))?;
        if m.get("id").and_then(Value::as_str) != Some(identity) {
            return Err(format!("Component ID mismatch: {identity}"));
        }
        for key in ["name", "type"] {
            if m.get(key).and_then(Value::as_str).is_none_or(|s| s.trim().is_empty()) {
                return Err(format!("Component {identity} requires {key}"));
            }
        }
        let name = m["name"].as_str().expect("checked");
        if !names.insert(name.to_string()) {
            return Err(format!("Duplicate component name: {name}"));
        }
        if let Some(b) = m.get("binding").filter(|b| !b.is_null()) {
            let b = b.as_str().filter(|s| !s.trim().is_empty()).ok_or("A component binding requires an imported native name")?;
            if !bindings.insert(b.to_string()) {
                return Err(format!("Imported component {b} is bound more than once"));
            }
        }
        if let Some(b) = m.get("body_id").filter(|b| !b.is_null())
            && !b.as_str().is_some_and(body)
        {
            return Err(format!("Component {identity} refers to missing CAD body {b}"));
        }
        let parameters = m.get("parameters").cloned().unwrap_or_else(|| json!({}));
        let p = parameters.as_object().ok_or_else(|| format!("Component {identity} parameters must be an object"))?;
        for (k, v) in p {
            if k.is_empty() || !v.as_f64().is_some_and(f64::is_finite) || v.is_boolean() {
                return Err(format!("Component {identity} parameter {k} must be a finite number"));
            }
        }
        recipe_outputs(c)?;
    }
    let mut occupied = BTreeSet::new();
    for (identity, c) in g["connections"].as_object().expect("checked") {
        let m = c.as_object().filter(|m| m.len() == 2 && m.get("id").and_then(Value::as_str) == Some(identity)).ok_or_else(|| format!("Invalid connection {identity}"))?;
        let ports = m.get("ports").and_then(Value::as_array).filter(|p| !p.is_empty()).ok_or_else(|| format!("Connection {identity} requires ports"))?;
        for port in ports {
            let p = port.as_object().filter(|p| p.len() == 2 && p.contains_key("component_id") && p.contains_key("port")).ok_or_else(|| format!("Invalid endpoint in connection {identity}"))?;
            let component = p["component_id"].as_str().unwrap_or("");
            if !g["components"].as_object().expect("checked").contains_key(component) {
                return Err(format!("Connection {identity} refers to missing component {component}"));
            }
            let name = p["port"].as_str().filter(|s| !s.is_empty()).ok_or_else(|| format!("Connection {identity} requires a port name"))?;
            if !occupied.insert((component.to_string(), name.to_string())) {
                return Err(format!("Port ({component:?}, {name:?}) belongs to more than one connection"));
            }
        }
    }
    Ok(graph.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_graph_refuses_missing_bodies_and_shared_ports() {
        let manifest = json!({"nodes": [{"id": "b1", "kind": "body"}]});
        let mut g = json!({"version": 1, "components": {
            "c1": {"id": "c1", "name": "Heater", "type": "thermal.capacitance", "body_id": "b1", "derivation": {"kind": "body_thermal_capacity"}},
            "c2": {"id": "c2", "name": "Sink", "type": "thermal.conductor", "parameters": {"g": 2.0}},
        }, "connections": {"k1": {"id": "k1", "ports": [{"component_id": "c1", "port": "p"}, {"component_id": "c2", "port": "a"}]}}});
        assert!(validate(&g, &manifest).is_ok());
        g["components"]["c2"]["body_id"] = json!("nope");
        assert!(validate(&g, &manifest).unwrap_err().contains("missing CAD body"));
        g["components"]["c2"]["body_id"] = Value::Null;
        g["connections"]["k2"] = json!({"id": "k2", "ports": [{"component_id": "c1", "port": "p"}]});
        assert!(validate(&g, &manifest).unwrap_err().contains("more than one connection"));
    }
}

/// One graph edit (RoboCAD's `edit_graph`): `command` is `{op: add,
/// component}` (a new id unless the component has one), `{op: update, id,
/// component}` (fields merged), `{op: remove, id}` (its ports leave their
/// connections; empty connections go), `{op: connect, ports}` (joining any
/// connection a port is already on), `{op: open, id}` (a connection
/// removed) or `{op: replace, graph}`. The new graph, validated, and the
/// id the edit made or touched.
pub fn edit(graph: &Value, manifest: &Value, command: &Value) -> Result<(Value, Option<String>), String> {
    let mut g = if graph.is_object() { graph.clone() } else { empty() };
    let new_id = || format!("{}{}{}", crate::edit::new_id(), crate::edit::new_id(), &crate::edit::new_id()[..8]);
    let op = command["op"].as_str().ok_or("a system edit needs op (add, update, remove, connect, open, replace)")?;
    let mut touched = command["id"].as_str().map(str::to_string);
    match op {
        "add" => {
            let mut c = command["component"].clone();
            let id = c["id"].as_str().filter(|s| !s.is_empty() && *s != "draft").map(str::to_string).unwrap_or_else(new_id);
            if g["components"].get(&id).is_some() {
                return Err(format!("Component {id} already exists"));
            }
            c["id"] = json!(id);
            for (k, v) in c.clone().as_object().into_iter().flatten() {
                if v.is_null() {
                    c.as_object_mut().expect("object").remove(k);
                }
            }
            g["components"][&id] = c;
            touched = Some(id);
        }
        "update" => {
            let id = touched.clone().ok_or("update needs the component's id")?;
            let mut c = g["components"].get(&id).cloned().ok_or_else(|| format!("no component {id}"))?;
            for (k, v) in command["component"].as_object().into_iter().flatten() {
                if v.is_null() {
                    c.as_object_mut().expect("object").remove(k);
                } else {
                    c[k.as_str()] = v.clone();
                }
            }
            if c["id"].as_str() != Some(id.as_str()) {
                return Err("Component identity cannot be changed".into());
            }
            g["components"][&id] = c;
        }
        "remove" => {
            let id = touched.clone().ok_or("remove needs the component's id")?;
            g["components"].as_object_mut().ok_or("components must be an object")?.remove(&id).ok_or_else(|| format!("no component {id}"))?;
            let connections = g["connections"].as_object_mut().ok_or("connections must be an object")?;
            for c in connections.values_mut() {
                if let Some(ports) = c["ports"].as_array_mut() {
                    ports.retain(|p| p["component_id"].as_str() != Some(id.as_str()));
                }
            }
            connections.retain(|_, c| c["ports"].as_array().is_some_and(|p| !p.is_empty()));
        }
        "connect" => {
            let mut ports: Vec<Value> = command["ports"].as_array().cloned().ok_or("connect needs ports")?;
            let connections = g["connections"].as_object_mut().ok_or("connections must be an object")?;
            // Connecting to an existing node extends that node.
            let mut joined_id = None;
            let mut joined = Vec::new();
            for (cid, c) in connections.clone() {
                if c["ports"].as_array().into_iter().flatten().any(|p| ports.contains(p)) {
                    joined_id.get_or_insert(cid.clone());
                    joined.extend(c["ports"].as_array().cloned().unwrap_or_default());
                    connections.remove(&cid);
                }
            }
            for p in joined {
                if !ports.contains(&p) {
                    ports.push(p);
                }
            }
            let id = touched.clone().or(joined_id).unwrap_or_else(new_id);
            if connections.contains_key(&id) {
                return Err(format!("Connection {id} already exists"));
            }
            connections.insert(id.clone(), json!({"id": id, "ports": ports}));
            touched = Some(id);
        }
        "open" => {
            let id = touched.clone().ok_or("open needs the connection's id")?;
            g["connections"].as_object_mut().ok_or("connections must be an object")?.remove(&id).ok_or_else(|| format!("no connection {id}"))?;
        }
        "replace" => g = command["graph"].clone(),
        other => return Err(format!("Unknown system edit {other}")),
    }
    let g = validate(&g, manifest)?;
    Ok((g, touched))
}

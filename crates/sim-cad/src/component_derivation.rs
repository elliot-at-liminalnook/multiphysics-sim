//! Explicit geometry-to-component recipes (RoboCAD's
//! `component_derivation.py`); dimensions cross from mm to SI here. A
//! component whose graph entry carries a `derivation` recipe gets the
//! recipe's parameters from its attached CAD body instead of authored
//! values; every derivation records its inputs, formula and limitations
//! as evidence. Recipe validation itself lives in
//! [`crate::component_graph::recipe_outputs`].
use crate::archive::ArchiveDocument;
use crate::component_graph::recipe_outputs;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// The parameters each recipe supplies, with their SI units.
fn units(kind: &str) -> &'static [(&'static str, &'static str)] {
    match kind {
        "body_thermal_capacity" => &[("heat_capacity", "J/K")],
        _ => &[("length", "m"), ("diameter", "m"), ("rise", "m")],
    }
}

fn v3(v: &Value) -> Result<[f64; 3], String> {
    let a = v.as_array().filter(|a| a.len() == 3).ok_or("expected a 3-vector")?;
    Ok([a[0].as_f64().ok_or("expected a 3-vector")?, a[1].as_f64().ok_or("expected a 3-vector")?, a[2].as_f64().ok_or("expected a 3-vector")?])
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * a.abs().max(b.abs())
}

/// `heat_capacity = volume × density × specific_heat` (volume in mm³,
/// density in kg/m³): the mass in kg and the capacity in J/K.
fn thermal(volume_mm3: f64, density_kg_m3: f64, cp: f64) -> (f64, f64) {
    let mass = volume_mm3 * 1e-9 * density_kg_m3;
    (mass, mass * cp)
}

/// The fluid-volume recipe on a body's faces (RoboCAD `FaceRef` JSON) and
/// its exact volume in mm³: the parameter values (SI) and the extra
/// evidence inputs. The body must be one closed circular cylinder.
fn fluid(faces: &[Value], volume_mm3: f64, name: &str, direction: f64) -> Result<(Map<String, Value>, Map<String, Value>), String> {
    let cylinders: Vec<&Value> = faces.iter().filter(|f| f["kind"] == "cylinder").collect();
    let mut caps: Vec<&Value> = faces.iter().filter(|f| f["kind"] == "plane").collect();
    if faces.len() != 3 || cylinders.len() != 1 || caps.len() != 2 {
        return Err(format!("{name}: select a closed circular cylinder representing the fluid volume, not the duct wall"));
    }
    let cylinder = cylinders[0];
    let radius = cylinder["radius"].as_f64().ok_or("cylinder face has no radius")?;
    let raw_axis = v3(&cylinder["axis_dir"])?;
    let norm = dot(raw_axis, raw_axis).sqrt();
    if !(norm > 0.0) {
        return Err("cylinder face has no axis".into());
    }
    let axis = [raw_axis[0] / norm, raw_axis[1] / norm, raw_axis[2] / norm];
    let mut sorted: Vec<([f64; 3], [f64; 3], f64)> = caps
        .drain(..)
        .map(|f| Ok((v3(&f["centroid"])?, v3(&f["normal"])?, f["area"].as_f64().ok_or("cap face has no area")?)))
        .collect::<Result<_, String>>()?;
    sorted.sort_by(|a, b| dot(a.0, axis).partial_cmp(&dot(b.0, axis)).unwrap_or(std::cmp::Ordering::Equal));
    let vector = [sorted[1].0[0] - sorted[0].0[0], sorted[1].0[1] - sorted[0].0[1], sorted[1].0[2] - sorted[0].0[2]];
    let length = dot(vector, axis);
    let area = std::f64::consts::PI * radius * radius;
    let mut aligned = sorted.iter().all(|c| (dot(c.1, axis).abs() - 1.0).abs() < 1e-7);
    aligned &= (0..3).map(|i| (vector[i] - length * axis[i]).powi(2)).sum::<f64>() < 1e-10 * length.powi(2).max(1.0);
    if length <= 0.0 || radius <= 0.0 || !aligned || !close(volume_mm3, area * length) {
        return Err(format!("{name}: fluid-volume cylinder must have full circular caps normal to its axis"));
    }
    if sorted.iter().any(|c| !close(c.2, area)) {
        return Err("Fluid-volume end caps must be full circles".into());
    }
    let (a, b) = if direction < 0.0 { (sorted[1].0, sorted[0].0) } else { (sorted[0].0, sorted[1].0) };
    let values: Map<String, Value> = [
        ("length".to_string(), json!(length * 0.001)),
        ("diameter".to_string(), json!(2.0 * radius * 0.001)),
        ("rise".to_string(), json!((b[2] - a[2]) * 0.001)),
    ]
    .into_iter()
    .collect();
    let inputs: Map<String, Value> = [
        ("representation".to_string(), json!("fluid_volume")),
        ("axis".to_string(), json!(axis)),
        ("end_a_mm".to_string(), json!(a)),
        ("end_b_mm".to_string(), json!(b)),
        ("area_m2".to_string(), json!(area * 1e-6)),
    ]
    .into_iter()
    .collect();
    Ok((values, inputs))
}

/// RoboCAD's `derive_graph`: a copy of `graph` with every recipe replaced
/// by the parameters it derives from the attached body in `doc`, and one
/// evidence record per derivation. Bodies, volumes and face lists are
/// memoised per geometry hash within the call (RoboCAD's derivation
/// cache stages `body_properties` and `body_faces`).
pub fn derive_graph(graph: &Value, doc: &ArchiveDocument, cancelled: &dyn Fn() -> bool) -> Result<(Value, Vec<Value>), String> {
    let mut resolved = graph.clone();
    let mut evidence = Vec::new();
    let mut volumes: HashMap<String, f64> = HashMap::new();
    let mut face_lists: HashMap<String, Vec<Value>> = HashMap::new();
    let components = resolved["components"].as_object_mut().ok_or("component graph requires a components object")?;
    for (identity, component) in components.iter_mut() {
        let outputs = recipe_outputs(component)?;
        if outputs.is_empty() {
            continue;
        }
        let name = component["name"].as_str().unwrap_or("").to_string();
        let body_id = component["body_id"].as_str().unwrap_or("").to_string();
        let node = doc.node(&body_id).ok_or_else(|| format!("{name}: attached body {body_id} is not in the document"))?.clone();
        let bytes = crate::geometry::resolved_brep(doc, &body_id).map_err(|e| format!("{name}: attached body has no solid geometry: {e}"))?;
        let geometry_hash = format!("{:x}", Sha256::digest(&bytes));
        let volume = match volumes.get(&geometry_hash) {
            Some(v) => *v,
            None => {
                let v = crate::geometry::body_geometry(&bytes, true, 0.1, cancelled)?.properties.volume_mm3;
                volumes.insert(geometry_hash.clone(), v);
                v
            }
        };
        if !volume.is_finite() || volume <= 0.0 {
            return Err(format!("{name}: derivation requires a positive finite solid volume"));
        }
        let recipe = component["derivation"].clone();
        let kind = recipe["kind"].as_str().unwrap_or("");
        let mut inputs: Map<String, Value> = [
            ("body_id".to_string(), json!(body_id)),
            ("geometry_hash".to_string(), json!(geometry_hash)),
            ("volume_mm3".to_string(), json!(volume)),
        ]
        .into_iter()
        .collect();
        let (values, formula, limitations): (Map<String, Value>, &str, &str) = if kind == "body_thermal_capacity" {
            let material_id = node["material"].as_str().unwrap_or("");
            let material = doc.manifest["materials"].as_array().into_iter().flatten().find(|m| m["id"] == material_id).ok_or_else(|| format!("{name}: assign a material to the body"))?;
            let density = material["density"].as_f64().unwrap_or(f64::NAN) * 1000.0; // g/cm³ → kg/m³
            let authored = recipe.get("specific_heat").filter(|v| !v.is_null()).and_then(Value::as_f64);
            let cp = authored.unwrap_or_else(|| crate::materials::engineering(material, None)["specific_heat"].as_f64().unwrap_or(f64::NAN));
            if !density.is_finite() || density <= 0.0 || !cp.is_finite() || cp <= 0.0 {
                return Err("Thermal derivation requires positive density and specific heat".into());
            }
            let (mass, heat_capacity) = thermal(volume, density, cp);
            inputs.extend([
                ("material_id".to_string(), json!(material["id"])),
                ("density_kg_m3".to_string(), json!(density)),
                ("specific_heat_j_kg_k".to_string(), json!(cp)),
                ("specific_heat_source".to_string(), json!(if authored.is_some() { "recipe" } else { "material" })),
                ("mass_kg".to_string(), json!(mass)),
            ]);
            (
                [("heat_capacity".to_string(), json!(heat_capacity))].into_iter().collect(),
                "heat_capacity = volume × density × specific_heat",
                "Uniform body temperature; constant material properties; full CAD solid volume.",
            )
        } else {
            let faces = match face_lists.get(&geometry_hash) {
                Some(f) => f.clone(),
                None => {
                    let topo = crate::kernel::full_topology(&bytes, 2)?;
                    let f = topo["faces"].as_array().cloned().ok_or("the topology answer has no faces")?;
                    face_lists.insert(geometry_hash.clone(), f.clone());
                    f
                }
            };
            let direction = recipe.get("flow_direction").and_then(Value::as_f64).unwrap_or(1.0);
            let (values, extra) = fluid(&faces, volume, &name, direction)?;
            inputs.extend(extra);
            (values, "length = cap separation; diameter = 2 × radius; rise = z(b) − z(a)", "Straight uniform circular passage; native lumped water pipe model; no bends or local losses inferred.")
        };
        let output_units: Map<String, Value> = units(kind)
            .iter()
            .filter_map(|(k, u)| values.get(*k).map(|v| (k.to_string(), json!({"value": v, "unit": u}))))
            .collect();
        let parameters = component
            .as_object_mut()
            .ok_or_else(|| format!("{name}: component must be an object"))?
            .entry("parameters".to_string())
            .or_insert_with(|| json!({}));
        let parameters = parameters.as_object_mut().ok_or_else(|| format!("{name}: parameters must be an object"))?;
        parameters.extend(values);
        component.as_object_mut().expect("checked").remove("derivation");
        evidence.push(json!({
            "component_id": identity, "name": name, "recipe": recipe, "inputs": inputs,
            "outputs": output_units, "formula": formula, "limitations": [limitations],
        }));
    }
    Ok((resolved, evidence))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn cylinder_faces() -> Vec<Value> {
        let area = PI * 100.0;
        vec![
            json!({"kind": "cylinder", "centroid": [0, 0, 25], "normal": [1, 0, 0], "area": 2.0 * PI * 500.0, "axis_dir": [0, 0, 2], "radius": 10}),
            json!({"kind": "plane", "centroid": [0, 0, 50], "normal": [0, 0, 1], "area": area}),
            json!({"kind": "plane", "centroid": [0, 0, 0], "normal": [0, 0, -1], "area": area}),
        ]
    }

    #[test]
    fn a_fluid_cylinder_derives_length_diameter_and_signed_rise() {
        let volume = PI * 100.0 * 50.0;
        let (values, inputs) = fluid(&cylinder_faces(), volume, "Pipe", 1.0).unwrap();
        let near = |v: &Value, x: f64| (v.as_f64().unwrap() - x).abs() < 1e-12;
        assert!(near(&values["length"], 0.05) && near(&values["diameter"], 0.02) && near(&values["rise"], 0.05));
        assert_eq!(inputs["end_a_mm"], json!([0.0, 0.0, 0.0]));
        let (values, _) = fluid(&cylinder_faces(), volume, "Pipe", -1.0).unwrap();
        assert!(near(&values["rise"], -0.05));
    }

    #[test]
    fn a_fluid_volume_refuses_anything_but_a_closed_cylinder() {
        let faces = &cylinder_faces()[..2];
        assert!(fluid(faces, PI * 100.0 * 50.0, "Pipe", 1.0).unwrap_err().contains("closed circular cylinder"));
        assert!(fluid(&cylinder_faces(), 1.0, "Pipe", 1.0).unwrap_err().contains("full circular caps"));
    }

    #[test]
    fn thermal_capacity_is_volume_times_density_times_specific_heat() {
        // 1000 mm³ of a 1240 kg/m³ material at 1800 J/(kg·K).
        let (mass, capacity) = thermal(1000.0, 1240.0, 1800.0);
        assert!((mass - 0.00124).abs() < 1e-12);
        assert!((capacity - 2.232).abs() < 1e-9);
    }
}

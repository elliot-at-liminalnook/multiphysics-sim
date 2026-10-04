//! Engineering properties of a document's materials: RoboCAD's
//! `default_engineering` (document.py) with the print registry's values for
//! the filaments it backs (`print_registry.cad_engineering`), under the
//! document's own overrides (`Material.props`). Every value says where it
//! came from (`sources`): the document, the print registry, or RoboCAD's
//! catalogue estimate.
use serde_json::{Map, Value, json};

/// (E Pa, ν, σ_y Pa, σ_u Pa, Tg °C, k W/m·K, cp J/kg·K, α 1/K, µ_k self,
/// µ_k vs steel, allowable bearing Pa, anisotropy, layer adhesion).
type Row = (f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64);

/// RoboCAD's `_ENG` table.
const CATALOGUE: [(&str, Row); 13] = [
    ("pla", (3.5e9, 0.36, 50e6, 60e6, 60.0, 0.13, 1800.0, 68e-6, 0.35, 0.30, 15e6, 0.6, 0.7)),
    ("petg", (2.1e9, 0.38, 45e6, 50e6, 80.0, 0.20, 1200.0, 60e-6, 0.40, 0.32, 20e6, 0.7, 0.8)),
    ("abs", (2.0e9, 0.37, 35e6, 40e6, 105.0, 0.17, 1400.0, 90e-6, 0.35, 0.30, 15e6, 0.6, 0.7)),
    ("asa", (2.0e9, 0.37, 38e6, 45e6, 100.0, 0.17, 1400.0, 90e-6, 0.35, 0.30, 15e6, 0.6, 0.7)),
    ("tpu", (0.03e9, 0.48, 6e6, 30e6, -30.0, 0.19, 1500.0, 150e-6, 0.8, 0.6, 3e6, 0.9, 0.9)),
    ("nylon", (1.6e9, 0.40, 45e6, 50e6, 50.0, 0.25, 1700.0, 100e-6, 0.25, 0.20, 25e6, 0.7, 0.8)),
    ("resin", (2.2e9, 0.38, 40e6, 50e6, 70.0, 0.18, 1500.0, 90e-6, 0.4, 0.3, 12e6, 1.0, 1.0)),
    ("al", (69e9, 0.33, 275e6, 310e6, 500.0, 167.0, 896.0, 23e-6, 0.45, 0.45, 100e6, 1.0, 1.0)),
    ("steel", (200e9, 0.29, 500e6, 600e6, 1000.0, 45.0, 470.0, 12e-6, 0.4, 0.4, 250e6, 1.0, 1.0)),
    ("brass", (100e9, 0.34, 200e6, 350e6, 800.0, 110.0, 380.0, 19e-6, 0.35, 0.35, 80e6, 1.0, 1.0)),
    ("pcb", (20e9, 0.14, 150e6, 300e6, 130.0, 0.3, 1100.0, 15e-6, 0.4, 0.4, 40e6, 1.0, 1.0)),
    ("rubber", (0.005e9, 0.49, 3e6, 15e6, -50.0, 0.16, 2000.0, 200e-6, 1.0, 0.9, 2e6, 1.0, 1.0)),
    ("glass", (3.0e9, 0.37, 60e6, 70e6, 105.0, 0.19, 1470.0, 70e-6, 0.4, 0.35, 20e6, 1.0, 1.0)),
];

/// RoboCAD's `DEFAULT_MATERIALS` (document.py): the stock library a new
/// document starts with (id, name, density g/cm³, colour, roughness, metallic, tags).
pub fn default_library() -> Value {
    let m = |id: &str, name: &str, density: f64, color: [f64; 3], roughness: f64, metallic: f64, tags: &[&str]| json!({"id": id, "name": name, "density": density, "color": color, "roughness": roughness, "metallic": metallic, "tags": tags, "engineering": {}});
    json!([
        m("pla", "PLA", 1.24, [0.85, 0.85, 0.87], 0.6, 0.0, &["print", "plastic"]),
        m("petg", "PETG", 1.27, [0.75, 0.82, 0.9], 0.35, 0.0, &["print", "plastic"]),
        m("abs", "ABS", 1.04, [0.2, 0.2, 0.22], 0.55, 0.0, &["print", "plastic"]),
        m("asa", "ASA", 1.07, [0.9, 0.55, 0.2], 0.5, 0.0, &["print", "plastic"]),
        m("tpu", "TPU 95A", 1.21, [0.3, 0.3, 0.3], 0.9, 0.0, &["print", "flexible"]),
        m("nylon", "Nylon PA12", 1.01, [0.92, 0.92, 0.88], 0.7, 0.0, &["print", "plastic"]),
        m("resin", "Resin (standard)", 1.15, [0.6, 0.6, 0.6], 0.2, 0.0, &["print", "resin"]),
        m("al", "Aluminium 6061", 2.70, [0.8, 0.82, 0.85], 0.35, 1.0, &["metal"]),
        m("steel", "Steel", 7.85, [0.55, 0.56, 0.58], 0.4, 1.0, &["metal"]),
        m("brass", "Brass", 8.5, [0.85, 0.7, 0.35], 0.3, 1.0, &["metal"]),
        m("pcb", "PCB FR4", 1.85, [0.1, 0.45, 0.25], 0.6, 0.0, &["electronics"]),
        m("rubber", "Rubber", 1.1, [0.15, 0.15, 0.15], 0.95, 0.0, &["flexible"]),
        m("glass", "Acrylic", 1.18, [0.8, 0.9, 1.0], 0.05, 0.0, &["clear"]),
    ])
}

/// The keys a material's engineering block has (RoboCAD's `ENGINEERING_KEYS`).
pub const KEYS: [&str; 11] = ["youngs_modulus", "poisson", "yield_strength", "ultimate_strength", "glass_transition_c", "thermal_conductivity", "specific_heat", "thermal_expansion", "friction", "print", "bearing_pressure"];

/// Where a material's engineering value came from.
pub const DOCUMENT: &str = "set in this document";
pub const REGISTRY: &str = "print registry";
pub const CATALOGUE_ESTIMATE: &str = "RoboCAD catalogue estimate (typical values, not measured)";

/// The print registry, as the caller read it (`library/printing/registry.json`
/// and its SHA-256), for the filaments it backs.
pub struct Registry<'a> {
    pub json: &'a Value,
    pub sha256: &'a str,
}

fn row(id: &str, tags: &[String]) -> (Row, bool) {
    match CATALOGUE.iter().find(|(k, _)| *k == id) {
        Some((_, r)) => (*r, true),
        None if tags.iter().any(|t| t == "metal") => (CATALOGUE[8].1, false),
        None => (CATALOGUE[0].1, false),
    }
}

/// A registry quantity's value (`{"value": x, ...}` or a bare number).
fn quantity(v: &Value) -> Option<f64> {
    v.get("value").and_then(Value::as_f64).or_else(|| v.as_f64())
}

/// The registry filament backing CAD material `id` (key, entry).
fn filament<'a>(registry: &'a Registry, id: &str) -> Option<(&'a str, &'a Value)> {
    registry.json["materials"].as_object()?.iter().find(|(_, m)| m["cad_material"] == id).map(|(k, m)| (k.as_str(), m))
}

/// Material `m`'s (a manifest `materials` entry) engineering properties,
/// with `sources` naming each key's origin (see the module doc).
pub fn engineering(m: &Value, registry: Option<&Registry>) -> Value {
    let id = m["id"].as_str().unwrap_or("");
    let tags: Vec<String> = m["tags"].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(str::to_string)).collect();
    let ((e, nu, sy, su, tg, k, cp, alpha, mu_self, mu_steel, bearing, aniso, adhesion), known) = row(id, &tags);
    let is_print = ["pla", "petg", "abs", "asa", "tpu", "nylon"].contains(&id) || (tags.iter().any(|t| t == "print") && !tags.iter().any(|t| t == "resin"));
    let r3 = |x: f64| (x * 1000.0).round() / 1000.0;
    let mut d = json!({
        "youngs_modulus": e, "poisson": nu, "yield_strength": sy, "ultimate_strength": su, "glass_transition_c": tg,
        "thermal_conductivity": k, "specific_heat": cp, "thermal_expansion": alpha,
        "friction": {"self": {"static": r3(mu_self * 1.2), "kinetic": mu_self}, "steel": {"static": r3(mu_steel * 1.2), "kinetic": mu_steel}, "world": {"static": r3(mu_self * 1.2), "kinetic": mu_self}},
        "bearing_pressure": bearing,
        "print": if is_print { json!({"anisotropy_z": aniso, "layer_adhesion_factor": adhesion}) } else { Value::Null },
    });
    let base = if known { CATALOGUE_ESTIMATE.to_string() } else { format!("{CATALOGUE_ESTIMATE}; `{id}` is not in the catalogue, so {} values stand in", if tags.iter().any(|t| t == "metal") { "steel" } else { "PLA" }) };
    let mut sources: Map<String, Value> = KEYS.iter().map(|key| (key.to_string(), json!(base))).collect();
    if let Some(registry) = registry
        && let Some((key, f)) = filament(registry, id)
        && let (Some(e_in), Some(e_across), Some(s_in), Some(s_across)) = (quantity(&f["modulus_in_layer"]), quantity(&f["modulus_across_layers"]), quantity(&f["tensile_in_layer"]), quantity(&f["tensile_across_layers"]))
    {
        let from = format!("{REGISTRY} `{key}` (sha256 {})", registry.sha256);
        d["youngs_modulus"] = json!(e_in);
        d["yield_strength"] = json!(s_in);
        d["ultimate_strength"] = json!(s_in);
        for (out, field) in [("poisson", "poisson"), ("glass_transition_c", "glass_transition_c")] {
            if let Some(x) = quantity(&f[field]) {
                d[out] = json!(x);
                sources.insert(out.into(), json!(from));
            }
        }
        if let Some(b) = quantity(&f["bearing"]) {
            // CAD's bearing value is an allowable: the registry's failure pressure ÷ 3.
            d["bearing_pressure"] = json!(b / 3.0);
            sources.insert("bearing_pressure".into(), json!(format!("{from}: failure pressure ÷ 3")));
        }
        d["print"] = json!({"anisotropy_z": e_across / e_in, "layer_adhesion_factor": s_across / s_in, "registry_material": key, "registry_sha256": registry.sha256});
        for key in ["youngs_modulus", "yield_strength", "ultimate_strength", "print"] {
            sources.insert(key.into(), json!(from));
        }
    }
    if let Some(over) = m["engineering"].as_object() {
        for (key, v) in over {
            match (v.as_object(), d[key].as_object_mut()) {
                (Some(v), Some(old)) if matches!(key.as_str(), "friction" | "print") => old.extend(v.clone()),
                _ => d[key] = v.clone(),
            }
            sources.insert(key.clone(), json!(DOCUMENT));
        }
    }
    d["sources"] = Value::Object(sources);
    d
}

/// RoboCAD's `_friction_pair`: (static, kinetic) between material ids `a`
/// and `b` of `materials` (each entry's engineering block); a missing id is steel.
pub fn friction_pair(materials: &Map<String, Value>, a: Option<&str>, b: Option<&str>) -> (f64, f64) {
    let pair = |f: &Value| (f["static"].as_f64().unwrap_or(0.45), f["kinetic"].as_f64().unwrap_or(0.4));
    let fr = |id: Option<&str>| id.and_then(|i| materials.get(i)).map(|m| &m["friction"]);
    let Some(pa) = fr(a) else {
        return match fr(b) {
            None => (0.45, 0.4),
            Some(pb) => pair(if pb["steel"].is_object() { &pb["steel"] } else { &pb["self"] }),
        };
    };
    let key = if b == a { "self" } else { b.unwrap_or("steel") };
    if pa[key].is_object() {
        return pair(&pa[key]);
    }
    if let Some(pb) = fr(b) {
        let back = &pb[a.unwrap_or("steel")];
        let f2 = pair(if back.is_object() { back } else { &pb["self"] });
        let f1 = pair(&pa["self"]);
        return (0.5 * (f1.0 + f2.0), 0.5 * (f1.1 + f2.1));
    }
    pair(if pa["steel"].is_object() { &pa["steel"] } else { &pa["self"] })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_defaults_registry_values_and_document_overrides_layer_with_their_sources() {
        let pla = json!({"id": "pla", "name": "PLA", "density": 1.24, "tags": ["print", "plastic"], "engineering": {}});
        let e = engineering(&pla, None);
        assert_eq!(e["youngs_modulus"], json!(3.5e9));
        assert_eq!(e["friction"]["self"]["static"], json!(0.42));
        assert_eq!(e["sources"]["youngs_modulus"], json!(CATALOGUE_ESTIMATE));
        let registry_json = json!({"materials": {"pla-basic": {"cad_material": "pla", "modulus_in_layer": {"value": 3.2e9}, "modulus_across_layers": {"value": 2.4e9}, "tensile_in_layer": {"value": 50e6}, "tensile_across_layers": {"value": 25e6}, "poisson": {"value": 0.35}, "bearing": {"value": 60e6}, "glass_transition_c": {"value": 57.0}}}});
        let registry = Registry { json: &registry_json, sha256: "abc" };
        let e = engineering(&pla, Some(&registry));
        assert_eq!((e["youngs_modulus"].as_f64(), e["bearing_pressure"].as_f64()), (Some(3.2e9), Some(20e6)));
        assert_eq!(e["print"]["anisotropy_z"], json!(0.75));
        assert!(e["sources"]["yield_strength"].as_str().unwrap().starts_with(REGISTRY));
        let over = json!({"id": "pla", "tags": ["print"], "engineering": {"yield_strength": 40e6, "friction": {"self": {"static": 0.5, "kinetic": 0.4}}}});
        let e = engineering(&over, Some(&registry));
        assert_eq!(e["yield_strength"], json!(40e6));
        assert_eq!(e["friction"]["steel"]["kinetic"], json!(0.30), "untouched friction pairs stay");
        assert_eq!(e["sources"]["yield_strength"], json!(DOCUMENT));
        // Unknown ids name their stand-in.
        let custom = engineering(&json!({"id": "carbon", "tags": ["metal"]}), None);
        assert_eq!(custom["youngs_modulus"], json!(200e9));
        assert!(custom["sources"]["poisson"].as_str().unwrap().contains("steel values stand in"));
    }

    #[test]
    fn friction_pairs_follow_robocads_rule() {
        let mut m = Map::new();
        m.insert("pla".into(), engineering(&json!({"id": "pla", "tags": ["print"]}), None));
        m.insert("steel".into(), engineering(&json!({"id": "steel", "tags": ["metal"]}), None));
        assert_eq!(friction_pair(&m, Some("pla"), Some("pla")), (0.42, 0.35));
        assert_eq!(friction_pair(&m, Some("pla"), Some("steel")), (0.36, 0.30));
        assert_eq!(friction_pair(&m, None, None), (0.45, 0.4));
        assert_eq!(friction_pair(&m, Some("pla"), Some("world")), (0.42, 0.35));
    }
}

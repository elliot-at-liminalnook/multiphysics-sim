//! The print registry (`library/printing/registry.json`): printers,
//! filaments, infill scaling and printed-joint values, each with its unit,
//! provenance and uncertainty. One file feeds CAD, the stress check and the
//! planner; measured values enter only through [`crate::promote`].

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

pub const SCHEMA: &str = "sim.print-registry/1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provenance {
    /// Measured on this printer and filament (a promoted coupon test).
    Measured,
    /// Computed from other values.
    Derived,
    /// A typical or assumed value, not measured here.
    Estimated,
    /// From a maker's specification sheet.
    Datasheet,
}

/// A value with its unit and where it came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quantity<T = f64> {
    pub value: T,
    pub unit: String,
    pub provenance: Provenance,
    /// Relative uncertainty (0.25 = ±25 %).
    pub uncertainty: f64,
    pub source: String,
    /// Evidence for measured values (hash of the results file, sample count).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Printer {
    pub name: String,
    pub build_mm: Quantity<[f64; 3]>,
    pub margin_mm: Quantity,
    pub nozzle_mm: Quantity,
    pub line_width_mm: Quantity,
    pub layer_heights_mm: Vec<f64>,
    pub travel_overhead: Quantity,
    pub layer_change_s: Quantity,
}

impl Printer {
    /// The usable box for one piece: the build volume less the margin on each side in x and y.
    pub fn usable_mm(&self) -> [f64; 3] {
        let m = self.margin_mm.value;
        let b = self.build_mm.value;
        [b[0] - 2. * m, b[1] - 2. * m, b[2]]
    }
}

// No deny_unknown_fields: serde does not support it with `flatten`. A
// misspelt field still fails, because the real one is then missing.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// The CAD material id this filament backs (`pla`, `petg`).
    pub cad_material: String,
    pub density: Quantity,
    pub modulus_in_layer: Quantity,
    pub modulus_across_layers: Quantity,
    pub shear_modulus: Quantity,
    pub poisson: Quantity,
    pub tensile_in_layer: Quantity,
    pub tensile_across_layers: Quantity,
    pub compressive: Quantity,
    pub interlayer_shear: Quantity,
    pub in_layer_shear: Quantity,
    pub bearing: Quantity,
    pub glass_transition_c: Quantity,
    pub max_volumetric_speed: Quantity,
    pub filament_price_per_kg: Quantity,
    /// Measured joint capacities and other promoted values (`dovetail_capacity`, …).
    #[serde(default, flatten)]
    pub measured_extra: BTreeMap<String, Quantity>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Infill {
    pub modulus_exponent: Quantity,
    pub strength_exponent: Quantity,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub schema: String,
    pub revision: u32,
    pub note: String,
    pub printers: BTreeMap<String, Printer>,
    pub materials: BTreeMap<String, Material>,
    pub infill: BTreeMap<String, Infill>,
    /// Joint geometry (holes, clearances, lengths): used by CAD; capacities read it too.
    pub joints: Value,
    pub test_plan: Value,
    pub history: Vec<Value>,
}

/// A loaded registry with its file fingerprint.
#[derive(Clone, Debug)]
pub struct Loaded {
    pub registry: Registry,
    /// SHA-256 of the file bytes (the same digest CAD computes).
    pub sha256: String,
    pub path: std::path::PathBuf,
}

pub fn load(path: &Path) -> Result<Loaded, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let registry = parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Loaded { registry, sha256: crate::sha256::hex(&bytes), path: path.to_path_buf() })
}

pub fn parse(bytes: &[u8]) -> Result<Registry, String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|e| format!("not JSON: {e}"))?;
    let registry: Registry = serde_json::from_value(value.clone()).map_err(|e| format!("{e} ({})", locate(&value, &e.to_string())))?;
    registry.validate()?;
    Ok(registry)
}

/// serde's errors name the field but not always its path; add the top-level section.
fn locate(_value: &Value, message: &str) -> String {
    let sections = ["printers", "materials", "infill", "joints", "test_plan", "history"];
    sections.iter().find(|s| message.contains(*s)).map(|s| format!("in `{s}`")).unwrap_or_else(|| "check field names and types against sim.print-registry/1".into())
}

impl Registry {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SCHEMA {
            return Err(format!("schema: expected `{SCHEMA}`, found `{}`", self.schema));
        }
        let mut errors = Vec::new();
        fn check(errors: &mut Vec<String>, path: String, q: &Quantity, lo: f64, hi: f64) {
            if !q.value.is_finite() || q.value < lo || q.value > hi {
                errors.push(format!("{path}.value = {} is outside [{lo}, {hi}] {}", q.value, q.unit));
            }
            if !(0.0..=2.0).contains(&q.uncertainty) {
                errors.push(format!("{path}.uncertainty = {} must be a relative value in [0, 2]", q.uncertainty));
            }
            if q.source.trim().is_empty() {
                errors.push(format!("{path}.source is empty: say where the value comes from"));
            }
            if q.provenance == Provenance::Measured && q.evidence.is_none() {
                errors.push(format!("{path}: a measured value needs `evidence` (use `sim-print promote`)"));
            }
        }
        for (id, p) in &self.printers {
            let at = format!("printers.{id}");
            for (k, v) in p.build_mm.value.iter().enumerate() {
                if !(20.0..=2000.0).contains(v) {
                    errors.push(format!("{at}.build_mm.value[{k}] = {v} mm is not a plausible build size"));
                }
            }
            check(&mut errors, format!("{at}.margin_mm"), &p.margin_mm, 0., 50.);
            check(&mut errors, format!("{at}.nozzle_mm"), &p.nozzle_mm, 0.1, 2.0);
            check(&mut errors, format!("{at}.line_width_mm"), &p.line_width_mm, 0.1, 2.5);
            check(&mut errors, format!("{at}.travel_overhead"), &p.travel_overhead, 0., 3.);
            check(&mut errors, format!("{at}.layer_change_s"), &p.layer_change_s, 0., 60.);
            if p.layer_heights_mm.is_empty() || p.layer_heights_mm.iter().any(|h| !(0.02..=1.0).contains(h)) {
                errors.push(format!("{at}.layer_heights_mm must list heights in (0.02, 1.0] mm"));
            }
        }
        for (id, m) in &self.materials {
            let at = format!("materials.{id}");
            check(&mut errors, format!("{at}.density"), &m.density, 300., 3000.);
            for (name, q) in [("modulus_in_layer", &m.modulus_in_layer), ("modulus_across_layers", &m.modulus_across_layers), ("shear_modulus", &m.shear_modulus)] {
                check(&mut errors, format!("{at}.{name}"), q, 1e6, 50e9);
            }
            check(&mut errors, format!("{at}.poisson"), &m.poisson, 0.0, 0.499);
            for (name, q) in [
                ("tensile_in_layer", &m.tensile_in_layer),
                ("tensile_across_layers", &m.tensile_across_layers),
                ("compressive", &m.compressive),
                ("interlayer_shear", &m.interlayer_shear),
                ("in_layer_shear", &m.in_layer_shear),
                ("bearing", &m.bearing),
            ] {
                check(&mut errors, format!("{at}.{name}"), q, 1e5, 2e9);
            }
            check(&mut errors, format!("{at}.glass_transition_c"), &m.glass_transition_c, -100., 400.);
            check(&mut errors, format!("{at}.max_volumetric_speed"), &m.max_volumetric_speed, 0.5, 200.);
            check(&mut errors, format!("{at}.filament_price_per_kg"), &m.filament_price_per_kg, 0., 1000.);
            for (name, q) in &m.measured_extra {
                check(&mut errors, format!("{at}.{name}"), q, 0., 1e12);
            }
            if m.modulus_across_layers.value > m.modulus_in_layer.value * 1.2 {
                errors.push(format!("{at}.modulus_across_layers exceeds modulus_in_layer: prints are stiffest along their layers"));
            }
        }
        for (id, f) in &self.infill {
            check(&mut errors, format!("infill.{id}.modulus_exponent"), &f.modulus_exponent, 0.5, 3.);
            check(&mut errors, format!("infill.{id}.strength_exponent"), &f.strength_exponent, 0.5, 3.);
        }
        if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
    }

    pub fn printer(&self, id: &str) -> Result<&Printer, String> {
        self.printers.get(id).ok_or_else(|| format!("printer `{id}` is not in the registry (have: {})", self.printers.keys().cloned().collect::<Vec<_>>().join(", ")))
    }
    pub fn material(&self, id: &str) -> Result<&Material, String> {
        self.materials.get(id).ok_or_else(|| format!("material `{id}` is not in the registry (have: {})", self.materials.keys().cloned().collect::<Vec<_>>().join(", ")))
    }
    pub fn infill_law(&self, pattern: &str) -> Result<&Infill, String> {
        self.infill.get(pattern).ok_or_else(|| format!("infill pattern `{pattern}` is not in the registry (have: {})", self.infill.keys().cloned().collect::<Vec<_>>().join(", ")))
    }
    /// A joint value by path (`dovetail.friction`), as a number.
    pub fn joint_number(&self, path: &str) -> Result<f64, String> {
        let mut v = &self.joints;
        for part in path.split('.') {
            v = v.get(part).ok_or_else(|| format!("joints.{path}: `{part}` is missing"))?;
        }
        v.get("value").unwrap_or(v).as_f64().ok_or_else(|| format!("joints.{path} is not a number"))
    }
}

/// The repository's registry: `SIM_PRINT_REGISTRY`, else `library/printing/registry.json`
/// found upwards from the current directory.
pub fn default_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("SIM_PRINT_REGISTRY") {
        return p.into();
    }
    let mut dir = std::env::current_dir().unwrap_or_default();
    loop {
        let candidate = dir.join("library/printing/registry.json");
        if candidate.exists() {
            return candidate;
        }
        if !dir.pop() {
            return "library/printing/registry.json".into();
        }
    }
}

//! Promote coupon measurements into the registry.
//!
//! A results file (`sim.print-test/1`, filled in after breaking the coupons
//! CAD generated) lists each test's breaking loads. Each becomes a strength:
//!
//! * `tensile_in_layer`, `tensile_across_layers`: F / gauge area;
//! * `pin_shear`: pin bearing, F / (d · L) on the shorter hole → `bearing`;
//! * `insert_pullout`: τ = F / (π · D · L · η) with the registry's grip share η
//!   → `interlayer_shear` (so the pull-out model reproduces the measurement);
//! * `dovetail_pull`: the tab's measured capacity (N) → `dovetail_pull_n`.
//!
//! With at least `test_plan.min_samples` breaks, the mean becomes the value
//! (provenance `measured`, uncertainty the coefficient of variation) with the
//! evidence: samples, mean, spread, the design value mean − 2·std, the results
//! file's SHA-256 and the value it replaced. The registry's revision goes up
//! and its history records the change. Nothing is written without `--write`.

use crate::registry::{Provenance, Quantity, Registry};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const SCHEMA: &str = "sim.print-test/1";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Results {
    pub schema: String,
    pub material: String,
    pub printer: String,
    /// The registry the coupons were made from.
    pub registry_sha256: String,
    #[serde(default)]
    pub settings: Value,
    #[serde(default)]
    pub printed: String,
    #[serde(default)]
    pub tested: String,
    #[serde(default)]
    pub operator: String,
    pub tests: Vec<Test>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Test {
    /// `tensile_in_layer`, `tensile_across_layers`, `pin_shear`, `insert_pullout`, `dovetail_pull`.
    pub coupon: String,
    /// Breaking loads (N), one per coupon; null for one not tested.
    pub failure_n: Vec<Option<f64>>,
    /// Geometry of the coupon as printed (mm): gauge area, pin diameter and length, insert knurl and length.
    #[serde(default)]
    pub geometry: Value,
    #[serde(default)]
    pub how_it_broke: Vec<String>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Promotion {
    pub coupon: String,
    pub quantity: String,
    pub samples: usize,
    pub mean: f64,
    pub std: f64,
    pub design_value: f64,
    pub unit: String,
    pub previous: Option<Quantity>,
    pub note: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Outcome {
    pub promotions: Vec<Promotion>,
    pub refused: Vec<String>,
    pub warnings: Vec<String>,
    /// The new registry (as JSON) when anything was promoted.
    #[serde(skip)]
    pub registry: Option<Value>,
}

fn geometry(t: &Test, key: &str) -> Result<f64, String> {
    t.geometry.get(key).and_then(|v| v.as_f64()).ok_or_else(|| format!("{}: geometry.{key} (mm) is needed to turn loads into a strength", t.coupon))
}

/// Compute promotions and the new registry JSON (not written here).
pub fn promote(registry_json: &Value, registry: &Registry, registry_sha256: &str, results_bytes: &[u8], date: &str) -> Result<Outcome, String> {
    let results: Results = serde_json::from_slice(results_bytes).map_err(|e| format!("results: {e}"))?;
    if results.schema != SCHEMA {
        return Err(format!("results: schema must be `{SCHEMA}`"));
    }
    let material = registry.material(&results.material)?.clone();
    registry.printer(&results.printer)?;
    let min_samples = registry.test_plan.get("min_samples").and_then(|v| v.as_u64()).unwrap_or(3) as usize;
    let results_sha = crate::sha256::hex(results_bytes);
    let mut warnings = Vec::new();
    if results.registry_sha256 != registry_sha256 {
        warnings.push(format!("the coupons were made from registry {}…, now {}…: check the coupon geometry still matches", &results.registry_sha256.get(..12).unwrap_or(""), &registry_sha256[..12]));
    }
    let mut out = registry_json.clone();
    let mut promotions = Vec::new();
    let mut refused = Vec::new();
    for t in &results.tests {
        let loads: Vec<f64> = t.failure_n.iter().flatten().copied().filter(|x| x.is_finite() && *x > 0.).collect();
        if loads.len() < min_samples {
            refused.push(format!("{}: {} valid break(s); at least {min_samples} are needed", t.coupon, loads.len()));
            continue;
        }
        let (quantity, unit, values, note): (&str, &str, Vec<f64>, String) = match t.coupon.as_str() {
            "tensile_in_layer" | "tensile_across_layers" => {
                let area = geometry(t, "gauge_area_mm2")?;
                (t.coupon.as_str(), "Pa", loads.iter().map(|f| f / (area * 1e-6)).collect(), format!("F / A over a {area} mm² gauge"))
            }
            "pin_shear" => {
                let (d, l) = (geometry(t, "pin_diameter_mm")?, geometry(t, "engaged_mm")?);
                ("bearing", "Pa", loads.iter().map(|f| f / (d * l * 1e-6)).collect(), format!("pin bearing F / (d·L), d = {d} mm, L = {l} mm"))
            }
            "insert_pullout" => {
                let (dk, l) = (geometry(t, "knurl_mm")?, geometry(t, "insert_length_mm")?);
                let eta = registry.joint_number("heat_set_insert.grip_share")?;
                ("interlayer_shear", "Pa", loads.iter().map(|f| f / (std::f64::consts::PI * dk * l * eta * 1e-6)).collect(), format!("τ = F / (π·D·L·η), D = {dk} mm, L = {l} mm, η = {eta} (registry grip share)"))
            }
            "dovetail_pull" => ("dovetail_pull_n", "N", loads.clone(), format!("measured capacity of the tab geometry {}", t.geometry)),
            other => {
                refused.push(format!("{other}: unknown coupon (tensile_in_layer, tensile_across_layers, pin_shear, insert_pullout, dovetail_pull)"));
                continue;
            }
        };
        let n = values.len() as f64;
        let mean = values.iter().sum::<f64>() / n;
        let std = (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.).max(1.)).sqrt();
        let design = mean - 2. * std;
        if design <= 0. {
            warnings.push(format!("{}: the spread is so large that mean − 2·std is not positive; test more coupons", t.coupon));
        }
        let at = out["materials"][&results.material].clone();
        let previous: Option<Quantity> = at.get(quantity).and_then(|v| serde_json::from_value(v.clone()).ok());
        let evidence = json!({
            "results_sha256": results_sha, "coupon": t.coupon, "samples": values, "breaks_n": loads, "n": values.len(),
            "mean": mean, "std": std, "design_value": design, "method": note, "geometry": t.geometry, "settings": results.settings,
            "printer": results.printer, "printed": results.printed, "tested": results.tested, "operator": results.operator,
            "how_it_broke": t.how_it_broke, "promoted": date,
            "previous": previous.as_ref().map(|p| json!({"value": p.value, "provenance": p.provenance, "source": p.source})),
        });
        let q = Quantity { value: mean, unit: unit.into(), provenance: Provenance::Measured, uncertainty: (std / mean).clamp(0., 2.), source: format!("coupon test {} (n = {}), {}", t.coupon, values.len(), note), evidence: Some(evidence) };
        out["materials"][&results.material][quantity] = serde_json::to_value(&q).unwrap();
        promotions.push(Promotion { coupon: t.coupon.clone(), quantity: quantity.into(), samples: values.len(), mean, std, design_value: design, unit: unit.into(), previous, note });
    }
    let registry_out = if promotions.is_empty() {
        None
    } else {
        let rev = registry.revision + 1;
        out["revision"] = json!(rev);
        let change = promotions.iter().map(|p| {
            let before = p.previous.as_ref().map(|q| format!("{:.4e} {} ({:?})", q.value, q.unit, q.provenance).to_lowercase()).unwrap_or_else(|| "none".into());
            format!("{}.{} = {:.4e} {} measured (n = {}, design value {:.4e}); was {before}", results.material, p.quantity, p.mean, p.unit, p.samples, p.design_value)
        }).collect::<Vec<_>>().join("; ");
        out["history"].as_array_mut().ok_or("registry history is not a list")?.push(json!({"revision": rev, "date": date, "change": format!("Promoted from coupon tests (results sha256 {}…): {change}", &results_sha[..12])}));
        // The result must still be a valid registry.
        crate::registry::parse(serde_json::to_string(&out).unwrap().as_bytes()).map_err(|e| format!("the promoted registry would be invalid: {e}"))?;
        Some(out)
    };
    Ok(Outcome { promotions, refused, warnings, registry: registry_out })
}

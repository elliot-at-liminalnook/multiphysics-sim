//! Evaluate shared sampled-geometry audit output, without duplicating geometry.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gates {
    pub feet: Vec<String>,
    pub after_startup_s: f64,
    pub minimum_clearance_m: f64,
    pub minimum_excursions: usize,
    pub maximum_inter_link_penetration_m: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Foot {
    pub peak_clearance_m: f64,
    pub minimum_clearance_m: f64,
    pub excursions: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub passed: bool,
    pub rejection_reasons: Vec<String>,
    pub feet: BTreeMap<String, Foot>,
    pub maximum_inter_link_penetration_m: f64,
    pub samples: usize,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub audit: Value,
    /// Hash the actual capture bytes supplied to the geometry audit host.
    pub expected_capture_blake3: String,
    pub expected_duration_s: f64,
    /// Exact sample times from the bound capture; prevent dropped audit frames.
    pub expected_frame_times_s: Vec<f64>,
    pub gates: Gates,
}
pub fn evaluate(r: &Request) -> Result<Report, String> {
    let g = &r.gates;
    let a = &r.audit;
    if r.expected_capture_blake3.len() != 64
        || !r
            .expected_capture_blake3
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
        || a["capture_blake3"].as_str() != Some(r.expected_capture_blake3.as_str())
        || !r.expected_duration_s.is_finite()
        || r.expected_duration_s <= 0.
        || g.feet.is_empty()
        || g.feet.iter().any(|n| n.is_empty())
        || g.feet.iter().collect::<BTreeSet<_>>().len() != g.feet.len()
        || !g.after_startup_s.is_finite()
        || g.after_startup_s < 0.
        || g.after_startup_s >= r.expected_duration_s
        || !g.minimum_clearance_m.is_finite()
        || g.minimum_clearance_m <= 0.
        || g.minimum_excursions == 0
        || !g.maximum_inter_link_penetration_m.is_finite()
        || g.maximum_inter_link_penetration_m < 0.
    {
        return Err("matched capture hash, positive duration/clearance and explicit unique foot gates required".into());
    }
    let frames = a["frames"]
        .as_array()
        .filter(|f| !f.is_empty())
        .ok_or("missing geometry audit frames")?;
    if frames.len() != r.expected_frame_times_s.len()
        || r.expected_frame_times_s
            .iter()
            .any(|t| !t.is_finite() || *t < 0.)
        || r.expected_frame_times_s.windows(2).any(|w| w[0] >= w[1])
    {
        return Err("geometry audit must cover every bound capture frame".into());
    }
    let mut reasons = vec![];
    if a["capture_completed"] != true || !a["capture_error"].is_null() {
        reasons.push("incomplete or failed geometry source capture".into());
    }
    let mut feet: BTreeMap<String, Foot> = g
        .feet
        .iter()
        .map(|n| {
            (
                n.clone(),
                Foot {
                    peak_clearance_m: f64::NEG_INFINITY,
                    minimum_clearance_m: f64::INFINITY,
                    excursions: 0,
                },
            )
        })
        .collect();
    let mut above = BTreeMap::<String, bool>::new();
    let mut previous = -1.;
    let mut max_penetration = 0_f64;
    let mut samples = 0;
    for (frame, expected_time) in frames.iter().zip(&r.expected_frame_times_s) {
        let t = frame["time_s"]
            .as_f64()
            .filter(|x| x.is_finite() && *x >= 0.)
            .ok_or("invalid geometry time")?;
        if (t - expected_time).abs() > 1e-10 {
            return Err("geometry audit frame does not match capture time".into());
        }
        if t <= previous {
            return Err("geometry audit times must increase".into());
        }
        previous = t;
        let p = frame["maximum_inter_link_penetration_m"]
            .as_f64()
            .filter(|x| x.is_finite() && *x >= 0.)
            .ok_or("invalid penetration")?;
        max_penetration = max_penetration.max(p);
        if t < g.after_startup_s {
            continue;
        }
        samples += 1;
        let rows = frame["floor_clearances"]
            .as_array()
            .ok_or("missing floor observations")?;
        for name in &g.feet {
            let matching = rows
                .iter()
                .filter(|f| f["link"].as_str() == Some(name))
                .collect::<Vec<_>>();
            if matching.len() != 1 {
                return Err(format!("missing/duplicate foot geometry: {name}"));
            }
            if matching[0]["surface_samples"]
                .as_u64()
                .is_none_or(|n| n == 0)
            {
                return Err("foot has no sampled contact surface".into());
            }
            let gap = matching[0]["minimum_clearance_m"]
                .as_f64()
                .filter(|x| x.is_finite())
                .ok_or("invalid floor clearance")?;
            let high = gap >= g.minimum_clearance_m;
            let foot = feet.get_mut(name).unwrap();
            foot.peak_clearance_m = foot.peak_clearance_m.max(gap);
            foot.minimum_clearance_m = foot.minimum_clearance_m.min(gap);
            if above.get(name) == Some(&false) && high {
                foot.excursions += 1;
            }
            above.insert(name.clone(), high);
        }
    }
    if (previous - r.expected_duration_s).abs() > 1e-8 {
        reasons.push("geometry audit does not cover the required horizon".into());
    }
    if samples == 0 {
        return Err("no geometry samples after startup".into());
    }
    if max_penetration > g.maximum_inter_link_penetration_m {
        reasons.push("sampled inter-link penetration gate".into());
    }
    for (name, foot) in &feet {
        if foot.peak_clearance_m < g.minimum_clearance_m || foot.excursions < g.minimum_excursions {
            reasons.push(format!("insufficient repeated foot lift: {name}"));
        }
    }
    Ok(Report {
        passed: reasons.is_empty(),
        rejection_reasons: reasons,
        feet,
        maximum_inter_link_penetration_m: max_penetration,
        samples,
    })
}
pub fn register(registry: &mut sim_core::BehaviorRegistry) -> Result<(), String> {
    use sim_core::primitive::{Descriptor, Field};
    registry.register_primitive(
        Descriptor::new(
            "experiment.evaluate_geometry",
            "Evaluate sampled foot clearance and inter-link penetration gates",
            vec![Field::structured(
                "$",
                "m,s; counts dimensionless",
                "geometry_evaluation::Request",
            )],
            vec![Field::structured(
                "$",
                "m; sampled excursions",
                "geometry_evaluation::Report",
            )],
            &[
                "Requires shared geometry audit bound to actual capture bytes",
                "No continuous collision guarantee or hardware validation",
            ],
        ),
        |r: Request| evaluate(&r),
    )
}

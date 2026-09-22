//! Matched timestep sensitivity over recorded states, without advancing physics.
use crate::fidelity::{ContextDifference, EnvironmentCapture, ExecutionContext};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gates {
    pub step_divisor: usize,
    pub body_link: String,
    pub maximum_net_distance_difference_m: f64,
    pub maximum_endpoint_body_position_difference_m: f64,
    pub maximum_endpoint_body_up_z_difference: f64,
    /// Absolute coordinate-position tolerance by authored SI unit (rad or m).
    pub maximum_actuated_position_difference: BTreeMap<String, f64>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub reference: EnvironmentCapture,
    pub refined: EnvironmentCapture,
    pub gates: Gates,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoordinateError {
    pub name: String,
    pub unit: String,
    pub difference: f64,
    pub tolerance: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub passed: bool,
    pub rejection_reasons: Vec<String>,
    pub context_changes: Vec<ContextDifference>,
    pub observed_s: f64,
    pub net_distance_difference_m: f64,
    pub endpoint_body_position_difference_m: f64,
    pub endpoint_body_up_z_difference: f64,
    pub actuated_positions: Vec<CoordinateError>,
    pub scope: String,
}

pub fn compare(
    a: &EnvironmentCapture,
    b: &EnvironmentCapture,
    g: &Gates,
) -> Result<Report, String> {
    if g.step_divisor < 2
        || g.body_link.trim().is_empty()
        || [
            g.maximum_net_distance_difference_m,
            g.maximum_endpoint_body_position_difference_m,
            g.maximum_endpoint_body_up_z_difference,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.)
        || g.maximum_actuated_position_difference.is_empty()
        || g.maximum_actuated_position_difference
            .iter()
            .any(|(u, v)| !matches!(u.as_str(), "rad" | "m") || !v.is_finite() || *v < 0.)
    {
        return Err(
            "explicit body, divisor and finite nonnegative SI endpoint tolerances required".into(),
        );
    }
    let ac = &a.recording.config;
    let bc = &b.recording.config;
    if !ac.step_s.is_finite()
        || ac.step_s <= 0.
        || !bc.step_s.is_finite()
        || bc.step_s != ac.step_s / g.step_divisor as f64
        || ac.steps.checked_mul(g.step_divisor) != Some(bc.steps)
        || ac.report_every.checked_mul(g.step_divisor) != Some(bc.report_every)
        || a.recording.runtime_identity.is_none()
        || b.recording.runtime_identity.is_none()
    {
        return Err(
            "matched finer physics clock, reporting cadence, horizon and runtime identity required"
                .into(),
        );
    }
    let changes = ExecutionContext::new(&a.recording, &a.task)
        .differences(&ExecutionContext::new(&b.recording, &b.task))?;
    if changes.iter().any(|d| {
        !matches!(
            d.path.as_str(),
            "/config/step_s" | "/config/steps" | "/config/report_every"
        )
    }) {
        return Err(
            "timestep comparison cannot change model, policy, task, seed or runtime".into(),
        );
    }
    let (am, aa, ao) = a.validate()?;
    let (bm, ba, bo) = b.validate()?;
    if am.len() < 2
        || am.len() != bm.len()
        || am
            .iter()
            .zip(&bm)
            .any(|(a, b)| (a.time_s - b.time_s).abs() > 1e-10)
        || aa[..am.len() - 1] != ba[..bm.len() - 1]
    {
        return Err("complete matching frame clocks and identical held actions required".into());
    }
    if a.metadata["joint_indices"] != b.metadata["joint_indices"]
        || a.metadata["frame_coordinates"] != b.metadata["frame_coordinates"]
    {
        return Err("actuator/coordinate topology or authored units differ".into());
    }
    if am[0] != bm[0] {
        return Err("timestep captures must start at the same recorded state".into());
    }
    let mut reasons = vec![];
    if ao.eligible_score.is_none() || bo.eligible_score.is_none() {
        reasons.push("incomplete or failed numerical comparison episode".into());
    }
    let a_end = am.last().unwrap();
    let b_end = bm.last().unwrap();
    let body = |frame: &crate::motion_data::MotionSnapshot| {
        frame
            .poses
            .iter()
            .find(|p| p.name == g.body_link)
            .cloned()
            .ok_or("missing named body pose")
    };
    let a0 = body(&am[0])?;
    let b0 = body(&bm[0])?;
    let ap = body(a_end)?;
    let bp = body(b_end)?;
    let distance = |p: &crate::motion_data::LinkMotion, p0: &crate::motion_data::LinkMotion| {
        (p.position_m[0] - p0.position_m[0]).hypot(p.position_m[1] - p0.position_m[1])
    };
    let net_distance_difference_m = (distance(&ap, &a0) - distance(&bp, &b0)).abs();
    let endpoint_body_position_difference_m = (0..3).fold(0_f64, |norm, i| {
        norm.hypot(ap.position_m[i] - bp.position_m[i])
    });
    let endpoint_body_up_z_difference = (ap.rotation[2][2] - bp.rotation[2][2]).abs();
    for (error, budget, label) in [
        (
            net_distance_difference_m,
            g.maximum_net_distance_difference_m,
            "net distance timestep gate",
        ),
        (
            endpoint_body_position_difference_m,
            g.maximum_endpoint_body_position_difference_m,
            "body position timestep gate",
        ),
        (
            endpoint_body_up_z_difference,
            g.maximum_endpoint_body_up_z_difference,
            "body up-z timestep gate",
        ),
    ] {
        if !error.is_finite() {
            return Err("nonfinite timestep difference".into());
        }
        if error > budget {
            reasons.push(label.into());
        }
    }
    let indices: Vec<usize> =
        serde_json::from_value(a.metadata["joint_indices"].clone()).map_err(|e| e.to_string())?;
    let coordinates = a.metadata["frame_coordinates"]
        .as_array()
        .ok_or("missing coordinate metadata")?;
    if indices.is_empty()
        || indices.iter().collect::<BTreeSet<_>>().len() != indices.len()
        || coordinates.len() != a_end.joint_positions.len()
        || coordinates.len() != b_end.joint_positions.len()
    {
        return Err("unique nonempty actuator mapping and consistent coordinates required".into());
    }
    let mut actuated_positions = vec![];
    for i in indices {
        let c = coordinates
            .get(i)
            .ok_or("actuator coordinate index out of range")?;
        if c["index"].as_u64() != Some(i as u64) {
            return Err("coordinate index metadata mismatch".into());
        }
        let name = c["name"]
            .as_str()
            .filter(|n| !n.is_empty())
            .ok_or("missing coordinate name")?
            .to_owned();
        let unit = c["position_unit"]
            .as_str()
            .ok_or("missing position unit")?
            .to_owned();
        let tolerance = *g
            .maximum_actuated_position_difference
            .get(&unit)
            .ok_or("missing actuated position unit tolerance")?;
        let difference = (a_end.joint_positions[i] - b_end.joint_positions[i]).abs();
        if !difference.is_finite() {
            return Err("nonfinite coordinate difference".into());
        }
        if difference > tolerance {
            reasons.push(format!("actuated coordinate timestep gate: {name}"));
        }
        actuated_positions.push(CoordinateError {
            name,
            unit,
            difference,
            tolerance,
        });
    }
    Ok(Report{passed:reasons.is_empty(),rejection_reasons:reasons,context_changes:changes,observed_s:a_end.time_s,
        net_distance_difference_m,endpoint_body_position_difference_m,endpoint_body_up_z_difference,actuated_positions,
        scope:"Matched physics-timestep endpoint sensitivity only. Body up-z uses the named body's local z axis in world z. Neither endpoint agreement nor completion replaces motion, tracking, contact and geometry acceptance; no continuous trajectory or hardware-accuracy certificate.".into()})
}
pub fn register(registry: &mut sim_core::BehaviorRegistry) -> Result<(), String> {
    use sim_core::primitive::{Descriptor, Field};
    registry.register_primitive(
        Descriptor::new(
            "experiment.evaluate_timestep",
            "Evaluate endpoint sensitivity with only physics timestep changed",
            vec![Field::structured(
                "$",
                "s,m,rad; up-z dimensionless",
                "numerical_validation::Request",
            )],
            vec![Field::structured(
                "$",
                "m,rad; up-z dimensionless",
                "numerical_validation::Report",
            )],
            &[
                "Requires equal runtime, model, seed, initial state and held actions",
                "Checks full completion and explicit endpoint budgets only",
            ],
        ),
        |r: Request| compare(&r.reference, &r.refined, &r.gates),
    )
}

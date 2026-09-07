//! Exact plan-time tracking, including pauses. No fitted time warp or pose
//! interpolation can hide actuator error or world-frame body drift.
use crate::{
    session::EpisodeFrame,
    tracking::{sample_markers, sample_markers_in_link_frame, CaptureConfig},
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
fn number(v: &Value) -> Result<f64, String> {
    v.as_f64()
        .filter(|x| x.is_finite())
        .ok_or("missing/nonfinite motion time".into())
}
fn frame(v: &Value) -> Result<EpisodeFrame, String> {
    Ok(EpisodeFrame {
        time_s: number(&v["time_s"])?,
        done: false,
        poses: serde_json::from_value(v["poses"].clone()).map_err(|e| e.to_string())?,
        joint_positions: vec![],
        telemetry: Default::default(),
        contacts: vec![],
        error: None,
    })
}

// Compare physical numeric fields in their declared schema. JSON integer 0 and
// float 0.0 represent the same f64 quantity; no approximate equality is used.
fn same_typed<T: serde::de::DeserializeOwned + serde::Serialize>(
    a: &Value,
    b: &Value,
) -> Result<bool, String> {
    let canonical = |v: &Value| -> Result<Value, String> {
        let typed: T = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
        serde_json::to_value(typed).map_err(|e| e.to_string())
    };
    Ok(canonical(a)? == canonical(b)?)
}

/// Requires exact reference samples. Regenerate a denser reference if pause
/// timing does not align with its grid. This is not a landing acceptance gate.
pub fn compare_motion(
    a: &Value,
    b: &Value,
    markers: &CaptureConfig,
    reference_link: &str,
) -> Result<Value, String> {
    const EPS: f64 = 1e-9;
    if a["completed"] != true
        || b["completed"] != true
        || !a["error"].is_null()
        || a["motor_experiment"]["target_trajectory"].is_null()
        || !same_typed::<sim_domain_control::trajectory::TrajectoryConfig>(
            &a["motor_experiment"]["target_trajectory"],
            &b["trajectory"],
        )?
    {
        return Err("complete captures of the same named target trajectory are required".into());
    }
    for key in ["source", "independent_coordinates"] {
        if a[key].is_null() || a[key] != b[key] {
            return Err(format!("motion provenance mismatch: {key}"));
        }
    }
    if !same_typed::<sim_domain_robot::articulated::embedding::EmbeddingConfig>(
        &a["embedding"],
        &b["embedding"],
    )? || !same_typed::<Vec<f64>>(&a["initial_coordinates"], &b["initial_coordinates"])?
    {
        return Err("motion provenance mismatch: embedding or initial coordinates".into());
    }
    if markers
        .expected_cad_sha256
        .as_ref()
        .is_some_and(|h| Some(h.as_str()) != a["source"]["cad_sha256"].as_str())
    {
        return Err("marker CAD hash mismatch".into());
    }
    if markers.coordinate_frame.trim().is_empty() || reference_link.trim().is_empty() {
        return Err("explicit world and reference-link frames are required".into());
    }
    let placement = b
        .get("initial_base_translation_m")
        .unwrap_or(&b["config"]["initial_base_translation_m"]);
    if !same_typed::<Option<[f64; 3]>>(&a["initial_base_translation_m"], placement)? {
        return Err("different initial world placement".into());
    }
    let frames = a["frames"].as_array().ok_or("missing simulation frames")?;
    let reference = b["frames"].as_array().ok_or("missing reference frames")?;
    for fs in [frames, reference] {
        if fs.len() < 2 || number(&fs[0]["time_s"])? != 0.0 {
            return Err("capture must start at zero and contain two frames".into());
        }
        for pair in fs.windows(2) {
            if number(&pair[1]["time_s"])? <= number(&pair[0]["time_s"])? {
                return Err("nonmonotonic frames".into());
            }
        }
    }
    let end = number(&reference.last().unwrap()["time_s"])?;
    let knots = b["trajectory"]["keyframes"]
        .as_array()
        .ok_or("missing reference trajectory knots")?;
    if knots.is_empty() || (number(&knots.last().unwrap()["time_s"])? - end).abs() > EPS {
        return Err("reference capture omits the trajectory endpoint".into());
    }
    let gated = !a["motion_gate"].is_null();
    if gated && (number(&a["motion_gate"]["clock"]["duration_s"])? - end).abs() > EPS {
        return Err("motion clock and reference durations differ".into());
    }
    let mut sums: BTreeMap<(String, String), (f64, f64, f64, f64)> = BTreeMap::new();
    let mut samples = vec![];
    let (mut last_t, mut last_phase, mut max_gap) = (0.0_f64, 0.0_f64, 0.0_f64);
    for f in frames {
        let t = number(&f["time_s"])?;
        let phase = if gated {
            number(&f["reference_time_s"])?
        } else {
            t
        };
        if phase < 0.0
            || phase > end + EPS
            || phase < last_phase - EPS
            || phase - last_phase > t - last_t + EPS
        {
            return Err(
                "reference time must be covered, monotonic and no faster than simulation time"
                    .into(),
            );
        }
        let matches: Vec<_> = reference
            .iter()
            .filter(|r| number(&r["time_s"]).is_ok_and(|x| (x - phase).abs() <= EPS))
            .collect();
        if matches.len() != 1 {
            return Err(format!("reference time {phase}s needs exactly one captured reference sample; regenerate a denser reference"));
        }
        let (actual, planned) = (frame(f)?, frame(matches[0])?);
        let mut errors = json!({});
        for (label, x, y) in [
            (
                "world",
                sample_markers(&actual, &markers.markers)?,
                sample_markers(&planned, &markers.markers)?,
            ),
            (
                "body_relative",
                sample_markers_in_link_frame(&actual, &markers.markers, reference_link)?,
                sample_markers_in_link_frame(&planned, &markers.markers, reference_link)?,
            ),
        ] {
            for (id, p) in x.points {
                let target = y.points[&id].position_m;
                let delta: [f64; 3] = std::array::from_fn(|i| p.position_m[i] - target[i]);
                let norm = delta.iter().map(|x| x * x).sum::<f64>().sqrt();
                let sum = sums.entry((label.into(), id.clone())).or_default();
                if norm > sum.0 {
                    sum.0 = norm;
                    sum.2 = t;
                    sum.3 = phase;
                }
                sum.1 += norm * norm;
                errors[label][&id] = json!({"actual_m":p.position_m,"reference_m":target,"error_m":delta,"distance_m":norm,"xy_distance_m":delta[0].hypot(delta[1])});
            }
        }
        // The marker sampling above already validates uniqueness and rigidity
        // of both reference-link transforms. Retain their world-pose error too.
        let body = actual
            .poses
            .iter()
            .find(|p| p.name == reference_link)
            .unwrap();
        let target = planned
            .poses
            .iter()
            .find(|p| p.name == reference_link)
            .unwrap();
        let translation: [f64; 3] =
            std::array::from_fn(|i| body.position_m[i] - target.position_m[i]);
        let rotation = nalgebra::Matrix3::from_fn(|i, j| body.rotation[i][j]);
        let target_rotation = nalgebra::Matrix3::from_fn(|i, j| target.rotation[i][j]);
        let angle =
            nalgebra::UnitQuaternion::from_matrix(&(rotation * target_rotation.transpose()))
                .angle();
        let body_error = json!({"coordinate_frame":markers.coordinate_frame,"translation_m":translation,"rotation_angle_rad":angle});
        samples.push(json!({"time_s":t,"reference_time_s":phase,"errors":errors,"reference_link_pose_error":body_error}));
        max_gap = max_gap.max(t - last_t);
        last_t = t;
        last_phase = phase;
    }
    if (last_phase - end).abs() > EPS {
        return Err("reference motion did not finish".into());
    }
    if (number(&a["simulated_s"])? - last_t).abs() > EPS {
        return Err("simulation capture omits the terminal sample".into());
    }
    let summaries:Vec<_>=sums.into_iter().map(|((frame,id),(max,sum,t,p))|json!({"frame":frame,"id":id,"maximum_error_m":max,"rms_error_m":(sum/frames.len() as f64).sqrt(),"worst_time_s":t,"worst_reference_time_s":p})).collect();
    Ok(
        json!({"version":2,"reference_link":reference_link,"world_frame":markers.coordinate_frame,"alignment":if gated{"recorded_reference_time"}else{"simulation_time"},"samples":frames.len(),"maximum_sample_gap_s":max_gap,"duration_s":last_t,"reference_duration_s":end,"markers":summaries,"sample_errors":samples,"scope":"Exact sampled plan-time alignment. World errors retain body drift; body-relative errors describe mechanism tracking. RMS weights each simulation sample equally, including pauses and final hold. Not calibrated hardware, continuous clearance, landing, slip or balance acceptance."}),
    )
}

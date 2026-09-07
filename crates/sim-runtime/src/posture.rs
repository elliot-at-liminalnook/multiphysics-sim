//! Sampled pose-hold diagnostics shared by native/browser experiment hosts.
//! Reports motion, not force equilibrium, continuous clearance or walking success.
use crate::{
    session::EpisodeFrame,
    tracking::{Marker, sample_markers},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_cad_sha256: Option<String>,
    pub body_link: String,
    pub body_up_local: [f64; 3],
    pub world_up: [f64; 3],
    pub markers: Vec<Marker>,
    pub start_s: f64,
    pub end_s: f64,
    pub maximum_sample_gap_s: f64,
}
#[derive(Debug, Serialize)]
pub struct HoldReport {
    pub samples: usize,
    pub maximum_sample_gap_s: f64,
    pub maximum_body_tilt_rad: f64,
    pub maximum_body_horizontal_displacement_m: f64,
    pub maximum_body_height_change_m: f64,
    pub final_body_displacement_m: [f64; 3],
    pub maximum_marker_displacement_m: BTreeMap<String, f64>,
    pub final_marker_displacement_m: BTreeMap<String, [f64; 3]>,
    pub scope: String,
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn unit(a: [f64; 3]) -> bool {
    a.iter().all(|v| v.is_finite()) && (dot(a, a) - 1.0).abs() <= 1e-10
}
/// Compare each pose/marker to the first sample over an explicitly covered
/// interval. No thresholds are silently chosen and no physical pass is implied.
pub fn summarize_hold(
    frames: &[EpisodeFrame],
    completed: bool,
    source_cad_sha256: Option<&str>,
    config: &HoldConfig,
) -> Result<HoldReport, String> {
    if config
        .expected_cad_sha256
        .as_ref()
        .is_some_and(|hash| hash.trim().is_empty() || Some(hash.as_str()) != source_cad_sha256)
    {
        return Err("hold CAD source mismatch".into());
    }
    if !completed
        || frames.len() < 2
        || config.body_link.trim().is_empty()
        || !unit(config.body_up_local)
        || !unit(config.world_up)
        || !config.start_s.is_finite()
        || config.start_s < 0.0
        || !config.end_s.is_finite()
        || config.end_s <= config.start_s
        || !config.maximum_sample_gap_s.is_finite()
        || config.maximum_sample_gap_s <= 0.0
        || (frames[0].time_s - config.start_s).abs() > 1e-9
        || (frames.last().unwrap().time_s - config.end_s).abs() > 1e-9
    {
        return Err("incomplete hold interval or invalid configuration".into());
    }
    let mut report = HoldReport { samples: frames.len(), maximum_sample_gap_s:0.0,
        maximum_body_tilt_rad:0.0, maximum_body_horizontal_displacement_m:0.0,
        maximum_body_height_change_m:0.0, final_body_displacement_m:[0.0;3],
        maximum_marker_displacement_m:BTreeMap::new(), final_marker_displacement_m:BTreeMap::new(),
        scope:"Sampled motion relative to the first frame, with declared local/world up directions. No static-equilibrium, between-sample clearance, actuator-load, walking, or hardware-accuracy gate is established.".into() };
    let mut initial_body = None;
    let initial_markers = sample_markers(&frames[0], &config.markers)?;
    for (i, frame) in frames.iter().enumerate() {
        if frame.error.is_some() || !frame.time_s.is_finite() {
            return Err("failed/nonfinite hold frame".into());
        }
        if i > 0 {
            let gap = frame.time_s - frames[i - 1].time_s;
            if gap <= 0.0 || gap > config.maximum_sample_gap_s + 1e-9 {
                return Err("invalid hold sample coverage".into());
            }
            report.maximum_sample_gap_s = report.maximum_sample_gap_s.max(gap);
        }
        for p in &frame.poses {
            if !p.valid_rigid_transform() {
                return Err("invalid hold pose".into());
            }
        }
        let mut matching = frame.poses.iter().filter(|p| p.name == config.body_link);
        let body = matching.next().ok_or("missing hold body")?;
        if matching.next().is_some() {
            return Err("ambiguous hold body".into());
        }
        let origin = *initial_body.get_or_insert(body.position_m);
        let delta = sub(body.position_m, origin);
        let height = dot(delta, config.world_up);
        let horizontal = sub(delta, config.world_up.map(|v| v * height));
        let up = std::array::from_fn(|j| dot(body.rotation[j], config.body_up_local));
        report.maximum_body_tilt_rad = report
            .maximum_body_tilt_rad
            .max(dot(up, config.world_up).clamp(-1.0, 1.0).acos());
        report.maximum_body_height_change_m = report.maximum_body_height_change_m.max(height.abs());
        report.maximum_body_horizontal_displacement_m = report
            .maximum_body_horizontal_displacement_m
            .max(dot(horizontal, horizontal).sqrt());
        report.final_body_displacement_m = delta;
        let markers = sample_markers(frame, &config.markers)?;
        for (id, p) in markers.points {
            let delta = sub(p.position_m, initial_markers.points[&id].position_m);
            let max = report
                .maximum_marker_displacement_m
                .entry(id.clone())
                .or_default();
            *max = max.max(dot(delta, delta).sqrt());
            report.final_marker_displacement_m.insert(id, delta);
        }
    }
    Ok(report)
}

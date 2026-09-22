//! Shared sampled capture gates for directional motion search and validation.
use crate::fidelity::EnvironmentCapture;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gates {
    pub direction_world: [f64; 3],
    pub minimum_body_up_z: f64,
    pub maximum_tracking_rms_rad: f64,
    pub maximum_tracking_peak_rad: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub eligible_speed_m_s: Option<f64>,
    pub forward_displacement_m: f64,
    pub observed_s: f64,
    pub minimum_body_up_z: f64,
    pub tracking_rms_rad: Vec<f64>,
    pub tracking_peak_rad: Vec<f64>,
    pub rejection_reasons: Vec<String>,
}
pub fn evaluate(capture: &EnvironmentCapture, g: &Gates) -> Result<Report, String> {
    if g.direction_world.iter().any(|x| !x.is_finite())
        || g.direction_world[2] != 0.
        || (g.direction_world.iter().map(|x| x * x).sum::<f64>() - 1.).abs() > 1e-9
        || !g.minimum_body_up_z.is_finite()
        || !(-1. ..=1.).contains(&g.minimum_body_up_z)
        || [g.maximum_tracking_rms_rad, g.maximum_tracking_peak_rad]
            .iter()
            .any(|x| !x.is_finite() || *x <= 0.)
    {
        return Err(
            "explicit planar unit direction and finite positive tracking gates required".into(),
        );
    }
    let outcome = capture.outcome()?;
    let mut reasons = vec![];
    if outcome.eligible_score.is_none() {
        reasons.push("incomplete, terminated or failed episode".into());
    }
    let indices: Vec<usize> = serde_json::from_value(capture.metadata["joint_indices"].clone())
        .map_err(|e| e.to_string())?;
    if indices.is_empty() {
        return Err("missing independent coordinate mapping".into());
    }
    let mut rms = vec![0_f64; indices.len()];
    let mut peak = rms.clone();
    let mut up = 1_f64;
    let mut count = 0usize;
    for (frame, t) in capture.frames.iter().zip(&capture.transitions) {
        let speed = t
            .speed
            .as_ref()
            .ok_or("directional evaluation requires recorded body speed monitor")?;
        up = up.min(speed.body_up_z);
        if speed.fallen && !reasons.iter().any(|r| r == "fallen") {
            reasons.push("fallen".into());
        }
        if speed.body_floor_contact && !reasons.iter().any(|r| r == "body-floor contact") {
            reasons.push("body-floor contact".into());
        }
        if t.time_s == 0. {
            continue;
        }
        let targets = frame["servo_targets_rad"]
            .as_array()
            .ok_or("missing applied targets")?;
        if targets.len() != indices.len() {
            return Err("target/motor topology mismatch".into());
        }
        count += 1;
        for (i, j) in indices.iter().enumerate() {
            let target = targets[i]
                .as_f64()
                .filter(|x| x.is_finite())
                .ok_or("invalid target")?;
            let angle = frame["joint_positions"][*j]
                .as_f64()
                .filter(|x| x.is_finite())
                .ok_or("invalid coordinate")?;
            let error = (target - angle).abs();
            rms[i] = (rms[i] * ((count - 1) as f64 / count as f64).sqrt())
                .hypot(error / (count as f64).sqrt());
            peak[i] = peak[i].max(error);
        }
    }
    if up < g.minimum_body_up_z {
        reasons.push("body orientation gate".into());
    }
    if count == 0 {
        reasons.push("no committed motion samples".into());
    }
    if rms.iter().any(|e| *e > g.maximum_tracking_rms_rad) {
        reasons.push("tracking RMS gate".into());
    }
    if peak.iter().any(|e| *e > g.maximum_tracking_peak_rad) {
        reasons.push("tracking peak gate".into());
    }
    let last = capture.transitions.last().ok_or("empty capture")?;
    let displacement = last
        .speed
        .as_ref()
        .ok_or("missing final body motion")?
        .displacement_xy_m;
    let forward = displacement[0] * g.direction_world[0] + displacement[1] * g.direction_world[1];
    let score = forward / outcome.observed_s;
    if !score.is_finite() {
        reasons.push("no finite completed speed".into());
    }
    Ok(Report {
        eligible_speed_m_s: reasons.is_empty().then_some(score),
        forward_displacement_m: forward,
        observed_s: outcome.observed_s,
        minimum_body_up_z: up,
        tracking_rms_rad: rms,
        tracking_peak_rad: peak,
        rejection_reasons: reasons,
    })
}

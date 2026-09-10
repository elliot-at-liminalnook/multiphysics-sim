//! Recorded motion to reduced-position numerical seeds; no contact schedule.
use nalgebra::UnitQuaternion;
use serde::Deserialize;
use serde_json::json;
use sim_domain_robot::math::{M, quat};
use sim_runtime::{
    contact_implicit::ContactImplicitConfig,
    session::{LinkPose, Scene, Session},
};
#[derive(Deserialize)]
struct Frame {
    time_s: f64,
    poses: Vec<LinkPose>,
    joint_positions: Vec<f64>,
}
#[derive(Deserialize)]
struct Metadata {
    coordinate_names: Vec<String>,
    joint_indices: Vec<usize>,
}
#[derive(Deserialize)]
struct Capture {
    completed: bool,
    metadata: Metadata,
    frames: Vec<Frame>,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 4 {
        return Err(
            "usage: sample_capture_seed scene.json capture.json recipe.json start_s".into(),
        );
    }
    let scene: Scene = serde_json::from_slice(&std::fs::read(&a[0])?)?;
    let capture: Capture = serde_json::from_slice(&std::fs::read(&a[1])?)?;
    let recipe: serde_json::Value = serde_json::from_slice(&std::fs::read(&a[2])?)?;
    let config: ContactImplicitConfig = serde_json::from_value(recipe["config"].clone())?;
    let start: f64 = a[3].parse()?;
    if !capture.completed
        || !start.is_finite()
        || capture.metadata.coordinate_names != config.independent_coordinates
        || capture.metadata.joint_indices.len() != config.independent_coordinates.len()
        || capture.frames.len() < 2
        || capture
            .frames
            .windows(2)
            .any(|f| f[1].time_s <= f[0].time_s)
    {
        return Err(
            "complete time-ordered capture and matching independent coordinates required".into(),
        );
    }
    let session = Session::new(scene, 0)?;
    if session.robot.art.bases.len() != 1 {
        return Err("one floating base required".into());
    }
    let base = &session.robot.art.bases[0];
    if base.grounded {
        return Err("floating base required".into());
    }
    let name = &session.robot.art.links[base.link].name;
    let state = session.robot.generalized();
    let b = base.state;
    let r0 = quat(
        state.states[b + 3],
        state.states[b + 4],
        state.states[b + 5],
        state.states[b + 6],
    );
    let mut samples = vec![];
    for frame in &capture.frames {
        let p = frame
            .poses
            .iter()
            .find(|p| &p.name == name)
            .ok_or("missing root pose")?;
        let rotation = M::from_row_slice(&p.rotation.into_iter().flatten().collect::<Vec<_>>());
        if (rotation.transpose() * rotation - M::identity()).norm() > 1e-8
            || (rotation.determinant() - 1.).abs() > 1e-8
        {
            return Err("invalid captured rotation".into());
        }
        let phi = (UnitQuaternion::from_matrix(&rotation) * r0.inverse()).scaled_axis();
        let mut q = p.position_m.to_vec();
        q.extend(phi.iter());
        for &i in &capture.metadata.joint_indices {
            q.push(
                *frame
                    .joint_positions
                    .get(i)
                    .ok_or("missing joint position")?,
            );
        }
        if q.iter().any(|v| !v.is_finite()) {
            return Err("nonfinite capture pose".into());
        }
        samples.push((frame.time_s, q));
    }
    let k = config.position_reference.len() - 1;
    let period = k as f64 * config.step_s;
    if start < samples[0].0 || start + period > samples.last().unwrap().0 {
        return Err("capture does not cover requested cycle".into());
    }
    let mut positions = vec![];
    for i in 0..=k {
        let t = start + i as f64 * config.step_s;
        let upper = samples
            .partition_point(|s| s.0 < t)
            .clamp(1, samples.len() - 1);
        let (ta, qa) = &samples[upper - 1];
        let (tb, qb) = &samples[upper];
        let u = (t - ta) / (tb - ta);
        positions.push(
            qa.iter()
                .zip(qb)
                .map(|(a, b)| a + (b - a) * u)
                .collect::<Vec<_>>(),
        );
    }
    let delta = positions[k]
        .iter()
        .zip(&positions[0])
        .map(|(b, a)| b - a)
        .collect::<Vec<_>>();
    let origin = positions[0].clone();
    for (i, q) in positions.iter_mut().enumerate() {
        for j in 0..q.len() {
            if j < 2 {
                q[j] += config.position_reference[0][j] - origin[j];
            } else {
                q[j] -= delta[j] * i as f64 / k as f64;
            }
        }
    }
    positions[0][..2].copy_from_slice(&config.position_reference[0][..2]);
    positions[k] = positions[0]
        .iter()
        .enumerate()
        .map(|(j, v)| if j < 2 { v + delta[j] } else { *v })
        .collect();
    println!(
        "{}",
        serde_json::to_string(
            &json!({"positions":positions,"source_capture":a[1],"start_s":start,"period_s":period,
        "removed_endpoint_drift":delta.iter().enumerate().map(|(j,v)|if j<2 {0.}else{*v}).collect::<Vec<_>>(),
        "scope":"Recorded poses converted with shared CAD base rotation and independent-coordinate mapping; linear interpolation and non-XY endpoint detrending form a numerical seed only. Samples used as cubic controls are smoothed, not exact replay. No captured contact schedule is imposed; no feasibility claim."})
        )?
    );
    Ok(())
}

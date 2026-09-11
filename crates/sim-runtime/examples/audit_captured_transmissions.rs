//! Check captured transmission motion through the shared original constraint
//! equations. This evaluates no dynamics and does not infer hardware losses.
use serde_json::{Value, json};
use sim_runtime::{
    motion_data::MotionSnapshot,
    session::{Scene, Session},
};
use std::{collections::BTreeMap, fs};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err(
            "usage: audit_captured_transmissions motion-capture.json fresh-report.json".into(),
        );
    }
    let bytes = fs::read(&args[0])?;
    let capture: Value = serde_json::from_slice(&bytes)?;
    if !capture["error"].is_null() {
        return Err("capture has a numerical error".into());
    }
    let scene: Scene = serde_json::from_value(capture["recording"]["scene"].clone())?;
    let session = Session::new(
        scene,
        capture["recording"]["seed"]
            .as_u64()
            .ok_or("missing seed")?,
    )?;
    let art = &session.robot.art;
    if art.transmissions.is_empty() {
        return Err("recorded robot declares no transmissions".into());
    }
    let captured = capture["metadata"]["frame_coordinates"]
        .as_array()
        .ok_or("missing coordinates")?;
    let names: Vec<_> = art.dofs().map(|(_, d)| d.name.as_str()).collect();
    if captured.len() != names.len()
        || captured.iter().zip(&names).enumerate().any(|(i, (c, n))| {
            c["index"].as_u64() != Some(i as u64) || c["name"].as_str() != Some(*n)
        })
    {
        return Err("capture coordinate order differs from the recorded CAD model".into());
    }
    let frames = capture["frames"].as_array().ok_or("missing frames")?;
    if frames.len() < 2 {
        return Err("at least two motion samples are required".into());
    }
    let mut maxima: BTreeMap<String, (f64, f64)> = art
        .transmissions
        .iter()
        .map(|t| (t.name.clone(), (0., 0.)))
        .collect();
    let mut state = session.robot.generalized();
    let mut previous = None;
    for frame in frames {
        let motion = MotionSnapshot::from_frame(frame)?;
        if motion.joint_positions.len() != names.len()
            || previous.is_some_and(|t| motion.time_s <= t)
        {
            return Err("invalid capture coverage or clock".into());
        }
        previous = Some(motion.time_s);
        state.q = motion.joint_positions;
        state.qd = motion.joint_velocities;
        // Transmission position/velocity rows depend only on these generalized
        // coordinates. Do not report loop, acceleration or force rows: this
        // capture does not reconstruct their full solver state/rates.
        for row in art.original_closure(&state) {
            if let Some((p, v)) = maxima.get_mut(&row.name) {
                if !row.position.is_finite() || !row.velocity.is_finite() {
                    return Err("nonfinite transmission residual".into());
                }
                *p = p.max(row.position.abs());
                *v = v.max(row.velocity.abs());
            }
        }
    }
    let rows: Vec<_> = art
        .transmissions
        .iter()
        .map(|t| {
            let (p, v) = maxima[&t.name];
            let unit = captured[t.driver]["position_unit"].clone();
            json!({"name":t.name,"driver":names[t.driver],"driven":names[t.driven],"ratio":t.ratio,
            "position_unit":unit,"velocity_unit":captured[t.driver]["velocity_unit"],
            "maximum_absolute_position_residual":p,"maximum_absolute_velocity_residual":v})
        })
        .collect();
    let report = json!({"version":1,"capture":args[0],"capture_blake3":blake3::hash(&bytes).to_hex().to_string(),
        "capture_physics_context":capture["metadata"]["physics_context"],"cad_source":session.scene.robot.source,
        "samples":frames.len(),"start_s":frames[0]["time_s"],"end_s":previous,"rows":rows,
        "equation":"q_driver - ratio * q_driven = 0; same relation for velocity, evaluated by Articulated::original_closure",
        "scope":"Sampled kinematic consistency with the recorded CAD declarations. No independent hardware ratio, backlash, efficiency, torque or continuous-time certification. Other closure rows and accelerations are deliberately not inferred from incomplete solver state."});
    let output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[1])?;
    serde_json::to_writer_pretty(output, &report)?;
    Ok(())
}

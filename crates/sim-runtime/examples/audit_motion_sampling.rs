//! Inspect sampling error without changing the controller or physics clock.
use serde_json::json;
use sim_domain_control::displacement::DisplacementAxes;
use sim_runtime::{
    embedded::{CaptureMode, EmbeddedSession},
    environment::EnvironmentRecording,
    motion_data::MotionSnapshot,
    motion_response::{BodySample, HeadingSample, summarize},
};
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        return Err("usage: audit_motion_sampling input-recording.json reference-link prefix-seconds fresh-report.json".into());
    }
    let bytes = fs::read(&args[0])?;
    let record: EnvironmentRecording = serde_json::from_slice(&bytes)?;
    let dt = record.runtime.config.step_s;
    let seconds: f64 = args[2].parse()?;
    let steps = seconds / dt;
    let stride = (record.task.period_s / dt).round() as usize;
    if !seconds.is_finite()
        || seconds <= 0.
        || stride == 0
        || !record.task.period_s.is_finite()
        || !steps.is_finite()
        || (steps - steps.round()).abs() > 1e-8
        || steps > record.runtime.completed_steps as f64
    {
        return Err("prefix must lie on the captured physics clock".into());
    }
    let (mut runtime, _) = EmbeddedSession::prepare_replay(record.runtime, CaptureMode::Latest)?;
    let mut samples = vec![];
    for i in 0..=steps.round() as usize {
        if i > 0 {
            runtime.advance(1)?;
        }
        let snapshot = MotionSnapshot::from_frame(&runtime.frame()?)?;
        let body = snapshot
            .poses
            .iter()
            .find(|p| p.name == args[1])
            .ok_or("missing reference link")?;
        let forward = std::array::from_fn(|i| body.rotation[i][0]);
        let [angle_rad, rate_rad_s] =
            sim_domain_control::heading::world_z_heading(forward, body.angular_velocity_rad_s)?;
        samples.push(BodySample {
            time_s: snapshot.time_s,
            position_world_m: body.position_m,
            velocity_world_m_s: body.velocity_m_s,
            heading_world_z: Some(HeadingSample {
                angle_rad,
                rate_rad_s,
            }),
        });
    }
    let mut intervals = vec![1, stride];
    let mut k = 2;
    while k < stride {
        if stride % k == 0 {
            intervals.push(k);
        }
        k *= 2;
    }
    intervals.sort();
    intervals.dedup();
    let mut rows = vec![];
    for skip in intervals {
        if (samples.len() - 1) % skip != 0 {
            continue;
        }
        let sampled: Vec<_> = samples.iter().step_by(skip).cloned().collect();
        let response = summarize(&sampled, DisplacementAxes::Xy)?;
        rows.push(json!({"sample_interval_s":skip as f64*dt,"response":response,
            "heading_integral_minus_sampled_orientation_rad":response.integrated_heading_change_rad.unwrap()-response.sampled_unwrapped_heading_change_rad.unwrap()}));
    }
    let report = json!({"version":1,"input":args[0],"input_blake3":blake3::hash(&bytes).to_hex().to_string(),
        "physics_context":runtime.physics_context(),"rows":rows,"endpoint":samples.last(),
        "scope":"Same shared embedded replay and authored controller clock. Only read-only observation cadence changes. Orientation unwrapping still assumes less than pi rotation between physics endpoints; no continuous-time or hardware certificate."});
    let file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&args[3])?;
    serde_json::to_writer_pretty(file, &report)?;
    Ok(())
}

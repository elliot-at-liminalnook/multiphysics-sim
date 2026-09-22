//! Offline gates over a saved runtime capture and its shared geometry audit.
use sim_runtime::{fidelity::EnvironmentCapture, geometry_evaluation, motion_evaluation};
use std::{
    fs,
    io::{BufReader, BufWriter, Write},
    path::Path,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 5 {
        return Err("usage: evaluate_validation_capture capture.json geometry.json motion-gates.json geometry-gates.json fresh-report.json".into());
    }
    let bytes = fs::read(&a[0])?;
    let capture: EnvironmentCapture = serde_json::from_slice(&bytes)?;
    let motion =
        motion_evaluation::evaluate(&capture, &serde_json::from_slice(&fs::read(&a[2])?)?)?;
    let audit = serde_json::from_reader(BufReader::new(fs::File::open(&a[1])?))?;
    let expected_frame_times_s = capture
        .frames
        .iter()
        .map(|f| f["time_s"].as_f64().ok_or("missing capture time"))
        .collect::<Result<Vec<_>, _>>()?;
    let geometry = geometry_evaluation::evaluate(&geometry_evaluation::Request {
        audit,
        expected_capture_blake3: blake3::hash(&bytes).to_hex().to_string(),
        expected_duration_s: capture.recording.config.steps as f64
            * capture.recording.config.step_s,
        expected_frame_times_s,
        gates: serde_json::from_slice(&fs::read(&a[3])?)?,
    })?;
    let passed = motion.eligible_speed_m_s.is_some() && geometry.passed;
    let value = serde_json::json!({"version":1,"capture":a[0],"capture_blake3":blake3::hash(&bytes).to_hex().to_string(),"runtime":capture.recording.runtime_identity,"evaluator_runtime":sim_runtime::physics_context::RuntimeIdentity::current(),"passed":passed,"motion":motion,"geometry":geometry,"scope":"Sampled modeled-motion validation only; no measured loaded hardware claim"});
    let f = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(Path::new(&a[4]))?;
    let mut w = BufWriter::new(f);
    serde_json::to_writer(&mut w, &value)?;
    w.flush()?;
    w.get_ref().sync_all()?;
    println!("Validation passed={passed}");
    Ok(())
}

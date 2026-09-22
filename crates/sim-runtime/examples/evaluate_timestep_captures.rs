//! Thin durable host for the shared matched-timestep endpoint evaluator.
use sim_runtime::{fidelity::EnvironmentCapture, numerical_validation};
use std::{
    fs,
    io::{BufWriter, Write},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 4 {
        return Err("usage: evaluate_timestep_captures reference.json refined.json gates.json fresh-report.json".into());
    }
    let reference_bytes = fs::read(&a[0])?;
    let refined_bytes = fs::read(&a[1])?;
    let reference: EnvironmentCapture = serde_json::from_slice(&reference_bytes)?;
    let refined: EnvironmentCapture = serde_json::from_slice(&refined_bytes)?;
    let gates = serde_json::from_slice(&fs::read(&a[2])?)?;
    let report = numerical_validation::compare(&reference, &refined, &gates)?;
    let value = serde_json::json!({"version":1,"reference":a[0],"refined":a[1],
        "reference_blake3":blake3::hash(&reference_bytes).to_hex().to_string(),
        "refined_blake3":blake3::hash(&refined_bytes).to_hex().to_string(),
        "reference_runtime":reference.recording.runtime_identity,
        "refined_runtime":refined.recording.runtime_identity,
        "evaluator_runtime":sim_runtime::physics_context::RuntimeIdentity::current(),
        "gates":gates,"report":report});
    let f = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&a[3])?;
    let mut w = BufWriter::new(f);
    serde_json::to_writer(&mut w, &value)?;
    w.flush()?;
    w.get_ref().sync_all()?;
    println!("Timestep endpoint gates passed={}", report.passed);
    Ok(())
}

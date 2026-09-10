//! Compare existing production captures with an explicit, reviewable plan.
use sim_runtime::fidelity::{ComparisonPlan, EnvironmentCapture, compare};
use std::io::{BufWriter, Write};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        return Err("usage: compare_environment_fidelity reference.json candidate.json plan.json fresh-report.json".into());
    }
    let reference: EnvironmentCapture = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let candidate: EnvironmentCapture = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let plan: ComparisonPlan = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let report = compare(&reference, &candidate, &plan)?;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[3])?;
    // CAD meshes make contexts large; buffer serialization rather than issuing
    // one filesystem write per JSON token, and surface final flush failures.
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, &report)?;
    writer.flush()?;
    println!(
        "{}",
        serde_json::json!({"compared_frames":report.compared_frames,"matched_input_duration_s":report.matched_input_duration_s,
        "trajectory_within_tolerances":report.trajectory_within_tolerances,"categorical_outcomes_match":report.categorical_outcomes_match})
    );
    Ok(())
}

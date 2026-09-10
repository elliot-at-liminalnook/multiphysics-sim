//! Verify that a physical capture actually executed an authored command plan.
use serde_json::{Value, json};
use sim_domain_control::trajectory::Trajectory;
use sim_runtime::predictive_control::ForecastActionReference;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err(
            "usage: audit_actuator_reference reference.json capture.json new-report.json".into(),
        );
    }
    let reference: ForecastActionReference = serde_json::from_slice(&fs::read(&args[0])?)?;
    let capture: Value = serde_json::from_slice(&fs::read(&args[1])?)?;
    if capture["error"] != Value::Null
        || capture["recording"]["scene"]["robot"]["source"]["cad_sha256"].as_str()
            != Some(&reference.expected_cad_sha256)
    {
        return Err("capture numerical error or reference CAD mismatch".into());
    }
    let channels: Vec<sim_core::Channel> =
        serde_json::from_value(capture["metadata"]["policy_contract"]["actuators"].clone())?;
    if channels.len() != reference.actuators.len()
        || channels.iter().zip(&reference.actuators).any(|(a, b)| {
            a.name != b.name || a.kind != b.kind || a.kind != sim_core::QuantityKind::Angle
        })
    {
        return Err("reference/capture typed actuator order mismatch".into());
    }
    let trajectory = Trajectory::new(reference.trajectory)?;
    if trajectory.dimension() != channels.len() {
        return Err("reference dimension mismatch".into());
    }
    let period = capture["metadata"]["policy_contract"]["period_s"]
        .as_f64()
        .ok_or("missing policy period")?;
    let frames = capture["frames"]
        .as_array()
        .ok_or("missing physical frames")?;
    if frames.len() < 2 || !period.is_finite() || period <= 0. {
        return Err("invalid capture clock/frames".into());
    }
    let mut maximum_error = 0f64;
    let mut mismatched = 0usize;
    for pair in frames.windows(2) {
        let time = pair[1]["policy"]["time_s"]
            .as_f64()
            .ok_or("missing command clock")?;
        let before = pair[0]["time_s"].as_f64().ok_or("missing state clock")?;
        let after = pair[1]["time_s"].as_f64().ok_or("missing state clock")?;
        if !time.is_finite()
            || !before.is_finite()
            || !after.is_finite()
            || (time - before).abs() > 1e-8
            || (after - before - period).abs() > 1e-8
        {
            return Err("command/state clock mismatch".into());
        }
        let planned = trajectory.sample(time)?;
        for (i, channel) in channels.iter().enumerate() {
            let actual = pair[1]["policy"]["targets"][&channel.name]
                .as_f64()
                .ok_or("missing applied command")?;
            if !actual.is_finite() {
                return Err("nonfinite applied command".into());
            }
            let error = (actual - planned.values[i]).abs();
            maximum_error = maximum_error.max(error);
            mismatched += usize::from(error > 1e-12);
        }
    }
    let report = json!({"version":1,"intervals":frames.len()-1,"actuators":channels.len(),
        "maximum_command_error_rad":maximum_error,"mismatched_commands":mismatched,
        "scope":"Shared Rust trajectory sampling compared against every reported applied motor target at its actual policy clock. Matching verifies execution of the precomputed plan; it does not certify model accuracy, hardware calibration or locomotion speed."});
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[2])?;
    serde_json::to_writer_pretty(file, &report)?;
    if mismatched > 0 {
        return Err(
            "capture did not execute the authored command plan; inspect saved audit".into(),
        );
    }
    Ok(())
}

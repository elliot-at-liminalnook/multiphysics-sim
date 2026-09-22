//! Reproducible shared-controller prediction of retained FPGA motor experiments.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{fs, io::Write};
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() == 4 && args[1] == "verify" {
        let recording: sim_runtime::controller_refinement::fpga::Recording =
            serde_json::from_slice(&fs::read(&args[2])?)?;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Results {
            predictions: Vec<sim_runtime::controller_refinement::fpga_review::Prediction>,
        }
        let results: Results = serde_json::from_slice(&fs::read(&args[3])?)?;
        let mut ids = Vec::new();
        for prediction in &results.predictions {
            prediction.validate(&recording)?;
            ids.push(prediction.id);
        }
        ids.sort();
        if ids != recording.plan.ids {
            return Err("Result must cover each recorded motor exactly once".into());
        }
        println!("Verified {} motor predictions; this verifies evidence consistency, not accuracy acceptance", ids.len());
        return Ok(());
    }
    if args.len() != 5 {
        return Err("review_fpga_controller RECORDING MODEL_SETTINGS_OR_CANDIDATE replay|closed-loop|replay-voltage|closed-loop-voltage NEW_OUTPUT".into());
    }
    let r: sim_runtime::controller_refinement::fpga::Recording =
        serde_json::from_slice(&fs::read(&args[1])?)?;
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&args[2])?)?;
    use sim_runtime::controller_refinement::fpga_review::Mode;
    let mode = match args[3].as_str() {
        "closed-loop" => Mode::ClosedLoop,
        "replay" => Mode::Replay,
        "replay-voltage" => Mode::ReplayMeasuredVoltage,
        "closed-loop-voltage" => Mode::ClosedLoopMeasuredVoltage,
        _ => return Err("Unknown prediction mode".into()),
    };
    let mut predictions = Vec::new();
    for &id in &r.plan.ids {
        let model = if value.get("shared").is_some() {
            serde_json::from_value::<sim_runtime::controller_refinement::calibration::Family>(
                value.clone(),
            )?
            .model(id)?
        } else {
            serde_json::from_value::<sim_runtime::experiment_study::ModelSettings>(value.clone())?
        };
        predictions.push(if mode.measured_voltage() {
            serde_json::to_value(sim_runtime::controller_refinement::fpga_voltage::predict(
                &r, id, &model, mode, &std::sync::atomic::AtomicBool::new(false), |_, _| {},
            )?)?
        } else {
            sim_runtime::controller_refinement::fpga::predict(&r, id, &model, mode.closed_loop())?
        });
    }
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&args[4])?;
    file.write_all(&serde_json::to_vec_pretty(
        &serde_json::json!({"predictions":predictions}),
    )?)?;
    Ok(())
}

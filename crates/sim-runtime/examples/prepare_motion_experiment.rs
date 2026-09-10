//! Extract immutable experiment inputs through shared recorded-action validation.
use serde_json::Value;
use sim_runtime::{embedded::EmbeddedRecording, environment::Task};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err(
            "usage: prepare_motion_experiment completed-capture.json fresh-directory".into(),
        );
    }
    let capture: Value = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let record: EmbeddedRecording = serde_json::from_value(capture["recording"].clone())?;
    let task: Task = serde_json::from_value(capture["task"].clone())?;
    if capture["completed"] != true
        || !capture["error"].is_null()
        || record.failure.is_some()
        || record.completed_steps != record.config.steps
    {
        return Err("requires a complete successful environment capture".into());
    }
    let frames = capture["frames"]
        .as_array()
        .ok_or("missing captured frames")?;
    let inputs = &record
        .scene
        .controller
        .as_ref()
        .ok_or("missing controller")?
        .inputs;
    let actions = sim_runtime::forecast_actions::from_recording(&record, frames, inputs)?;
    std::fs::create_dir(&args[1])?;
    for (name, value) in [
        ("scene", serde_json::to_value(&record.scene)?),
        ("config", serde_json::to_value(&record.config)?),
        ("task", serde_json::to_value(task)?),
        ("actions", serde_json::to_value(actions)?),
        (
            "source",
            serde_json::json!({"capture":args[0],"runtime_identity":record.runtime_identity,"seed":record.seed,
        "scope":"Exact parsed scene/config/task and recorded held commands. A new run records its own source identity and seed; no prediction model is relabeled."}),
        ),
    ] {
        let file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(std::path::Path::new(&args[1]).join(format!("{name}.json")))?;
        let mut output = std::io::BufWriter::new(file);
        serde_json::to_writer(&mut output, &value)?;
        std::io::Write::flush(&mut output)?;
    }
    Ok(())
}

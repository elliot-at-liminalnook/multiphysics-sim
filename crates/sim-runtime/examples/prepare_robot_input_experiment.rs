//! Synthetic provenance fixture on any existing experiment, using shared Scene APIs.
use serde_json::json;
use sim_runtime::experiment::{Experiment, ExperimentSpec};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: prepare_robot_input_experiment spec.json fresh-spec.json".into());
    }
    let mut spec: ExperimentSpec = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let mut input = serde_json::to_value(&spec.scene)?["robot"].clone();
    input["world"]
        .as_object_mut()
        .ok_or("missing world object")?
        .remove("ambient_c");
    input["cad_input_fixture"] =
        json!({"kind":"synthetic field-presence test","source_experiment":args[0]});
    spec.scene.replace_robot_input(input.clone())?;
    spec.scene.robot.world.ambient_c = 21.;
    let binding = spec.scene.input_binding()?;
    if binding.overrides.len() != 1
        || binding.overrides[0].pointer != "/world/ambient_c"
        || !matches!(
            binding.overrides[0].original,
            sim_runtime::robot_input::OriginalValue::Absent
        )
    {
        return Err("fixture did not retain the absent ambient input as one override".into());
    }
    let spec = Experiment::bind(spec)?.spec;
    let bytes = serde_json::to_vec(&spec)?;
    let restored: ExperimentSpec = serde_json::from_slice(&bytes)?;
    if restored
        .scene
        .robot_input
        .as_ref()
        .ok_or("lost robot input")?
        .document()
        != &input
    {
        return Err("serialized experiment lost original robot input".into());
    }
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[1])?;
    std::io::Write::write_all(&mut output, &bytes)?;
    Ok(())
}

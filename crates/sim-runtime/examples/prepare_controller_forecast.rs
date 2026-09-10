//! Package an explicitly shortened recorded prefix for shared API acceptance.
//! No robot property or controller is changed; the horizon override is recorded.
use serde::Serialize;
use sim_runtime::{environment::EnvironmentRecording, motion_forecast::*};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: prepare_controller_forecast episode-recording.json fresh-directory reference-link".into());
    }
    let mut record: EnvironmentRecording = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    if record.error.is_some()
        || record.runtime.failure.is_some()
        || record.runtime.completed_steps == 0
    {
        return Err("requires a successful recorded prefix".into());
    }
    let original_horizon_steps = record.runtime.config.steps;
    record.runtime.config.steps = record.runtime.completed_steps;
    let scene = &record.runtime.scene;
    let recipe = ForecastRecipe {
        expected_cad_sha256: scene.robot.source["cad_sha256"]
            .as_str()
            .ok_or("missing CAD identity")?
            .into(),
        reference_link: args[2].clone(),
        axes: (0..3)
            .map(|axis| MotionAxis::Link {
                name: format!("reference.{axis}"),
                link: args[2].clone(),
                axis,
            })
            .collect(),
        actuator_targets: vec![],
        // A preparation recipe names the runtime that must generate its labels.
        // Legacy source recordings retain their unknown identity; replay output
        // must carry this source identity before sample extraction can succeed.
        physics_context: Some(sim_runtime::physics_context::PhysicsContext::from_runtime(
            scene,
            &record.runtime.config,
        )?),
        controller_context: Some(
            sim_runtime::forecast_actions::ControllerContext::from_runtime(
                scene,
                &record.runtime.config,
            )?,
        ),
        controller_inputs: scene
            .controller
            .as_ref()
            .ok_or("missing controller")?
            .inputs
            .clone(),
        horizons_steps: vec![1, 2],
        period_s: record.task.period_s,
        reference: KinematicReference::ConstantAcceleration,
        terrain_relative_links: vec![],
        imu_observations: scene
            .robot
            .sensors
            .iter()
            .filter(|s| s.kind == "imu")
            .map(|s| s.name.clone())
            .collect(),
    };
    recipe.validate()?;
    std::fs::create_dir(&args[1])?;
    write_json(&args[1], "recording.json", &record)?;
    write_json(&args[1], "recipe.json", &recipe)?;
    write_json(
        &args[1],
        "data.json",
        &serde_json::json!({"scene":scene,"config":record.runtime.config,"task":record.task}),
    )?;
    write_json(
        &args[1],
        "scope.json",
        &serde_json::json!({"source_recording":args[0],
        "source_runtime_identity":record.runtime.runtime_identity,
        "expected_runtime_identity":recipe.physics_context.as_ref().map(|c|&c.runtime),
        "original_horizon_steps":original_horizon_steps,"acceptance_horizon_steps":record.runtime.config.steps,
        "scope":"Recorded prefix promoted to a complete short API acceptance episode. Only the configured step horizon changes. Does not qualify sustained speed or prediction accuracy."}),
    )?;
    Ok(())
}
fn write_json(
    root: &str,
    name: &str,
    value: &impl Serialize,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(std::path::Path::new(root).join(name))?;
    serde_json::to_writer(file, value)?;
    Ok(())
}

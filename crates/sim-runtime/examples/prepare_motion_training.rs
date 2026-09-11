//! Configure the existing shared forecaster from a capture's typed coordinates.
use serde_json::{Value, json};
use sim_runtime::{
    embedded::EmbeddedRecording, motion_forecast::*, physics_context::PhysicsContext,
};
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        return Err("usage: prepare_motion_training capture.json reference-link training-end-s fresh-experiment.json".into());
    }
    let capture: Value = serde_json::from_slice(&fs::read(&args[0])?)?;
    let record: EmbeddedRecording = serde_json::from_value(capture["recording"].clone())?;
    let frames = capture["frames"].as_array().ok_or("missing frames")?;
    let period = frames.get(1).ok_or("missing second frame")?["time_s"]
        .as_f64()
        .ok_or("missing time")?
        - frames[0]["time_s"].as_f64().ok_or("missing time")?;
    let end = frames.last().ok_or("empty capture")?["time_s"]
        .as_f64()
        .ok_or("missing end")?;
    let split: f64 = args[2].parse()?;
    if !split.is_finite() || split <= 12. * period || split + 13. * period >= end {
        return Err("both training and validation need complete prediction windows".into());
    }
    let coordinates = capture["metadata"]["frame_coordinates"]
        .as_array()
        .ok_or("missing coordinate contract")?;
    let mut axes = vec![];
    for (index, coordinate) in coordinates.iter().enumerate() {
        let position_kind = match coordinate["position_unit"].as_str() {
            Some("rad") => sim_core::QuantityKind::Angle,
            Some("m") => sim_core::QuantityKind::Length,
            _ => return Err("unsupported coordinate unit".into()),
        };
        axes.push(MotionAxis::Joint {
            name: coordinate["name"]
                .as_str()
                .ok_or("missing coordinate name")?
                .into(),
            index,
            position_kind,
        });
    }
    for axis in 0..3 {
        axes.push(MotionAxis::Link {
            name: format!("reference.{axis}"),
            link: args[1].clone(),
            axis,
        });
    }
    let recipe = ForecastRecipe {
        expected_cad_sha256: record.scene.robot.source["cad_sha256"]
            .as_str()
            .ok_or("missing CAD identity")?
            .into(),
        reference_link: args[1].clone(),
        axes,
        actuator_targets: vec![],
        controller_inputs: record
            .scene
            .controller
            .as_ref()
            .ok_or("missing controller")?
            .inputs
            .clone(),
        controller_context: Some(
            sim_runtime::forecast_actions::ControllerContext::from_runtime(
                &record.scene,
                &record.config,
            )?,
        ),
        physics_context: Some(PhysicsContext::from_recording(&record)?),
        horizons_steps: vec![1, 5, 10],
        period_s: period,
        reference: KinematicReference::ConstantAcceleration,
        terrain_relative_links: vec![args[1].clone()],
        imu_observations: record
            .scene
            .robot
            .sensors
            .iter()
            .filter(|s| s.kind == "imu")
            .map(|s| s.name.clone())
            .collect(),
    };
    recipe.validate()?;
    // Validate both windows now, before creating an expensive training job.
    let training = samples_from_capture(&capture, &recipe, 0., split)?;
    let validation = samples_from_capture(&capture, &recipe, split + period, end)?;
    let experiment = json!({"version":1,"recipe":recipe,
        "training":[{"path":args[0],"start_s":0.,"end_s":split}],
        "validation":[{"path":args[0],"start_s":split+period,"end_s":end}],
        "width":64,"seed":2302,"epochs":30,"batch_size":64,"learning_rate":0.001});
    let file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&args[3])?;
    serde_json::to_writer_pretty(file, &experiment)?;
    eprintln!(
        "{} training / {} validation samples; disjoint chronological windows from one episode, not independent-episode acceptance",
        training.len(),
        validation.len()
    );
    Ok(())
}

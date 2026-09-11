//! Exercise typed prediction/training and read-only online queries on a real
//! captured episode. This is API acceptance, not held-out prediction validation.
use serde_json::{Value, json};
use sim_runtime::{
    embedded::EmbeddedRecording,
    environment::{EmbeddedEnvironment, EnvironmentRecording, Task},
    motion_data::MotionSnapshot,
    motion_forecast::*,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !(3..=4).contains(&args.len()) {
        return Err(
            "usage: check_controller_forecast native-capture.json recipe.json fresh-report.json [trained-model.json]"
                .into(),
        );
    }
    let capture: Value = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let recipe: ForecastRecipe = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let frames = capture["frames"]
        .as_array()
        .ok_or("missing capture frames")?;
    let start = frames[0]["time_s"].as_f64().ok_or("missing start")?;
    let end = frames.last().unwrap()["time_s"]
        .as_f64()
        .ok_or("missing end")?;
    let samples = samples_from_capture(&capture, &recipe, start, end)?;
    let mut model = if let Some(path) = args.get(3) {
        let model: TrajectoryForecaster = serde_json::from_slice(&std::fs::read(path)?)?;
        if serde_json::to_value(&model.recipe)? != serde_json::to_value(&recipe)? {
            return Err("supplied model recipe differs from query recipe".into());
        }
        model
    } else {
        TrajectoryForecaster::initialize(recipe, &samples, 4, 7)?
    };
    let initial_loss = model.normalized_loss(&samples)?;
    let losses = if args.len() == 3 {
        model.fit(&samples, 4, 2, 0.001)?
    } else {
        vec![]
    };
    model.validate()?;
    let record: EmbeddedRecording = serde_json::from_value(capture["recording"].clone())?;
    let task: Task = serde_json::from_value(capture["task"].clone())?;
    let loaded = EmbeddedEnvironment::new(
        record.scene.clone(),
        record.config.clone(),
        task.clone(),
        record.seed,
    )?;
    let (mut env, actions) = loaded.prepare_replay(EnvironmentRecording {
        version: 1,
        kind: "sampled_environment_recording".into(),
        task,
        runtime: record,
        error: None,
    })?;
    let horizon = *model.recipe.horizons_steps.last().unwrap();
    let mut queries = Vec::new();
    let mut previous = MotionSnapshot::from_frame(&env.frame()?)?;
    for (i, sample) in samples.iter().enumerate() {
        env.step(&actions[i])?;
        if (env.transition().time_s - sample.time_s).abs() > 1e-10 {
            return Err("capture must begin at reset and use every environment transition".into());
        }
        let current = env.frame()?;
        let recorded = serde_json::to_value(env.episode_recording())?;
        let future = &actions[i + 1..i + 1 + horizon];
        let prediction = env.predict_controller_trajectory(&model, &previous, future)?;
        if prediction.inputs != sample.inputs || prediction.prior != sample.prior {
            return Err("online forecast differs from recorded training input/reference".into());
        }
        if recorded != serde_json::to_value(env.episode_recording())? {
            return Err("forecast changed recording".into());
        }
        queries.push(json!({"previous":previous,"future_actions":future,"prediction":prediction}));
        previous = MotionSnapshot::from_frame(&current)?;
    }
    let report = json!({"version":1,"passed":true,"source_capture":args[0],"model":model,"queries":queries,
        "training_samples":if args.len()==3 {samples.len()} else {0},
        "query_samples":samples.len(),"initial_capture_loss":initial_loss,"training_losses":losses,
        "supplied_model":args.get(3),
        "scope":"Complete typed controller action contract and live read-only query agreement with recorded inputs/references. Without a supplied model, all samples enter a short fitting exercise. A supplied model is queried without fitting. This checks API consistency, not held-out accuracy, improved control or learned morphology transfer."});
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[2])?;
    serde_json::to_writer(file, &report)?;
    println!("{} controller forecast queries passed", queries.len());
    Ok(())
}

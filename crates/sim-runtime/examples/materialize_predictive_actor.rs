//! Bind a trained predictive actor to an existing experiment without changing
//! its robot, task, command schedule, horizon or physics context.
use sim_domain_control::neural::Network;
use sim_runtime::{
    environment::{EmbeddedEnvironment, EnvironmentRecording},
    predictive_policy::ForecastBundle,
};
use std::{fs, io::Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        return Err("usage: materialize_predictive_actor input-recording.json forecast-bundle.json actor.json fresh-recording.json".into());
    }
    let mut record: EnvironmentRecording = serde_json::from_slice(&fs::read(&args[0])?)?;
    let bundle: ForecastBundle = serde_json::from_slice(&fs::read(&args[1])?)?;
    let actor: Network = serde_json::from_slice(&fs::read(&args[2])?)?;
    bundle.validate()?;
    actor.validate()?;
    let policy = record
        .runtime
        .config
        .policy
        .as_mut()
        .ok_or("missing policy")?;
    if policy.neural_residual.is_some()
        || policy.trajectory_forecast.is_some()
        || policy.neural_exploration.is_some()
        || policy.forecast_action_search.is_some()
    {
        return Err("source must have an unmodified deterministic baseline policy".into());
    }
    policy.neural_residual = Some(actor);
    policy.trajectory_forecast = Some(bundle);
    policy.neural_command_saturation = true;
    // Shared runtime validates channel order, units, observed features, and the
    // forecast's physical provenance. Never relabel a model to fit a new scene.
    let environment = EmbeddedEnvironment::new(
        record.runtime.scene.clone(),
        record.runtime.config.clone(),
        record.task.clone(),
        record.runtime.seed,
    )?;
    environment.prepare_replay(record.clone())?;
    let mut output = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&args[3])?;
    serde_json::to_writer(&mut output, &record)?;
    output.write_all(b"\n")?;
    Ok(())
}

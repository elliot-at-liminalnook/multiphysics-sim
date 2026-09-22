//! Exercise a task through the same sampled environment exported to WASM.
use serde_json::json;
use sim_runtime::{
    embedded::Config,
    environment::{EmbeddedEnvironment, EnvironmentRecording, Task},
    session::Scene,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    let profile_path = if args.len() >= 2 && args[args.len() - 2] == "--profile" {
        let path = args.pop();
        args.pop();
        path
    } else {
        None
    };
    let experiment: Option<sim_runtime::experiment::ExperimentSpec> =
        if args.len() == 2 && args[0] == "--experiment" {
            Some(serde_json::from_slice(&std::fs::read(&args[1])?)?)
        } else {
            None
        };
    let motion = if let Some(spec) = &experiment {
        Some(spec.parameterization.materialize(
            &spec.scene,
            &spec.source_actions,
            &spec.baseline,
        )?)
    } else if args.len() == 4 && args[0] == "--motion" {
        let document: serde_json::Value = serde_json::from_slice(&std::fs::read(&args[1])?)?;
        let variant: sim_runtime::motion_parameters::MotionVariant =
            serde_json::from_value(document["variant"].clone())?;
        variant.validate()?;
        Some(variant)
    } else {
        None
    };
    let replay = args.len() == 2 && args[0] == "--replay";
    if !replay && experiment.is_none() && !(3..=4).contains(&args.len()) {
        return Err(
            "usage: run_environment scene.json config.json task.json [actions.json] [--profile report.json], or --replay episode.recording.json, or --motion materialized-motion.json config.json task.json, or --experiment spec.json".into(),
        );
    }
    let (mut env, actions, steps, episode_steps) = if replay {
        let record: EnvironmentRecording = serde_json::from_slice(&std::fs::read(&args[1])?)?;
        let steps = record.runtime.completed_steps;
        let episode_steps = record.runtime.config.steps;
        let loaded = EmbeddedEnvironment::new(
            record.runtime.scene.clone(),
            record.runtime.config.clone(),
            record.task.clone(),
            record.runtime.seed,
        )?;
        // Reuse the browser's validation, seed handling and held-action reconstruction.
        let (env, actions) = loaded.prepare_replay(record)?;
        (env, Some(actions), steps, episode_steps)
    } else if let Some(variant) = &motion {
        let (config, task, seed) = if let Some(spec) = &experiment {
            // Keep the same validation, complete command schedule, and seed as search_motion.
            sim_runtime::experiment::Experiment::bind(spec.clone())?;
            (spec.config.clone(), spec.task.clone(), spec.seed)
        } else {
            (
                serde_json::from_slice::<Config>(&std::fs::read(&args[2])?)?,
                serde_json::from_slice::<Task>(&std::fs::read(&args[3])?)?,
                0,
            )
        };
        let steps = config.steps;
        let env = EmbeddedEnvironment::new(variant.scene.clone(), config, task, seed)?;
        (env, Some(variant.actions.clone()), steps, steps)
    } else {
        let scene: Scene = serde_json::from_slice(&std::fs::read(&args[0])?)?;
        let config: Config = serde_json::from_slice(&std::fs::read(&args[1])?)?;
        let task: Task = serde_json::from_slice(&std::fs::read(&args[2])?)?;
        let steps = config.steps;
        let env = EmbeddedEnvironment::new(scene, config, task, 0)?;
        let actions: Option<Vec<Vec<f64>>> = args
            .get(3)
            .map(|p| -> Result<_, Box<dyn std::error::Error>> {
                Ok(serde_json::from_slice(&std::fs::read(p)?)?)
            })
            .transpose()?;
        (env, actions, steps, steps)
    };
    let initial_action = env.inputs().iter().map(|c| c.initial).collect::<Vec<_>>();
    let mut frames = vec![env.frame()?];
    let mut transitions = vec![env.transition().clone()];
    // Standalone diagnostics; timing never enters the environment recipe/replay.
    if profile_path.is_some() {
        env.retain_solver_diagnostics(true);
        sim_solve::profile::enable();
        sim_solve::profile::reset();
    }
    let start = std::time::Instant::now();
    let mut error = None;
    let mut transition_wall_s = Vec::new();
    while env.transition().completed_steps < steps
        && !env.transition().terminated
        && !env.transition().truncated
    {
        let action = match &actions {
            Some(a) => a
                .get(transitions.len() - 1)
                .ok_or("action schedule exhausted before episode end")?,
            None => &initial_action,
        };
        let transition_start = std::time::Instant::now();
        match env.step(action) {
            Ok(t) => transitions.push(t),
            Err(e) => {
                error = Some(e);
                break;
            }
        }
        frames.push(env.frame()?);
        transition_wall_s.push(transition_start.elapsed().as_secs_f64());
    }
    let wall_s = start.elapsed().as_secs_f64();
    // A successfully replayed prefix is not a completed benchmark episode.
    let completed = error.is_none() && env.transition().completed_steps == episode_steps;
    let requested_steps_completed = error.is_none() && env.transition().completed_steps == steps;
    if let Some(path) = profile_path {
        let buckets = sim_solve::profile::all()
            .iter()
            .map(|b| json!({"name":b.name,"seconds":b.seconds(),"calls":b.calls()}))
            .collect::<Vec<_>>();
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&json!({
                "completed":completed,"wall_s":wall_s,"buckets":buckets,
                "accepted_implicit_steps":env.implicit_step_diagnostics(),
                "accepted_intervals":env.interval_diagnostics(),
                "motor_trial_statistics":env.motor_solve_statistics(),
                "scope":"Standalone native environment diagnostic after construction; excludes final capture serialization. Buckets can nest and are not additive. Accepted implicit-step diagnostics exclude rejected solves and the separate hybrid motor/event path. Motor-trial statistics cover successful outer intervals and include successful discarded event-location trials; failed continuous-trial internal work is excluded. Profiling overhead is included; use unprofiled runs for performance acceptance."
            }))?,
        )?;
    }
    println!(
        "{}",
        serde_json::to_string(&json!({"version":1,"kind":"sampled_environment_capture",
        "completed":completed,"error":error,"contract":env.contract(),"task":env.task(),"metadata":env.metadata(),
        "motion_parameters":motion.as_ref().map(|m|json!({"recipe":m.parameterization,"values":m.values,"source_actions":m.source_actions})),
        "requested_steps_completed":requested_steps_completed,"requested_capture_steps":steps,
        "recording":env.recording(),"transitions":transitions,"frames":frames,"wall_s":wall_s,
        "transition_wall_s":transition_wall_s,
        "scope":"Teacher-only endpoint task diagnostic; includes observation and capture overhead. No learning or hardware accuracy claim."}))?
    );
    if error.is_some() {
        std::process::exit(1);
    }
    Ok(())
}

//! Exercise a task through the same sampled environment exported to WASM.
use serde_json::json;
use sim_runtime::{
    embedded::Config,
    environment::{EmbeddedEnvironment, Task},
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
    if !(3..=4).contains(&args.len()) {
        return Err(
            "usage: run_environment scene.json config.json task.json [actions.json] [--profile report.json]".into(),
        );
    }
    let scene: Scene = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let config: Config = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let task: Task = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let steps = config.steps;
    let mut env = EmbeddedEnvironment::new(scene, config, task, 0)?;
    let actions: Option<Vec<Vec<f64>>> = args
        .get(3)
        .map(|p| -> Result<_, Box<dyn std::error::Error>> {
            Ok(serde_json::from_slice(&std::fs::read(p)?)?)
        })
        .transpose()?;
    let initial_action = env.inputs().iter().map(|c| c.initial).collect::<Vec<_>>();
    let mut frames = vec![env.frame()?];
    let mut transitions = vec![env.transition().clone()];
    // Standalone diagnostics; timing never enters the environment recipe/replay.
    if profile_path.is_some() {
        sim_solve::profile::enable();
        sim_solve::profile::reset();
    }
    let start = std::time::Instant::now();
    let mut error = None;
    let mut transition_wall_s = Vec::new();
    while !env.transition().terminated && !env.transition().truncated {
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
    let completed = error.is_none() && env.transition().completed_steps == steps;
    if let Some(path) = profile_path {
        let buckets = sim_solve::profile::all()
            .iter()
            .map(|b| json!({"name":b.name,"seconds":b.seconds(),"calls":b.calls()}))
            .collect::<Vec<_>>();
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&json!({
                "completed":completed,"wall_s":wall_s,"buckets":buckets,
                "scope":"Standalone native environment diagnostic after construction; excludes final capture serialization. Buckets can nest and are not additive. Profiling overhead is included; use unprofiled runs for performance acceptance."
            }))?,
        )?;
    }
    println!(
        "{}",
        serde_json::to_string(&json!({"version":1,"kind":"sampled_environment_capture",
        "completed":completed,"error":error,"contract":env.contract(),"task":env.task(),
        "recording":env.recording(),"transitions":transitions,"frames":frames,"wall_s":wall_s,
        "transition_wall_s":transition_wall_s,
        "scope":"Teacher-only endpoint task diagnostic; includes observation and capture overhead. No learning or hardware accuracy claim."}))?
    );
    if error.is_some() {
        std::process::exit(1);
    }
    Ok(())
}

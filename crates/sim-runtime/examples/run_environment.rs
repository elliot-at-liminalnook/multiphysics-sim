//! Exercise a task through the same sampled environment exported to WASM.
use serde_json::json;
use sim_runtime::{
    embedded::Config,
    environment::{EmbeddedEnvironment, Task},
    session::Scene,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if !(3..=4).contains(&args.len()) {
        return Err(
            "usage: run_environment scene.json config.json task.json [actions.json]".into(),
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
    let start = std::time::Instant::now();
    let mut error = None;
    while !env.transition().terminated && !env.transition().truncated {
        let action = match &actions {
            Some(a) => a
                .get(transitions.len() - 1)
                .ok_or("action schedule exhausted before episode end")?,
            None => &initial_action,
        };
        match env.step(action) {
            Ok(t) => transitions.push(t),
            Err(e) => {
                error = Some(e);
                break;
            }
        }
        frames.push(env.frame()?);
    }
    let wall_s = start.elapsed().as_secs_f64();
    let completed = error.is_none() && env.transition().completed_steps == steps;
    println!(
        "{}",
        serde_json::to_string(&json!({"version":1,"kind":"sampled_environment_capture",
        "completed":completed,"error":error,"contract":env.contract(),"task":env.task(),
        "recording":env.recording(),"transitions":transitions,"frames":frames,"wall_s":wall_s,
        "scope":"Teacher-only endpoint task diagnostic; includes observation and capture overhead. No learning or hardware accuracy claim."}))?
    );
    if error.is_some() {
        std::process::exit(1);
    }
    Ok(())
}

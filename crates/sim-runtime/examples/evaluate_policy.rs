//! Complete-episode evaluation using the shared runtime evaluator.
use sim_runtime::{
    embedded::Config, environment::Task, policy_evaluation::evaluate_episode, session::Scene,
};
use std::{fs, io::Write};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if !(5..=6).contains(&args.len()) {
        return Err("usage: evaluate_policy scene.json config.json task.json actions.json new-result.json [seed]".into());
    }
    let scene: Scene = serde_json::from_slice(&fs::read(&args[0])?)?;
    let config: Config = serde_json::from_slice(&fs::read(&args[1])?)?;
    let task: Task = serde_json::from_slice(&fs::read(&args[2])?)?;
    let actions: Vec<Vec<f64>> = serde_json::from_slice(&fs::read(&args[3])?)?;
    let seed = args.get(5).map(|s| s.parse()).transpose()?.unwrap_or(0);
    let started = std::time::Instant::now();
    let report = evaluate_episode(scene, config, task, &actions, seed);
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&args[4])?;
    serde_json::to_writer(&mut file, &report)?;
    file.write_all(b"\n")?;
    eprintln!(
        "evaluation {:.1}s; score {:?}; error {:?}",
        started.elapsed().as_secs_f64(),
        report.score,
        report.error
    );
    if report.score.is_none() {
        return Err("no complete episode score; inspect saved evaluation".into());
    }
    Ok(())
}

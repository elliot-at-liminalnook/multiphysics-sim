//! Compare optimizers on identical recorded on-policy data, then evaluate the
//! resulting actor through the same full-horizon Rust environment.
use serde::{Deserialize, Serialize};
use sim_domain_control::ppo::GaussianExploration;
use sim_runtime::{
    embedded::Config,
    environment::Task,
    policy_evaluation::{BaselineExpectation, evaluate_episode},
    ppo_training::*,
    session::Scene,
};
use std::{fs, io::Write, path::Path};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Experiment {
    version: u32,
    initial_state: String,
    rollouts: Vec<String>,
    evaluation_template: String,
    optimizer: PpoSettings,
    #[serde(default)]
    evaluate: Option<bool>,
}
#[derive(Deserialize)]
struct Template {
    scene: Scene,
    config: Config,
    task: Task,
    actions: Vec<Vec<f64>>,
    seed: u64,
    exploration: GaussianExploration,
    baseline_expectation: Option<BaselineExpectation>,
}
fn write(path: impl AsRef<Path>, value: &impl Serialize) -> Result<(), Box<dyn std::error::Error>> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    serde_json::to_writer(&mut f, value)?;
    f.write_all(b"\n")?;
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("usage: replay_ppo_update experiment.json fresh-output-directory".into());
    }
    let raw = fs::read(&args[0])?;
    let recipe: Experiment = serde_json::from_slice(&raw)?;
    if recipe.version != 1 || recipe.rollouts.is_empty() {
        return Err("invalid optimizer replay recipe".into());
    }
    let mut state: PpoState = serde_json::from_slice(&fs::read(&recipe.initial_state)?)?;
    let template: Template = serde_json::from_slice(&fs::read(&recipe.evaluation_template)?)?;
    let rollouts = recipe
        .rollouts
        .iter()
        .map(|p| Ok(serde_json::from_slice::<SpeedRollout>(&fs::read(p)?)?))
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    let mut expected = template.config.clone();
    let p = expected
        .policy
        .as_mut()
        .ok_or("missing evaluation policy")?;
    p.neural_residual = Some(state.actor.clone());
    p.neural_command_saturation = true;
    p.neural_exploration = Some(template.exploration.clone());
    let context =
        serde_json::json!({"scene":template.scene,"config":expected,"task":template.task});
    for r in &rollouts {
        if serde_json::json!({"scene":r.recording.runtime.scene,"config":r.recording.runtime.config,"task":r.recording.task})
            != context
        {
            return Err("rollout and evaluation physics/task/controller context differ".into());
        }
    }
    let root = Path::new(&args[1]);
    fs::create_dir(root)?;
    fs::write(root.join("experiment.json"), raw)?;
    let started = std::time::Instant::now();
    let report = update_speed_policy(
        &mut state,
        &rollouts,
        &template.exploration,
        &recipe.optimizer,
    )?;
    write(root.join("update.json"), &report)?;
    write(root.join("state.json"), &state)?;
    eprintln!(
        "optimizer replay finished in {:.1}s; exact KL {:?}",
        started.elapsed().as_secs_f64(),
        report.exact_policy_kl
    );
    if recipe.evaluate != Some(false) {
        let mut config = template.config;
        let p = config.policy.as_mut().unwrap();
        p.neural_residual = Some(state.actor.clone());
        p.neural_exploration = None;
        p.neural_command_saturation = true;
        let horizon = config.step_s * config.steps as f64;
        let evaluation = evaluate_episode(
            template.scene,
            config,
            template.task,
            &template.actions,
            template.seed,
        );
        write(root.join("validation.json"), &evaluation)?;
        write(
            root.join("result.json"),
            &serde_json::json!({"speed_m_s":evaluation.score.map(|s|s/horizon),"baseline_expectation":template.baseline_expectation,
            "scope":"Identical saved rollouts, changed optimizer settings, independent complete-horizon physical evaluation. Surrogate loss and KL are optimizer diagnostics, not speed scores or physical limits."}),
        )?;
        eprintln!(
            "complete-horizon speed {:?} m/s; error {:?}",
            evaluation.score.map(|s| s / horizon),
            evaluation.error
        );
    }
    Ok(())
}

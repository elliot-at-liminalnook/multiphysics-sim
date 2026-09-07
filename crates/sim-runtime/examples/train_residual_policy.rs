//! Episodic policy search through the shared Rust environment, with full recipes.
use serde::{Deserialize, Serialize};
use sim_domain_control::{neural::Network, policy_search::{SearchConfig, search}};
use sim_runtime::{embedded::Config, environment::Task, policy_evaluation::evaluate_episode, session::Scene};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Experiment {
    version:u32,
    scene:Scene,
    config:Config,
    task:Task,
    actions:Vec<Vec<f64>>,
    environment_seed:u64,
    search:SearchConfig,
}
fn main()->Result<(),Box<dyn std::error::Error>> {
    let args:Vec<_>=std::env::args().skip(1).collect();
    if args.len()!=2 {return Err("usage: train_residual_policy experiment.json output-directory".into());}
    let recipe_bytes=std::fs::read(&args[0])?;
    let recipe:Experiment=serde_json::from_slice(&recipe_bytes)?;
    if recipe.version!=1 {return Err("training experiment version must be 1".into());}
    let initial=recipe.config.policy.as_ref().and_then(|p|p.neural_residual.clone()).ok_or("training requires an explicit neural policy")?;
    std::fs::create_dir_all(&args[1])?;
    // Preserve the exact supplied recipe before any expensive work.
    std::fs::write(format!("{}/experiment.json",args[1]),recipe_bytes)?;
    let mut evaluation=0usize;
    let start=std::time::Instant::now();
    let result=search(initial,&recipe.search,|network:&Network| {
        let index=evaluation;evaluation+=1;
        let mut config=recipe.config.clone();config.policy.as_mut().unwrap().neural_residual=Some(network.clone());
        let report = evaluate_episode(recipe.scene.clone(), config, recipe.task.clone(), &recipe.actions, recipe.environment_seed);
        let run = report.score.ok_or_else(|| report.error.clone().unwrap_or_else(|| "episode has no completed score".into()));
        let record=serde_json::json!({"evaluation":index,"score":run.as_ref().ok(),"error":run.as_ref().err(),"policy":network,"episode":report});
        std::fs::write(format!("{}/evaluation-{index:04}.json",args[1]),serde_json::to_vec(&record).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        eprintln!("evaluation {index}: {:?}; elapsed {:.1}s",run,start.elapsed().as_secs_f64());
        run
    })?;
    let mut config=recipe.config.clone();config.policy.as_mut().unwrap().neural_residual=Some(result.policy.clone());
    std::fs::write(format!("{}/config.json",args[1]),serde_json::to_vec(&config)?)?;
    std::fs::write(format!("{}/policy.json",args[1]),serde_json::to_vec_pretty(&result.policy)?)?;
    std::fs::write(format!("{}/search.json",args[1]),serde_json::to_vec_pretty(&result)?)?;
    eprintln!("{} evaluations; reward {} -> {}; held-out validation still required",evaluation,result.initial_score,result.best_score);
    Ok(())
}

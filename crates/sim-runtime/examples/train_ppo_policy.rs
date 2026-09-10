//! Finite-horizon speed PPO, saving every rollout, update and independent check.
use serde::{Deserialize,Serialize};
use sim_runtime::{embedded::Config,environment::Task,session::Scene,ppo_training::*,policy_evaluation::{evaluate_episode,BaselineExpectation}};
use sim_domain_control::ppo::GaussianExploration;
use std::{fs,path::Path};

#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Experiment {
    version:u32,scene:Scene,config:Config,task:Task,actions:Vec<Vec<f64>>,
    iterations:usize,episodes_per_iteration:usize,seed:u64,
    exploration:GaussianExploration,optimizer:PpoSettings,
    #[serde(default,skip_serializing_if="Option::is_none")]
    baseline_expectation:Option<BaselineExpectation>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    initial_state:Option<PpoState>,
}
fn write(path:impl AsRef<Path>,value:&impl Serialize)->Result<(),Box<dyn std::error::Error>>{
    use std::io::Write;
    let mut f=fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    serde_json::to_writer(&mut f,value)?;f.write_all(b"\n")?;Ok(())
}
fn main()->Result<(),Box<dyn std::error::Error>>{
    let args=std::env::args().skip(1).collect::<Vec<_>>();
    if args.len()!=2{return Err("usage: train_ppo_policy experiment.json fresh-output-directory".into());}
    let raw=fs::read(&args[0])?;let recipe:Experiment=serde_json::from_slice(&raw)?;
    if recipe.version!=1||recipe.iterations==0||recipe.episodes_per_iteration==0||recipe.task.speed.is_none(){return Err("invalid speed PPO experiment".into());}
    recipe.optimizer.validate()?;
    if let Some(expected)=&recipe.baseline_expectation{expected.validate()?;}
    let actor=recipe.config.policy.as_ref().and_then(|p|p.neural_residual.clone()).ok_or("explicit initial actor required")?;
    recipe.exploration.validate(actor.outputs.len())?;
    let mut state=if let Some(state)=&recipe.initial_state {
        state.validate()?;
        if serde_json::to_value(&state.actor)?!=serde_json::to_value(&actor)?{return Err("resume actor must match the explicit initial policy".into());}
        state.clone()
    }else{PpoState::new(actor)?};
    let root=Path::new(&args[1]);fs::create_dir(root)?;
    fs::write(root.join("experiment.json"),raw)?;
    write(root.join("initial-state.json"),&state)?;
    let evaluate=|actor:&sim_domain_control::neural::Network|{
        let mut c=recipe.config.clone();let p=c.policy.as_mut().unwrap();
        p.neural_residual=Some(actor.clone());p.neural_command_saturation=true;p.neural_exploration=None;
        evaluate_episode(recipe.scene.clone(),c,recipe.task.clone(),&recipe.actions,recipe.seed)
    };
    eprintln!("validating deterministic baseline over {} s",recipe.config.step_s*recipe.config.steps as f64);
    let baseline=evaluate(&state.actor);write(root.join("baseline.json"),&baseline)?;
    if let Some(expected)=&recipe.baseline_expectation{expected.verify(&baseline)?;}
    let mut best=baseline.score.ok_or("baseline has no complete fall-free score; inspect baseline.json")?;
    let mut best_policy=state.actor.clone();let start=std::time::Instant::now();
    for iteration in 0..recipe.iterations {
        let mut rollouts=vec![];
        write(root.join(format!("iteration-{iteration:03}-start.json")),&state)?;
        for episode in 0..recipe.episodes_per_iteration {
            let seed=recipe.seed.wrapping_add((state.updates as u64).wrapping_mul(recipe.episodes_per_iteration as u64).wrapping_add(episode as u64).wrapping_add(1));
            eprintln!("iteration {iteration}, episode {episode}, seed {seed}: collecting full-horizon stochastic rollout");
            let rollout=collect_speed_episode(recipe.scene.clone(),recipe.config.clone(),recipe.task.clone(),&recipe.actions,&state.actor,&state.critic,&recipe.exploration,seed)?;
            write(root.join(format!("iteration-{iteration:03}-episode-{episode:03}.json")),&rollout)?;
            eprintln!("episode ended at {} s; fell={}; error={:?}; elapsed {:.1}s",rollout.final_transition.time_s,rollout.final_transition.terminated,rollout.error,start.elapsed().as_secs_f64());
            if rollout.error.is_some(){return Err("numerical rollout failure preserved; no policy update performed".into());}
            rollouts.push(rollout);
        }
        let update=update_speed_policy(&mut state,&rollouts,&recipe.exploration,&recipe.optimizer)?;
        write(root.join(format!("iteration-{iteration:03}-update.json")),&update)?;
        write(root.join(format!("iteration-{iteration:03}-state.json")),&state)?;
        let evaluation=evaluate(&state.actor);write(root.join(format!("iteration-{iteration:03}-validation.json")),&evaluation)?;
        let accepted=evaluation.score.is_some_and(|s|s>best);
        if accepted {best=evaluation.score.unwrap();best_policy=state.actor.clone();}
        eprintln!("iteration {iteration}: score {:?} m, accepted={accepted}, best {:.9} m/s",evaluation.score,best/(recipe.config.step_s*recipe.config.steps as f64));
    }
    write(root.join("final-state.json"),&state)?;write(root.join("best-policy.json"),&best_policy)?;
    write(root.join("result.json"),&serde_json::json!({"best_distance_m":best,"best_speed_m_s":best/(recipe.config.step_s*recipe.config.steps as f64),
        "scope":"Deterministic complete-horizon net speed with no sampled falls. Learned stochastic policy uses PPO-Lagrangian; only fall cost, no gait-quality reward. Detailed timestep/contact validation remains required for improvements."}))?;
    Ok(())
}

//! Reproducible policy search across declared development environments.
//! Paths are relative to the invocation directory; resolved inputs are archived.
use serde::{Deserialize, Serialize};
use sim_domain_control::{
    neural::Network,
    policy_search::{SearchConfig, search},
};
use sim_runtime::{
    embedded::Config,
    environment::Task,
    policy_evaluation::{evaluate_episode, worst_reward_rate},
    session::Scene,
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CaseFiles {
    name: String,
    config: String,
    task: String,
    actions: String,
    seed: u64,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    version: u32,
    scene: String,
    cases: Vec<CaseFiles>,
    search: SearchConfig,
}
#[derive(Serialize)]
struct Case {
    name: String,
    config: Config,
    task: Task,
    actions: Vec<Vec<f64>>,
    seed: u64,
}
fn read<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: train_policy_suite recipe.json NEW-output-directory".into());
    }
    let recipe: Recipe = read(&args[0])?;
    if recipe.version != 1 || recipe.cases.is_empty() {
        return Err("suite version must be 1 with at least one case".into());
    }
    let scene: Scene = read(&recipe.scene)?;
    let mut names = std::collections::BTreeSet::new();
    let mut cases = vec![];
    for case in &recipe.cases {
        if case.name.trim().is_empty() || !names.insert(case.name.clone()) {
            return Err("suite case names must be nonempty and unique".into());
        }
        cases.push(Case {
            name: case.name.clone(),
            config: read(&case.config)?,
            task: read(&case.task)?,
            actions: read(&case.actions)?,
            seed: case.seed,
        });
    }
    let initial = cases[0]
        .config
        .policy
        .as_ref()
        .and_then(|p| p.neural_residual.clone())
        .ok_or("suite requires an explicit neural policy")?;
    for case in &cases {
        let network = case
            .config
            .policy
            .as_ref()
            .and_then(|p| p.neural_residual.as_ref())
            .ok_or("every case requires a neural policy")?;
        if serde_json::to_value(network)? != serde_json::to_value(&initial)? {
            return Err("suite initial networks must match exactly".into());
        }
    }
    // Refuse to overwrite earlier experiment evidence.
    std::fs::create_dir(&args[1])?;
    let out = std::path::Path::new(&args[1]);
    std::fs::copy(&args[0], out.join("recipe.json"))?;
    std::fs::write(
        out.join("resolved.json"),
        serde_json::to_vec(
            &serde_json::json!({"scene":scene,"cases":cases,"search":recipe.search,"aggregation":"worst_reward_per_simulated_second"}),
        )?,
    )?;
    let mut evaluation = 0usize;
    let start = std::time::Instant::now();
    let result = search(initial, &recipe.search, |network: &Network| {
        let index = evaluation;
        evaluation += 1;
        let mut reports = vec![];
        for (case_index, case) in cases.iter().enumerate() {
            let mut config = case.config.clone();
            config.policy.as_mut().unwrap().neural_residual = Some(network.clone());
            let report = evaluate_episode(
                scene.clone(),
                config,
                case.task.clone(),
                &case.actions,
                case.seed,
            );
            std::fs::write(
                out.join(format!("evaluation-{index:04}-case-{case_index:02}.json")),
                serde_json::to_vec(&serde_json::json!({"name":case.name,"report":report}))
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            eprintln!(
                "evaluation {index} / {}: score {:?}, error {:?}; elapsed {:.1}s",
                case.name,
                report.score,
                report.error,
                start.elapsed().as_secs_f64()
            );
            reports.push(report);
        }
        let score = worst_reward_rate(&reports);
        std::fs::write(out.join(format!("evaluation-{index:04}.json")), serde_json::to_vec(&serde_json::json!({"evaluation":index,"score":score.as_ref().ok(),"error":score.as_ref().err(),"policy":network})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        score
    })?;
    std::fs::write(out.join("search.json"), serde_json::to_vec_pretty(&result)?)?;
    std::fs::write(
        out.join("policy.json"),
        serde_json::to_vec_pretty(&result.policy)?,
    )?;
    for (i, case) in cases.iter().enumerate() {
        let mut config = case.config.clone();
        config.policy.as_mut().unwrap().neural_residual = Some(result.policy.clone());
        std::fs::write(
            out.join(format!("case-{i:02}.config.json")),
            serde_json::to_vec(&config)?,
        )?;
    }
    eprintln!(
        "{evaluation} evaluations: {} -> {}; independent acceptance and browser delivery still required",
        result.initial_score, result.best_score
    );
    Ok(())
}

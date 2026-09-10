//! Stateful local/global acquisition, with all state supplied and returned as JSON.
use serde::{Deserialize, Serialize};
use sim_solve::{
    bayesian::{Config, Observation, Problem},
    local_global::{RegionConfig, RegionState, suggest},
};
use std::io::Write;
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Request {
    problem: Problem,
    observations: Vec<Observation>,
    config: Config,
    region: RegionConfig,
    state: Option<RegionState>,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: suggest_local_global request.json new-proposal.json".into());
    }
    let request: Request = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let proposal = suggest(
        &request.problem,
        &request.observations,
        &request.config,
        &request.region,
        request.state.as_ref(),
    )?;
    let report = serde_json::json!({"version":1,"request":request,"result":proposal,
        "scope":"Adaptive isotropic local acquisition with periodic global steps, using a global GP and constrained LogEI. Inspired by trust-region BO; not a TuRBO/TREGO reproduction or a convergence/physical-optimum guarantee."});
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[1])?
        .write_all(&serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}

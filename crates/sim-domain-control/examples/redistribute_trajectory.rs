//! Offline reference transformation through the shared trajectory library.
use serde::Deserialize;
use serde_json::json;
use sim_domain_control::trajectory::{RateRedistributionConfig, Trajectory, TrajectoryConfig};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    trajectory: TrajectoryConfig,
    rate_budgets: Vec<f64>,
    redistribution: RateRedistributionConfig,
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 1 {
        return Err("usage: redistribute_trajectory recipe.json".into());
    }
    let recipe: Recipe =
        serde_json::from_slice(&std::fs::read(&args[0]).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let source = Trajectory::new(recipe.trajectory)?;
    let result =
        source.redistribute_periodic_rates(&recipe.rate_budgets, &recipe.redistribution)?;
    let curve = Trajectory::new(result.trajectory.clone())?;
    println!("{}",serde_json::to_string(&json!({"result":result,
        "original_peak_rates":source.maximum_absolute_rates()?,"redistributed_peak_rates":curve.maximum_absolute_rates()?,
        "scope":"Explicit anchor-constrained rate-envelope redistribution followed by periodic B-spline smoothing. Output is C2 but changes the reference path and does not certify geometry, acceleration/torque feasibility, support timing or stability. Budgets and channel units are supplied by the caller; no physical defaults."})).map_err(|e|e.to_string())?);
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1)
    }
}

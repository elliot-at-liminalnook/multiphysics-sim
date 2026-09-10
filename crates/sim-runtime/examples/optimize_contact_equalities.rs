//! Shared equality-constrained solver on an explicit final contact model.
use serde::Deserialize;
use sim_runtime::{
    contact_implicit::{ContactImplicitConfig, ContactImplicitPlanner},
    session::{Scene, Session},
    tracking::CaptureConfig,
};
use sim_solve::{equality_dogleg::EqualityDoglegConfig, least_squares::VariableBound};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    config: ContactImplicitConfig,
    initial_positions: Vec<Vec<f64>>,
    bounds: Vec<Vec<VariableBound>>,
    search: EqualityDoglegConfig,
    provenance: serde_json::Value,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 3 {
        return Err("usage: optimize_contact_equalities scene markers recipe".into());
    }
    let scene: Scene = serde_json::from_slice(&std::fs::read(&a[0])?)?;
    let markers: CaptureConfig = serde_json::from_slice(&std::fs::read(&a[1])?)?;
    let recipe: Recipe = serde_json::from_slice(&std::fs::read(&a[2])?)?;
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let planner =
        ContactImplicitPlanner::new(&session.robot.art, &seed, &markers, recipe.config.clone())?;
    let mut count = 0;
    let result = planner.optimize_equalities(
        &recipe.initial_positions,
        &recipe.bounds,
        &recipe.search,
        |r| {
            count += 1;
            if count % 200 == 0 {
                eprintln!(
                    "evaluation {count}: force {}, moment {}, torque {:?}",
                    r.maximum_force_error_n, r.maximum_moment_error_nm, r.minimum_torque_margin_nm
                );
            }
        },
    )?;
    println!(
        "{}",
        serde_json::to_string(
            &serde_json::json!({"positions":result.positions,"result":result,"config":recipe.config,"provenance":recipe.provenance,
        "scope":"Dense equality-constrained Gauss-Newton with configured dogleg or exact-L1 globalization, explicit finite bounds and checked linear solves. Base balance is an equality; actuator, penetration, task and work residuals remain objective terms. Independent geometry, dense temporal auditing, runtime tracking and speed optimality are separate."})
        )?
    );
    Ok(())
}

//! Offline slip minimization with explicit sampled physical inequalities.
use serde::Deserialize;
use sim_runtime::{
    contact_implicit::{ContactImplicitConfig, ContactImplicitPlanner, ContactSlipObjective},
    session::{Scene, Session},
    tracking::CaptureConfig,
};
use sim_solve::{
    inequality_augmented_lagrangian::{AugmentedLagrangianConfig, AugmentedWarmStart},
    least_squares::VariableBound,
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    config: ContactImplicitConfig,
    initial_positions: Vec<Vec<f64>>,
    bounds: Vec<Vec<VariableBound>>,
    search: AugmentedLagrangianConfig,
    slip_objective: ContactSlipObjective,
    #[serde(default)]
    warm_start: Option<AugmentedWarmStart>,
    provenance: serde_json::Value,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 3 {
        return Err("usage: optimize_contact_inequalities scene markers recipe".into());
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
    let result = planner.optimize_slip_with_inequalities_warm_started(
        &recipe.initial_positions,
        &recipe.bounds,
        &recipe.search,
        &recipe.slip_objective,
        recipe.warm_start.as_ref(),
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
    let slip = planner.slip_report(&result.report, &recipe.slip_objective)?;
    println!(
        "{}",
        serde_json::to_string(
            &serde_json::json!({"positions":result.positions,"result":result,"slip":slip,"slip_objective":recipe.slip_objective,"config":recipe.config,"provenance":recipe.provenance,
        "scope":"Finite local augmented-Lagrangian slip optimization with signed sampled balance, penetration and actuator inequalities. Independent physical gates, dense CAD collision, runtime tracking, slip and speed validation remain required. Solver stationarity is not global optimality or a physical maximum."})
        )?
    );
    Ok(())
}

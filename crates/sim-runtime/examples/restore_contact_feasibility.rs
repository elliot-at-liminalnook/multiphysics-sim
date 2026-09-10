//! Restore sampled contact feasibility while preserving explicitly fixed bounds.
use serde::Deserialize;
use sim_runtime::{
    contact_implicit::{ContactImplicitConfig, ContactImplicitPlanner, ContactSlipObjective},
    session::{Scene, Session},
    tracking::CaptureConfig,
};
use sim_solve::least_squares::{DerivativeRefinement, LeastSquaresConfig, VariableBound};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    config: ContactImplicitConfig,
    initial_positions: Vec<Vec<f64>>,
    bounds: Vec<Vec<VariableBound>>,
    search: LeastSquaresConfig,
    scaling_exponent: f64,
    derivative_refinement: Option<DerivativeRefinement>,
    #[serde(default)]
    slip_objective: Option<ContactSlipObjective>,
    provenance: serde_json::Value,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 3 {
        return Err("usage: restore_contact_feasibility scene markers recipe".into());
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
    let result = planner.restore_feasibility_with_slip(
        &recipe.initial_positions,
        &recipe.bounds,
        &recipe.search,
        recipe.scaling_exponent,
        recipe.derivative_refinement.as_ref(),
        recipe.slip_objective.as_ref(),
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
    let slip = recipe
        .slip_objective
        .as_ref()
        .map(|o| planner.slip_report(&result.report, o))
        .transpose()?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "positions":result.positions,"result":result,"slip":slip,"slip_objective":recipe.slip_objective,"config":recipe.config,"provenance":recipe.provenance,
            "scope":"Sampled balance, actuator violation and penetration restoration through shared damped least squares. Task/effort/sliding-work costs excluded; an explicit optional sampled-slip shaping objective may be added. Explicit bounds preserve requested displacement. Independent dense geometry/contact, slip and runtime checks remain required; stationarity is not physical feasibility or speed optimality."
        }))?
    );
    Ok(())
}

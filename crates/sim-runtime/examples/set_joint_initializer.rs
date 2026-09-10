//! Apply bounded configuration edits through the shared optimizer decoder.
use serde::{Deserialize, Serialize};
use sim_runtime::contact_planning::{ContactPlanRecipe, JointContactMotion, JointContactVariable};
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    robot: ContactPlanRecipe,
    candidate: JointContactMotion,
    variables: Vec<JointContactVariable>,
    search: serde_json::Value,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("usage: set_joint_initializer recipe.json overrides.json".into());
    }
    let mut recipe: Recipe = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let overrides: Vec<(usize, f64)> = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    recipe.candidate = recipe.candidate.with_bounded_overrides(
        &recipe.variables,
        recipe.robot.direction_world,
        &overrides,
    )?;
    println!("{}", serde_json::to_string(&recipe)?);
    Ok(())
}

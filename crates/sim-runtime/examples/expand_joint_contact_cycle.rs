//! Expand an existing bounded joint search without changing physical properties.
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
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: expand_joint_contact_cycle recipe.json repetitions".into());
    }
    let mut recipe: Recipe = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let count: usize = args[1].parse()?;
    let timed = recipe.candidate.force_timing.is_some();
    let (candidate, variables) = recipe.candidate.repeated_cycle_with_variables(
        &recipe.variables,
        count,
        recipe.robot.direction_world,
    )?;
    recipe.candidate = if timed && count != 1 {
        candidate.with_contact_timed_forces()?
    } else {
        candidate
    };
    recipe.variables = variables;
    recipe.robot.uniform_samples = recipe
        .robot
        .uniform_samples
        .checked_mul(count)
        .ok_or("sample count overflow")?;
    recipe.robot.additional_phases = (0..count)
        .flat_map(|repeat| {
            recipe
                .robot
                .additional_phases
                .iter()
                .map(move |p| (*p + repeat as f64) / count as f64)
        })
        .collect();
    println!("{}", serde_json::to_string(&recipe)?);
    Ok(())
}

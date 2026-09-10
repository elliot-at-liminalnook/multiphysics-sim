//! Evaluate general periodic contact sequences through shared CAD dynamics.
//! Point-force allocation is a planning screen, not an executed walking result.
use serde::{Deserialize, Serialize};
use sim_domain_control::contact_phase::ContactPhaseConfig;
use sim_runtime::{
    contact_planning::{ContactPlanRecipe, ContactPlanner},
    session::{Scene, Session},
    tracking::CaptureConfig,
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    robot: ContactPlanRecipe,
    motion: ContactPhaseConfig,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() == 3 && args[0] == "--repeat" {
        let mut recipe: Recipe = serde_json::from_slice(&std::fs::read(&args[1])?)?;
        recipe.motion = recipe.motion.repeated_cycle(args[2].parse()?)?;
        println!("{}", serde_json::to_string(&recipe)?);
        return Ok(());
    }
    if args.len() != 3 {
        return Err("usage: evaluate_contact_sequence scene.json markers.json recipe.json".into());
    }
    let scene: Scene = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let markers: CaptureConfig = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let recipe: Recipe = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let planner = ContactPlanner::new(&session.robot.art, &seed, &markers, recipe.robot)?;
    let report = planner.evaluate(&recipe.motion)?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

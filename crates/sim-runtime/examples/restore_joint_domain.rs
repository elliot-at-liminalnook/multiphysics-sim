//! Restore the CAD evaluation domain on a dense mesh through the shared solver.
use serde::{Deserialize, Serialize};
use sim_runtime::{
    contact_planning::{
        ContactPlanRecipe, ContactPlanner, JointContactMotion, JointContactVariable,
    },
    session::{Scene, Session},
    tracking::CaptureConfig,
};
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    robot: ContactPlanRecipe,
    candidate: JointContactMotion,
    variables: Vec<JointContactVariable>,
    search: serde_json::Value,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Config {
    audit_uniform_samples: usize,
    bisection_iterations: usize,
}
fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 5 {
        return Err("usage: restore_joint_domain scene.json markers.json reference.recipe.json target.recipe.json config.json".into());
    }
    let read = |p: &str| std::fs::read(p).map_err(|e| e.to_string());
    let scene: Scene = serde_json::from_slice(&read(&args[0])?).map_err(|e| e.to_string())?;
    let markers: CaptureConfig =
        serde_json::from_slice(&read(&args[1])?).map_err(|e| e.to_string())?;
    let reference: Recipe = serde_json::from_slice(&read(&args[2])?).map_err(|e| e.to_string())?;
    let target: Recipe = serde_json::from_slice(&read(&args[3])?).map_err(|e| e.to_string())?;
    let config: Config = serde_json::from_slice(&read(&args[4])?).map_err(|e| e.to_string())?;
    let mut reference_robot = reference.robot.clone();
    reference_robot.uniform_samples = target.robot.uniform_samples;
    reference_robot.additional_phases = target.robot.additional_phases.clone();
    if serde_json::to_value(reference_robot).map_err(|e| e.to_string())?
        != serde_json::to_value(&target.robot).map_err(|e| e.to_string())?
    {
        return Err(
            "reference and target must use identical robot, world and physical gates".into(),
        );
    }
    if config.audit_uniform_samples <= target.robot.uniform_samples {
        return Err("restoration audit must be denser than target optimization mesh".into());
    }
    let mut audit_robot = target.robot.clone();
    audit_robot.additional_phases.extend(
        (0..target.robot.uniform_samples)
            .map(|i| (i as f64 + 0.5) / target.robot.uniform_samples as f64),
    );
    audit_robot.additional_phases.sort_by(f64::total_cmp);
    audit_robot.additional_phases.dedup();
    audit_robot.uniform_samples = config.audit_uniform_samples;
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let planner = ContactPlanner::new(&session.robot.art, &seed, &markers, audit_robot.clone())?;
    eprintln!(
        "restoring dense CAD domain: {} uniform samples, {} bisections",
        config.audit_uniform_samples, config.bisection_iterations
    );
    let result = planner.restore_joint_evaluation_domain(
        &reference.candidate,
        &target.candidate,
        &target.variables,
        config.bisection_iterations,
    )?;
    let mut restored_recipe = target.clone();
    restored_recipe.candidate = result.restoration.evaluation.0.clone();
    let mut audit_recipe = restored_recipe.clone();
    audit_recipe.robot = audit_robot;
    eprintln!(
        "restored fraction {}, {} attempts, sampled physical feasibility {}",
        result.restoration.fraction,
        result.restoration.attempts.len(),
        result.restoration.evaluation.1.sampled_feasible
    );
    println!(
        "{}",
        serde_json::json!({"config":config,"restoration":result,"restored_recipe":restored_recipe,"audit_recipe":audit_recipe,"scope":"Dense CAD-domain restoration only, using original target fixed fields and bounded decisions. Full physical report retained; successful restoration is not force feasibility, continuous collision clearance or a runtime speed result."})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

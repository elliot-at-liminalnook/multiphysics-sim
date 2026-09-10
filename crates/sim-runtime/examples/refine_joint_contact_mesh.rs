//! Reuse shared adaptive phase selection on an independently audited joint motion.
use serde::{Deserialize, Serialize};
use sim_runtime::{
    contact_planning::{
        ContactPlanRecipe, ContactPlanner, JointContactMotion, JointContactVariable,
        select_contact_refinement_phases,
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
    maximum_added_phases: usize,
    minimum_phase_separation: f64,
}
fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 4 {
        return Err(
            "usage: refine_joint_contact_mesh scene.json markers.json recipe.json refinement.json"
                .into(),
        );
    }
    let read = |p: &str| std::fs::read(p).map_err(|e| e.to_string());
    let scene: Scene = serde_json::from_slice(&read(&args[0])?).map_err(|e| e.to_string())?;
    let markers: CaptureConfig =
        serde_json::from_slice(&read(&args[1])?).map_err(|e| e.to_string())?;
    let recipe: Recipe = serde_json::from_slice(&read(&args[2])?).map_err(|e| e.to_string())?;
    let config: Config = serde_json::from_slice(&read(&args[3])?).map_err(|e| e.to_string())?;
    if config.audit_uniform_samples <= recipe.robot.uniform_samples {
        return Err("refinement audit must be denser than optimization mesh".into());
    }
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let planner = ContactPlanner::new(&session.robot.art, &seed, &markers, recipe.robot.clone())?;
    let coarse = planner.evaluate_joint_uncached(&recipe.candidate)?;
    let mut audit_recipe = recipe.robot.clone();
    audit_recipe.additional_phases.extend(
        (0..recipe.robot.uniform_samples)
            .map(|i| (i as f64 + 0.5) / recipe.robot.uniform_samples as f64),
    );
    audit_recipe.additional_phases.sort_by(f64::total_cmp);
    audit_recipe.additional_phases.dedup();
    audit_recipe.uniform_samples = config.audit_uniform_samples;
    let audit_planner =
        ContactPlanner::new(&session.robot.art, &seed, &markers, audit_recipe.clone())?;
    let audit = audit_planner.evaluate_joint_uncached(&recipe.candidate)?;
    let period = recipe.candidate.motion.period_s;
    let occupied = coarse
        .motion_report
        .frames
        .iter()
        .map(|f| (f.time_s / period).rem_euclid(1.))
        .collect::<Vec<_>>();
    let added = select_contact_refinement_phases(
        &audit.motion_report,
        period,
        [
            recipe.robot.force_tolerance_n,
            recipe.robot.moment_tolerance_nm,
            recipe.robot.torque_tolerance_nm,
            recipe.robot.penetration_tolerance_m,
            0.,
        ],
        &occupied,
        config.maximum_added_phases,
        config.minimum_phase_separation,
    )?;
    let mut refined = recipe.clone();
    refined.robot.additional_phases.extend_from_slice(&added);
    refined.robot.additional_phases.sort_by(f64::total_cmp);
    refined.robot.additional_phases.dedup();
    let refined_planner =
        ContactPlanner::new(&session.robot.art, &seed, &markers, refined.robot.clone())?;
    let refined_report = refined_planner.evaluate_joint_uncached(&refined.candidate)?;
    println!(
        "{}",
        serde_json::json!({"config":config,"coarse_report":coarse,"audit_recipe":audit_recipe,"audit_report":audit,"added_phases":added,"refined_recipe":refined,"refined_report":refined_report,"scope":"Adaptive joint collocation using the shared physical-violation ranking. Motion, forces, bounds and physical tolerances unchanged; only missed audit phases enter the next optimization mesh. Strict inter-link overlap threshold is zero. This is not a feasible gait, runtime validation or global speed limit."})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

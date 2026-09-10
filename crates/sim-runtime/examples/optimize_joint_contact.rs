use serde::Deserialize;
use sim_runtime::{
    contact_planning::{
        ContactPlanRecipe, ContactPlanner, JointContactMotion, JointContactVariable,
    },
    session::{Scene, Session},
    tracking::CaptureConfig,
};
use sim_solve::inequality_augmented_lagrangian::AugmentedLagrangianConfig;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    robot: ContactPlanRecipe,
    candidate: JointContactMotion,
    variables: Vec<JointContactVariable>,
    search: AugmentedLagrangianConfig,
}
fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3
        && !(args.len() == 4
            && matches!(
                args[3].as_str(),
                "--evaluate" | "--seed-forces" | "--analytic-forces"
            ))
    {
        return Err(
            "usage: optimize_joint_contact scene.json markers.json recipe.json [--evaluate|--seed-forces|--analytic-forces]".into(),
        );
    }
    let read = |p: &str| std::fs::read(p).map_err(|e| e.to_string());
    let scene: Scene = serde_json::from_slice(&read(&args[0])?).map_err(|e| e.to_string())?;
    let markers: CaptureConfig =
        serde_json::from_slice(&read(&args[1])?).map_err(|e| e.to_string())?;
    let recipe: Recipe = serde_json::from_slice(&read(&args[2])?).map_err(|e| e.to_string())?;
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let planner = ContactPlanner::new(&session.robot.art, &seed, &markers, recipe.robot)?;
    if args.len() == 4 && args[3] == "--seed-forces" {
        println!(
            "{}",
            serde_json::to_string(&planner.seed_joint_forces(&recipe.candidate)?)
                .map_err(|e| e.to_string())?
        );
    } else if args.len() == 4 && args[3] == "--evaluate" {
        println!(
            "{}",
            serde_json::to_string(&planner.evaluate_joint(&recipe.candidate)?)
                .map_err(|e| e.to_string())?
        );
    } else {
        let mut count = 0;
        let progress = |r: &sim_runtime::contact_planning::JointContactReport| {
            count += 1;
            if count == 1 || count % 10 == 0 {
                eprintln!(
                    "evaluation {count}: speed {:.6}, force {:.4} N, moment {:.4} Nm, torque {:.4} Nm, cone {:.4} N, maximum constraint {:.6}, feasible {}",
                    r.motion_report.speed_m_s,
                    r.motion_report.maximum_force_error_n,
                    r.motion_report.maximum_moment_error_nm,
                    r.motion_report.minimum_torque_margin_nm,
                    r.maximum_cone_violation_n,
                    r.constraints
                        .inequalities
                        .iter()
                        .copied()
                        .fold(0.0_f64, f64::max),
                    r.sampled_feasible
                );
            }
        };
        let result = if args.len() == 4 {
            planner.optimize_joint_with_force_jacobian(
                &recipe.candidate,
                &recipe.variables,
                &recipe.search,
                progress,
            )?
        } else {
            planner.optimize_joint(
                &recipe.candidate,
                &recipe.variables,
                &recipe.search,
                progress,
            )?
        };
        println!(
            "{}",
            serde_json::to_string(&result).map_err(|e| e.to_string())?
        );
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

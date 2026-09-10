use serde::Deserialize;
use sim_domain_control::contact_phase::ContactPhaseConfig;
use sim_runtime::{
    contact_planning::{
        AdaptiveContactConfig, ContactPlanRecipe, ContactPlanReport, ContactPlanner,
        ContactVariable,
    },
    session::{Scene, Session},
    tracking::CaptureConfig,
};
use sim_solve::least_squares::LeastSquaresConfig;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    robot: ContactPlanRecipe,
    motion: ContactPhaseConfig,
    variables: Vec<ContactVariable>,
    search: LeastSquaresConfig,
    #[serde(default)]
    adaptive: Option<AdaptiveContactConfig>,
}
fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("usage: optimize_contact_motion scene.json markers.json recipe.json".into());
    }
    let read = |p: &str| std::fs::read(p).map_err(|e| e.to_string());
    let scene: Scene = serde_json::from_slice(&read(&args[0])?).map_err(|e| e.to_string())?;
    let markers: CaptureConfig =
        serde_json::from_slice(&read(&args[1])?).map_err(|e| e.to_string())?;
    let recipe: Recipe = serde_json::from_slice(&read(&args[2])?).map_err(|e| e.to_string())?;
    let mut session = Session::new(scene, 0)?;
    // Analysis-only model: explicit allocated point loads replace contact forces.
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let mut planner = ContactPlanner::new(&session.robot.art, &seed, &markers, recipe.robot)?;
    let mut count = 0;
    let progress = |r: &ContactPlanReport| {
        count += 1;
        if count % 10 == 0 {
            eprintln!(
                "evaluation {count}: speed {:.6}, force {:.4} N, moment {:.4} Nm, torque margin {:.4} Nm, penetration {:.6} m",
                r.speed_m_s,
                r.maximum_force_error_n,
                r.maximum_moment_error_nm,
                r.minimum_torque_margin_nm,
                r.maximum_penetration_m
            );
        }
    };
    let output = if let Some(adaptive) = &recipe.adaptive {
        serde_json::to_string(&planner.optimize_adaptive(
            &recipe.motion,
            &recipe.variables,
            &recipe.search,
            adaptive,
            progress,
        )?)
    } else {
        serde_json::to_string(&planner.optimize(
            &recipe.motion,
            &recipe.variables,
            &recipe.search,
            progress,
        )?)
    }
    .map_err(|e| e.to_string())?;
    println!("{output}");
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

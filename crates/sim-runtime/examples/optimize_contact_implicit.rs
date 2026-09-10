use serde::Deserialize;
use serde_json::json;
use sim_runtime::{
    contact_implicit::{ContactImplicitConfig, ContactImplicitPlanner},
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
    smoothing_schedule_m: Vec<f64>,
    #[serde(default)]
    stiffness_schedule_n_m: Vec<f64>,
    #[serde(default)]
    hessian_scaling_exponent: f64,
    #[serde(default)]
    derivative_refinement: Option<DerivativeRefinement>,
    provenance: serde_json::Value,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 3 {
        return Err("usage: optimize_contact_implicit scene.json markers.json recipe.json".into());
    }
    let scene: Scene = serde_json::from_slice(&std::fs::read(&a[0])?)?;
    let markers: CaptureConfig = serde_json::from_slice(&std::fs::read(&a[1])?)?;
    let recipe: Recipe = serde_json::from_slice(&std::fs::read(&a[2])?)?;
    if recipe.smoothing_schedule_m.is_empty()
        || recipe.smoothing_schedule_m.len() > 20
        || recipe
            .smoothing_schedule_m
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.)
        || recipe.smoothing_schedule_m.last() != Some(&recipe.config.contact.smoothing_m)
    {
        return Err(
            "explicit positive smoothing schedule must finish at declared planning model".into(),
        );
    }
    let stiffness_schedule = if recipe.stiffness_schedule_n_m.is_empty() {
        vec![recipe.config.contact.stiffness_n_m; recipe.smoothing_schedule_m.len()]
    } else {
        if recipe.stiffness_schedule_n_m.len() != recipe.smoothing_schedule_m.len()
            || recipe
                .stiffness_schedule_n_m
                .iter()
                .any(|v| !v.is_finite() || *v <= 0.)
            || recipe.stiffness_schedule_n_m.last() != Some(&recipe.config.contact.stiffness_n_m)
        {
            return Err("explicit stiffness continuation must match smoothing stages and finish at declared stiffness".into());
        }
        recipe.stiffness_schedule_n_m.clone()
    };
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false; // Explicit analysis instance: shared smooth point forces replace runtime contact.
    let seed = session.robot.generalized();
    let mut guess = recipe.initial_positions;
    let mut stages = vec![];
    for (&smoothing, stiffness) in recipe.smoothing_schedule_m.iter().zip(stiffness_schedule) {
        let mut config = recipe.config.clone();
        config.contact.smoothing_m = smoothing;
        config.contact.stiffness_n_m = stiffness;
        let planner =
            ContactImplicitPlanner::new(&session.robot.art, &seed, &markers, config.clone())?;
        let mut count = 0;
        let result=planner.optimize_scaled_refining(&guess,&recipe.bounds,&recipe.search,recipe.hessian_scaling_exponent,recipe.derivative_refinement.as_ref(),|r|{count+=1;if count%20==0 {eprintln!("smoothing {smoothing} evaluation {count}: force {:.4}, moment {:.4}, torque {:?}",r.maximum_force_error_n,r.maximum_moment_error_nm,r.minimum_torque_margin_nm);}})?;
        eprintln!(
            "stage {smoothing}: {:?}, planning tolerances {}",
            result.search.termination, result.report.within_planning_tolerances
        );
        guess = result.positions.clone();
        stages.push(json!({"config":config,"result":result}));
    }
    println!(
        "{}",
        serde_json::to_string(
            &json!({"stages":stages,"positions":guess,"provenance":recipe.provenance,"hessian_scaling_exponent":recipe.hessian_scaling_exponent,"derivative_refinement":recipe.derivative_refinement,
        "scope":"Offline finite-horizon position-only contact-implicit search with explicit smoothing continuation. No contact schedule is supplied. Weighted least squares is not IDTO's equality-constrained dogleg solver. Detailed runtime, geometry, terminal/periodic behavior and global speed optimality remain separate."})
        )?
    );
    Ok(())
}

//! Enlarge a joint planner's body basis through shared exact knot insertion.
use serde::{Deserialize, Serialize};
use sim_domain_control::trajectory::Trajectory;
use sim_runtime::{
    contact_planning::{
        ContactDecision, ContactPlanRecipe, ContactPlanner, JointContactDecision,
        JointContactMotion, JointContactVariable,
    },
    session::{Scene, Session},
    tracking::CaptureConfig,
};
use sim_solve::{
    inequality_augmented_lagrangian::AugmentedLagrangianConfig, least_squares::VariableBound,
};
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    robot: ContactPlanRecipe,
    candidate: JointContactMotion,
    variables: Vec<JointContactVariable>,
    search: AugmentedLagrangianConfig,
}
fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("usage: refine_joint_contact_body scene.json markers.json recipe.json".into());
    }
    let read = |p: &str| std::fs::read(p).map_err(|e| e.to_string());
    let scene: Scene = serde_json::from_slice(&read(&args[0])?).map_err(|e| e.to_string())?;
    let markers: CaptureConfig =
        serde_json::from_slice(&read(&args[1])?).map_err(|e| e.to_string())?;
    let original: Recipe = serde_json::from_slice(&read(&args[2])?).map_err(|e| e.to_string())?;
    let body = Trajectory::new(original.candidate.motion.body.clone())?;
    let refined = body.refined_periodic_config()?;
    let old_count = original.candidate.motion.body.keyframes.len() - 1;
    let channels = original.candidate.motion.body.keyframes[0].values.len();
    let mut seen = std::collections::BTreeSet::new();
    let mut bounds: Vec<Option<VariableBound>> = vec![None; channels];
    for variable in &original.variables {
        if let JointContactDecision::Motion {
            decision: ContactDecision::BodyControl { control, channel },
        } = variable.decision
        {
            if control >= old_count || channel >= channels || !seen.insert((control, channel)) {
                return Err("invalid or duplicate body control decision".into());
            }
            if let Some(previous) = &bounds[channel] {
                if previous.lower != variable.bound.lower || previous.upper != variable.bound.upper
                {
                    return Err("body refinement requires uniform per-channel control bounds; no bounds are silently widened".into());
                }
            } else {
                bounds[channel] = Some(variable.bound.clone());
            }
        }
    }
    if seen.len() != old_count * channels {
        return Err("every unique body control/channel must already be a decision".into());
    }
    let mut recipe = original.clone();
    recipe.candidate.motion.body = refined.clone();
    recipe.variables.clear();
    let mut inserted = false;
    for variable in &original.variables {
        if matches!(
            variable.decision,
            JointContactDecision::Motion {
                decision: ContactDecision::BodyControl { .. }
            }
        ) {
            if !inserted {
                for control in 0..refined.keyframes.len() - 1 {
                    for (channel, bound) in bounds.iter().enumerate() {
                        let bound = bound.as_ref().unwrap();
                        let value = refined.keyframes[control].values[channel];
                        if !value.is_finite() || value < bound.lower || value > bound.upper {
                            return Err("refined control outside original bounds".into());
                        }
                        recipe.variables.push(JointContactVariable {
                            decision: JointContactDecision::Motion {
                                decision: ContactDecision::BodyControl { control, channel },
                            },
                            bound: bound.clone(),
                        });
                    }
                }
                inserted = true;
            }
        } else {
            recipe.variables.push(variable.clone());
        }
    }
    let refined_body = Trajectory::new(refined)?;
    let mut value_error = 0.0_f64;
    let mut rate_error = 0.0_f64;
    let mut acceleration_error = 0.0_f64;
    let samples = 4097;
    for i in 0..samples {
        let t = original.candidate.motion.period_s * i as f64 / (samples - 1) as f64;
        let a = body.sample(t)?;
        let b = refined_body.sample(t)?;
        for (x, y) in a.values.iter().zip(&b.values) {
            value_error = value_error.max((x - y).abs());
        }
        for (x, y) in a.rates.iter().zip(&b.rates) {
            rate_error = rate_error.max((x - y).abs());
        }
        for (x, y) in a.accelerations.iter().zip(&b.accelerations) {
            acceleration_error = acceleration_error.max((x - y).abs());
        }
    }
    if value_error > 1e-12 || rate_error > 1e-10 || acceleration_error > 1e-8 {
        return Err("exact spline refinement lost numerical trajectory fidelity".into());
    }
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let planner = ContactPlanner::new(&session.robot.art, &seed, &markers, recipe.robot.clone())?;
    let original_report = planner.evaluate_joint_uncached(&original.candidate)?;
    let refined_report = planner.evaluate_joint_uncached(&recipe.candidate)?;
    let same_mesh = original_report.motion_report.frames.len()
        == refined_report.motion_report.frames.len()
        && original_report
            .motion_report
            .frames
            .iter()
            .zip(&refined_report.motion_report.frames)
            .all(|(a, b)| {
                a.clock.phase_rate == b.clock.phase_rate
                    && a.clock.phase_acceleration_per_s == b.clock.phase_acceleration_per_s
                    && (a.time_s - b.time_s).abs() < 1e-12
            });
    let inequality_error = if same_mesh
        && original_report.constraints.inequalities.len()
            == refined_report.constraints.inequalities.len()
    {
        Some(
            original_report
                .constraints
                .inequalities
                .iter()
                .zip(&refined_report.constraints.inequalities)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0_f64, f64::max),
        )
    } else {
        None
    };
    if inequality_error.is_some_and(|e| e > 1e-6) {
        return Err("same-mesh CAD inequality regression after body refinement".into());
    }
    println!(
        "{}",
        serde_json::json!({"recipe":recipe,"original_report":original_report,"refined_report":refined_report,"old_body_controls":old_count,"new_body_controls":2*old_count,"original_variables":original.variables.len(),"refined_variables":original.variables.len()+old_count*channels,"trajectory_samples":samples,"maximum_value_error_m_or_rad":value_error,"maximum_rate_error_per_s":rate_error,"maximum_acceleration_error_per_s2":acceleration_error,"same_evaluation_mesh":same_mesh,"maximum_normalized_inequality_difference":inequality_error,"control_bounds_preserved":true,"scope":"Shared exact periodic cubic knot insertion enlarges the body-motion basis. Original motion curve, phase/timing/force decisions, speed objective, actuator model and physical tolerances are preserved. Additional body knots may enlarge the audit mesh; if meshes match, all normalized physical inequalities are compared. This prepares a richer joint search, not a feasible gait or a physical speed limit."})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

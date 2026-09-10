//! Independently check the affine map used by hard conic servo-command limits.
use nalgebra::DVector;
use serde::Deserialize;
use sim_runtime::{
    contact_planning::{
        ContactPlanRecipe, ContactPlanner, JointContactDecision, JointContactMotion,
        JointContactReport, JointContactVariable,
    },
    session::{Scene, Session},
    tracking::CaptureConfig,
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    robot: ContactPlanRecipe,
    candidate: JointContactMotion,
    variables: Vec<JointContactVariable>,
    search: serde_json::Value,
}
fn commands(report: &JointContactReport) -> Result<DVector<f64>, String> {
    let mut values = Vec::new();
    for f in &report.motion_report.frames {
        values.extend_from_slice(
            &f.servo_command
                .as_ref()
                .ok_or("missing command audit")?
                .inequalities,
        );
    }
    Ok(DVector::from_vec(values))
}
fn set(
    candidate: &mut JointContactMotion,
    variable: &JointContactVariable,
    value: f64,
) -> Result<(), String> {
    candidate.set_force_node(&variable.decision, value)
}
fn run() -> Result<(), String> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 3 {
        return Err(
            "usage: audit_joint_servo_command_map scene.json markers.json recipe.json".into(),
        );
    }
    let read = |p: &str| std::fs::read(p).map_err(|e| e.to_string());
    let scene: Scene = serde_json::from_slice(&read(&a[0])?).map_err(|e| e.to_string())?;
    let markers: CaptureConfig =
        serde_json::from_slice(&read(&a[1])?).map_err(|e| e.to_string())?;
    let recipe: Recipe = serde_json::from_slice(&read(&a[2])?).map_err(|e| e.to_string())?;
    let _ = &recipe.search;
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let planner = ContactPlanner::new(&session.robot.art, &seed, &markers, recipe.robot)?;
    let report = planner.evaluate_joint_uncached(&recipe.candidate)?;
    let full = recipe
        .variables
        .iter()
        .filter(|v| matches!(v.decision, JointContactDecision::Force { .. } | JointContactDecision::AdditionalForce { .. }))
        .cloned()
        .collect::<Vec<_>>();
    if full.len() < 2 {
        return Err("at least two selected force values required".into());
    }
    let selected = full.iter().step_by(2).cloned().rev().collect::<Vec<_>>();
    let mut cases = Vec::new();
    for (name, variables) in [("all", full.clone()), ("reordered_partial", selected)] {
        let mut zero = recipe.candidate.clone();
        for v in &variables {
            set(&mut zero, v, 0.)?;
        }
        let (baseline, matrix) = planner.joint_servo_command_jacobian(&zero, &variables)?;
        let offset = commands(&baseline)?;
        let independent = planner.evaluate_joint_uncached(&zero)?;
        if serde_json::to_vec(&baseline).map_err(|e| e.to_string())?
            != serde_json::to_vec(&independent).map_err(|e| e.to_string())?
        {
            return Err("baseline cache mismatch".into());
        }
        let mut errors = Vec::new();
        for probe in 0..3 {
            let values = DVector::from_iterator(
                variables.len(),
                variables.iter().enumerate().map(|(j, v)| {
                    let fraction = ((j * 17 + probe * 29) % 97) as f64 / 97.;
                    (1. - fraction) * v.bound.lower + fraction * v.bound.upper
                }),
            );
            let mut candidate = zero.clone();
            for (v, x) in variables.iter().zip(values.iter()) {
                set(&mut candidate, v, *x)?;
            }
            let full = planner.evaluate_joint_uncached(&candidate)?;
            let error = (commands(&full)? - (&matrix * &values + &offset)).amax();
            if !error.is_finite() || error > 1e-8 {
                return Err(format!(
                    "independent command map mismatch in {name}: {error}"
                ));
            }
            errors.push(error);
        }
        cases.push(serde_json::json!({"case":name,"columns":variables.len(),"rows":matrix.nrows(),"bounded_probe_errors":errors,"baseline_cache_byte_equal":true}));
    }
    if planner
        .joint_servo_command_jacobian(&recipe.candidate, &[full[0].clone(), full[0].clone()])
        .is_ok()
    {
        return Err("duplicate command-map decision accepted".into());
    }
    println!(
        "{}",
        serde_json::json!({"reference_report":report,"cases":cases,"duplicate_rejected":true,"scope":"Exact command-map checks against independent full CAD loads with bounded force probes, reordered partial variables and retained nonselected force offsets. These are audit loads, not feasible gaits or runtime speed results."})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

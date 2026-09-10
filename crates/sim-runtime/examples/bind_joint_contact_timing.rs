//! Bind existing event-aligned force nodes to contact timing and audit replay.
use serde::{Deserialize, Serialize};
use sim_domain_control::trajectory::Trajectory;
use sim_runtime::{
    contact_planning::{
        ContactPlanRecipe, ContactPlanner, JointContactMotion, JointContactVariable,
    },
    session::{Scene, Session},
    tracking::CaptureConfig,
};
use sim_solve::inequality_augmented_lagrangian::AugmentedLagrangianConfig;
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
        return Err("usage: bind_joint_contact_timing scene.json markers.json recipe.json".into());
    }
    let read = |p: &str| std::fs::read(p).map_err(|e| e.to_string());
    let scene: Scene = serde_json::from_slice(&read(&args[0])?).map_err(|e| e.to_string())?;
    let markers: CaptureConfig =
        serde_json::from_slice(&read(&args[1])?).map_err(|e| e.to_string())?;
    let original: Recipe = serde_json::from_slice(&read(&args[2])?).map_err(|e| e.to_string())?;
    let mut recipe = original.clone();
    recipe.candidate = original.candidate.with_contact_timed_forces()?;
    let mut maximum_curve_error_n = 0.0_f64;
    let mut maximum_knot_time_error = 0.0_f64;
    for (old, new) in original
        .candidate
        .force_templates
        .iter()
        .flatten()
        .zip(recipe.candidate.force_templates.iter().flatten())
    {
        if old.keyframes.len() != new.keyframes.len() {
            return Err("force knot count changed".into());
        }
        for (a, b) in old.keyframes.iter().zip(&new.keyframes) {
            if a.values != b.values {
                return Err("force coefficients changed".into());
            }
            maximum_knot_time_error = maximum_knot_time_error.max((a.time_s - b.time_s).abs());
        }
        let a = Trajectory::new(old.clone())?;
        let b = Trajectory::new(new.clone())?;
        for i in 0..=4096 {
            let phase = i as f64 / 4096.;
            for (x, y) in a.sample(phase)?.values.iter().zip(b.sample(phase)?.values) {
                maximum_curve_error_n = maximum_curve_error_n.max((x - y).abs());
            }
        }
    }
    if maximum_curve_error_n > 1e-10 {
        return Err("binding changed initial force curve".into());
    }
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let planner = ContactPlanner::new(&session.robot.art, &seed, &markers, original.robot.clone())?;
    let original_report = planner.evaluate_joint_uncached(&original.candidate)?;
    let bound_report = planner.evaluate_joint_uncached(&recipe.candidate)?;
    // Deliberately leave stored times stale: every evaluator must resolve them.
    let mut probe = recipe.clone();
    probe
        .candidate
        .motion
        .feet
        .get_mut(1)
        .ok_or("timing probe requires two feet")?
        .phase_offset += 1e-4;
    let probe_report = planner.evaluate_joint(&probe.candidate)?;
    let resolved = probe.candidate.materialized_force_timing()?;
    let resolved_report = planner.evaluate_joint_uncached(&resolved)?;
    let cached_report = planner.evaluate_joint(&probe.candidate)?;
    let encode = |r: &sim_runtime::contact_planning::JointContactReport| {
        serde_json::to_vec(r).map_err(|e| e.to_string())
    };
    if encode(&probe_report)? != encode(&resolved_report)?
        || encode(&probe_report)? != encode(&cached_report)?
    {
        return Err("stale timing resolution / full CAD / cache replay mismatch".into());
    }
    let mut fixed = resolved.clone();
    fixed.force_timing = None;
    if encode(&probe_report)? != encode(&planner.evaluate_joint_uncached(&fixed)?)? {
        return Err("materialized fixed-reference replay mismatch".into());
    }
    println!("{}", serde_json::to_string(&serde_json::json!({
        "recipe": recipe, "original_report": original_report, "bound_report": bound_report,
        "probe_recipe": probe, "probe_report": probe_report,
        "maximum_curve_error_n": maximum_curve_error_n,
        "maximum_knot_time_error": maximum_knot_time_error,
        "probe_foot_phase_increment": 1e-4,
        "stale_resolved_cached_and_fixed_reports_identical": true,
        "scope": "Initial force coefficients, all decision bounds and objective unchanged; strict event ordering retained. One timing probe and sampled initial curve fidelity, not a speed or feasibility certificate."
    })).map_err(|e| e.to_string())?);
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

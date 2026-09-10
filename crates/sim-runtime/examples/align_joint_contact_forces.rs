//! Prepare force variables from shared contact-event alignment and CAD loads.
use serde::{Deserialize, Serialize};
use sim_runtime::{
    contact_planning::{
        ContactPlanRecipe, ContactPlanner, JointContactDecision, JointContactMotion,
        JointContactVariable,
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
    if args.len() != 3 && !(args.len() == 4 && args[3] == "--seed-forces") {
        return Err(
            "usage: align_joint_contact_forces scene.json markers.json recipe.json [--seed-forces]"
                .into(),
        );
    }
    let read = |p: &str| std::fs::read(p).map_err(|e| e.to_string());
    let scene: Scene = serde_json::from_slice(&read(&args[0])?).map_err(|e| e.to_string())?;
    let markers: CaptureConfig =
        serde_json::from_slice(&read(&args[1])?).map_err(|e| e.to_string())?;
    let original: Recipe = serde_json::from_slice(&read(&args[2])?).map_err(|e| e.to_string())?;
    if original.candidate.motion.feet.iter().any(|f| !f.additional_steps.is_empty()) {
        return Err("this legacy alignment CLI accepts one stance per foot; use the shared per-stance alignment and force-variable APIs".into());
    }
    let mut bounds = std::collections::BTreeMap::<(usize, usize, usize), VariableBound>::new();
    let mut seen = std::collections::BTreeSet::new();
    let expected = original
        .candidate
        .force_templates
        .iter()
        .flatten()
        .map(|t| t.keyframes.len().saturating_sub(2) * 3)
        .sum::<usize>();
    // The shared layout validates every original template before we index it.
    let resampled = original.candidate.with_event_aligned_linear_forces()?;
    let curve_samples = 4097;
    let mut maximum_curve_change = 0.0_f64;
    let mut linear_curve_change = 0.0_f64;
    let mut linear_templates_checked = 0;
    for (old, new) in original
        .candidate
        .force_templates
        .iter()
        .flatten()
        .zip(resampled.force_templates.iter().flatten())
    {
        let linear = matches!(
            old.interpolation,
            sim_domain_control::trajectory::Interpolation::Linear
        );
        if linear {
            linear_templates_checked += 1;
            if old
                .keyframes
                .iter()
                .any(|k| !new.keyframes.iter().any(|n| n.time_s == k.time_s))
            {
                return Err("linear refinement discarded an original knot".into());
            }
        }
        let a = sim_domain_control::trajectory::Trajectory::new(old.clone())?;
        let b = sim_domain_control::trajectory::Trajectory::new(new.clone())?;
        for i in 0..curve_samples {
            let t = i as f64 / (curve_samples - 1) as f64;
            for (x, y) in a.sample(t)?.values.iter().zip(&b.sample(t)?.values) {
                let error = (x - y).abs();
                maximum_curve_change = maximum_curve_change.max(error);
                if linear {
                    linear_curve_change = linear_curve_change.max(error);
                }
            }
        }
    }
    if linear_curve_change > 1e-10 {
        return Err("linear force curve refinement lost numerical fidelity".into());
    }
    for v in &original.variables {
        if let JointContactDecision::Force {
            clock,
            foot,
            node,
            axis,
        } = v.decision
        {
            let template = original
                .candidate
                .force_templates
                .get(clock)
                .and_then(|t| t.get(foot))
                .ok_or("invalid original force variable")?;
            if node == 0
                || node >= template.keyframes.len() - 1
                || axis >= 3
                || !seen.insert((clock, foot, node, axis))
            {
                return Err("invalid or duplicate force node variable".into());
            }
            let key = (clock, foot, axis);
            if let Some(b) = bounds.get(&key) {
                if b.lower != v.bound.lower || b.upper != v.bound.upper {
                    return Err("alignment requires uniform per-foot/clock/axis force bounds; no bounds are silently widened".into());
                }
            } else {
                bounds.insert(key, v.bound.clone());
            }
        }
    }
    if seen.len() != expected {
        return Err("every interior force value must already be a variable".into());
    }
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let planner = ContactPlanner::new(&session.robot.art, &seed, &markers, original.robot.clone())?;
    let original_report = planner.evaluate_joint_uncached(&original.candidate)?;
    let resampled_report = planner.evaluate_joint_uncached(&resampled)?;
    let mut recipe = original.clone();
    recipe.candidate = if args.len() == 4 {
        planner.seed_joint_forces(&resampled)?
    } else {
        resampled.clone()
    };
    recipe.variables.clear();
    let mut inserted = false;
    for variable in &original.variables {
        if matches!(variable.decision, JointContactDecision::Force { .. }) {
            if !inserted {
                for (clock, templates) in recipe.candidate.force_templates.iter().enumerate() {
                    for (foot, template) in templates.iter().enumerate() {
                        for node in 1..template.keyframes.len() - 1 {
                            for axis in 0..3 {
                                let bound = bounds
                                    .get(&(clock, foot, axis))
                                    .ok_or("missing original force bound")?;
                                let value = template.keyframes[node].values[axis];
                                if !value.is_finite() || value < bound.lower || value > bound.upper
                                {
                                    return Err(
                                        "aligned force seed exceeds original coefficient bound"
                                            .into(),
                                    );
                                }
                                recipe.variables.push(JointContactVariable {
                                    decision: JointContactDecision::Force {
                                        clock,
                                        foot,
                                        node,
                                        axis,
                                    },
                                    bound: bound.clone(),
                                });
                            }
                        }
                    }
                }
                inserted = true;
            }
        } else {
            recipe.variables.push(variable.clone());
        }
    }
    let report = planner.evaluate_joint_uncached(&recipe.candidate)?;
    println!(
        "{}",
        serde_json::json!({"recipe":recipe,"original_report":original_report,"resampled_report":resampled_report,"resampled_force_templates":resampled.force_templates,"report":report,"curve_samples_per_template":curve_samples,"linear_templates_checked":linear_templates_checked,"maximum_resampled_force_curve_change_n":maximum_curve_change,"maximum_linear_force_curve_change_n":linear_curve_change,"initialized_from_cad":args.len()==4,"original_variables":original.variables.len(),"original_force_nodes":original.candidate.force_templates.iter().map(|t|t.iter().map(|f|f.keyframes.len()).collect::<Vec<_>>()).collect::<Vec<_>>(),"scope":"Shared event-aligned linear force layout at the current motion, preserving all body/foot/timing decisions, physical model, force coefficient bounds and search controls. Original linear knots are retained, giving exact curve refinement; conversion from quintic changes the force curve and is audited explicitly. Optional CAD force initialization is only a seed. Knots remain in normalized stance coordinates during each solve; later timing changes may require another explicit refinement. No physical feasibility or speed gain is claimed."})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

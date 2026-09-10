//! Compare force-only reloads and key invalidations with independent CAD evaluations.
use serde::Deserialize;
use sim_runtime::{
    contact_planning::{
        ContactPlanRecipe, ContactPlanner, JointContactDecision, JointContactMotion,
        JointContactVariable,
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
    #[serde(rename = "search")]
    _search: serde_json::Value,
}
fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("usage: audit_joint_force_cache scene.json markers.json recipe.json".into());
    }
    let read = |p: &str| std::fs::read(p).map_err(|e| e.to_string());
    let scene: Scene = serde_json::from_slice(&read(&args[0])?).map_err(|e| e.to_string())?;
    let markers: CaptureConfig =
        serde_json::from_slice(&read(&args[1])?).map_err(|e| e.to_string())?;
    let recipe: Recipe = serde_json::from_slice(&read(&args[2])?).map_err(|e| e.to_string())?;
    if recipe.candidate.motion.feet.iter().any(|f| !f.additional_steps.is_empty()) {
        return Err("this legacy audit/initialization CLI accepts one stance per foot; use the per-stance shared APIs and updated Jacobian audits".into());
    }
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let planner = ContactPlanner::new(&session.robot.art, &seed, &markers, recipe.robot)?;
    let mut cases = vec![("unchanged".to_string(), recipe.candidate.clone(), true)];
    for (i, v) in recipe
        .variables
        .iter()
        .filter(|v| matches!(v.decision, JointContactDecision::Force { .. }))
        .step_by(10)
        .enumerate()
    {
        let mut c = recipe.candidate.clone();
        if let JointContactDecision::Force {
            clock,
            foot,
            node,
            axis,
        } = v.decision
        {
            let x = &mut c.force_templates[clock][foot].keyframes[node].values[axis];
            *x = (*x + 0.001 * (v.bound.upper - v.bound.lower)).min(v.bound.upper);
        }
        cases.push((format!("force_{i}"), c, true));
    }
    if recipe.candidate.motion.feet.len() < 2 {
        return Err("cache audit requires two or more feet".into());
    }
    let mut c = recipe.candidate.clone();
    c.motion.feet[0].center_world_m[0] += 1e-5;
    cases.push(("placement".into(), c, false));
    let mut c = recipe.candidate.clone();
    c.motion.feet[1].phase_offset += 1e-5;
    cases.push(("phase".into(), c, false));
    let mut c = recipe.candidate.clone();
    c.motion.feet[0].stance_fraction += 1e-5;
    cases.push(("stance_duration".into(), c, false));
    let mut c = recipe.candidate.clone();
    c.force_templates[0][0].keyframes[1].time_s += 1e-5;
    cases.push(("force_knot_time".into(), c, false));
    let mut c = recipe.candidate.clone();
    c.force_templates[0][0].interpolation =
        sim_domain_control::trajectory::Interpolation::QuinticRestToRest;
    cases.push(("force_interpolation".into(), c, false));
    planner.evaluate_joint(&recipe.candidate)?;
    let mut output = Vec::new();
    for (name, candidate, expect_hit) in cases {
        let before = planner.joint_cache_statistics();
        let start = std::time::Instant::now();
        let cached = planner.evaluate_joint(&candidate)?;
        let cached_s = start.elapsed().as_secs_f64();
        let after = planner.joint_cache_statistics();
        if (after.hits > before.hits) != expect_hit {
            return Err(format!("unexpected cache invalidation: {name}"));
        }
        let start = std::time::Instant::now();
        let full = planner.evaluate_joint_uncached(&candidate)?;
        let full_s = start.elapsed().as_secs_f64();
        let a = serde_json::to_string(&cached).map_err(|e| e.to_string())?;
        let b = serde_json::to_string(&full).map_err(|e| e.to_string())?;
        if a != b {
            let index = a
                .chars()
                .zip(b.chars())
                .position(|(a, b)| a != b)
                .unwrap_or(a.len().min(b.len()));
            return Err(format!(
                "cache/full mismatch {name} at {index}: {:?} versus {:?}",
                a.chars()
                    .skip(index.saturating_sub(60))
                    .take(160)
                    .collect::<String>(),
                b.chars()
                    .skip(index.saturating_sub(60))
                    .take(160)
                    .collect::<String>()
            ));
        }
        output.push(serde_json::json!({"case":name,"cache_hit":expect_hit,"byte_identical":true,"cached_s":cached_s,"full_s":full_s}));
    }
    println!(
        "{}",
        serde_json::json!({"cases":output,"statistics":planner.joint_cache_statistics(),
        "scope":"All report fields compared byte-for-byte against fresh CAD evaluations for force probes and cache invalidations. Timing is native evaluation cost, not browser or physical gait speed."})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

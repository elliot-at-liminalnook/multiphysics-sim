//! Contact-geometry relaxation using the unchanged compiled CAD foot samples.
use serde::Deserialize;
use sim_domain_robot::motion_capability::constrained_point_forces;
use sim_runtime::{
    contact_planning::{ContactPlanRecipe, ContactPlanner, JointContactMotion},
    session::{Scene, Session},
    tracking::CaptureConfig,
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    robot: ContactPlanRecipe,
    candidate: JointContactMotion,
    #[serde(rename = "variables")]
    _variables: serde_json::Value,
    #[serde(rename = "search")]
    _search: serde_json::Value,
}
fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 5 {
        return Err("usage: audit_joint_surface_support scene.json motion-markers.json surface-markers.json recipe.json eligibility-gap-m".into());
    }
    let gap: f64 = args[4].parse().map_err(|_| "invalid eligibility gap")?;
    if !gap.is_finite() || gap < 0.0 {
        return Err("finite nonnegative explicit eligibility gap required".into());
    }
    let read = |p: &str| std::fs::read(p).map_err(|e| e.to_string());
    let scene: Scene = serde_json::from_slice(&read(&args[0])?).map_err(|e| e.to_string())?;
    let markers: CaptureConfig =
        serde_json::from_slice(&read(&args[1])?).map_err(|e| e.to_string())?;
    let surfaces: CaptureConfig =
        serde_json::from_slice(&read(&args[2])?).map_err(|e| e.to_string())?;
    let recipe: Recipe = serde_json::from_slice(&read(&args[3])?).map_err(|e| e.to_string())?;
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let art = &session.robot.art;
    for foot in &markers.markers {
        let link = art
            .links
            .iter()
            .find(|l| l.name == foot.link)
            .ok_or("unknown foot link")?;
        let supplied = surfaces
            .markers
            .iter()
            .filter(|m| m.link == foot.link)
            .collect::<Vec<_>>();
        if supplied.len() != link.contact.len()
            || link.contact.iter().any(|p| {
                !supplied
                    .iter()
                    .any(|m| (*p - nalgebra::Vector3::from(m.local_point_m)).norm() <= 1e-12)
            })
        {
            return Err("surface audit requires every compiled foot contact sample".into());
        }
    }
    let seed = session.robot.generalized();
    let planner = ContactPlanner::new(art, &seed, &markers, recipe.robot.clone())?;
    let original = planner.evaluate_joint_uncached(&recipe.candidate)?;
    let frames = planner.joint_contact_surface_frames(&recipe.candidate, &surfaces)?;
    if frames.len() != original.motion_report.frames.len() {
        return Err("surface frame count mismatch".into());
    }
    let mut rows = Vec::new();
    let mut geometry_error = 0.0_f64;
    let mut clearance_error = 0.0_f64;
    for (frame, original) in frames.into_iter().zip(&original.motion_report.frames) {
        geometry_error = geometry_error.max(frame.original_marker_geometry_error_m);
        let eligible = frame
            .points
            .iter()
            .filter(|p| frame.planned_contacts[p.foot] && p.floor_gap_m <= gap)
            .collect::<Vec<_>>();
        let positions = eligible
            .iter()
            .map(|p| p.position_world_m)
            .collect::<Vec<_>>();
        let counts = (0..markers.markers.len())
            .map(|foot| eligible.iter().filter(|p| p.foot == foot).count())
            .collect::<Vec<_>>();
        let gaps = (0..markers.markers.len())
            .map(|foot| {
                frame
                    .points
                    .iter()
                    .filter(|p| p.foot == foot)
                    .map(|p| p.floor_gap_m)
                    .fold(f64::INFINITY, f64::min)
            })
            .collect::<Vec<_>>();
        for (foot, minimum) in gaps.iter().enumerate() {
            let previous = original
                .floor_clearances
                .iter()
                .find(|c| c.link == markers.markers[foot].link)
                .ok_or("missing original foot clearance")?;
            clearance_error = clearance_error.max((previous.minimum_clearance_m - minimum).abs());
        }
        let (residual, forces, converged, cones) = if positions.is_empty() {
            (
                frame.required_wrench.iter().map(|v| -v).collect::<Vec<_>>(),
                vec![],
                true,
                true,
            )
        } else {
            let allocation = constrained_point_forces(
                &positions,
                frame.reference_world_m,
                std::array::from_fn(|i| frame.required_wrench[i]),
                recipe.robot.moment_length_scale_m,
                art.model.world.floor_friction,
                &vec![1.; positions.len()],
                &recipe.robot.force_allocation,
            )?;
            (
                allocation.wrench_residual.to_vec(),
                allocation.forces_world_n,
                allocation.optimizer.as_ref().is_some_and(|r| r.converged),
                allocation.unilateral_friction_satisfied,
            )
        };
        let force = residual[..3]
            .iter()
            .map(|v| v.abs())
            .fold(0.0_f64, f64::max);
        let moment = residual[3..]
            .iter()
            .map(|v| v.abs())
            .fold(0.0_f64, f64::max);
        let pass = converged
            && cones
            && force <= recipe.robot.force_tolerance_n
            && moment <= recipe.robot.moment_tolerance_nm;
        rows.push(serde_json::json!({"phase":frame.phase,"clock":frame.clock,"planned_contacts":frame.planned_contacts,
            "eligible_counts":counts,"minimum_foot_gaps_m":gaps,"eligible_marker_ids":eligible.iter().map(|p|&p.marker_id).collect::<Vec<_>>(),
            "eligible_positions_world_m":positions,"forces_world_n":forces,"wrench_residual":residual,
            "force_error_n":force,"moment_error_nm":moment,"allocator_converged":converged,"cones_satisfied":cones,"balance_passed":pass}));
    }
    if geometry_error > 1e-7 || clearance_error > 1e-7 {
        return Err(format!(
            "surface/reference geometry disagrees: {geometry_error}, {clearance_error} m"
        ));
    }
    let passed = rows.iter().filter(|r| r["balance_passed"] == true).count();
    println!(
        "{}",
        serde_json::json!({"frames":rows.len(),"passed_balance_frames":passed,
        "eligibility_gap_m":gap,"surface_markers":surfaces.markers.len(),"all_compiled_foot_samples_verified":true,
        "original_marker_geometry_error_m":geometry_error,"original_clearance_error_m":clearance_error,
        "force_allocation":recipe.robot.force_allocation,"friction":art.model.world.floor_friction,
        "maximum_force_error_n":rows.iter().map(|r|r["force_error_n"].as_f64().unwrap()).fold(0.0_f64,f64::max),
        "maximum_moment_error_nm":rows.iter().map(|r|r["moment_error_nm"].as_f64().unwrap()).fold(0.0_f64,f64::max),
        "rows":rows,"scope":"Instantaneous finite-surface support diagnostic using all unchanged compiled CAD foot samples and exact reference poses. Only planned stance samples within the explicitly supplied floor gap may carry force. The gap is an optimistic contact-eligibility relaxation, not physical compliance or injected force. Allocation enforces unilateral/friction cones but omits actuator, force-curve, velocity/slip and total per-foot force bounds. Passing is not a gait certificate; failed allocation alone is not an infeasibility proof."})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

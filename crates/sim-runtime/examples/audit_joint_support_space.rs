//! Separate instantaneous support-wrench limits from force-trajectory limits.
use nalgebra::{DMatrix, DVector};
use serde::Deserialize;
use sim_runtime::{
    contact_planning::{
        ContactPlanRecipe, ContactPlanner, JointContactDecision, JointContactMotion,
        JointContactVariable,
    },
    session::{Scene, Session},
    tracking::CaptureConfig,
};
use sim_solve::{affine_feasibility::affine_residual_lower_bound, least_squares::VariableBound};
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
        return Err("usage: audit_joint_support_space scene.json markers.json recipe.json".into());
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
    let planner = ContactPlanner::new(&session.robot.art, &seed, &markers, recipe.robot.clone())?;
    // Validate force templates through the shared planner before indexing them.
    let systems = planner.joint_force_frame_systems(&recipe.candidate)?;
    let baseline = planner.evaluate_joint_uncached(&recipe.candidate)?;
    // Convex interpolation stays within the hull of all allowed node values.
    // Use its enclosing coordinate box as a relaxation at every stance instant.
    let mut boxes = recipe
        .candidate
        .force_templates
        .iter()
        .map(|templates| {
            templates
                .iter()
                .map(|t| {
                    std::array::from_fn::<_, 3, _>(|axis| VariableBound {
                        lower: t
                            .keyframes
                            .iter()
                            .map(|k| k.values[axis])
                            .fold(f64::INFINITY, f64::min),
                        upper: t
                            .keyframes
                            .iter()
                            .map(|k| k.values[axis])
                            .fold(f64::NEG_INFINITY, f64::max),
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for variable in &recipe.variables {
        if let JointContactDecision::Force {
            clock,
            foot,
            node,
            axis,
        } = variable.decision
        {
            let template = recipe
                .candidate
                .force_templates
                .get(clock)
                .and_then(|v| v.get(foot))
                .ok_or("force index out of range")?;
            if node == 0 || node >= template.keyframes.len() - 1 || axis >= 3 {
                return Err("invalid force-node decision".into());
            }
            let b = &mut boxes[clock][foot][axis];
            b.lower = b.lower.min(variable.bound.lower);
            b.upper = b.upper.max(variable.bound.upper);
        }
    }
    if systems.len() != baseline.motion_report.frames.len() {
        return Err("support system frame mismatch".into());
    }
    let mut reconstruction_error = 0.0_f64;
    let mut rows = Vec::new();
    for (system, frame) in systems.into_iter().zip(&baseline.motion_report.frames) {
        let active = system
            .planned_contacts
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.then_some(i))
            .collect::<Vec<_>>();
        let tol = |i| {
            if i < 3 {
                recipe.robot.force_tolerance_n
            } else {
                recipe.robot.moment_tolerance_nm
            }
        };
        let b = DVector::from_iterator(
            6,
            system
                .zero_force_residual
                .iter()
                .enumerate()
                .map(|(i, r)| r / tol(i)),
        );
        let forces = DVector::from_iterator(
            frame.support_forces_world_n.len() * 3,
            frame.support_forces_world_n.iter().flatten().copied(),
        );
        let reconstructed = &system.force_to_wrench * forces
            + DVector::from_column_slice(&system.zero_force_residual);
        for i in 0..6 {
            reconstruction_error = reconstruction_error
                .max(((reconstructed[i] - frame.wrench_residual[i]) / tol(i)).abs());
        }
        if active.is_empty() {
            rows.push(serde_json::json!({"phase":system.phase,"clock":system.clock,"active_feet":active,
                "maximum_residual_lower_bound":b.amax(),"least_squares_maximum_residual":b.amax(),"scope":"flight: no support forces"}));
            continue;
        }
        let a = DMatrix::from_fn(6, active.len() * 3, |r, c| {
            system.force_to_wrench[(r, active[c / 3] * 3 + c % 3)] / tol(r)
        });
        let bounds = active
            .iter()
            .flat_map(|foot| boxes[system.clock_index][*foot].clone())
            .collect::<Vec<_>>();
        let result = affine_residual_lower_bound(&a, &b, &bounds)?;
        rows.push(serde_json::json!({"phase":system.phase,"clock":system.clock,"active_feet":active,
            "maximum_residual_lower_bound":result.maximum_residual_lower_bound,
            "least_squares_maximum_residual":result.least_squares_maximum_residual,
            "dual_column_residual_maximum":result.dual_column_residual_maximum,
            "dual_weights":result.dual_weights,"least_squares_residuals":result.least_squares_residuals}));
    }
    rows.sort_by(|a, b| {
        b["maximum_residual_lower_bound"]
            .as_f64()
            .unwrap()
            .total_cmp(&a["maximum_residual_lower_bound"].as_f64().unwrap())
    });
    let violations = rows
        .iter()
        .filter(|r| r["maximum_residual_lower_bound"].as_f64().unwrap() > 1.0)
        .count();
    if reconstruction_error > 1e-8 {
        return Err(format!(
            "support map disagrees with full CAD evaluation by {reconstruction_error}"
        ));
    }
    println!(
        "{}",
        serde_json::json!({"frames":rows.len(),"frames_proven_outside_tolerance":violations,
        "independent_reconstruction_error":reconstruction_error,
        "maximum_residual_lower_bound":rows[0]["maximum_residual_lower_bound"],"force_boxes":boxes,"rows":rows,
        "scope":"Instantaneous force relaxation at fixed CAD motion. Forces vary independently at every instant and are zero in swing; bounds enclose all declared force-node values. Friction, motor constraints and temporal force-curve restrictions are omitted. A numerical bound above one excludes sampled balance for this motion even with these extra freedoms; a lower bound below one does not certify feasibility. Not a global robot speed limit or interval-arithmetic hardware proof."})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

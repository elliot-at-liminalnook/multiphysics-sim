//! Reproducible batch initialization through the shared CAD joint planner.
use serde::Deserialize;
use sim_runtime::{
    contact_planning::{ContactPlanRecipe, ContactPlanner, JointContactMotion},
    session::{Scene, Session},
    tracking::CaptureConfig,
};
use std::io::{BufWriter, Write};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {
    robot: ContactPlanRecipe,
    starts: Vec<Start>,
    #[cfg(feature = "conic")]
    #[serde(default)]
    conic: Option<ConicScreen>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Start {
    id: String,
    candidate: JointContactMotion,
    #[cfg(feature = "conic")]
    #[serde(default)]
    align_and_bind_forces: bool,
}
#[cfg(feature = "conic")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConicScreen {
    solver: sim_solve::conic::ConicConfig,
    force_bounds: Vec<Vec<[sim_solve::least_squares::VariableBound; 3]>>,
}
fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("usage: screen_joint_contact_starts scene.json markers.json batch.json".into());
    }
    let read = |p: &str| std::fs::read(p).map_err(|e| e.to_string());
    let scene: Scene = serde_json::from_slice(&read(&args[0])?).map_err(|e| e.to_string())?;
    let markers: CaptureConfig =
        serde_json::from_slice(&read(&args[1])?).map_err(|e| e.to_string())?;
    let batch: Batch = serde_json::from_slice(&read(&args[2])?).map_err(|e| e.to_string())?;
    let mut ids = std::collections::BTreeSet::new();
    if batch.starts.is_empty()
        || batch
            .starts
            .iter()
            .any(|s| s.id.is_empty() || !ids.insert(&s.id))
    {
        return Err("nonempty batch with unique nonempty start IDs required".into());
    }
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let planner = ContactPlanner::new(&session.robot.art, &seed, &markers, batch.robot)?;
    let mut output = BufWriter::new(std::io::stdout().lock());
    for (index, start) in batch.starts.iter().enumerate() {
        let result = (|| {
            #[cfg(feature = "conic")]
            if let Some(config) = &batch.conic {
                let candidate = if start.align_and_bind_forces {
                    start
                        .candidate
                        .with_event_aligned_linear_forces()?
                        .with_contact_timed_forces()?
                } else {
                    start.candidate.clone()
                };
                let variables = candidate.force_variables_for_bounds(&config.force_bounds)?;
                let result =
                    planner.optimize_joint_forces_conic(&candidate, &variables, &config.solver)?;
                let physical=result.report.as_ref().map(|r|serde_json::json!({
                    "speed_m_s":r.motion_report.speed_m_s,"force_error_n":r.motion_report.maximum_force_error_n,
                    "moment_error_nm":r.motion_report.maximum_moment_error_nm,"torque_margin_nm":r.motion_report.minimum_torque_margin_nm,
                    "cone_violation_n":r.maximum_cone_violation_n,"sampled_feasible":r.sampled_feasible,
                    "maximum_inequality":r.constraints.inequalities.iter().copied().fold(0.0_f64,f64::max),"frames":r.motion_report.frames.len(),
                    "maximum_interlink_penetration_m":r.motion_report.frames.iter().map(|f|f.maximum_inter_link_penetration_m).fold(0.0_f64,f64::max),
                    "maximum_floor_penetration_m":r.motion_report.frames.iter().map(|f|f.maximum_floor_penetration_m).fold(0.0_f64,f64::max)
                }));
                return Ok(
                    serde_json::json!({"candidate":result.candidate,"force_variables":variables,"physical":physical,
                    "conic":{"status":result.search.status,"solved":result.search.solved,"has_primal_candidate":result.search.has_primal_candidate,
                        "iterations":result.search.iterations,"minimax_balance":result.search.reported_objective,"dual_objective":result.search.reported_dual_objective,
                        "primal_dual_gap":result.search.primal_dual_objective_gap,"primal_violation":result.search.maximum_primal_cone_violation,
                        "dual_violation":result.search.maximum_dual_cone_violation,"stationarity_residual":result.search.maximum_stationarity_residual},
                    "independent_affine_error":result.independent_affine_error,"force_box_violation_n":result.force_box_violation_n,
                    "scope":result.scope}),
                );
            }
            let candidate = planner.seed_joint_forces(&start.candidate)?;
            let report = planner.evaluate_joint(&candidate)?;
            let motion = &report.motion_report;
            Ok::<_, String>(serde_json::json!({
                "candidate":candidate,
                "speed_m_s":motion.speed_m_s,
                "force_error_n":motion.maximum_force_error_n,
                "moment_error_nm":motion.maximum_moment_error_nm,
                "torque_margin_nm":motion.minimum_torque_margin_nm,
                "cone_violation_n":report.maximum_cone_violation_n,
                "maximum_inequality":report.constraints.inequalities.iter().copied().fold(0.0_f64,f64::max),
                "sampled_feasible":report.sampled_feasible,
                "frames":motion.frames.len(),
                "inequalities":report.constraints.inequalities
            }))
        })();
        let row = match result {
            Ok(value) => serde_json::json!({"id":start.id,"result":value}),
            Err(error) => serde_json::json!({"id":start.id,"error":error}),
        };
        serde_json::to_writer(&mut output, &row).map_err(|e| e.to_string())?;
        writeln!(&mut output).map_err(|e| e.to_string())?;
        output.flush().map_err(|e| e.to_string())?;
        eprintln!(
            "screened {}/{}: {}",
            index + 1,
            batch.starts.len(),
            start.id
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

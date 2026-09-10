//! Prepare discrete contact-order neighbors for subsequent coupled local solves.
use serde::{Deserialize, Serialize};
use sim_runtime::{
    contact_planning::{
        ContactPlanRecipe, ContactPlanner, JointContactDecision, JointContactMotion,
        JointContactVariable,
    },
    session::{Scene, Session},
    tracking::CaptureConfig,
};
use sim_solve::conic::ConicConfig;
use std::{io::Write, path::Path, time::Instant};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    robot: ContactPlanRecipe,
    candidate: JointContactMotion,
    variables: Vec<JointContactVariable>,
    search: serde_json::Value,
}
fn write(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, value).map_err(|e| e.to_string())?;
    file.write_all(b"\n").map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 4 {
        return Err("usage: prepare_contact_order_neighbors scene.json markers.json source.recipe.json fresh-directory".into());
    }
    let source: Recipe = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let scene: Scene = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let markers: CaptureConfig = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let neighbors = source
        .candidate
        .adjacent_contact_order_neighbors(&source.variables, source.robot.direction_world)?;
    let out = Path::new(&args[3]);
    std::fs::create_dir(out)?;
    write(&out.join("source.recipe.json"), &source)?;
    write(&out.join("neighbors.json"), &neighbors)?;
    let mut starts = vec![(
        "control".to_string(),
        source.candidate.clone(),
        source.variables.clone(),
    )];
    // Apply the same knot-refinement pipeline without changing contact order.
    // This separates richer force curves from the effect of event-order changes.
    let bounds = source.candidate.uniform_force_bounds(&source.variables)?;
    let mut refined = source.candidate.materialized_force_timing()?;
    refined.force_timing = None;
    refined = refined
        .with_event_aligned_linear_forces()?
        .with_contact_timed_forces()?;
    let mut refined_variables = source
        .variables
        .iter()
        .filter(|v| matches!(v.decision, JointContactDecision::Motion { .. }))
        .cloned()
        .collect::<Vec<_>>();
    refined_variables.extend(refined.force_variables_for_bounds(&bounds)?);
    starts.push(("control-refined".to_string(), refined, refined_variables));
    let mut skipped = vec![];
    for neighbor in neighbors {
        if let (Some(candidate), Some(variables)) = (neighbor.candidate, neighbor.variables) {
            starts.push((format!("edge-{:03}", neighbor.edge), candidate, variables));
        } else {
            skipped.push(serde_json::json!({"edge":neighbor.edge,"swapped":neighbor.swapped,"error":neighbor.preparation_error}));
        }
    }
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let planner = ContactPlanner::new(&session.robot.art, &seed, &markers, source.robot.clone())?;
    let mut queue = vec![];
    for (id, candidate, variables) in starts {
        let started = Instant::now();
        let conic_path = out.join(format!("{id}.conic.json"));
        let conic =
            planner.optimize_joint_forces_conic(&candidate, &variables, &ConicConfig::default());
        let mut error = None;
        let mut priority = None;
        let mut native_status = None;
        let selected = match conic {
            Ok(result) => {
                write(&conic_path, &result)?;
                native_status = Some(result.search.status.clone());
                priority = result.report.as_ref().map(|r| {
                    r.constraints
                        .inequalities
                        .iter()
                        .copied()
                        .fold(0.0_f64, f64::max)
                });
                // Conic failure does not discard the motion: the full local solve
                // can alter body/feet/timing and uses the original force start.
                result.candidate.unwrap_or(candidate)
            }
            Err(message) => {
                write(&conic_path, &serde_json::json!({"error":message}))?;
                error = Some(message);
                candidate
            }
        };
        let recipe_path = out.join(format!("{id}.recipe.json"));
        write(
            &recipe_path,
            &Recipe {
                robot: source.robot.clone(),
                candidate: selected,
                variables,
                search: source.search.clone(),
            },
        )?;
        let row = serde_json::json!({"id":id,"recipe":recipe_path,"conic":conic_path,"conic_status":native_status,
            "conic_error":error,"priority_maximum_inequality":priority,"preparation_wall_s":started.elapsed().as_secs_f64()});
        eprintln!("{}", row);
        queue.push(row);
    }
    queue.sort_by(|a, b| {
        let control = |v: &serde_json::Value| v["id"].as_str() == Some("control");
        control(b)
            .cmp(&control(a))
            .then_with(|| {
                a["priority_maximum_inequality"]
                    .as_f64()
                    .unwrap_or(f64::INFINITY)
                    .total_cmp(
                        &b["priority_maximum_inequality"]
                            .as_f64()
                            .unwrap_or(f64::INFINITY),
                    )
            })
            .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
    });
    let summary = serde_json::json!({"version":1,"scene":args[0],"markers":args[1],"source":args[2],"items":queue,
        "unprepared_edges":skipped,"scope":"Adjacent cyclic contact-order starts, retaining model and search bounds. Fixed-motion conic residuals order the queue; every prepared start remains eligible for a coupled motion/force NLP, including failed conic solves. Preparation/NLP failure does not exclude a physical gait family. This is one graph neighborhood, not MCTS, INSAT or a global-optimality certificate."});
    write(&out.join("queue.json"), &summary)?;
    println!(
        "{}",
        serde_json::json!({"prepared":summary["items"].as_array().unwrap().len(),"unprepared":summary["unprepared_edges"].as_array().unwrap().len()})
    );
    Ok(())
}

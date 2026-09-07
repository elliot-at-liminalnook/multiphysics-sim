//! Local/CI validation front-end; JSON reports and nonzero exit on any failure.
use sim_dynamics::jacobian_check::CheckConfig;
use sim_runtime::{
    session::{Recording, Scene, Session},
    validation::{audit_jacobians, compare_recording, CompareConfig},
};
fn read<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn run() -> Result<bool, String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let usage="usage: sim-validate jacobian scene.json [after_frames] [check_config.json] | compare recording.json [compare_config.json] | constraints scene.json [frames] [rank_config.json] | steps scene.json [frames] [attempt_limit] | stage-jacobian scene.json warmup_frames duration_s [check_config.json]";
    let mode = args.first().ok_or(usage)?;
    let path = args.get(1).ok_or(usage)?;
    match mode.as_str() {
        "stage-jacobian" => {
            let scene: Scene = read(path)?;
            let warmup: usize = args.get(2).ok_or("missing warmup frame count")?.parse().map_err(|_| "invalid warmup frame count")?;
            let duration: f64 = args.get(3).ok_or("missing probe duration")?.parse().map_err(|_| "invalid probe duration")?;
            if !duration.is_finite() || duration<=0.0 || duration>scene.period_s {
                return Err("probe duration must be positive and at most one report period".into());
            }
            let config: CheckConfig = args.get(4).map(|p|read(p)).transpose()?.unwrap_or_default();
            let mut session=Session::new(scene,0)?;
            let action:Vec<_>=session.inputs.iter().map(|c|c.initial).collect();
            for _ in 0..warmup { session.step(&action)?; }
            session.set_attempt_audit_limit(128)?;
            let advance_error=session.robot.advance(duration).err();
            let labels=sim_runtime::validation::state_labels(&session.robot.runtime);
            let mut checks=Vec::new();
            for (island_index,island) in session.robot.runtime.islands.iter().enumerate() {
                if island.implicit_attempts.len()==128 {return Err("stage audit capacity reached".into());}
                let coordinates:Vec<_>=island.system.full_of.iter().map(|f|
                    labels[&island.system.state_ids[*f]].clone()).collect();
                for (attempt_index,point) in island.implicit_attempts.iter().enumerate() {
                    if point.last_linearization.is_none() {continue;}
                    let report=sim_dynamics::jacobian_check::check_implicit_jacobian(&island.system,point,&config)?;
                    checks.push(serde_json::json!({"island":island_index,"attempt":attempt_index,
                        "coordinates":coordinates,"solve":point,"report":report}));
                }
            }
            let passed=advance_error.is_none() && !checks.is_empty() && checks.iter().all(|v|v["report"]["passed"]==true);
            println!("{}",serde_json::to_string_pretty(&serde_json::json!({"passed":passed,
                "advance_error":advance_error,"config":config,"source":session.scene.robot.source,
                "options":session.scene.options,"checks":checks,"notes":[
                    "Provided derivatives are reassembled at the last recorded fresh matrix point, not read from cached factors.",
                    "The live residual must reproduce the recorded base bit for bit; external inputs must remain unchanged.",
                    "State probes are Newton increment columns, with algebraic weight 1, differential weight theta and rate factor 1/h; adapter rate probes test zero auxiliary derivatives.",
                    "The bounded trial list includes rejected steps; no derivative gate is waived for an inconclusive probe."
                ]})).unwrap());
            Ok(passed)
        }
        "steps" => {
            let scene: Scene = read(path)?;
            let count: usize = args.get(2).map(|v| v.parse()).transpose().map_err(|_| "invalid frame count")?.unwrap_or(1);
            let limit: usize = args.get(3).map(|v| v.parse()).transpose().map_err(|_| "invalid attempt limit")?.unwrap_or(512);
            if limit == 0 || limit > 10000 { return Err("attempt limit must be 1..10000".into()); }
            let mut session = Session::new(scene, 0)?;
            session.set_attempt_audit_limit(limit)?;
            let action: Vec<_> = session.inputs.iter().map(|c| c.initial).collect();
            let mut error = None;
            for _ in 0..count {
                match session.step(&action) {
                    Ok(frame) if frame.error.is_some() => { error=frame.error; break; }
                    Err(e) => { error=Some(e); break; }
                    _ => {}
                }
            }
            let mut report = sim_runtime::validation::implicit_attempt_report(&session)?;
            report["attempt_limit_per_island"] = limit.into();
            report["capacity_reached"] = session.robot.runtime.islands.iter().any(|i| i.implicit_attempts.len() == limit).into();
            report["error"] = serde_json::json!(error);
            println!("{}",serde_json::to_string_pretty(&report).unwrap());
            Ok(error.is_none())
        }
        "constraints" => {
            use sim_domain_robot::articulated::constraints::RankConfig;
            let scene: Scene = read(path)?;
            let count: usize = args.get(2).map(|v| v.parse()).transpose()
                .map_err(|_| "invalid frame count")?.unwrap_or(10);
            let config: RankConfig = args.get(3).map(|p| read(p)).transpose()?.unwrap_or_default();
            let mut session = Session::new(scene, 0)?;
            let action: Vec<_> = session.inputs.iter().map(|c| c.initial).collect();
            let mut frames = Vec::new();
            for index in 0..=count {
                if index > 0 { session.step(&action)?; }
                let frame = session.frame();
                if let Some(error) = &frame.error { return Err(error.clone()); }
                let g = session.robot.generalized();
                let audit = session.robot.art.audit_constraints(&g, &config)?;
                frames.push(serde_json::json!({"frame":index, "time_s":frame.time_s,
                    "joint_positions":g.q, "contacts":frame.contacts,
                    "solver":session.robot.runtime.islands.iter().map(|i| i.stats).collect::<Vec<_>>(),
                    "constraints":audit}));
            }
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "source":session.scene.robot.source, "options":session.scene.options,
                "frames":frames, "notes":[
                    "Diagnostic only: pose-local QR/SVD does not authorize deleting constraints.",
                    "All original bilateral rows are included; this is not the full Newton Jacobian or a contact rank test.",
                    "Rank uses the exact linear velocity map, physical row/column scales, and explicitly reported thresholds.",
                    "Position and velocity closure use committed endpoint states. Acceleration/stabilized fields use report-interval joint speed differences with unreconstructed base/modal accelerations; they are estimates, not converged solver residuals."
                ]})).unwrap());
            Ok(true)
        }
        "jacobian" => {
            let scene: Scene = read(path)?;
            let count: usize = args
                .get(2)
                .map(|v| v.parse().map_err(|_| "invalid frame count".to_string()))
                .transpose()?
                .unwrap_or(0);
            let config: CheckConfig = args
                .get(3)
                .map(|p| read(p))
                .transpose()?
                .unwrap_or_default();
            let mut session = Session::new(scene, 0)?;
            let action: Vec<_> = session.inputs.iter().map(|c| c.initial).collect();
            for _ in 0..count {
                session.step(&action)?;
            }
            let checks = audit_jacobians(&session, &config)?;
            let passed = !checks.is_empty() && checks.iter().all(|c| c.report.passed);
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"passed":passed,"config":config,"checks":checks})
                )
                .unwrap()
            );
            Ok(passed)
        }
        "compare" => {
            let recording: Recording = read(path)?;
            let config: CompareConfig = args
                .get(2)
                .map(|p| read(p))
                .transpose()?
                .unwrap_or_default();
            let report = compare_recording(&recording, &config)?;
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
            Ok(report.passed)
        }
        _ => Err(usage.into()),
    }
}
fn main() {
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(error) => {
            eprintln!("{}", serde_json::json!({"passed":false,"error":error}));
            std::process::exit(2);
        }
    }
}

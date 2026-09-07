//! Compare contact-free closure evaluation with full dynamics at saved stages.
//! This measures an evaluation kernel, not timestep convergence or accuracy.
//! Usage: compare_constraint_rows capture.json [repeats]
use serde_json::json;
use sim_dynamics::ImplicitAttempt;
use sim_runtime::session::{Scene, Session};
use std::{hint::black_box, time::Instant};

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args.len() > 2 {
        return Err("usage: compare_constraint_rows capture.json [repeats]".into());
    }
    let repeats = args
        .get(1)
        .map(|s| s.parse::<usize>())
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or(100);
    if repeats == 0 || repeats > 10000 {
        return Err("repeats must be 1..10000".into());
    }
    let capture: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&args[0]).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let scene: Scene =
        serde_json::from_value(capture["recording"]["scene"].clone()).map_err(|e| e.to_string())?;
    let seed = capture["recording"]["seed"]
        .as_u64()
        .ok_or("missing seed")?;
    let session = Session::new(scene, seed)?;
    let islands = capture["attempt_audit"]["islands"]
        .as_array()
        .ok_or("missing audit")?;
    if islands.len() != 1 || islands[0]["capacity_reached"] != false {
        return Err("requires one uncapped audited island".into());
    }
    let attempts = islands[0]["attempts"]
        .as_array()
        .ok_or("missing attempts")?;
    if attempts.is_empty() {
        return Err("no saved stages".into());
    }
    let art = &session.robot.art;
    let mut reports = Vec::new();
    for (index, value) in attempts.iter().enumerate() {
        let point: ImplicitAttempt =
            serde_json::from_value(value["solve"].clone()).map_err(|e| e.to_string())?;
        let g = session
            .robot
            .generalized_at_solver_point(0, &point)?
            .ok_or("stage has no robot")?;
        let expected = art.evaluate(&g).loop_rows;
        if expected.is_empty() {
            return Err("robot has no loop or transmission rows".into());
        }
        let mut probes = vec![g.clone()];
        // Motion probes also guard against accidentally reusing base kinematics.
        for col in 0..g.q.len() {
            for delta in [-1e-4, -5e-5, 5e-5, 1e-4] {
                let mut changed = g.clone();
                changed.q[col] += delta * (1.0 + g.q[col].abs());
                probes.push(changed);
            }
        }
        for (probe_index, probe) in probes.iter().enumerate() {
            let full = art.evaluate(probe).loop_rows;
            let closure = art.evaluate_constraint_rows(probe);
            if full.len() != closure.len()
                || full
                    .iter()
                    .zip(&closure)
                    .any(|(a, b)| !a.is_finite() || a.to_bits() != b.to_bits())
            {
                return Err(format!(
                    "closure mismatch at stage {index}, probe {probe_index}"
                ));
            }
        }
        let mut elapsed = [0.0; 2];
        for repeat in 0..repeats {
            // Alternate order, include allocations, keep results observable.
            for lane in [repeat % 2, 1 - repeat % 2] {
                let start = Instant::now();
                if lane == 0 {
                    black_box(art.evaluate(black_box(&g)));
                } else {
                    black_box(art.evaluate_constraint_rows(black_box(&g)));
                }
                elapsed[lane] += start.elapsed().as_secs_f64();
            }
        }
        reports.push(json!({"attempt":index,"stage_time_s":point.stage_time,
            "committed":point.committed,"rows":expected.len(),"equal_probes":probes.len(),
            "full_mean_s":elapsed[0]/repeats as f64,
            "constraint_only_mean_s":elapsed[1]/repeats as f64}));
    }
    println!("{}", serde_json::to_string_pretty(&json!({
        "capture":args[0],"repeats":repeats,"stages":reports,
        "scope":"Saved stage states and joint-position probes; exact row comparison and kernel timing only. Does not advance or validate a timestep."
    })).map_err(|e| e.to_string())?);
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

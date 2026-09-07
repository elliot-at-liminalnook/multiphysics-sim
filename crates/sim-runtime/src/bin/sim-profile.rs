//! Reproducible native runtime audit. Reports nested solver timers separately
//! from whole-session time and measures reporting costs at a fixed final state.
use serde_json::json;
use sim_runtime::session::{Scene, Session};
use sim_solve::profile;
use sim_dynamics::System;
use std::{hint::black_box, time::Instant};

fn buckets() -> serde_json::Value {
    json!(profile::all()
        .iter()
        .map(|b| json!({
            "name": b.name, "seconds": b.seconds(), "calls": b.calls()
        }))
        .collect::<Vec<_>>())
}
fn per_call_us<T>(repeats: usize, mut f: impl FnMut() -> T) -> f64 {
    for _ in 0..10 {
        black_box(f());
    }
    let start = Instant::now();
    for _ in 0..repeats {
        black_box(f());
    }
    start.elapsed().as_secs_f64() * 1e6 / repeats as f64
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let path = args
        .first()
        .ok_or("usage: sim-profile scene.json [frames=10] [microbenchmark_repeats=1000] [step_breakpoints.json]")?;
    let frames: usize = args
        .get(1)
        .map(|v| v.parse())
        .transpose()
        .map_err(|_| "invalid frames")?
        .unwrap_or(10);
    let repeats: usize = args
        .get(2)
        .map(|v| v.parse())
        .transpose()
        .map_err(|_| "invalid repeats")?
        .unwrap_or(1000);
    if frames == 0 || repeats == 0 {
        return Err("frames and repeats must be positive".into());
    }
    let step_breakpoints_s: Vec<f64> = args.get(3).map(|path| {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())
    }).transpose()?.unwrap_or_default();
    let start = Instant::now();
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let read_s = start.elapsed().as_secs_f64();
    let start = Instant::now();
    let scene: Scene = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let parse_s = start.elapsed().as_secs_f64();
    profile::enable();
    profile::reset();
    let start = Instant::now();
    let mut session = Session::new(scene, 0)?;
    for island in &mut session.robot.runtime.islands {
        island.set_step_breakpoints(step_breakpoints_s.clone()).map_err(|e| e.to_string())?;
    }
    let build_s = start.elapsed().as_secs_f64();
    let build_profile = buckets();
    let action: Vec<_> = session.inputs.iter().map(|i| i.initial).collect();
    let mut frame = session.frame();
    profile::reset();
    let mut timings = Vec::new();
    let start = Instant::now();
    for _ in 0..frames {
        let tick = Instant::now();
        let newton_before = profile::NEWTON.calls();
        let iterations_before = profile::ITERATIONS.calls();
        let jacobians_before = profile::FRESH.calls();
        let jacobian_s_before = profile::JACOBIAN.seconds();
        frame = session.step(&action)?;
        timings.push(json!({"time_s":frame.time_s, "wall_s":tick.elapsed().as_secs_f64(), "contacts":frame.contacts.len(),
            "newton_calls":profile::NEWTON.calls()-newton_before,
            "newton_iterations":profile::ITERATIONS.calls()-iterations_before,
            "fresh_jacobians":profile::FRESH.calls()-jacobians_before,
            "jacobian_s":profile::JACOBIAN.seconds()-jacobian_s_before}));
    }
    let session_step_s = start.elapsed().as_secs_f64();
    let step_profile = buckets();
    // Geometry/contact comparison uses exactly the same generalized state.
    // It is a per-evaluation audit, not an alternate contact-free trajectory.
    let g = session.robot.generalized();
    let full_us = per_call_us(repeats, || session.robot.art.evaluate(&g));
    let no_contact_us = per_call_us(repeats, || session.robot.art.evaluate_with(&g, false));
    let frame_us = per_call_us(repeats, || session.frame());
    let serialize_us = per_call_us(repeats, || serde_json::to_vec(&frame).unwrap());
    let stats: Vec<_> = session
        .robot
        .runtime
        .islands
        .iter()
        .map(|i| &i.stats)
        .collect();
    let sparsity: Vec<_> = session.robot.runtime.islands.iter().map(|i| {
        let p = i.system.sparsity().expect("compiled structural sparsity");
        json!({"unknowns":i.state.len(), "structural_nonzeros":p.rows.iter().map(Vec::len).sum::<usize>(),
            "greedy_colors":p.colours(), "kind":"conservative union of state/rate dependencies for the implicit residual"})
    }).collect();
    let report = json!({
        "version":1, "scene_path":path, "seed":0,
        "scene_options":session.scene.options, "controller_period_s":session.contract.period,
        "integrators":session.robot.runtime.islands.iter().map(|i| i.integrator).collect::<Vec<_>>(),
        "compiled_derivative_worker_capacity":sim_compile::derivative_worker_capacity(),
        "compiled_derivative_batch_columns":sim_core::linearization_batch_columns(sim_compile::derivative_worker_capacity()),
        "requested_rayon_threads":std::env::var("RAYON_NUM_THREADS").ok(),
        "step_breakpoints_s":step_breakpoints_s,
        "action_period_s":session.scene.period_s,
        "links":session.robot.model.links.len(), "actuators":session.contract.actuators.len(),
        "unknowns":session.robot.runtime.islands.iter().map(|i|i.state.len()).sum::<usize>(),
        "articulated_states":session.robot.art.state_count,
        "mechanical_dofs":session.robot.art.dofs().count(),
        "compiled_behaviors":session.robot.runtime.islands.iter().map(|i|i.system.behaviors.len()).sum::<usize>(),
        "read_s":read_s, "parse_s":parse_s, "build_s":build_s, "build_profile":build_profile,
        "simulated_s":frame.time_s, "session_step_s":session_step_s,
        "step_profile":step_profile, "frames":timings, "solver_stats":stats,
        "structural_sparsity":sparsity,
        "event_trace":session.robot.runtime.islands.iter().map(|i| &i.events).collect::<Vec<_>>(),
        "outer_slice_refinements":session.robot.step_refinements,
        "fixed_state_microbenchmarks":{"repeats":repeats, "state_time_s":frame.time_s,
            "equations_with_configured_contact_us":full_us, "equations_without_contact_us":no_contact_us,
            "frame_us":frame_us, "frame_json_us":serialize_us},
        "notes":["Solver timers are nested; do not sum them.",
                 "Worker capacity and maximum batch columns describe scheduling policy, not measured concurrency; serial and small-component paths do not split columns.",
                 "Component FD residual-call counts exclude internal probes in experimental hybrid Jacobians.",
                 "The jumps timer includes controller callbacks and other event handlers.",
                 "Fixed-state contact removal is diagnostic and does not change the simulated trajectory.",
                 "This command does not render or measure CAD UI responsiveness."],
        "final_frame":frame
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

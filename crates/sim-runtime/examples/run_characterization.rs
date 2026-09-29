//! Simulated leg characterization campaign (PLAN.md stages A–K), low fidelity.
//!
//! A hidden "true" leg (the simulated bench with parameters unknown to the
//! campaign) stands in for hardware. The campaign starts from a prior model
//! built from the CAD profile's estimates, rehearses the whole plan on that
//! prior (dry run), runs it on the true leg with per-segment divergence
//! checks, fits every quantity with uncertainty, replays recorded segments
//! through prior and fitted models, and writes promotion outputs, a gait
//! search patch and a test ranking.
//!
//!     run_characterization plan.json output-directory [--no-collision-check]
use serde_json::{Value, json};
use sim_runtime::acquisition::{
    characterization::{self as ch, Plan, SimRig},
    virtual_bench::{Bench, MotorModel},
};
use std::{collections::BTreeMap, path::Path};

const RAD: f64 = std::f64::consts::TAU / 4096.;

/// The simulated leg's hidden truth (knee sticky, belt/hip loaded by gravity,
/// worm with backlash). Close to the identified responses of 2026-09-22.
fn truth() -> [MotorModel; 3] {
    let knee = MotorModel { speed_gain: 2900., lag_s: 0.07, breakaway_duty: 0.14, moving_friction_duty: 0.07, backlash_counts: 4., compliance_counts_per_duty: 20., ..Default::default() };
    let worm = MotorModel { speed_gain: 3290., lag_s: 0.061, breakaway_duty: 0.066, moving_friction_duty: 0.045, backlash_counts: 10., compliance_counts_per_duty: 8., ..Default::default() };
    let belt = MotorModel { speed_gain: 3030., lag_s: 0.068, breakaway_duty: 0.08, moving_friction_duty: 0.06, gravity_duty: 0.03, gravity_zero_counts: 1100., backlash_counts: 6., compliance_counts_per_duty: 40., ..Default::default() };
    [knee, worm, belt]
}
/// The prior: what the CAD profile claims (no-load 5.51 rad/s, gear friction
/// 0.025 N·m of 2.94 N·m stall, no backlash, no gravity model).
fn prior() -> MotorModel {
    MotorModel { speed_gain: 5.5115660589294615 / RAD, lag_s: 0.03, breakaway_duty: 0.0085, moving_friction_duty: 0.0085, ..Default::default() }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        return Err("usage: run_characterization plan.json output-directory [--no-collision-check]".into());
    }
    let mut plan: Plan = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let out = Path::new(&args[1]);
    std::fs::create_dir_all(out)?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    // Stage I reference: the qualified gait baseline's +X leg motors, one
    // period twice, recentred on each axis's middle.
    if plan.i_reference.is_empty() {
        let compiled: Value = serde_json::from_slice(&std::fs::read(root.join("examples/full-robot/measured-actuator-integration/gait-search-comparison-2026-09-19/comparison/2301-CmaEs-000/compiled.json"))?)?;
        let frames = compiled["trajectory"]["keyframes"].as_array().ok_or("no keyframes")?;
        for (id, column) in [(1u8, 5usize), (2, 4), (3, 3)] {
            let Some(axis) = plan.axes.iter().find(|a| a.id == id) else { continue };
            let first = frames[0]["values"][column].as_f64().unwrap();
            let period = frames.last().unwrap()["time_s"].as_f64().unwrap();
            let pts: Vec<(f64, f64)> = (0..2).flat_map(|rep| frames.iter().step_by(16).map(move |f| {
                (f["time_s"].as_f64().unwrap() + rep as f64 * period, f["values"][column].as_f64().unwrap())
            })).map(|(t, q)| (t, axis.center() + (q - first) / RAD)).collect();
            plan.i_reference.push((id, pts));
        }
    }

    // Joint-combination vetting with CAD (kinematic mirror, +X leg).
    let collision = !args.iter().any(|a| a == "--no-collision-check");
    let mirror = if collision {
        let scene: sim_runtime::session::Scene = serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(root.join("examples/full-robot/measured-actuator-integration/browser-control-400hz/scene.json"))?))?;
        Some(std::cell::RefCell::new(sim_runtime::kinematic_mirror::KinematicMirror::new(scene, 0.25)?))
    } else {
        None
    };
    let axes = plan.axes.clone();
    let clear = |combo: &[(u8, f64)]| -> ch::R<()> {
        let Some(m) = &mirror else { return Ok(()) };
        let mut m = m.borrow_mut();
        let coords = m.coordinates();
        let mut q: Vec<f64> = coords.iter().map(|c| c.home).collect();
        for (id, counts) in combo {
            let joint = match id { 1 => "+X | Foot servo output", 2 => "+X | Worm servo output", _ => "+X | Hip servo output" };
            let center = axes.iter().find(|a| a.id == *id).map(|a| a.center()).unwrap_or(2048.);
            if let Some(i) = coords.iter().position(|c| c.joint == joint) {
                q[i] = coords[i].home + (counts - center) * RAD;
            }
        }
        let hits = m.interference(&q)?;
        // 0.5 mm: below that the sampled collision meshes cannot resolve contact.
        let plus_x: Vec<_> = hits.iter().filter(|(a, b, d)| (a.starts_with("+X") || b.starts_with("+X")) && *d > 0.0005).collect();
        if plus_x.is_empty() { Ok(()) } else { Err(format!("{:?}", plus_x)) }
    };

    let bench_at = |models: [MotorModel; 3]| {
        let starts: Vec<f64> = (1..=3u8).map(|id| plan.axes.iter().find(|a| a.id == id).map(|a| a.center()).unwrap_or(2048.)).collect();
        SimRig::new(Bench::new([starts[0], starts[1], starts[2]], models), 0.01)
    };
    let log = |m: &str| eprintln!("{m}");

    // 1. Rehearsal: the whole plan on the prior model (does the script, travel
    //    and gating behave before anything touches the leg?).
    let started = std::time::Instant::now();
    let mut dry = bench_at([prior(), prior(), prior()]);
    let rehearsal = ch::run(&plan, &mut dry, None, &clear, &mut |m| log(&format!("rehearsal {m}")))?;
    // 2. The campaign on the "true" leg, open-loop segments checked against the prior.
    let mut real = bench_at(truth());
    let report = ch::run(&plan, &mut real, Some(prior()), &clear, &mut |m| log(&format!("campaign {m}")))?;
    // 3. Replay recorded segments through prior and fitted models.
    let mut replay = Vec::new();
    for (id, fits) in &report.fitted {
        let fitted = ch::fitted_model(&prior(), fits);
        replay.push(json!({"axis": id,
            "prior_rms_counts": ch::replay_error(&report.samples, *id, &prior(), 0.01),
            "fitted_rms_counts": ch::replay_error(&report.samples, *id, &fitted, 0.01),
            "truth_rms_counts": ch::replay_error(&report.samples, *id, &truth()[(*id - 1) as usize], 0.01)}));
    }
    // 4. Promotion, gait-search patch and test ranking.
    let coordinates: BTreeMap<u8, Vec<String>> = [(1u8, "Foot"), (2, "Worm"), (3, "Hip")].iter().map(|(id, j)| {
        (*id, ["-Y", "+X", "+Y", "-X"].iter().map(|leg| format!("joint.{leg} | {j} servo output")).collect())
    }).collect();
    let prior_profile = json!({"no_load_speed_rad_s": 5.5115660589294615, "gear_friction": 0.025, "backlash": 0.0, "latency": 0.002});
    let promotion = ch::promotion(&report, &prior_profile, &coordinates, "Simulated campaign on the virtual leg (hidden truth); demonstrates the pipeline, not hardware values");
    let ranking = ch::select_tests(&report, &plan.sensitivity);
    let truth_table: Vec<Value> = truth().iter().enumerate().map(|(k, m)| json!({"axis": k + 1, "speed_gain": m.speed_gain, "time_constant": m.lag_s, "moving_friction": m.moving_friction_duty, "breakaway": m.breakaway_duty, "backlash": m.backlash_counts, "compliance": m.compliance_counts_per_duty, "gravity_amplitude": m.gravity_duty, "gravity_zero": m.gravity_zero_counts})).collect();

    let summary = json!({
        "wall_s": started.elapsed().as_secs_f64(),
        "rehearsal": {"simulated_s": rehearsal.simulated_s, "stages": rehearsal.stages.iter().map(|s| json!({"stage": s.stage, "axis": s.id, "completed": s.completed, "abort": s.abort})).collect::<Vec<_>>()},
        "campaign": {"simulated_s": report.simulated_s, "stages": report.stages.iter().map(|s| json!({"stage": s.stage, "axis": s.id, "completed": s.completed, "abort": s.abort, "simulated_s": s.simulated_s})).collect::<Vec<_>>()},
        "fitted": report.fitted, "truth": truth_table, "replay": replay, "ranking": ranking,
        "collision_check": collision,
    });
    std::fs::write(out.join("summary.json"), serde_json::to_vec_pretty(&summary)?)?;
    std::fs::write(out.join("stages.json"), serde_json::to_vec_pretty(&report.stages)?)?;
    std::fs::write(out.join("promotion.json"), serde_json::to_vec_pretty(&promotion)?)?;
    std::fs::write(out.join("rehearsal.json"), serde_json::to_vec_pretty(&rehearsal.stages)?)?;
    serde_json::to_writer(std::io::BufWriter::new(std::fs::File::create(out.join("samples.json"))?), &report.samples)?;
    println!("{}", serde_json::to_string_pretty(&json!({"wall_s": summary["wall_s"], "campaign": summary["campaign"], "replay": replay, "top_ranking": ranking.iter().take(5).collect::<Vec<_>>()}))?);
    Ok(())
}

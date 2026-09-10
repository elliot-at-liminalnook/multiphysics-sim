//! Exact cached/uncached comparison on a supplied CAD path and every coordinate probe.
use serde::Deserialize;
use serde_json::json;
use sim_runtime::{
    contact_implicit::{ContactImplicitConfig, ContactImplicitPlanner},
    session::{Scene, Session},
    tracking::CaptureConfig,
};
use std::time::{Duration, Instant};
#[derive(Deserialize)]
struct Recipe {
    config: ContactImplicitConfig,
}
#[derive(Deserialize)]
struct Path {
    positions: Vec<Vec<f64>>,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 4 {
        return Err("usage: benchmark_contact_implicit scene markers recipe positions".into());
    }
    let scene: Scene = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let markers: CaptureConfig = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let recipe: Recipe = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let path: Path = serde_json::from_slice(&std::fs::read(&args[3])?)?;
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let planner = ContactImplicitPlanner::new(&session.robot.art, &seed, &markers, recipe.config)?;
    let mut cached = planner.evaluator();
    let mut uncached_time = Duration::ZERO;
    let mut cached_time = Duration::ZERO;
    let mut evaluations = 0;
    let n = path.positions[0].len();
    for probe in 0..=2 * (path.positions.len() - 1) * n {
        let mut q = path.positions.clone();
        if probe > 0 {
            let coordinate = (probe - 1) / 2;
            q[1 + coordinate / n][coordinate % n] += if probe % 2 == 1 { 1e-6 } else { -1e-6 };
        }
        let start = Instant::now();
        let expected = planner.evaluate(&q)?;
        uncached_time += start.elapsed();
        let start = Instant::now();
        let actual = cached.evaluate(&q)?;
        cached_time += start.elapsed();
        if serde_json::to_vec(&actual)? != serde_json::to_vec(&expected)? {
            return Err(format!("cached/uncached mismatch at probe {probe}").into());
        }
        evaluations += 1;
    }
    println!(
        "{}",
        json!({"evaluations":evaluations,"exact_serialized_equality":true,
        "uncached_seconds":uncached_time.as_secs_f64(),"cached_seconds":cached_time.as_secs_f64(),
        "speedup":uncached_time.as_secs_f64()/cached_time.as_secs_f64(),
        "computed_frames":cached.computed_frames,"reused_frames":cached.reused_frames,
        "scope":"One baseline and both signs of every future coordinate finite-difference probe. Timed evaluation only; serialization/equality excluded. Single-process wall time is not an MPC or realtime claim."})
    );
    Ok(())
}

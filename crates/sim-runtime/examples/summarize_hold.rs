//! Diagnose a completed integrate_embedding capture with shared pose metrics.
use serde_json::{Value, json};
use sim_runtime::{
    posture::{HoldConfig, summarize_hold},
    session::EpisodeFrame,
};
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: summarize_hold capture.json hold-config.json".into());
    }
    let read = |p: &str| -> Result<Value, String> {
        serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    };
    let d = read(&args[0])?;
    let config: HoldConfig = serde_json::from_value(read(&args[1])?).map_err(|e| e.to_string())?;
    let frames = d["frames"]
        .as_array()
        .ok_or("missing frames")?
        .iter()
        .map(|f| -> Result<_, String> {
            Ok(EpisodeFrame {
                time_s: f["time_s"].as_f64().ok_or("missing frame time")?,
                done: false,
                poses: serde_json::from_value(f["poses"].clone()).map_err(|e| e.to_string())?,
                joint_positions: vec![],
                telemetry: Default::default(),
                contacts: vec![],
                error: None,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let report = summarize_hold(
        &frames,
        d["completed"] == true && d["error"].is_null(),
        d["source"]["cad_sha256"].as_str(),
        &config,
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"source":d["source"],"config":config,"report":report})
        )
        .map_err(|e| e.to_string())?
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

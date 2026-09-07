//! Capture CAD marker motion or compare imported tracking evidence.
use sim_runtime::{
    session::Recording,
    tracking::{
        CaptureConfig, TrackingEvidence, TrackingRequirements, capture_tracking, compare_tracking,
    },
};
fn read<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    serde_json::from_slice(&std::fs::read(path).map_err(|e| format!("{path}: {e}"))?)
        .map_err(|e| format!("{path}: {e}"))
}
fn run() -> Result<bool, String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("capture") if args.len() == 3 => {
            let recording: Recording = read(&args[1])?;
            let config: CaptureConfig = read(&args[2])?;
            let report = capture_tracking(&recording, &config)?;
            println!("{}", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?);
            Ok(true)
        }
        Some("compare") if args.len() == 4 => {
            let candidate: TrackingEvidence = read(&args[1])?;
            let reference: TrackingEvidence = read(&args[2])?;
            let requirements: TrackingRequirements = read(&args[3])?;
            let report = compare_tracking(&candidate, &reference, &requirements)?;
            println!("{}", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?);
            Ok(report.passed)
        }
        _ => Err("usage: sim-track capture recording.json markers.json | compare candidate.json reference.json requirements.json".into()),
    }
}
fn main() {
    match run() {
        Ok(true) => (),
        Ok(false) => std::process::exit(1),
        Err(error) => {
            eprintln!("{}", serde_json::json!({"passed":false,"error":error}));
            std::process::exit(2);
        }
    }
}

//! Prepare a reference variant through shared Rust trajectory algebra.
use serde::Deserialize;
use sim_domain_control::trajectory::{Trajectory, TrajectoryConfig};
use std::fs;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Transform {
    scales: Vec<f64>,
    centers: Vec<f64>,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err(
            "usage: affine_trajectory reference.json transform.json fresh-output.json".into(),
        );
    }
    let reference: TrajectoryConfig = serde_json::from_slice(&fs::read(&args[0])?)?;
    let transform: Transform = serde_json::from_slice(&fs::read(&args[1])?)?;
    let result =
        Trajectory::new(reference)?.affine_values(&transform.scales, &transform.centers)?;
    let output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[2])?;
    serde_json::to_writer(output, &result)?;
    Ok(())
}

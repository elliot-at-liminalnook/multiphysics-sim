//! Rotate authored controls in shared Rust, retaining exact curve shape.
use sim_domain_control::trajectory::{Trajectory, TrajectoryConfig};
use std::fs;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err(
            "usage: shift_periodic_trajectory reference.json integer-shifts.json fresh-output.json"
                .into(),
        );
    }
    let config: TrajectoryConfig = serde_json::from_slice(&fs::read(&args[0])?)?;
    let shifts: Vec<i64> = serde_json::from_slice(&fs::read(&args[1])?)?;
    let output = Trajectory::new(config)?.shifted_periodic_controls(&shifts)?;
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[2])?;
    serde_json::to_writer(file, &output)?;
    Ok(())
}

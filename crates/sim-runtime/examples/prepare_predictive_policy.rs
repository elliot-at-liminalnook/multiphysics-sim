//! Assemble a causal forecast bundle and extend an actor without changing its
//! output. The generic runtime supplies all observation construction/inference.
use sim_runtime::{embedded::Config, predictive_policy::ForecastBundle};
use std::{fs, io::Write};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 3 {
        return Err("usage: prepare_predictive_policy config.json new-config.json horizon-model.json ... (in increasing horizon order)".into());
    }
    let mut config: Config = serde_json::from_slice(&fs::read(&args[0])?)?;
    let heads = args[2..]
        .iter()
        .map(|p| Ok(serde_json::from_slice(&fs::read(p)?)?))
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    let bundle = ForecastBundle { version: 1, heads };
    bundle.validate()?;
    let policy = config.policy.as_mut().ok_or("explicit policy required")?;
    if policy.trajectory_forecast.is_some() {
        return Err("config already has a trajectory forecaster".into());
    }
    policy.neural_residual = Some(
        bundle.augment_actor(
            policy
                .neural_residual
                .as_ref()
                .ok_or("explicit actor required")?,
        )?,
    );
    policy.trajectory_forecast = Some(bundle);
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[1])?;
    serde_json::to_writer(&mut output, &config)?;
    output.write_all(b"\n")?;
    Ok(())
}

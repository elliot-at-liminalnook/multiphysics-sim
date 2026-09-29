//! Prepare one gait-search candidate (default: the config's baseline) through
//! the shared contact-exploration recipe and write its experiment spec, e.g.
//! as the input to `reduced_exploration prepare` for a fresh qualification.
//!
//!     prepare_gait_candidate comparison-config.json out-spec.json [values.json]
use serde_json::Value;
use sim_runtime::contact_exploration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !(2..=3).contains(&args.len()) {
        return Err("usage: prepare_gait_candidate comparison-config.json out-spec.json [values.json]".into());
    }
    let config: Value = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let mut recipe: contact_exploration::Recipe = serde_json::from_value(config["recipe"].clone())?;
    // Same actuator sync as the comparison host, so the baseline matches.
    recipe.sync_actuators(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))?;
    let values = match args.get(2) {
        Some(p) => serde_json::from_slice(&std::fs::read(p)?)?,
        None => serde_json::from_value(config["baseline"].clone())?,
    };
    let prepared = recipe.prepare(&values)?;
    let out = std::fs::OpenOptions::new().write(true).create_new(true).open(&args[1])?;
    serde_json::to_writer(std::io::BufWriter::new(out), &prepared.spec)?;
    println!("{}", serde_json::to_string(&prepared.screen)?);
    Ok(())
}

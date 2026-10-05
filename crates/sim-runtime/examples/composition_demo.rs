//! The composition demo: builds the thermostat room and the rover system
//! (`sim_runtime::composition_examples`) in a directory, runs both
//! headlessly through the same session Build mode uses, and prints what
//! happened. Open either file afterwards in the app:
//!
//! ```text
//! cargo run -p sim-runtime --example composition_demo -- target/composition-demo
//! cargo run -p sim-spatial -- --system target/composition-demo/rover.system.json
//! ```
use sim_runtime::{composition_examples, registry, system_builder};
use std::path::PathBuf;

fn summary(series: &[system_builder::Series], key: &str) -> String {
    let Some(s) = series.iter().find(|s| s.label.contains(key)) else { return format!("{key}: not recorded") };
    let (lo, hi) = s.values.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    format!("{:<28} first {:>10.4}  last {:>10.4}  min {:>10.4}  max {:>10.4} {}", s.label, s.values.first().unwrap_or(&f64::NAN), s.values.last().unwrap_or(&f64::NAN), lo, hi, s.unit)
}

fn main() -> Result<(), String> {
    let dir = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("target/composition-demo"));
    let dir = std::path::absolute(&dir).map_err(|e| e.to_string())?;
    let registry = registry();

    let room = composition_examples::thermostat(&dir, &registry)?;
    let document = sim_system::SystemStore::new(&room).load().map_err(|e| e.to_string())?;
    let config = system_builder::config_for(&document);
    let series = system_builder::simulate_at(&document, &registry, &dir, 3600.0, config, &["thermometer.temperature".into(), "thermostat.heater_power".into()])?;
    println!("{}  (1 h simulated)", room.display());
    for key in ["thermometer.temperature", "thermostat.heater_power"] {
        println!("  {}", summary(&series, key));
    }

    let rover = composition_examples::rover(&dir, &registry)?;
    let document = sim_system::SystemStore::new(&rover).load().map_err(|e| e.to_string())?;
    let config = system_builder::config_for(&document);
    let keys = ["left_controller.target", "right_controller.target", "rover/joint.left axle.encoder.angle", "rover/joint.right axle.encoder.angle", "battery.soc", "enclosure.node"];
    let series = system_builder::simulate_at(&document, &registry, &dir, 2.0, config, &keys.map(String::from))?;
    println!("{}  (2 s simulated)", rover.display());
    for key in keys {
        println!("  {}", summary(&series, key));
    }
    Ok(())
}

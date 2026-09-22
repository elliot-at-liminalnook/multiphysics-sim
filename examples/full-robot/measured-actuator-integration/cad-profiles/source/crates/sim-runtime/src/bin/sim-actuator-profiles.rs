//! Read-only CAD profile validation using the same registry and resolver as simulation.
use std::io::{Read, Write};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = sim_runtime::registry();
    if std::env::args().nth(1).as_deref() == Some("catalog") {
        println!("{}", sim_script::catalogue(&registry));
        return Ok(());
    }
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let mut model: sim_domain_robot::PhysicalModel = serde_json::from_str(&input)?;
    model.resolve_actuator_profiles(&registry)?;
    let resolved: std::collections::BTreeMap<_, _> = model
        .motors
        .iter()
        .filter_map(|m| m.resolved_actuator.as_ref().map(|p| (&m.id, p)))
        .collect();
    std::io::stdout().write_all(&serde_json::to_vec(&serde_json::json!({
        "version":1,"resolved":resolved,"calibrated":false,
        "interpretation":"Declaration/registry validation only. Evidence references are retained, not certification of measured accuracy."
    }))?)?;
    Ok(())
}

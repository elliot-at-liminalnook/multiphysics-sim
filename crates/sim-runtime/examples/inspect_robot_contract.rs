//! Read-only CAD contract inspection; no simulation or physical defaults authored.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: inspect_robot_contract model-or-scene.json")?;
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let contract = sim_runtime::robot_contract::inspect(value)?;
    println!("{}", serde_json::to_string_pretty(&contract)?);
    if contract.robot.has_errors() {
        std::process::exit(1);
    }
    Ok(())
}

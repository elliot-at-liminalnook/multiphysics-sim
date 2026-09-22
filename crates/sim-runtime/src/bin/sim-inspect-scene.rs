//! Capture the shared CAD assembly as a portable systems description.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: sim-inspect-scene scene.json description.json".into());
    }
    let bytes = std::fs::read(&args[0])?;
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let scene: sim_runtime::session::Scene = serde_json::from_slice(&bytes)?;
    let registry = sim_runtime::registry();
    let revision = scene
        .robot
        .source
        .get("cad_revision")
        .and_then(|v| v.as_u64())
        .unwrap_or(1);
    eprintln!("Building captured CAD system without advancing simulation…");
    let robot = sim_runtime::PhysicalRobot::build(scene.robot, &registry, &scene.options)?;
    let description =
        sim_runtime::system_inspection::describe_physical(&robot, &registry, &hash, revision)?;
    let encoded = serde_json::to_vec_pretty(&description)?;
    // The output is a separate inspection artifact. Source CAD and scene stay untouched.
    let path = std::path::Path::new(&args[1]);
    let temp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&temp, encoded)?;
    std::fs::rename(&temp, path)?;
    eprintln!(
        "{} components, {} ports, {} connections, {} groups → {}",
        description.components.len(),
        description.ports.len(),
        description.nets.len(),
        description.groups.len(),
        path.display()
    );
    Ok(())
}

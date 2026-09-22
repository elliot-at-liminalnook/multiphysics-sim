//! File adapter for the shared reference compiler.
use sim_runtime::{
    contact_reference::{self, Recipe},
    session::Scene,
    tracking::CaptureConfig,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("usage: compile_contact_reference scene.json markers.json recipe.json".into());
    }
    let scene: Scene = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let markers: CaptureConfig = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let recipe: Recipe = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    println!(
        "{}",
        serde_json::to_string(&contact_reference::compile(scene, markers, recipe)?)?
    );
    Ok(())
}

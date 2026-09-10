//! Exact numerical basis refinement through shared trajectory components.
use sim_runtime::{
    contact_implicit::{ContactImplicitConfig, ContactImplicitPlanner},
    session::{Scene, Session},
    tracking::CaptureConfig,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 4 {
        return Err("usage: refine_contact_periodic scene markers recipe result".into());
    }
    let scene: Scene = serde_json::from_slice(&std::fs::read(&a[0])?)?;
    let markers: CaptureConfig = serde_json::from_slice(&std::fs::read(&a[1])?)?;
    let recipe: serde_json::Value = serde_json::from_slice(&std::fs::read(&a[2])?)?;
    let result: serde_json::Value = serde_json::from_slice(&std::fs::read(&a[3])?)?;
    let config: ContactImplicitConfig = serde_json::from_value(recipe["config"].clone())?;
    let controls: Vec<Vec<f64>> = serde_json::from_value(result["positions"].clone())?;
    let mut session = Session::new(scene, 0)?;
    session.robot.art.contact_on = false;
    let seed = session.robot.generalized();
    let planner = ContactImplicitPlanner::new(&session.robot.art, &seed, &markers, config)?;
    let positions = planner.refine_periodic_controls(&controls)?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({"positions":positions,
        "scope":"Shared periodic cubic knot insertion doubles controls without changing the position, velocity or acceleration curve, up to floating-point roundoff. Caller must halve control step and update reference/bounds explicitly. No physical properties changed."}))?
    );
    Ok(())
}

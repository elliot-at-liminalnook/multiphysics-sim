//! Materialize a replayable motion candidate without changing physics or stepping.
use sim_domain_control::motion_parameters::Values;
use sim_runtime::{motion_parameters::MotionParameterization, session::Scene};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 5 {
        return Err("usage: materialize_motion scene.json actions.json parameterization.json values.json fresh-output.json".into());
    }
    let scene: Scene = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let actions = serde_json::from_slice::<Vec<Vec<f64>>>(&std::fs::read(&args[1])?)?;
    let recipe: MotionParameterization = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let values: Values = serde_json::from_slice(&std::fs::read(&args[3])?)?;
    let variant = recipe.materialize(&scene, &actions, &values)?;
    let mut output = std::io::BufWriter::new(
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[4])?,
    );
    serde_json::to_writer(
        &mut output,
        &serde_json::json!({"variant":variant,"metadata":recipe.metadata()}),
    )?;
    std::io::Write::flush(&mut output)?;
    Ok(())
}

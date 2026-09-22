fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args()
        .nth(1)
        .ok_or("usage: sim-example-diffusion description.json")?;
    let mut registry = sim_core::BehaviorRegistry::default();
    sim_example_diffusion::register(&mut registry)?;
    let model = sim_example_diffusion::relaxation(&registry)?;
    let capture = sim_inspect::model::describe(
        &model,
        &registry,
        "example.diffusion.relaxation.v1",
        1,
        &sim_inspect::model::IdentityBindings::default(),
    )?;
    let bytes = serde_json::to_vec_pretty(&capture.description)?;
    std::fs::write(output, bytes)?;
    Ok(())
}

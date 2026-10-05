//! Reproduce the spatial preview from the retained numerical baseline.
//! Geometry is separately authored presentation; no physical model is modified.
use sim_inspect::{
    SourceReference,
    model::{ComponentIdentity, IdentityBindings, describe},
};
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source_path = "examples/systems-viewer/evidence/pre-migration-baseline.json";
    let capture_bytes = fs::read(root.join(source_path))?;
    let artifact_hash = blake3::hash(&capture_bytes).to_hex().to_string();
    let captured: serde_json::Value = serde_json::from_slice(&capture_bytes)?;
    let case = captured["cases"]
        .as_array()
        .ok_or("missing cases")?
        .iter()
        .find(|c| c["name"] == "motor_composite")
        .ok_or("missing motor fixture")?;
    let bytes = serde_json::to_vec(&case["model"])?;
    let source_hash = blake3::hash(&bytes).to_hex().to_string();
    let model: sim_core::ModelWorld = serde_json::from_slice(&bytes)?;
    let mut identities = IdentityBindings::default();
    for (key, behavior) in &model.behaviors {
        let name = &model.objects[behavior.object].name;
        identities.components.insert(
            key,
            ComponentIdentity {
                id: format!("example/motor-thermal/{name}"),
                persistent: true,
                group: None,
                cad: None,
                source: Some(SourceReference {
                    artifact_hash: artifact_hash.clone(),
                    path: source_path.into(),
                    line: None,
                }),
            },
        );
    }
    let description = describe(
        &model,
        &sim_runtime::registry(),
        &source_hash,
        1,
        &identities,
    )?
    .description;
    let output = root.join("examples/systems-viewer/spatial");
    let mut spatial: sim_inspect::spatial::SpatialDescription =
        serde_json::from_slice(&fs::read(output.join("motor-thermal.spatial.json"))?)?;
    spatial.description_id = description.id.clone();
    spatial.validate(&description)?;
    fs::write(
        output.join("motor-thermal.description.json"),
        serde_json::to_vec_pretty(&description)?,
    )?;
    fs::write(
        output.join("motor-thermal.spatial.json"),
        serde_json::to_vec_pretty(&spatial)?,
    )?;
    fs::write(
        output.join("motor-thermal.model.json"),
        serde_json::to_vec_pretty(&model)?,
    )?;
    let mut launch = sim_runtime::system_worker::Launch {
        version: 1,
        run_id: "motor-thermal-preview".into(),
        model,
        source_hash: source_hash.clone(),
        revision: 1,
        config: sim_runtime::system_session::SessionConfig {
            interval: 0.01,
            integrator: sim_dynamics::Integrator::implicit_midpoint(),
            seed: 71,
            grid_snapping: false,
        },
        binding: None,
        base: None,
    };
    launch.binding = Some(sim_runtime::system_worker::SourceBinding {
        model_hash: launch.model_hash()?,
        description_id: description.id.clone(),
        identities,
    });
    launch.validate_binding(&sim_runtime::registry())?;
    fs::write(
        output.join("motor-thermal.live.json"),
        serde_json::to_vec_pretty(&launch)?,
    )?;
    println!(
        "Exported {} components, {} ports, {} nets, {} display parts; source {}",
        description.components.len(),
        description.ports.len(),
        description.nets.len(),
        spatial.parts.len(),
        source_hash
    );
    Ok(())
}

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    about = "Inspect a source-bound spatial assembly. Shared live observations; no physics stepping."
)]
struct Args {
    /// Shared discussion and saved-view sidecar.
    #[arg(long)]
    annotations: Option<PathBuf>,
    /// Loopback REST port (0 chooses a free port).
    #[arg(long, default_value_t = 8421)]
    api_port: u16,
    /// Run the command adapter without creating a window.
    #[arg(long, conflicts_with = "schematic")]
    headless: bool,
    #[arg(long)]
    description: Option<PathBuf>,
    #[arg(long)]
    spatial: Option<PathBuf>,
    /// Select a represented component by its shared source ID.
    #[arg(long)]
    select: Option<String>,
    #[arg(long)]
    exploded: bool,
    #[arg(long)]
    connections: bool,
    /// Open the matching schematic and link source selections.
    #[arg(long, conflicts_with = "selection_link")]
    schematic: bool,
    /// Enable process-backed simulation and connection graphs in the schematic.
    #[arg(long, requires = "schematic")]
    live: Option<PathBuf>,
    /// Spatial observation bindings for a linked live capture.
    #[arg(long)]
    animation: Option<PathBuf>,
    /// Join an existing ephemeral selection session.
    #[arg(long)]
    selection_link: Option<PathBuf>,
    #[arg(long)]
    compact: bool,
    /// Validate inputs without opening a window.
    #[arg(long)]
    validate_only: bool,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if !args.validate_only
        && args.animation.is_some()
        && args.selection_link.is_none()
        && !(args.schematic && args.live.is_some())
    {
        return Err("--animation requires --selection-link or --schematic --live".into());
    }
    if args.description.is_some() != args.spatial.is_some() {
        return Err("provide both --description and --spatial".into());
    }
    let base =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/systems-viewer/spatial");
    let description_path = args
        .description
        .unwrap_or_else(|| base.join("motor-thermal.description.json"));
    let description: sim_inspect::SystemDescription =
        serde_json::from_slice(&std::fs::read(&description_path)?)?;
    let spatial_path = args
        .spatial
        .unwrap_or_else(|| base.join("motor-thermal.spatial.json"));
    let spatial = serde_json::from_slice(&std::fs::read(&spatial_path)?)?;
    let mut scene = sim_spatial::SpatialScene::new(description, spatial)?;
    if let Some(path) = &args.animation {
        scene.set_animation(serde_json::from_slice(&std::fs::read(path)?)?)?;
    }
    use sim_inspect::spatial::SpatialCommand;
    if let Some(component) = args.select {
        scene.apply(SpatialCommand::Select { component })?;
    }
    scene.apply(SpatialCommand::SetExploded {
        enabled: args.exploded,
    })?;
    scene.apply(SpatialCommand::SetConnections {
        enabled: args.connections,
    })?;
    if args.validate_only {
        println!(
            "Validated {} spatial parts against {} components (description {}).",
            scene.spatial.parts.len(),
            scene.description.components.len(),
            scene.description.id
        );
        return Ok(());
    }
    let annotation_path = args.annotations.clone().unwrap_or_else(|| {
        PathBuf::from(format!("{}.annotations.json", description_path.display()))
    });
    scene.connect_annotations(annotation_path.clone());
    scene.compact = args.compact || args.schematic;
    let session = if args.schematic {
        Some(sim_inspect::selection::native::create_session(
            &scene.description,
            scene.selection.clone(),
        )?)
    } else {
        args.selection_link
    };
    let link = if let Some(directory) = session {
        eprintln!("Selection session: {}", directory.display());
        let client = sim_inspect::selection::native::SelectionClient::connect(
            std::sync::Arc::new(scene.description.clone()),
            directory.clone(),
            sim_inspect::selection::native::Peer::Assembly,
            scene.selection.clone(),
        )?;
        if args.schematic {
            let sibling = std::env::current_exe()?.with_file_name("sim-viewer");
            let mut command = std::process::Command::new(sibling);
            command
                .arg("--description")
                .arg(&description_path)
                .arg("--annotations")
                .arg(&annotation_path);
            if let Some(path) = &args.live {
                command.arg("--live").arg(path);
                if let Some(animation) = &args.animation {
                    command
                        .arg("--animation")
                        .arg(animation)
                        .arg("--spatial")
                        .arg(&spatial_path);
                }
            }
            let mut child = command
                .arg("--selection-link").arg(&directory).arg("--compact")
                .spawn().map_err(|e| format!("Could not open schematic: {e}. Run examples/systems-viewer/run-linked.sh to build both viewers."))?;
            // Reap the companion when it exits, without tying its lifetime to ours.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        if scene.animation.is_some() {
            scene.connect_live(directory);
        }
        Some(sim_spatial::SelectionLink(client))
    } else {
        None
    };
    let api = sim_spatial::rest::server(args.api_port)?;
    eprintln!("Physical REST: http://{}", api.address);
    if args.headless {
        sim_spatial::rest::headless(scene, link, api);
    }
    sim_spatial::run_with_api(scene, link, Some(api));
    Ok(())
}

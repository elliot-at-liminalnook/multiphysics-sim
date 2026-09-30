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
    /// Build mode: edit and run this `sim.system/1` file.
    #[arg(long, conflicts_with_all = ["description", "spatial", "live", "animation", "selection_link"])]
    system: Option<PathBuf>,
    /// Saved subsystem definitions offered in the palette (build mode).
    #[arg(long, default_value = "library/systems")]
    library: PathBuf,
    /// Display-model catalog (CAD-exported OBJ). Defaults to `models` next to
    /// the library directory.
    #[arg(long)]
    models: Option<PathBuf>,
    /// Validate inputs without opening a window.
    #[arg(long)]
    validate_only: bool,
    /// Lesson mode: read the lessons in DIR (each `<slug>/lesson.md`),
    /// with live scenes, notes and the builder one click away.
    #[arg(long, conflicts_with_all = ["description", "spatial", "live", "animation", "selection_link", "system"])]
    lessons: Option<PathBuf>,
    /// Walk through a scanned place (a `sim-place build` directory).
    #[arg(long, conflicts_with_all = ["description", "spatial", "live", "animation", "selection_link", "system", "lessons"])]
    place: Option<PathBuf>,
    /// Lesson to open first (slug); default: the first in reading order.
    #[arg(long, requires = "lessons")]
    lesson: Option<String>,
}

fn lessons_mode(args: &Args, dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let registry = sim_runtime::system_registry();
    let mut learn = sim_spatial::lesson::Learn::new(dir.to_path_buf(), args.library.clone(), registry.clone());
    if args.validate_only {
        for e in &learn.entries {
            match &e.error {
                Some(err) => println!("{}: {err}", e.slug),
                None => println!("{}: {} ({} scenes)", e.slug, e.title, e.scenes),
            }
        }
        return Ok(());
    }
    let slug = args.lesson.clone().or_else(|| learn.entries.iter().find(|e| e.error.is_none()).map(|e| e.slug.clone()));
    // The builder starts on the first scene's sandbox (or an empty system);
    // opening the lesson then activates that scene off the UI thread.
    let first = slug.as_ref().and_then(|s| {
        let lesson = sim_lesson::Lesson::load(&dir.join(s).join("lesson.md")).ok()?;
        let scene = lesson.scenes().next().map(|(_, sc)| sc.clone())?;
        sim_runtime::lesson::sandbox(&lesson, &scene, &registry, false).ok().map(|sb| sb.path)
    });
    let initial = match first {
        Some(p) => p,
        None => {
            let p = sim_runtime::lesson::sandbox_root().join("_empty").join("empty.system.json");
            if !p.exists() {
                sim_system::SystemStore::create(&p, &sim_system::SystemDocument::new("Lesson"))?;
            }
            p
        }
    };
    let builder = sim_spatial::Builder::open(initial, args.library.clone(), registry.clone())?;
    let compiled = sim_runtime::system_builder::compile(&builder.document, &registry, sim_runtime::system_builder::config_for(&builder.document))?;
    let spatial = compiled.spatial.clone().unwrap_or_else(|| compiled.flat.spatial(&compiled.description.id, &builder.document.title));
    let mut scene = sim_spatial::SpatialScene::for_builder(compiled.description.clone(), spatial)?;
    if let Some(animation) = compiled.animation.clone() {
        scene.set_animation(animation)?;
    }
    if let Some(slug) = &slug {
        if let Err(e) = learn.open(slug) {
            eprintln!("Lesson {slug}: {e}");
        }
    }
    let api = sim_spatial::rest::server_for(args.api_port, true, true)?;
    eprintln!("Physical REST (lessons): http://{}", api.address);
    let models = sim_spatial::models::ModelLibrary::open(args.models.clone().unwrap_or_else(|| args.library.parent().unwrap_or(std::path::Path::new(".")).join("models")));
    if let Some(e) = &models.error {
        eprintln!("Display models unavailable ({e}); drawing bounding shapes.");
    }
    sim_spatial::run_lessons(scene, builder, learn, api, models);
    Ok(())
}
fn build_mode(args: &Args, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let registry = sim_runtime::system_registry();
    let mut builder = sim_spatial::Builder::open(path.to_path_buf(), args.library.clone(), registry.clone())?;
    let compiled = sim_runtime::system_builder::compile(&builder.document, &registry, sim_runtime::system_builder::config_for(&builder.document))?;
    let spatial = compiled.spatial.clone().unwrap_or_else(|| compiled.flat.spatial(&compiled.description.id, &builder.document.title));
    let mut scene = sim_spatial::SpatialScene::for_builder(compiled.description.clone(), spatial)?;
    if let Some(animation) = compiled.animation.clone() {
        scene.set_animation(animation)?;
    }
    if args.validate_only {
        println!("Validated {} with {} components.", path.display(), scene.description.components.len());
        return Ok(());
    }
    scene.compact = args.compact;
    let annotation_path = args.annotations.clone().unwrap_or_else(|| PathBuf::from(format!("{}.annotations.json", path.display())));
    scene.connect_annotations(annotation_path);
    if args.schematic {
        let sibling = std::env::current_exe()?.with_file_name("sim-viewer");
        let mut child = std::process::Command::new(sibling)
            .arg("--system")
            .arg(path)
            .arg("--compact")
            .spawn()
            .map_err(|e| format!("Could not open the schematic: {e}. Build sim-viewer first."))?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
    let api = sim_spatial::rest::server_with(args.api_port, true)?;
    eprintln!("Physical REST (build mode): http://{}", api.address);
    let models_dir = args.models.clone().unwrap_or_else(|| args.library.parent().unwrap_or(std::path::Path::new(".")).join("models"));
    let mut models = sim_spatial::models::ModelLibrary::open(models_dir.clone());
    // A system's own display models (CAD exports next to the file) join the shared catalog.
    models.extend(&path.parent().unwrap_or(std::path::Path::new(".")).join("models"));
    if let Some(e) = &models.error {
        eprintln!("Display models unavailable ({e}); drawing bounding shapes.");
    }
    // Opening another file in this window needs these launch facts.
    builder.enable_open(sim_spatial::builder::open::Shell { launch: path.to_path_buf(), annotations: args.annotations.clone(), schematic: args.schematic.then(|| path.to_path_buf()), models: models_dir });
    sim_spatial::run_builder(scene, builder, api, models);
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if let Some(dir) = args.place.clone() {
        return sim_spatial::place_view::run_place(dir).map_err(Into::into);
    }
    if let Some(dir) = args.lessons.clone() {
        return lessons_mode(&args, &dir);
    }
    if let Some(path) = args.system.clone() {
        return build_mode(&args, &path);
    }
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
    let models = sim_spatial::models::ModelLibrary::open(args.models.clone().unwrap_or_else(|| PathBuf::from("library/models")));
    sim_spatial::run_with_api(scene, link, Some(api), models.error.is_none().then_some(models));
    Ok(())
}

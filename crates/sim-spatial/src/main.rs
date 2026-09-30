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
    /// Saved subsystem definitions offered in the palette (build and lesson
    /// modes; authored parts load from `parts` next to it). Default:
    /// `<workspace>/library/systems`. An explicit path is relative to the
    /// current directory.
    #[arg(long)]
    library: Option<PathBuf>,
    /// Workspace root for repository data (part registry, library, models,
    /// web/viewer/presets.json, runs/ outputs). Default: $SIM_WORKSPACE, else
    /// the nearest ancestor of the opened file, else of the current directory,
    /// holding a Cargo.toml with a [workspace] table next to a library/
    /// directory. `$SIM_PARTS_DIR` still overrides the parts directory.
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// Display-model catalog (CAD-exported OBJ). Defaults to `models` next to
    /// the library directory (`<workspace>/library/models`).
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
    /// Robot mode: open a CAD-exported `.simrobot.json` read-only (links
    /// drawn at the assembly pose; nothing is stepped or written).
    #[arg(long, conflicts_with_all = ["description", "spatial", "live", "animation", "selection_link", "system", "lessons", "place", "headless", "schematic"])]
    robot: Option<PathBuf>,
    /// Robot mode on a preset declared in `--robot-presets` (default
    /// `<workspace>/web/viewer/presets.json`; the preset's paths and its
    /// recordings under runs/robot-presets resolve against the workspace
    /// root): its scene, controller config and optional task run by the shared
    /// EmbeddedEnvironment/EmbeddedSession. With --validate-only, lists the
    /// presets and parses this one's inputs.
    #[arg(long, conflicts_with_all = ["description", "spatial", "live", "animation", "selection_link", "system", "lessons", "place", "headless", "schematic", "robot"])]
    robot_preset: Option<String>,
    /// The preset list read by --robot-preset and REST robot_presets/robot_preset.
    /// Default: `<workspace>/web/viewer/presets.json`. An explicit path is
    /// relative to the current directory.
    #[arg(long)]
    robot_presets: Option<PathBuf>,
    /// Lesson to open first (slug); default: the first in reading order.
    #[arg(long, requires = "lessons")]
    lesson: Option<String>,
}

/// The palette library: explicit --library, else `<workspace>/library/systems`
/// (an error naming the searched directories when no root was found).
fn library(args: &Args) -> Result<PathBuf, String> {
    match &args.library {
        Some(dir) => Ok(dir.clone()),
        None => sim_spatial::workspace::path("library/systems").map_err(|e| format!("{e} (or pass --library DIR)")),
    }
}
/// The display-model catalog: explicit --models, else `models` next to the library.
fn models_dir(args: &Args, library: &std::path::Path) -> PathBuf {
    args.models.clone().unwrap_or_else(|| library.parent().unwrap_or(std::path::Path::new(".")).join("models"))
}
/// Parts from $SIM_PARTS_DIR, else `<workspace>/library/parts`.
fn registry() -> sim_core::BehaviorRegistry {
    sim_runtime::system_registry_in(sim_spatial::workspace::get().as_ref())
}
/// The preset list: explicit --robot-presets, else `<workspace>/web/viewer/presets.json`.
fn presets(args: &Args) -> Result<PathBuf, String> {
    args.robot_presets.clone().map(Ok).unwrap_or_else(sim_spatial::robot_preset::default_file)
}

fn lessons_mode(args: &Args, dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let registry = registry();
    let library = library(args)?;
    let mut learn = sim_spatial::lesson::Learn::new(dir.to_path_buf(), library.clone(), registry.clone());
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
    let builder = sim_spatial::Builder::open(initial, library.clone(), registry.clone())?;
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
    let models = sim_spatial::models::ModelLibrary::open(models_dir(args, &library));
    if let Some(e) = &models.error {
        eprintln!("Display models unavailable ({e}); drawing bounding shapes.");
    }
    sim_spatial::run_lessons(scene, builder, learn, api, models);
    Ok(())
}
fn robot_mode(args: &Args, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    if args.validate_only {
        let loaded = sim_spatial::robot::load(path)?;
        let drawn = loaded.geometry.iter().filter(|g| g.is_some()).count();
        println!("Validated {} with {} links ({drawn} with collision geometry).", path.display(), loaded.model.links.len());
        return Ok(());
    }
    let api = sim_spatial::robot::server(args.api_port)?;
    eprintln!("Physical REST (robot mode): http://{}", api.address);
    sim_spatial::robot::run_robot(sim_spatial::robot::RobotView::open(path.to_path_buf()).with_presets(args.robot_presets.clone()), api);
    Ok(())
}
fn robot_preset_mode(args: &Args, id: &str) -> Result<(), Box<dyn std::error::Error>> {
    if args.validate_only {
        let root = sim_spatial::workspace::root()?;
        let presets = presets(args)?;
        for p in sim_spatial::robot_preset::list(&presets)? {
            let d = p.discovery(&root);
            println!("{} · mode {} · inputs exist {} · {}", p.id, p.mode, d["inputs_exist"], d["not_openable_reason"].as_str().unwrap_or("openable (build not attempted)"));
        }
        let preset = sim_spatial::robot_preset::select(&presets, root, id)?;
        let (loaded, run) = sim_spatial::robot::load_preset(preset, root)?;
        let drawn = loaded.geometry.iter().filter(|g| g.is_some()).count();
        println!("Validated preset {id}: {} with {} links ({drawn} with collision geometry); chunk {} steps × {} s; seed {}.", run.kind(), loaded.model.links.len(), run.chunk_steps(), run.config.step_s, run.seed);
        return Ok(());
    }
    let view = sim_spatial::robot::RobotView::open_preset(&presets(args)?, id)?;
    let api = sim_spatial::robot::server(args.api_port)?;
    eprintln!("Physical REST (robot mode, preset {id}): http://{}", api.address);
    sim_spatial::robot::run_robot(view, api);
    Ok(())
}
fn build_mode(args: &Args, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let registry = registry();
    let library = library(args)?;
    let mut builder = sim_spatial::Builder::open(path.to_path_buf(), library.clone(), registry.clone())?;
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
    let models_dir = models_dir(args, &library);
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
    // One workspace root for this launch, from --workspace/$SIM_WORKSPACE, the opened file or the current directory.
    let opened = args.system.as_deref().or(args.robot.as_deref()).or(args.lessons.as_deref()).or(args.place.as_deref()).or(args.description.as_deref());
    sim_spatial::workspace::init(args.workspace.as_deref(), opened);
    if let Some(dir) = args.place.clone() {
        return sim_spatial::place_view::run_place(dir).map_err(Into::into);
    }
    if let Some(path) = args.robot.clone() {
        return robot_mode(&args, &path);
    }
    if let Some(id) = args.robot_preset.clone() {
        return robot_preset_mode(&args, &id);
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
    let models = match args.models.clone().map(Ok).unwrap_or_else(|| sim_spatial::workspace::path("library/models")) {
        Ok(dir) => sim_spatial::models::ModelLibrary::open(dir),
        Err(e) => {
            let mut none = sim_spatial::models::ModelLibrary::default();
            none.error = Some(e);
            none
        }
    };
    sim_spatial::run_with_api(scene, link, Some(api), models.error.is_none().then_some(models));
    Ok(())
}

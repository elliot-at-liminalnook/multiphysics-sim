use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    about = "Inspect a source-bound spatial assembly. Shared live observations; no physics stepping."
)]
struct Args {
    /// Open FILE in the mode its type selects: a `*.system.json` file → build
    /// mode (--system), a `*.simrobot.json` file → robot mode (--robot), a
    /// directory holding `place.json` → place mode (--place), a directory with
    /// `<slug>/lesson.md` entries → lessons mode (--lessons). Detected by name
    /// or directory structure only; anything else is an error. Presets stay on
    /// --robot-preset.
    #[arg(value_name = "FILE", conflicts_with_all = ["system", "robot", "robot_preset", "lessons", "place", "description", "spatial", "live", "animation", "selection_link"])]
    file: Option<PathBuf>,
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
    /// Requires lessons mode (--lessons DIR or a lessons FILE).
    #[arg(long)]
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

/// The display-model catalog directory: explicit --models, else `models`
/// next to the library (`<workspace>/library/models`).
fn models_path(args: &Args) -> Result<PathBuf, String> {
    match &args.models {
        Some(dir) => Ok(dir.clone()),
        None => library(args).map(|l| models_dir(args, &l)),
    }
}
/// The shared display-model catalog every mode draws from.
fn model_library(args: &Args) -> sim_spatial::models::ModelLibrary {
    let library = match models_path(args) {
        Ok(dir) => sim_spatial::models::ModelLibrary::open(dir),
        Err(e) => {
            let mut none = sim_spatial::models::ModelLibrary::default();
            none.error = Some(e);
            none
        }
    };
    if let Some(e) = &library.error {
        eprintln!("Display models unavailable ({e}); drawing bounding shapes.");
    }
    library
}
/// What a switch needs to open other modes' documents later: the launch facts.
fn documents(args: &Args) -> sim_spatial::app::switch::Documents {
    let mut documents = sim_spatial::app::switch::Documents::default();
    documents.library = library(args);
    if let Ok(models) = models_path(args) {
        documents.models = models;
    }
    documents.presets = args.robot_presets.clone();
    documents
}

/// Open the one window in `launch.mode`, with the one REST server.
fn open_window(args: &Args, launch: impl FnOnce(sim_api::Server) -> sim_spatial::Launch) -> Result<(), Box<dyn std::error::Error>> {
    let api = sim_spatial::rest::bind(args.api_port)?;
    let address = api.address;
    let launch = launch(api);
    eprintln!("Physical REST ({} mode; every mode's commands): http://{address}", launch.mode.name());
    sim_spatial::app::run(launch);
    Ok(())
}

fn launch(mode: sim_spatial::ViewerMode, api: sim_api::Server, documents: sim_spatial::app::switch::Documents, models: sim_spatial::models::ModelLibrary) -> sim_spatial::Launch {
    sim_spatial::Launch { mode, api, documents, models, scene: None, link: None, builder: None, learn: None, robot: None, place: None }
}

fn lessons_mode(args: &Args, dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let registry = registry();
    let library = library(args)?;
    if args.validate_only {
        let learn = sim_spatial::lesson::Learn::new(dir.to_path_buf(), library.clone(), registry.clone());
        for e in &learn.entries {
            match &e.error {
                Some(err) => println!("{}: {err}", e.slug),
                None => println!("{}: {} ({} scenes)", e.slug, e.title, e.scenes),
            }
        }
        return Ok(());
    }
    let (learn, builder, scene, warning) = sim_spatial::lesson::open_lessons(dir.to_path_buf(), args.lesson.clone(), library, registry)?;
    if let Some(w) = warning {
        eprintln!("{w}");
    }
    let mut documents = documents(args);
    documents.lessons = Some((dir.to_path_buf(), learn.slug().map(str::to_string)));
    let models = model_library(args);
    open_window(args, |api| sim_spatial::Launch { learn: Some(learn), builder: Some(builder), scene: Some(scene), ..launch(sim_spatial::ViewerMode::Lessons, api, documents, models) })
}
fn robot_mode(args: &Args, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    if args.validate_only {
        let loaded = sim_spatial::robot::load(path)?;
        let drawn = loaded.geometry.iter().filter(|g| g.is_some()).count();
        println!("Validated {} with {} links ({drawn} with collision geometry).", path.display(), loaded.model.links.len());
        return Ok(());
    }
    let view = sim_spatial::robot::RobotView::open(path.to_path_buf()).with_presets(args.robot_presets.clone());
    let mut documents = documents(args);
    documents.robot = Some(sim_spatial::app::switch::Document::Path(path.to_path_buf()));
    let models = model_library(args);
    open_window(args, |api| sim_spatial::Launch { robot: Some(view), ..launch(sim_spatial::ViewerMode::Robot, api, documents, models) })
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
        if preset.is_recorded() {
            let (loaded, run) = sim_spatial::robot::load_recorded(preset, root)?;
            let c = &run.capture;
            println!("Validated recorded preset {id}: {} links; {} frames over {} s from {}; unmatched capture links: [{}]; loaded scene {:.2} s, capture {:.2} s, mapping {:.3} s (this build). No physics is built.",
                loaded.model.links.len(), c.frames.len(), c.duration_s(), run.capture_path.display(), run.unmatched.join(", "), run.scene_load_s, run.capture_load_s, run.map_s);
            return Ok(());
        }
        let (loaded, run) = sim_spatial::robot::load_preset(preset, root)?;
        let drawn = loaded.geometry.iter().filter(|g| g.is_some()).count();
        println!("Validated preset {id}: {} with {} links ({drawn} with collision geometry); chunk {} steps × {} s; seed {}.", run.kind(), loaded.model.links.len(), run.chunk_steps(), run.config.step_s, run.seed);
        return Ok(());
    }
    let view = sim_spatial::robot::RobotView::open_preset(&presets(args)?, id)?;
    let mut documents = documents(args);
    documents.robot = Some(sim_spatial::app::switch::Document::Preset(id.to_string()));
    let models = model_library(args);
    open_window(args, |api| sim_spatial::Launch { robot: Some(view), ..launch(sim_spatial::ViewerMode::Robot, api, documents, models) })
}
fn build_mode(args: &Args, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let registry = registry();
    let library = library(args)?;
    let mut builder = sim_spatial::Builder::open(path.to_path_buf(), library.clone(), registry.clone())?;
    let mut scene = sim_spatial::builder::compiled_scene(&builder)?;
    if args.validate_only {
        println!("Validated {} with {} components.", path.display(), scene.description.components.len());
        return Ok(());
    }
    scene.compact = args.compact;
    let annotation_path = args.annotations.clone().unwrap_or_else(|| PathBuf::from(format!("{}.annotations.json", path.display())));
    scene.connect_annotations(annotation_path);
    if args.schematic {
        let sibling = std::env::current_exe()?.with_file_name("sim-viewer");
        let child = std::process::Command::new(sibling)
            .arg("--system")
            .arg(path)
            .arg("--compact")
            .spawn()
            .map_err(|e| format!("Could not open the schematic: {e}. Build sim-viewer first."))?;
        sim_spatial::jobs::reap_child(child, "sim-viewer");
    }
    let models_dir = models_dir(args, &library);
    let mut models = sim_spatial::models::ModelLibrary::open(models_dir.clone());
    // A system's own display models (CAD exports next to the file) join the shared catalog.
    models.extend(&path.parent().unwrap_or(std::path::Path::new(".")).join("models"));
    if let Some(e) = &models.error {
        eprintln!("Display models unavailable ({e}); drawing bounding shapes.");
    }
    // Opening another file in this window needs these launch facts.
    builder.enable_open(sim_spatial::builder::open::Shell { launch: path.to_path_buf(), annotations: args.annotations.clone(), schematic: args.schematic.then(|| path.to_path_buf()), models: models_dir });
    let documents = documents(args);
    open_window(args, |api| sim_spatial::Launch { builder: Some(builder), scene: Some(scene), ..launch(sim_spatial::ViewerMode::Build, api, documents, models) })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = Args::parse();
    // A positional FILE becomes the matching mode flag, so it takes exactly that flag's path.
    if let Some(file) = args.file.take() {
        use sim_spatial::launch::LaunchKind;
        match sim_spatial::launch::classify(&file)? {
            LaunchKind::System => args.system = Some(file),
            LaunchKind::Robot if args.headless || args.schematic => {
                return Err(format!("{}: robot mode does not support --headless or --schematic", file.display()).into());
            }
            LaunchKind::Robot => args.robot = Some(file),
            LaunchKind::Place => args.place = Some(file),
            LaunchKind::Lessons => args.lessons = Some(file),
        }
    }
    if args.lesson.is_some() && args.lessons.is_none() {
        return Err("--lesson requires lessons mode (--lessons DIR or a lessons directory as FILE)".into());
    }
    // One workspace root for this launch, from --workspace/$SIM_WORKSPACE, the opened file or the current directory.
    let opened = args
        .system
        .as_deref()
        .or(args.robot.as_deref())
        .or(args.lessons.as_deref())
        .or(args.place.as_deref())
        .or(args.description.as_deref())
        .or(args.robot_presets.as_deref());
    sim_spatial::workspace::init(args.workspace.as_deref(), opened);
    if let Some(dir) = args.place.clone() {
        if args.validate_only {
            println!("{}", sim_spatial::place_view::validate_place(&dir)?);
            return Ok(());
        }
        let place = sim_spatial::place_view::PlaceView::open(dir.clone())?;
        let mut documents = documents(&args);
        documents.place = Some(dir);
        let models = model_library(&args);
        return open_window(&args, |api| sim_spatial::Launch { place: Some(place), ..launch(sim_spatial::ViewerMode::Place, api, documents, models) });
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
    let (default_description, default_spatial) = sim_spatial::default_inspect_paths();
    let description_path = args.description.clone().unwrap_or(default_description);
    let spatial_path = args.spatial.clone().unwrap_or(default_spatial);
    let mut scene = sim_spatial::load_inspect(&description_path, &spatial_path)?;
    if let Some(path) = &args.animation {
        scene.set_animation(serde_json::from_slice(&std::fs::read(path)?)?)?;
    }
    use sim_inspect::spatial::SpatialCommand;
    if let Some(component) = args.select.clone() {
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
    scene.set_compact(args.compact || args.schematic);
    let session = if args.schematic {
        Some(sim_inspect::selection::native::create_session(
            &scene.description,
            scene.selection.clone(),
        )?)
    } else {
        args.selection_link.clone()
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
            let child = command
                .arg("--selection-link").arg(&directory).arg("--compact")
                .spawn().map_err(|e| format!("Could not open schematic: {e}. Run examples/systems-viewer/run-linked.sh to build both viewers."))?;
            // Reap the companion when it exits, without tying its lifetime to ours.
            sim_spatial::jobs::reap_child(child, "sim-viewer");
        }
        if scene.animation.is_some() {
            scene.connect_live(directory);
        }
        Some(sim_spatial::SelectionLink(client))
    } else {
        None
    };
    if args.headless {
        // No window: the same server and inspect handler, polled by a loop.
        let api = sim_spatial::rest::bind(args.api_port)?;
        eprintln!("Physical REST (headless, inspect mode): http://{}", api.address);
        sim_spatial::rest::headless(scene, link, api);
    }
    let mut documents = documents(&args);
    documents.inspect = Some((description_path, spatial_path));
    let models = model_library(&args);
    open_window(&args, |api| sim_spatial::Launch { scene: Some(scene), link, ..launch(sim_spatial::ViewerMode::Inspect, api, documents, models) })
}

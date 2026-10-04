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
    /// `<slug>/lesson.md` entries → lessons mode (--lessons), a `*.rcad` file →
    /// CAD mode (local Rust archive and direct OCCT; no service is started).
    /// --select, --exploded, --connections, --compact and --annotations are
    /// refused with CAD. Detected by name or directory structure only;
    /// anything else is an error. Presets stay on --robot-preset.
    #[arg(value_name = "FILE", conflicts_with_all = ["system", "robot", "robot_preset", "lessons", "place", "description", "spatial", "live", "animation", "selection_link", "cad_url", "phenomena"])]
    file: Option<PathBuf>,
    /// Open a robot project (a `*.robot.json`, or the folder holding one):
    /// CAD mode on its design, its steps (Design → Model → Test → Learn →
    /// Make) in the bottom strip of every mode.
    #[arg(long, conflicts_with_all = ["file", "system", "robot", "robot_preset", "lessons", "place", "description", "spatial", "cad_url", "phenomena"])]
    project: Option<PathBuf>,
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
    /// Obsolete: use --hardware-config FILE. Explicitly refused before launch.
    #[arg(long, value_name = "URL", hide = true)]
    hardware: Option<String>,
    /// Obsolete: tokens do not authorize local hardware sessions.
    #[arg(long, value_name = "FILE", hide = true)]
    hardware_token_file: Option<PathBuf>,
    /// Obsolete: use --motor-bench-config FILE. Explicitly refused before launch.
    #[arg(long, value_name = "URL", hide = true)]
    motor_bench: Option<String>,
    /// Obsolete: tokens do not authorize local bench sessions.
    #[arg(long, value_name = "FILE", hide = true)]
    motor_bench_token_file: Option<PathBuf>,
    /// Local calibration driver configuration; opens the Leg panel without a server.
    #[arg(long, value_name = "FILE")]
    hardware_config: Option<PathBuf>,
    /// Local motor bench driver configuration for Sync motors.
    #[arg(long, value_name = "FILE")]
    motor_bench_config: Option<PathBuf>,
    /// Legacy service URL option; native CAD refuses it pending Rust migration (previously
    /// http://127.0.0.1:8420; loopback only). Never stopped by this window;
    /// unsaved edits stay in that service.
    /// --select, --exploded, --connections, --compact and --annotations are
    /// refused with it (CAD mode does not use them), as with a `.rcad` FILE.
    #[arg(long, value_name = "URL", conflicts_with_all = ["description", "spatial", "live", "animation", "selection_link", "system", "lessons", "place", "headless", "schematic", "robot", "robot_preset", "select", "exploded", "connections", "compact", "annotations"])]
    cad_url: Option<String>,
    /// Phenomena mode: the live gallery of the built-in exhibits
    /// (`sim_phenomena::exhibits`; sim-app's former default scene). One run
    /// thread owns them and advances the shown exhibit on simulation time.
    #[arg(long, conflicts_with_all = ["description", "spatial", "live", "animation", "selection_link", "system", "lessons", "place", "headless", "schematic", "robot", "robot_preset", "cad_url"])]
    phenomena: bool,
    /// The exhibit phenomena mode opens: a 1-based number or a title
    /// fragment (the first title containing it, case-insensitive). Default:
    /// $PHENOMENA_EXHIBIT (read at every launch, so a later switch to
    /// phenomena mode opens it too), else the first.
    #[arg(long, value_name = "N|TITLE", requires = "phenomena")]
    exhibit: Option<String>,
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
    args.robot_presets.clone().map(Ok).unwrap_or_else(sim_spatial::robot::preset::default_file)
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
/// The launch's documents: the facts a switch needs to open other modes'
/// documents later, and the document registry the launched mode's document
/// is opened in ([`Documents::open`]; [`launch`] remembers the exhibit).
struct Documents {
    config: sim_spatial::app::switch::Documents,
    registry: sim_spatial::document::DocumentRegistry,
    /// The exhibit phenomena mode opens, at launch or on a later switch to
    /// it: --exhibit, else sim-app's environment variable.
    exhibit: Option<String>,
}
impl Documents {
    /// `mode`'s document at launch, open in the registry.
    fn open(&mut self, mode: sim_spatial::ViewerMode, source: sim_spatial::document::Source) {
        self.registry.open(mode, sim_spatial::app::switch::sources::kind(mode), source);
    }
}
fn documents(args: &Args) -> Documents {
    let mut config = sim_spatial::app::switch::Documents::default();
    config.library = library(args);
    if let Ok(models) = models_path(args) {
        config.models = models;
    }
    config.presets = args.robot_presets.clone();
    config.hardware = sim_spatial::robot::hardware::HardwareConfig {
        calibration: args.hardware_config.clone().map(|config_file| sim_spatial::robot::hardware::LocalTarget { config_file }),
        bench: args.motor_bench_config.clone().map(|config_file| sim_spatial::robot::hardware::LocalTarget { config_file }),
    };
    let exhibit = args.exhibit.clone().or_else(|| std::env::var("PHENOMENA_EXHIBIT").ok().filter(|v| !v.trim().is_empty()));
    Documents { config, registry: Default::default(), exhibit }
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

/// The launch: `mode` with the documents [`Documents::open`] opened; the
/// exhibit is phenomena's open document when phenomena is launched, else
/// remembered when one was given.
fn launch(mode: sim_spatial::ViewerMode, api: sim_api::Server, documents: Documents, models: sim_spatial::models::ModelLibrary) -> sim_spatial::Launch {
    use sim_spatial::ViewerMode;
    use sim_spatial::app::switch::sources::kind;
    let Documents { config, mut registry, exhibit } = documents;
    let given = exhibit.is_some();
    let source = sim_spatial::document::Source::Exhibit { exhibit };
    if mode == ViewerMode::Phenomena {
        registry.open(mode, kind(mode), source);
    } else if given {
        registry.remember(ViewerMode::Phenomena, kind(ViewerMode::Phenomena), source);
    }
    sim_spatial::Launch { mode, api, documents: config, registry, models, scene: None, link: None, builder: None, learn: None, robot: None, place: None, cad: None, project: None, start: false }
}

/// CAD mode: a local archive. The typed open applies after mode entry;
/// validation-only reports the planned local path without querying the kernel.
fn cad_mode(args: &Args, target: sim_spatial::cad::CadTarget, project: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    use sim_spatial::cad::CadTarget;
    if matches!(&target, CadTarget::Service(_)) {
        return Err("CAD service attachment awaiting Rust migration; open a local .rcad archive".into());
    }
    if args.validate_only {
        if let CadTarget::File(path) = &target {
            println!("CAD mode will read {} locally through sim-cad and direct OCCT; archive/kernel validation was not executed.", path.display());
        }
        return Ok(());
    }
    let mut documents = documents(args);
    documents.open(sim_spatial::ViewerMode::Cad, sim_spatial::app::switch::sources::cad_source(&target));
    let models = model_library(args);
    let cad = sim_spatial::cad::CadDocument::new(target);
    open_window(args, |api| sim_spatial::Launch { cad: Some(cad), project, ..launch(sim_spatial::ViewerMode::Cad, api, documents, models) })
}

/// Phenomena mode: the built-in exhibits; nothing to load before the window
/// opens (the run thread builds them). `--validate-only` builds them here,
/// lists them and resolves `--exhibit`.
fn phenomena_mode(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let documents = documents(args);
    let exhibit = documents.exhibit.clone();
    if args.validate_only {
        let exhibits = sim_phenomena::exhibits::all();
        let titles: Vec<&str> = exhibits.iter().map(|e| e.title()).collect();
        let current = match &exhibit {
            Some(e) => sim_spatial::phenomena::ExhibitRef::parse(e).resolve(&titles)?,
            None => 0,
        };
        for (i, title) in titles.iter().enumerate() {
            println!("{}{:2} {title}", if i == current { "▸" } else { " " }, i + 1);
        }
        println!("Validated {} exhibits; phenomena mode would open {} ({}).", titles.len(), current + 1, titles[current]);
        return Ok(());
    }
    let models = model_library(args);
    open_window(args, |api| launch(sim_spatial::ViewerMode::Phenomena, api, documents, models))
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
    documents.open(sim_spatial::ViewerMode::Lessons, sim_spatial::document::Source::Lessons { dir: dir.to_path_buf(), lesson: learn.slug().map(str::to_string) });
    let models = model_library(args);
    open_window(args, |api| sim_spatial::Launch { learn: Some(learn), builder: Some(builder), scene: Some(scene), ..launch(sim_spatial::ViewerMode::Lessons, api, documents, models) })
}
fn robot_mode(args: &Args, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    if args.validate_only {
        match sim_spatial::robot::load_file(path)? {
            sim_spatial::robot::source::FileModel::Physical(loaded) => {
                let drawn = loaded.geometry.iter().filter(|g| g.is_some()).count();
                println!("Validated {} with {} links ({drawn} with collision geometry).", path.display(), loaded.model.links.len());
            }
            // A planar v2 summary: built through sim-phenomena's shared planar build, as robot mode's run thread builds it.
            sim_spatial::robot::source::FileModel::Planar(p) => {
                let (bodies, joints) = (p.model.bodies.len(), p.model.joints.len());
                let robot = sim_phenomena::scenarios::cad_robot::build_planar(p.model)?;
                // The build prints its warnings to stderr itself ("cad model: …").
                println!("Validated {} as a planar v2 summary: {bodies} bodies, {joints} joints in file, {} simulated, root `{}`{}, {} build warnings (sim-phenomena's planar build, not the v3 physical model).", path.display(), robot.joint_names.len(), robot.model.bodies[robot.root].name, if robot.root_fixed { " (fixed)" } else { "" }, robot.warnings.len());
            }
        }
        return Ok(());
    }
    let view = sim_spatial::robot::RobotView::open(path.to_path_buf()).with_presets(args.robot_presets.clone());
    let mut documents = documents(args);
    documents.open(sim_spatial::ViewerMode::Robot, sim_spatial::document::Source::path(path));
    let models = model_library(args);
    open_window(args, |api| sim_spatial::Launch { robot: Some(view), ..launch(sim_spatial::ViewerMode::Robot, api, documents, models) })
}
fn robot_preset_mode(args: &Args, id: &str) -> Result<(), Box<dyn std::error::Error>> {
    if args.validate_only {
        let root = sim_spatial::workspace::root()?;
        let presets = presets(args)?;
        for p in sim_spatial::robot::preset::list(&presets)? {
            let d = p.discovery(&root);
            println!("{} · mode {} · inputs exist {} · {}", p.id, p.mode, d["inputs_exist"], d["not_openable_reason"].as_str().unwrap_or("openable (build not attempted)"));
        }
        let preset = sim_spatial::robot::preset::select(&presets, root, id)?;
        if preset.is_recorded() {
            let (loaded, run) = sim_spatial::robot::load_recorded(preset, root)?;
            let c = &run.capture;
            println!("Validated recorded preset {id}: {} links; {} frames over {} s from {}; unmatched capture links: [{}]; loaded scene {:.2} s, capture {:.2} s, mapping {:.3} s (this build). No physics is built.",
                loaded.model.links.len(), c.frames.len(), c.duration_s(), run.capture_path.display(), run.unmatched.join(", "), run.scene_load_s, run.capture_load_s, run.map_s);
            return Ok(());
        }
        let (loaded, run) = sim_spatial::robot::load_preset(preset, root)?;
        let drawn = loaded.geometry.iter().filter(|g| g.is_some()).count();
        println!("Validated preset {id}: {} with {} links ({drawn} with collision geometry); chunk {} steps × {} s; seed {}.", run.kind(), loaded.model.links.len(), run.chunk_steps(), run.step_s(), run.seed);
        return Ok(());
    }
    let view = sim_spatial::robot::RobotView::open_preset(&presets(args)?, id)?;
    let mut documents = documents(args);
    documents.open(sim_spatial::ViewerMode::Robot, sim_spatial::document::Source::Preset { id: id.to_string() });
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
        let mut command = std::process::Command::new(sibling);
        command.arg("--system").arg(path).arg("--compact");
        // Detached (outlives this window) and reaped when it exits.
        sim_spatial::jobs::spawn_detached("sim-viewer", command).map_err(|e| format!("Could not open the schematic: {e}. Build sim-viewer first."))?;
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
    let mut documents = documents(args);
    documents.open(sim_spatial::ViewerMode::Build, sim_spatial::document::Source::path(builder.path()));
    open_window(args, |api| sim_spatial::Launch { builder: Some(builder), scene: Some(scene), ..launch(sim_spatial::ViewerMode::Build, api, documents, models) })
}

/// The inspect-mode flags CAD mode does not use (its view comes from
/// RoboCAD): refused with a `.rcad` FILE ([`cad_mode_refusal`]) and, by
/// clap, with --cad-url.
const NOT_IN_CAD_MODE: [&str; 5] = ["--select", "--exploded", "--connections", "--compact", "--annotations"];

/// The first of [`NOT_IN_CAD_MODE`] given, worded as the refusal.
fn cad_mode_refusal(args: &Args) -> Option<String> {
    let given = [args.select.is_some(), args.exploded, args.connections, args.compact, args.annotations.is_some()];
    NOT_IN_CAD_MODE.iter().zip(given).find(|(_, on)| *on).map(|(flag, _)| format!("{flag} is not used in CAD mode (a .rcad file or --cad-url)"))
}

/// A positional FILE becomes the matching mode flag, so it takes exactly
/// that flag's path; a `.rcad` FILE is returned (CAD mode). Flags the
/// chosen mode cannot use are refused here, before any window or service.
fn take_file(args: &mut Args) -> Result<Option<PathBuf>, String> {
    use sim_spatial::launch::LaunchKind;
    let Some(file) = args.file.take() else { return Ok(None) };
    match sim_spatial::launch::classify(&file)? {
        LaunchKind::System => args.system = Some(file),
        LaunchKind::Robot if args.headless || args.schematic => {
            return Err(format!("{}: robot mode does not support --headless or --schematic", file.display()));
        }
        LaunchKind::Robot => args.robot = Some(file),
        LaunchKind::Place => args.place = Some(file),
        LaunchKind::Lessons => args.lessons = Some(file),
        LaunchKind::Cad if args.headless || args.schematic => {
            return Err(format!("{}: CAD mode does not support --headless or --schematic", file.display()));
        }
        LaunchKind::Cad => {
            if let Some(refusal) = cad_mode_refusal(args) {
                return Err(format!("{}: {refusal}", file.display()));
            }
            return Ok(Some(file));
        }
    }
    Ok(None)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = Args::parse();
    for (supplied, name, replacement) in [
        (args.hardware.is_some(), "--hardware", "--hardware-config FILE"),
        (args.hardware_token_file.is_some(), "--hardware-token-file", "--hardware-config FILE"),
        (args.motor_bench.is_some(), "--motor-bench", "--motor-bench-config FILE"),
        (args.motor_bench_token_file.is_some(), "--motor-bench-token-file", "--motor-bench-config FILE"),
    ] {
        if supplied { return Err(format!("{name} is obsolete: native hardware runs in process; use {replacement}. No hardware server was contacted.").into()); }
    }
    if let Some(path) = args.project.clone() {
        let project = sim_runtime::robot_project::Project::open(&path)?;
        sim_spatial::workspace::init(args.workspace.as_deref(), Some(&project.path));
        return cad_mode(&args, sim_spatial::cad::CadTarget::File(project.cad()), Some(project.path));
    }
    let cad_file = take_file(&mut args)?;
    if args.lesson.is_some() && args.lessons.is_none() {
        return Err("--lesson requires lessons mode (--lessons DIR or a lessons directory as FILE)".into());
    }
    // One workspace root for this launch, from --workspace/$SIM_WORKSPACE, the opened file or the current directory.
    let opened = cad_file
        .as_deref()
        .or(args.system.as_deref())
        .or(args.robot.as_deref())
        .or(args.lessons.as_deref())
        .or(args.place.as_deref())
        .or(args.description.as_deref())
        .or(args.robot_presets.as_deref());
    sim_spatial::workspace::init(args.workspace.as_deref(), opened);
    if let Some(path) = cad_file {
        return cad_mode(&args, sim_spatial::cad::CadTarget::File(path), None);
    }
    if let Some(url) = args.cad_url.clone() {
        return cad_mode(&args, sim_spatial::cad::CadTarget::Service(url), None);
    }
    if args.phenomena {
        return phenomena_mode(&args);
    }
    if let Some(dir) = args.place.clone() {
        if args.validate_only {
            println!("{}", sim_spatial::place_view::validate_place(&dir)?);
            return Ok(());
        }
        let place = sim_spatial::place_view::PlaceView::open(dir.clone())?;
        let mut documents = documents(&args);
        documents.open(sim_spatial::ViewerMode::Place, sim_spatial::document::Source::path(dir));
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
            scene.shown.clone(),
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
            scene.shown.clone(),
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
            command.arg("--selection-link").arg(&directory).arg("--compact");
            // Reap the companion when it exits, without tying its lifetime to ours.
            sim_spatial::jobs::spawn_detached("sim-viewer", command).map_err(|e| format!("Could not open schematic: {e}. Run examples/systems-viewer/run-linked.sh to build both viewers."))?;
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
        sim_spatial::rest::headless(scene, link, api, (description_path.clone(), spatial_path.clone()));
    }
    let mut documents = documents(&args);
    // No document named at all: the window offers to start a robot (the project card's Start).
    let start = args.description.is_none() && args.spatial.is_none() && args.select.is_none();
    documents.open(sim_spatial::ViewerMode::Inspect, sim_spatial::document::Source::Assembly { description: description_path, spatial: spatial_path });
    let models = model_library(&args);
    open_window(&args, |api| sim_spatial::Launch { scene: Some(scene), link, start, ..launch(sim_spatial::ViewerMode::Inspect, api, documents, models) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use std::ffi::OsString;

    /// Each flag CAD mode does not use, with a value where it takes one.
    const IGNORED: [&[&str]; 5] = [&["--select", "b1"], &["--exploded"], &["--connections"], &["--compact"], &["--annotations", "notes.json"]];

    fn argv(head: &[OsString], flag: &[&str]) -> Vec<OsString> {
        let mut v = vec![OsString::from("sim-spatial")];
        v.extend(head.iter().cloned());
        v.extend(flag.iter().map(OsString::from));
        v
    }

    /// A directory under the temp dir, removed on drop.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!("sim-spatial-main-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
        fn file(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, "{}").unwrap();
            path
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_argument_table_is_consistent() {
        // Every id named in conflicts_with_all / requires exists, and so on.
        Args::command().debug_assert();
    }

    #[test]
    fn cad_url_refuses_the_flags_cad_mode_does_not_use() {
        let head = [OsString::from("--cad-url"), OsString::from("http://127.0.0.1:8420")];
        assert!(Args::try_parse_from(argv(&head, &[])).is_ok(), "--cad-url alone parses");
        for flag in IGNORED {
            let e = Args::try_parse_from(argv(&head, flag)).err().unwrap_or_else(|| panic!("--cad-url with {flag:?} is refused"));
            assert_eq!(e.kind(), clap::error::ErrorKind::ArgumentConflict, "{flag:?}: {e}");
            assert!(e.to_string().contains(flag[0]), "the refusal names {}: {e}", flag[0]);
        }
    }

    #[test]
    fn an_rcad_file_refuses_the_flags_cad_mode_does_not_use() {
        let dir = TempDir::new("rcad");
        let rcad = dir.file("x.rcad");
        let head = [rcad.clone().into_os_string()];
        let mut args = Args::try_parse_from(argv(&head, &[])).unwrap();
        assert_eq!(take_file(&mut args), Ok(Some(rcad.clone())), "a .rcad file alone opens CAD mode");
        for flag in IGNORED {
            let mut args = Args::try_parse_from(argv(&head, flag)).unwrap_or_else(|e| panic!("{flag:?} parses with a FILE (refused after classifying it): {e}"));
            let e = take_file(&mut args).expect_err("refused in CAD mode");
            assert_eq!(e, format!("{}: {} is not used in CAD mode (a .rcad file or --cad-url)", rcad.display(), flag[0]));
        }
    }

    #[test]
    fn other_files_and_inspect_mode_keep_those_flags() {
        let dir = TempDir::new("system");
        let system = dir.file("x.system.json");
        // Build mode uses --compact and --annotations.
        let mut args = Args::try_parse_from(argv(&[system.clone().into_os_string()], &["--compact", "--annotations", "notes.json"])).unwrap();
        assert_eq!(take_file(&mut args), Ok(None));
        assert_eq!(args.system.as_deref(), Some(system.as_path()));
        assert!(args.compact && args.annotations.is_some());
        // Inspect mode (no FILE) uses all five.
        let all: Vec<&str> = IGNORED.iter().flat_map(|f| f.iter().copied()).collect();
        let mut args = Args::try_parse_from(argv(&[], &all)).unwrap();
        assert_eq!(take_file(&mut args), Ok(None));
        assert!(args.select.is_some() && args.exploded && args.connections && args.compact && args.annotations.is_some());
    }
}

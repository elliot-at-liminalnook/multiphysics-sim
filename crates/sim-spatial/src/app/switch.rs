//! Switching modes in the running window (native-viewer.md §1), and the
//! window's other actions ([`WindowAction`]: `screenshot`, and `system_ui` in
//! modes without controls of their own).
//!
//! One request type ([`ModeSwitch`]) and one validating handler
//! ([`handle`], `ViewerSet::Actions`). Every entry point writes a
//! [`WindowAction`]: the mode switcher (bottom right, `super::switcher`),
//! `system_ui` controls `mode:<mode>` and REST `viewer_mode` (routed by
//! `super::route`), the builder's Lessons button, and the lesson screen's own
//! toggles ("Open in builder", "‹ lesson", `lesson_screen`, `lesson_open`),
//! which ask for Build or Lessons.
//!
//! - **Refusals** name the reason and keep the current mode: no document
//!   for the target mode, the builder's `system_open` blockers
//!   (`Builder::switch_blockers`, shared with open.rs) when Build/Lessons is
//!   left or Lessons is entered from Build, and a lesson's or a robot's work
//!   in progress when that mode is left, another switch in progress, or a
//!   document that fails to load.
//! - **Documents** load with the loaders the launch uses, off the UI thread
//!   (a `jobs` Compute job, or the robot view's own loader), and are
//!   installed by [`arrive`] on the target mode's OnEnter, before the scope's
//!   OnEnter spawns its entities.
//! - **Teardown**: mode entities carry `DespawnOnExit<ModeScope>`
//!   (`super::scope_new_entities`); each scope's OnExit here removes its
//!   resources. The builder stays in the window across every switch (paused
//!   outside Build/Lessons, its scene parked); the lesson, the robot view
//!   and the place are removed (their jobs cancel, their `RunThread`s join
//!   within `jobs::JOIN_BOUND` on a drop thread). `ModelLibrary`, the fonts,
//!   `Documents`, `Rest` and the workspace root always survive.
//! - **REST** callers wait on their reply token (`actions::Replies`); a
//!   cancel stops a switch whose document is still loading.
use super::actions::{self, Act, Origin, Replies, Spec, spec};
use super::{ModeScope, ViewerMode, ViewerSet};
use crate::builder::Builder;
use crate::jobs::{Job, Pool};
use crate::lesson::Learn;
use crate::models::ModelLibrary;
use crate::place_view::PlaceView;
use crate::robot::RobotView;
use crate::{SelectionLink, SpatialScene};
use bevy::ecs::message::Messages;
use bevy::prelude::*;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sim_api::Outcome;
use std::path::PathBuf;
use std::time::Instant;

/// The document a mode opens with when it has none of its own yet.
#[derive(Clone, Debug, PartialEq)]
pub enum Document {
    /// A file or directory (by mode: `*.description.json`, `*.system.json`,
    /// a lessons folder, `*.simrobot.json`, a place directory).
    Path(PathBuf),
    /// A robot preset id (`robot_presets`).
    Preset(String),
}
impl Document {
    fn describe(&self) -> String {
        match self {
            Document::Path(p) => p.display().to_string(),
            Document::Preset(id) => format!("preset {id}"),
        }
    }
    fn json(&self) -> Value {
        match self {
            Document::Path(p) => json!({"path": p}),
            Document::Preset(id) => json!({"preset": id}),
        }
    }
}

/// A request to change the window's mode (the one request type).
#[derive(Clone, Debug)]
pub struct ModeSwitch {
    pub mode: ViewerMode,
    pub document: Option<Document>,
}
impl ModeSwitch {
    /// `viewer_mode {mode, path?, preset?}`.
    fn from_args(args: Map<String, Value>) -> Result<Self, String> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Args {
            mode: String,
            #[serde(default)]
            path: Option<PathBuf>,
            #[serde(default)]
            preset: Option<String>,
        }
        let args: Args = serde_json::from_value(Value::Object(args)).map_err(|e| format!("viewer_mode: {e}"))?;
        let mode = ViewerMode::parse(&args.mode)?;
        let document = match (args.path, args.preset) {
            (Some(_), Some(_)) => return Err("viewer_mode takes path or preset, not both".into()),
            (Some(p), None) => Some(Document::Path(p)),
            (None, Some(id)) => Some(Document::Preset(id)),
            (None, None) => None,
        };
        Ok(ModeSwitch { mode, document })
    }
}

/// The window's actions, in every mode: the mode switch, `screenshot`, and
/// `system_ui` where the mode has no controls of its own (inspect, place; in
/// every mode for a `mode:*` control). REST arguments are kept as given and
/// checked by [`handle`], so their errors read as before.
#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum WindowAction {
    /// `{}` reports the mode; `{mode, path?, preset?}` switches.
    ViewerMode(Map<String, Value>),
    /// `{path}`: the window as drawn, after the next frame.
    Screenshot(Map<String, Value>),
    /// `{action: {operation: controls | activate, id?, ui_revision?}}`.
    SystemUi(Map<String, Value>),
    /// The switcher's buttons, the builder's Lessons button and the lesson
    /// screen's toggles.
    #[serde(skip)]
    Switch(ModeSwitch),
}

impl actions::Action for WindowAction {
    fn commands() -> Vec<Spec> {
        vec![
            spec("viewer_mode", actions::ALL, json!({"mode":"robot","preset":"robot-measured-400hz"}), "Switch this window's mode, or with no mode (args {}) report it: active, modes, pending (a document still loading), entering, message (the switcher's last line) and documents (what each mode reopens: inspect description/spatial, lessons dir and lesson, robot path or preset, place; library, models, presets). mode inspect | build | lessons | robot | place; optional path (inspect: a *.description.json with its *.spatial.json beside it; build: a *.system.json; lessons: a folder of <slug>/lesson.md; robot: a *.simrobot.json; place: a sim-place build directory) or preset (robot: an id listed by robot_presets). Without one, a mode reopens what it showed before in this window or at launch; inspect falls back to the example assembly. The same handler as the mode switcher (bottom right of the window), the builder's Lessons button and system_ui mode:<mode> (every mode). Refused, naming the reason, with the current mode kept: no document for the target mode; leaving build or lessons, entering lessons from build (by any entry point: the switcher, the Lessons button, system_ui mode:lessons, viewer_mode, lesson_open or lesson_screen) or replacing the builder with a new lesson's, while a text or discussion draft, placement drag, study, replay, Codex answer or open is in progress (the builder's system_open blockers), or leaving build or lessons while a lesson draft or contact sheet is, and a new lesson replacing a build-mode builder with a live run; leaving robot mode while a recording is being written or a replay runs; another switch in progress; a path of the wrong kind; a document that fails to load (named). Documents load off the UI thread; poll the job. Build and lessons share the builder: switching between them keeps it and the lesson (the lesson screen is drawn over the builder), and entering lessons pauses a live build run and keeps it (Run resumes it in build mode). The builder stays in the window across every switch: leaving build/lessons pauses a live run and parks its scene; build with a path while it has another file open is refused (use system_open in build mode). Leaving lessons closes the lesson (its recordings and narration stop; lessons reopens it); leaving robot mode stops its run, gait and playback threads; leaving inspect parks its scene and selection link. Commands of another mode are refused naming the active mode. Result: mode, previous, document, load_seconds (unchanged=true when the mode was already active)."),
            spec("screenshot", actions::ALL, json!({"path":"/tmp/view.png"}), "Save the window exactly as drawn (UI, overlays, lesson pages) to a PNG after the next frame; refused, naming the cause, while the window is not visible"),
        ]
    }
    /// `viewer_mode` with `args: null` reports the mode, as `{}` does.
    fn parse(command: &sim_api::Command) -> Result<Self, String> {
        if command.args.is_null() {
            let empty = sim_api::Command { command: command.command.clone(), args: json!({}) };
            return sim_api::decode::<Self>(&empty);
        }
        sim_api::decode::<Self>(command)
    }
    fn controls() -> &'static [&'static str] {
        &["mode:inspect", "mode:build", "mode:lessons", "mode:robot", "mode:place"]
    }
}
impl WindowAction {
    /// `system_ui` in modes without controls of their own.
    pub(crate) fn switcher_commands() -> Vec<Spec> {
        vec![spec("system_ui", actions::SWITCHER_ONLY, json!({"action":{"operation":"controls"}}), "Inspect and place mode: the mode switcher's controls mode:inspect | mode:build | mode:lessons | mode:robot | mode:place (controls; activate {id, ui_revision}, the same handler as viewer_mode and the switcher's buttons; ui_revision is not checked for mode:* controls). In build, lessons and robot mode system_ui is the mode's own (its controls list ends with these mode:* controls).")]
    }
    /// The switch a `system_ui` `mode:*` control asks for.
    pub(super) fn mode_control(args: &Map<String, Value>) -> Option<Result<ModeSwitch, String>> {
        let action = args.get("action")?;
        if action["operation"] != "activate" {
            return None;
        }
        let target = action["id"].as_str()?.strip_prefix("mode:")?;
        Some(ViewerMode::parse(target).map(|mode| ModeSwitch { mode, document: None }))
    }
    /// `system_ui` in a mode whose only controls are the switcher's.
    fn switcher_ui(mode: ViewerMode, args: &Map<String, Value>) -> Result<Value, String> {
        match args.get("action").and_then(|a| a["operation"].as_str()) {
            Some("controls") => Ok(json!({"ui_revision": 0, "ready": true, "controls": super::route::mode_controls(mode), "state": {"viewer_mode": mode}})),
            _ => Err(format!("system_ui in {} mode has only the mode switcher's controls: operation controls, or activate with a mode:<mode> id", mode.name())),
        }
    }
    /// The headless server (inspect mode, no window): what it can answer of these.
    pub(crate) fn headless(self) -> Result<Value, String> {
        match self {
            WindowAction::Screenshot(args) => crate::rest::screenshot_path(&Value::Object(args), false).map(|p| json!({"path": p, "note": "saved once the next frame renders"})),
            WindowAction::SystemUi(args) => Self::switcher_ui(ViewerMode::Inspect, &args),
            WindowAction::ViewerMode(_) | WindowAction::Switch(_) => Err("viewer_mode: this headless server has no window to switch (inspect mode only)".into()),
        }
    }
}


/// What this window can open in each mode without being told, and the
/// launch facts the loaders need. Shared: it survives every switch.
#[derive(Resource)]
pub struct Documents {
    /// The palette library (`--library`, else `<workspace>/library/systems`).
    pub library: Result<PathBuf, String>,
    /// The display-model catalog.
    pub models: PathBuf,
    /// `--robot-presets` (None: `<workspace>/web/viewer/presets.json`).
    pub presets: Option<PathBuf>,
    /// The inspected assembly (description, spatial).
    pub inspect: Option<(PathBuf, PathBuf)>,
    /// The lessons folder and lesson last open.
    pub lessons: Option<(PathBuf, Option<String>)>,
    /// The robot file or preset last open.
    pub robot: Option<Document>,
    /// The place last walked through.
    pub place: Option<PathBuf>,
    /// The hardware servers given at launch (`--hardware`, `--motor-bench`), for robot mode's Leg calibration panel.
    pub hardware: crate::robot::hardware::HardwareConfig,
    /// Inspect's scene (and selection link) while another mode is shown.
    parked_inspect: Option<Box<(SpatialScene, Option<SelectionLink>)>>,
    /// The builder's scene while a mode outside Build/Lessons is shown.
    parked_builder: Option<Box<SpatialScene>>,
}
impl Default for Documents {
    fn default() -> Self {
        Self {
            library: crate::workspace::path("library/systems"),
            models: crate::workspace::path("library/models").unwrap_or_else(|_| PathBuf::from("models")),
            presets: None,
            inspect: None,
            lessons: None,
            robot: None,
            place: None,
            hardware: Default::default(),
            parked_inspect: None,
            parked_builder: None,
        }
    }
}
impl Documents {
    pub fn json(&self) -> Value {
        json!({
            "inspect": self.inspect.as_ref().map(|(d, s)| json!({"description": d, "spatial": s})),
            "inspect_parked": self.parked_inspect.is_some(),
            "builder_scene_parked": self.parked_builder.is_some(),
            "lessons": self.lessons.as_ref().map(|(d, s)| json!({"dir": d, "lesson": s})),
            "robot": self.robot.as_ref().map(Document::json),
            "place": self.place,
            "library": match &self.library { Ok(p) => json!(p), Err(e) => json!({"error": e}) },
            "models": self.models,
            "presets": self.presets,
        })
    }
}

/// The one switch in progress and the latest outcome (REST callers wait on
/// their reply token, `actions::Replies`).
#[derive(Resource, Default)]
pub struct Switcher {
    pending: Option<Pending>,
    /// Loaded documents waiting for the target mode's OnEnter ([`arrive`]).
    arrival: Option<Box<Arrival>>,
    /// A switch whose `NextState` is set: (who asked, target, result once entered).
    entering: Option<(Origin, ViewerMode, Value)>,
    /// The latest outcome as the switcher shows it (refusals name the reason).
    pub message: Option<Result<String, String>>,
    /// Bumped whenever an outcome or a pending load changes.
    pub revision: u64,
}

struct Pending {
    origin: Origin,
    mode: ViewerMode,
    what: String,
    work: Work,
    started: Instant,
}
enum Work {
    Job(Job<Box<Arrival>>),
    /// The robot view's own loader (a preset's job or the file's first
    /// check), waited for before switching so a file that fails to load
    /// leaves the current mode.
    Robot(Box<RobotView>),
}

/// What a switch brings into the world on the target mode's OnEnter.
#[derive(Default)]
pub(crate) struct Arrival {
    scene: Option<SpatialScene>,
    link: Option<SelectionLink>,
    builder: Option<Builder>,
    learn: Option<Learn>,
    models: Option<ModelLibrary>,
    robot: Option<RobotView>,
    place: Option<PlaceView>,
    unpark_inspect: bool,
    unpark_builder: bool,
    inspect: Option<(PathBuf, PathBuf)>,
    lessons: Option<(PathBuf, Option<String>)>,
    document: Value,
}

/// A REST caller's answer (nothing for a click: the switcher shows the message).
fn answer(world: &mut World, origin: Origin, result: Result<Value, String>) {
    if let Origin::Rest(reply) = origin {
        world.resource_mut::<Replies>().answer(reply, Outcome::Done(result));
    }
}

impl Switcher {
    /// Show the outcome and answer whoever asked.
    fn finish(&mut self, world: &mut World, origin: Origin, result: Result<Value, String>) {
        self.show(&result);
        answer(world, origin, result);
    }
    fn show(&mut self, result: &Result<Value, String>) {
        self.message = Some(result.as_ref().map(|v| v["message"].as_str().unwrap_or_default().to_string()).map_err(Clone::clone));
        self.revision += 1;
    }
    /// `viewer_mode` with no mode, and `/v1/viewer_mode`.
    pub fn json(&self, mode: ViewerMode, documents: &Documents) -> Value {
        json!({
            "active": mode,
            "modes": ViewerMode::ALL,
            "pending": self.pending.as_ref().map(|p| json!({"mode": p.mode, "loading": p.what, "seconds": p.started.elapsed().as_secs_f64()})),
            "entering": self.entering.as_ref().map(|e| e.1),
            "message": self.message.as_ref().map(|m| match m { Ok(t) => json!({"ok": true, "text": t}), Err(e) => json!({"ok": false, "text": e}) }),
            "documents": documents.json(),
            "revision": self.revision,
        })
    }
}

/// Registers the handler, the teardown and the lesson screen requests (no
/// window needed: the state-transition test runs this on MinimalPlugins).
pub(crate) fn build(app: &mut App) {
    actions::register::<WindowAction>(app);
    app.init_resource::<Switcher>()
        .init_resource::<Replies>()
        .init_resource::<Documents>()
        .add_systems(Update, (actions::serve, lesson_screen_requests).chain().in_set(ViewerSet::Input))
        .add_systems(Update, handle.in_set(ViewerSet::Actions))
        .add_systems(Update, finish_load.in_set(ViewerSet::JobResults))
        .add_systems(OnExit(ModeScope::Inspect), leave_inspect)
        .add_systems(OnExit(ModeScope::Builder), leave_builder)
        .add_systems(OnExit(ModeScope::Robot), leave_robot)
        .add_systems(OnExit(ModeScope::Place), leave_place)
        .add_systems(OnExit(ViewerMode::Lessons), hide_lessons);
    for mode in ViewerMode::ALL {
        if mode == ViewerMode::Lessons {
            app.add_systems(OnEnter(mode), (arrive, show_lessons).chain());
        } else {
            app.add_systems(OnEnter(mode), arrive);
        }
    }
}

/// The one handler (Actions): confirms a switch once its state is entered,
/// stops a loading switch whose REST caller cancelled, then applies this
/// frame's window actions in order.
pub(crate) fn handle(world: &mut World) {
    let current = *world.resource::<State<ViewerMode>>().get();
    let acts: Vec<Act<WindowAction>> = world.resource_mut::<Messages<Act<WindowAction>>>().drain().collect();
    world.resource_scope(|world, mut switch: Mut<Switcher>| {
        if let Some((origin, target, summary)) = switch.entering.take() {
            if current == target {
                switch.finish(world, origin, Ok(summary));
            } else {
                switch.entering = Some((origin, target, summary));
            }
        }
        let cancelled = match switch.pending.as_ref().map(|p| p.origin) {
            Some(Origin::Rest(reply)) => world.resource::<Replies>().cancelled(reply),
            _ => false,
        };
        if cancelled {
            // Its job is dropped, so it cancels; the caller is told "cancelled".
            if let Some(p) = switch.pending.take() {
                crate::jobs::drop_off_thread(p.work, "a cancelled mode switch");
                switch.show(&Err("Mode switch cancelled; the current mode stays.".into()));
                answer(world, p.origin, Err("cancelled".into()));
            }
        }
        for Act { action, origin } in acts {
            if let Origin::Rest(reply) = origin {
                world.resource_mut::<Replies>().pick(reply);
                // Cancelled before it was applied (a command waiting on an
                // `ask_switch` that its caller cancelled): nothing happens.
                if world.resource::<Replies>().cancelled(reply) {
                    answer(world, origin, Err("cancelled".into()));
                    continue;
                }
            }
            let request = match action {
                WindowAction::Switch(request) => Ok(request),
                WindowAction::ViewerMode(args) if args.is_empty() => {
                    let status = switch.json(current, world.resource::<Documents>());
                    answer(world, origin, Ok(status));
                    continue;
                }
                WindowAction::ViewerMode(args) => ModeSwitch::from_args(args),
                WindowAction::SystemUi(args) => match WindowAction::mode_control(&args) {
                    Some(request) => request,
                    None => {
                        let result = WindowAction::switcher_ui(current, &args);
                        answer(world, origin, result);
                        continue;
                    }
                },
                WindowAction::Screenshot(args) => {
                    let result = screenshot(world, args);
                    answer(world, origin, result);
                    continue;
                }
            };
            // A malformed request is the caller's error, not the switcher's line.
            let request = match request {
                Ok(request) => request,
                Err(e) => {
                    answer(world, origin, Err(e));
                    continue;
                }
            };
            if let Err(e) = start(world, &mut switch, current, origin, request) {
                switch.finish(world, origin, Err(e));
            }
        }
    });
}

/// `screenshot`: the window as drawn, saved on the render thread after the next frame.
fn screenshot(world: &mut World, args: Map<String, Value>) -> Result<Value, String> {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    let occluded = world.get_resource::<crate::rest::Occlusion>().is_some_and(|o| o.0);
    let path = crate::rest::screenshot_path(&Value::Object(args), occluded)?;
    world.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
    let _ = world.write_message(bevy::window::RequestRedraw);
    Ok(json!({"path": path, "note": "saved once the next frame renders"}))
}

fn refusal(target: ViewerMode, current: ViewerMode, why: &str) -> String {
    format!("Not switching to {} mode: {why}. {} mode stays.", target.label(), current.label())
}

/// Where a mode opens another document of its own.
fn open_hint(mode: ViewerMode) -> &'static str {
    match mode {
        ViewerMode::Build => "system_open, the Systems tab",
        ViewerMode::Lessons => "lesson_open, the lesson list",
        ViewerMode::Robot => "robot_preset for a preset",
        ViewerMode::Inspect | ViewerMode::Place => "switch to another mode first",
    }
}

fn start(world: &mut World, switch: &mut Switcher, current: ViewerMode, origin: Origin, request: ModeSwitch) -> Result<(), String> {
    let target = request.mode;
    if let Some(p) = &switch.pending {
        return Err(refusal(target, current, &format!("the switch to {} mode is still loading {}", p.mode.label(), p.what)));
    }
    if let Some((_, m, _)) = &switch.entering {
        return Err(refusal(target, current, &format!("the switch to {} mode is still being applied", m.label())));
    }
    if target == current {
        if let Some(d) = &request.document {
            return Err(refusal(target, current, &format!("{} mode is already active; open {} from within it ({})", target.label(), d.describe(), open_hint(target))));
        }
        switch.finish(world, origin, Ok(json!({"mode": target, "previous": current, "unchanged": true, "message": format!("{} mode is already active.", target.label())})));
        return Ok(());
    }
    let blockers = leaving_blockers(world, current, target);
    if !blockers.is_empty() {
        return Err(refusal(target, current, &blockers.join("; ")));
    }
    match prepare(world, current, &request).map_err(|e| refusal(target, current, &e))? {
        Prepared::Now(arrival) => enter(world, switch, origin, current, target, arrival, Instant::now()),
        Prepared::Load(what, work) => {
            switch.message = Some(Ok(format!("Switching to {} mode: loading {what} off the UI thread…", target.label())));
            switch.revision += 1;
            switch.pending = Some(Pending { origin, mode: target, what, work, started: Instant::now() });
        }
    }
    Ok(())
}

/// What would be lost by leaving `current` for `target` now. Leaving the
/// builder's modes, and entering Lessons from Build, is refused on the
/// builder's `system_open` blockers and a pending open
/// (`Builder::switch_blockers`, open.rs); leaving them also on a lesson's
/// draft or capture; replacing the builder with a new lesson's also on a
/// live run of the system file (`Builder::replace_blockers`); leaving robot
/// mode on a recording being written or a replay.
/// Lessons → Build over an open lesson is the lesson screen's own toggle,
/// never blocked (as before the modes were one app). Build → Lessons keeps a
/// live run, paused (`show_lessons`).
fn leaving_blockers(world: &World, current: ViewerMode, target: ViewerMode) -> Vec<String> {
    let mut blockers = Vec::new();
    // A new lessons folder brings its own builder in place of this one.
    let replacing = target == ViewerMode::Lessons && !world.contains_resource::<Learn>();
    let overlay = current.builder_family() && target.builder_family() && !replacing;
    // The lesson screen would be drawn over a draft, a drag or work in progress.
    let entering_lessons = current == ViewerMode::Build && target == ViewerMode::Lessons;
    if let Some(b) = world.get_resource::<Builder>() {
        if replacing {
            blockers.extend(b.replace_blockers());
        } else if (current.builder_family() && !overlay) || entering_lessons {
            blockers.extend(b.switch_blockers());
        }
    }
    if current.builder_family() && !overlay {
        if let Some(l) = world.get_resource::<Learn>() {
            blockers.extend(l.switch_blockers());
        }
    }
    if current == ViewerMode::Robot {
        if let Some(view) = world.get_resource::<RobotView>() {
            blockers.extend(view.switch_blockers());
        }
    }
    blockers
}

enum Prepared {
    /// Nothing to load: the mode's own document is in the window (or parked).
    Now(Box<Arrival>),
    /// A document loads off the UI thread first.
    Load(String, Work),
}

/// The parts registry the launch uses (`$SIM_PARTS_DIR`, else the workspace's).
fn registry() -> sim_core::BehaviorRegistry {
    sim_runtime::system_registry_in(crate::workspace::get().as_ref())
}

/// The target mode's document: what the window already has, or a load.
fn prepare(world: &World, current: ViewerMode, request: &ModeSwitch) -> Result<Prepared, String> {
    let docs = world.resource::<Documents>();
    let target = request.mode;
    let path = match &request.document {
        None => None,
        Some(Document::Path(p)) => Some(p.clone()),
        Some(Document::Preset(id)) if target != ViewerMode::Robot => return Err(format!("preset `{id}` opens robot mode only")),
        Some(Document::Preset(_)) => None,
    };
    let in_family = current.builder_family();
    match target {
        ViewerMode::Inspect => {
            if path.is_none() && docs.parked_inspect.is_some() {
                return Ok(Prepared::Now(Box::new(Arrival { unpark_inspect: true, document: json!(docs.inspect.as_ref().map(|(d, _)| d)), ..Default::default() })));
            }
            let (description, spatial) = match &path {
                Some(p) => crate::inspect_pair(p)?,
                None => docs.inspect.clone().unwrap_or_else(crate::default_inspect_paths),
            };
            let what = description.display().to_string();
            let job = Job::spawn(Pool::Compute, 0, format!("{what}: the inspect loader"), move |_| {
                let mut scene = crate::load_inspect(&description, &spatial)?;
                scene.connect_annotations(PathBuf::from(format!("{}.annotations.json", description.display())));
                Ok(Box::new(Arrival { scene: Some(scene), document: json!(description), inspect: Some((description, spatial)), ..Default::default() }))
            });
            Ok(Prepared::Load(what, Work::Job(job)))
        }
        ViewerMode::Build => {
            if let Some(b) = world.get_resource::<Builder>() {
                match &path {
                    Some(p) if !crate::builder::open::same_file(p, b.path()) => {
                        if world.contains_resource::<Learn>() {
                            return Err(format!("the builder shows a lesson's sandbox copy ({}); leave lessons before opening {}", b.path().display(), p.display()));
                        }
                        if b.can_open() {
                            return Err(format!("this window's builder has {} open: switch to build mode, then open {} with system_open (the Systems tab), which keeps a live run", b.path().display(), p.display()));
                        }
                        // A lesson's sandbox builder (no Open) is replaced by the file below.
                    }
                    _ => return Ok(Prepared::Now(Box::new(Arrival { unpark_builder: !in_family, document: json!(b.path()), ..Default::default() }))),
                }
            }
            let path = path.ok_or("build mode needs a system file and this window has none open: give one, e.g. viewer_mode {\"mode\":\"build\",\"path\":\"….system.json\"}")?;
            if !path.is_file() {
                return Err(format!("{}: no such file", path.display()));
            }
            let library = docs.library.clone()?;
            let shell = crate::builder::open::Shell { launch: path.clone(), annotations: None, schematic: None, models: docs.models.clone() };
            let what = path.display().to_string();
            let job = Job::spawn(Pool::Compute, 0, format!("{what}: the build loader"), move |_| {
                let (builder, scene, models) = crate::builder::open::open_build(path.clone(), library, registry(), shell)?;
                Ok(Box::new(Arrival { builder: Some(builder), scene: Some(scene), models: Some(models), document: json!(path), ..Default::default() }))
            });
            Ok(Prepared::Load(what, Work::Job(job)))
        }
        ViewerMode::Lessons => {
            if let Some(l) = world.get_resource::<Learn>() {
                if let Some(p) = path.as_ref().filter(|p| !crate::builder::open::same_file(p, &l.dir)) {
                    return Err(format!("lessons from {} are open in this window; leave lessons before opening {}", l.dir.display(), p.display()));
                }
                return Ok(Prepared::Now(Box::new(Arrival { unpark_builder: !in_family, document: json!(l.dir), ..Default::default() })));
            }
            let (dir, slug) = match path {
                Some(p) => (p, None),
                None => docs.lessons.clone().ok_or("lessons mode needs a lessons folder (<slug>/lesson.md entries) and this window has none open: give one, e.g. viewer_mode {\"mode\":\"lessons\",\"path\":\"lessons\"}")?,
            };
            if crate::launch::classify(&dir) != Ok(crate::launch::LaunchKind::Lessons) {
                return Err(format!("{}: not a lessons folder (no <slug>/lesson.md entries)", dir.display()));
            }
            let library = docs.library.clone()?;
            let what = dir.display().to_string();
            let job = Job::spawn(Pool::Compute, 0, format!("{what}: the lessons loader"), move |_| {
                let (learn, builder, scene, warning) = crate::lesson::open_lessons(dir.clone(), slug.clone(), library, registry())?;
                Ok(Box::new(Arrival { learn: Some(learn), builder: Some(builder), scene: Some(scene), document: json!({"dir": dir, "warning": warning}), lessons: Some((dir, slug)), ..Default::default() }))
            });
            Ok(Prepared::Load(what, Work::Job(job)))
        }
        ViewerMode::Robot => {
            let document = request.document.clone().or_else(|| docs.robot.clone()).ok_or("robot mode needs a robot and this window has none open: give path (a *.simrobot.json) or preset (an id listed by robot_presets), e.g. viewer_mode {\"mode\":\"robot\",\"preset\":\"robot-measured-400hz\"}")?;
            let view = match &document {
                Document::Path(p) => {
                    if !p.to_string_lossy().ends_with(".simrobot.json") {
                        return Err(format!("{}: robot mode opens a *.simrobot.json file", p.display()));
                    }
                    if !p.is_file() {
                        return Err(format!("{}: no such file", p.display()));
                    }
                    RobotView::open(p.clone()).with_presets(docs.presets.clone())
                }
                Document::Preset(id) => {
                    let presets = docs.presets.clone().map(Ok).unwrap_or_else(crate::robot_preset::default_file)?;
                    RobotView::open_preset(&presets, id)?
                }
            };
            Ok(Prepared::Load(document.describe(), Work::Robot(Box::new(view))))
        }
        ViewerMode::Place => {
            let dir = path.or_else(|| docs.place.clone()).ok_or("place mode needs a scanned place (a sim-place build directory holding place.json) and this window has none open: give one, e.g. viewer_mode {\"mode\":\"place\",\"path\":\"…/place\"}")?;
            let what = dir.display().to_string();
            let job = Job::spawn(Pool::Compute, 0, format!("{what}: the place loader"), move |_| {
                let place = PlaceView::open(dir.clone())?;
                Ok(Box::new(Arrival { place: Some(place), document: json!(dir), ..Default::default() }))
            });
            Ok(Prepared::Load(what, Work::Job(job)))
        }
    }
}

/// Hand the arrival to the target mode's OnEnter and set the state.
fn enter(world: &mut World, switch: &mut Switcher, origin: Origin, from: ViewerMode, target: ViewerMode, mut arrival: Box<Arrival>, started: Instant) {
    let summary = json!({
        "mode": target, "previous": from, "document": arrival.document.take(),
        "load_seconds": started.elapsed().as_secs_f64(),
        "message": format!("Switched to {} mode.", target.label()),
    });
    switch.arrival = Some(arrival);
    switch.entering = Some((origin, target, summary));
    switch.revision += 1;
    world.resource_mut::<NextState<ViewerMode>>().set(target);
}

/// JobResults: a switch's document has loaded (enter) or failed (refuse,
/// naming it; the current mode stays). Re-checks the blockers, as
/// `finish_open` does: something may have started while loading.
pub(crate) fn finish_load(world: &mut World) {
    let current = *world.resource::<State<ViewerMode>>().get();
    world.resource_scope(|world, mut switch: Mut<Switcher>| {
        let Some(mut pending) = switch.pending.take() else { return };
        let done = match &mut pending.work {
            Work::Job(job) => job.poll(),
            Work::Robot(view) => view.opened().map(|r| r.map(|()| Box::new(Arrival { document: json!(pending.what), ..Default::default() }))),
        };
        let Some(result) = done else {
            switch.pending = Some(pending);
            return;
        };
        let (origin, target, started) = (pending.origin, pending.mode, pending.started);
        let mut arrival = match result {
            Ok(arrival) => arrival,
            Err(e) => {
                switch.finish(world, origin, Err(refusal(target, current, &format!("{} did not load: {}", pending.what, e.trim_end_matches('.')))));
                return;
            }
        };
        if let Work::Robot(view) = pending.work {
            arrival.robot = Some(*view);
        }
        let blockers = leaving_blockers(world, current, target);
        if !blockers.is_empty() {
            switch.finish(world, origin, Err(refusal(target, current, &blockers.join("; "))));
            crate::jobs::drop_off_thread(arrival, "a refused mode switch");
            return;
        }
        enter(world, &mut switch, origin, current, target, arrival, started);
    });
}

/// OnEnter of every mode: installs what the switch brought (documents
/// loaded off the UI thread, or the mode's parked scene) before the scope's
/// own OnEnter spawns the mode's entities. Nothing at launch: `run` inserts
/// the first mode's documents.
pub(crate) fn arrive(world: &mut World) {
    let Some(arrival) = world.resource_mut::<Switcher>().arrival.take() else { return };
    let Arrival { scene, link, builder, learn, models, robot, place, unpark_inspect, unpark_builder, inspect, lessons, document: _ } = *arrival;
    let (parked_inspect, parked_builder) = {
        let mut docs = world.resource_mut::<Documents>();
        if inspect.is_some() {
            docs.inspect = inspect;
            docs.parked_inspect = None;
        }
        if lessons.is_some() {
            docs.lessons = lessons;
        }
        if builder.is_some() {
            // A new builder brings its own scene.
            docs.parked_builder = None;
        }
        (if unpark_inspect { docs.parked_inspect.take() } else { None }, if unpark_builder { docs.parked_builder.take() } else { None })
    };
    if let Some(parked) = parked_inspect {
        let (scene, link) = *parked;
        world.insert_resource(scene);
        if let Some(link) = link {
            world.insert_resource(link);
        }
    }
    if let Some(scene) = parked_builder {
        world.insert_resource(*scene);
    }
    if let Some(builder) = builder {
        if let Some(old) = world.remove_resource::<Builder>() {
            crate::jobs::drop_off_thread(old, "the replaced builder");
        }
        world.insert_resource(builder);
    }
    // Within Build/Lessons the scene stays: a new builder recompiles it (as a lesson's scene builder does).
    if let Some(scene) = scene.filter(|_| !world.contains_resource::<SpatialScene>()) {
        world.insert_resource(scene);
    }
    if let Some(link) = link {
        world.insert_resource(link);
    }
    if let Some(learn) = learn {
        if let Some(old) = world.remove_resource::<Learn>() {
            crate::jobs::drop_off_thread(old, "the replaced lessons");
        }
        world.insert_resource(learn);
    }
    if let Some(models) = models {
        world.insert_resource(models);
    }
    if let Some(robot) = robot {
        world.insert_resource(robot);
    }
    if let Some(place) = place {
        world.insert_resource(place);
    }
}

/// OnExit(Inspect): the inspected scene and its selection link are parked,
/// to come back as they were.
fn leave_inspect(world: &mut World) {
    let scene = world.remove_resource::<SpatialScene>();
    let link = world.remove_resource::<SelectionLink>();
    if let Some(scene) = scene {
        world.resource_mut::<Documents>().parked_inspect = Some(Box::new((scene, link)));
    }
    clear_render(world);
}

/// OnExit(Build/Lessons): the builder stays, paused (no physics runs
/// unseen), with its scene parked; the lesson and its jobs are dropped.
fn leave_builder(world: &mut World) {
    if let Some(scene) = world.remove_resource::<SpatialScene>() {
        world.resource_mut::<Documents>().parked_builder = Some(Box::new(scene));
    }
    if let Some(learn) = world.remove_resource::<Learn>() {
        world.resource_mut::<Documents>().lessons = Some((learn.dir.clone(), learn.slug().map(str::to_string)));
        crate::jobs::drop_off_thread(learn, "the lessons");
    }
    if let Some(mut builder) = world.get_resource_mut::<Builder>() {
        builder.leave_scope();
    }
    // The lesson reader's text size is theirs, not the other modes'.
    if let Some(mut scale) = world.get_resource_mut::<UiScale>() {
        if scale.0 != 1.0 {
            scale.0 = 1.0;
        }
    }
    clear_render(world);
}

/// A scene `render` still running belongs to the scene that was left.
fn clear_render(world: &mut World) {
    if let Some(mut rest) = world.get_resource_mut::<crate::rest::Rest>() {
        rest.1 = None;
    }
}

/// OnExit(Robot): the view (its run, gait and playback threads, its loads)
/// is removed and dropped off the UI thread.
fn leave_robot(world: &mut World) {
    if let Some(view) = world.remove_resource::<RobotView>() {
        world.resource_mut::<Documents>().robot = Some(view.document());
        crate::jobs::drop_off_thread(view, "the robot view");
    }
}

/// OnExit(Place): the walkthrough's model is dropped.
fn leave_place(world: &mut World) {
    if let Some(place) = world.remove_resource::<PlaceView>() {
        world.resource_mut::<Documents>().place = Some(place.dir.clone());
    }
}

/// OnEnter(Lessons): the lesson screen is shown (its own bookkeeping), and
/// the builder under it is paused (`Builder::pause_for_learn`): a live run is
/// paused and kept, not dropped (Run resumes it in build mode). Every entry
/// to Lessons from Build goes through [`handle`], which refuses on a draft
/// or drag (`leaving_blockers`); one begun in the frame between that check
/// and this state change is ended (a drag) or kept without keys (a draft).
fn show_lessons(learn: Option<ResMut<Learn>>, builder: Option<ResMut<Builder>>) {
    if let Some(mut learn) = learn {
        if let Some(mut builder) = builder {
            builder.pause_for_learn();
        }
        learn.show(true);
    }
}
/// OnExit(Lessons): the builder is shown (or the lesson is closed next).
fn hide_lessons(learn: Option<ResMut<Learn>>) {
    if let Some(mut learn) = learn {
        learn.show(false);
    }
}

/// A REST command that asks for a mode switch on the caller's behalf (the
/// builder's "‹ lesson" control, `lesson_open` and `lesson_screen` in build
/// mode): writes the same `WindowAction::Switch` a button writes, with a
/// nested reply kept in the continuation, and waits ([`awaited_switch`]).
pub(crate) fn ask_switch(out: &mut MessageWriter<Act<WindowAction>>, call: &mut actions::Call, mode: ViewerMode) -> Outcome {
    let reply = call.replies.open();
    out.write(Act { action: WindowAction::Switch(ModeSwitch { mode, document: None }), origin: Origin::Rest(reply) });
    *call.continuation = json!({"switch": reply.id()});
    Outcome::Pending
}

/// For a command waiting on [`ask_switch`]: None when it is not waiting;
/// Pending until the switch has answered; then the switch's result (its
/// summary, or the refusal naming the blocker). A cancel before the answer
/// is passed on to the switch, which answers "cancelled" if it had not yet
/// been applied or was still loading (its load is dropped); a switch already
/// accepted completes and says so, so the caller is not told "cancelled"
/// about a mode change that happened.
pub(crate) fn awaited_switch(call: &mut actions::Call) -> Option<Outcome> {
    let reply = actions::Reply::from_id(call.continuation.get("switch").and_then(Value::as_u64)?);
    let answer = call.replies.take(reply);
    Some(match answer {
        None if call.cancelled && !call.replies.waiting(reply) => Outcome::Done(Err("cancelled".into())),
        None => {
            if call.cancelled {
                call.replies.cancel(reply);
            }
            Outcome::Pending
        }
        Some(Outcome::Done(result)) => Outcome::Done(result),
        Some(_) => Outcome::Done(Err("the mode switch gave no answer".into())),
    })
}

/// Input: the lesson screen's toggles become switch actions.
fn lesson_screen_requests(learn: Option<ResMut<Learn>>, mode: Res<State<ViewerMode>>, mut switch: MessageWriter<Act<WindowAction>>) {
    let Some(mut learn) = learn else { return };
    if learn.screen_request().is_none() {
        return;
    }
    let Some(lessons) = learn.take_screen_request() else { return };
    let target = if lessons { ViewerMode::Lessons } else { ViewerMode::Build };
    if *mode.get() != target {
        switch.write(Act::ui(WindowAction::Switch(ModeSwitch { mode: target, document: None })));
    }
}

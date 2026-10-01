//! Switching modes in the running window (native-viewer.md §1), and the
//! window's other actions ([`WindowAction`]: `screenshot`, and `system_ui` in
//! modes without controls of their own).
//!
//! One request type ([`ModeSwitch`]) and one validating handler
//! ([`handle`], `ViewerSet::Actions`). Every entry point writes a
//! [`WindowAction`]: the mode switcher (`super::switcher`), `system_ui`
//! controls `mode:<mode>` and REST `viewer_mode` (routed by
//! `super::route`), the builder's Lessons button, the lesson screen's own
//! toggles ("Open in builder", "‹ lesson", `lesson_screen`, `lesson_open`),
//! which ask for Build or Lessons, and the document picker's choices
//! (`super::picker`: its entries, its path field and `system_ui`
//! `picker:*`), which ask for a mode with a document. There is no keyboard
//! binding for a mode switch.
//!
//! - **The document picker**: an interactive switch (a click: `Origin::Ui`,
//!   or a `system_ui` `mode:<mode>` activation) to a mode with no document
//!   and nothing to reopen (`prepare::missing_document`) opens the picker
//!   for that mode instead of refusing; the current mode stays (refused
//!   instead while robot mode's Leg calibration panel is open, whose STOP
//!   the modal picker would cover). REST
//!   `viewer_mode` without a document is refused as before (CAD falls back
//!   to RoboCAD at the default URL). A switch that names a document records
//!   it in the recent documents once entered (`super::recent::record_job`,
//!   on `Pool::Io`).
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
//!   `Documents` (the launch configuration), the document registry, `Rest`
//!   and the workspace root always survive.
//! - **Documents** are the registry's (`crate::document::DocumentRegistry`,
//!   native-viewer.md §7; [`sources`]): one entry per mode. [`arrive`] opens
//!   the target mode's entry (a reopen of the same document is a reload: its
//!   id kept, its revision + 1; another document gets a new id and the old
//!   one's selection items go) or unparks its scene; each scope's OnExit
//!   parks the scene (Inspect, the builder) or closes the entry, keeping the
//!   source the mode reopens.
//! - **REST** callers wait on their reply token (`actions::Replies`); a
//!   cancel stops a switch whose document is still loading.
mod arrival;
mod leave;
mod prepare;
pub mod sources;

use super::actions::{self, Act, Origin, Replies, Spec, spec};
use super::{ModeScope, ViewerMode, ViewerSet};
use crate::builder::Builder;
use crate::cad::CadDocument;
use crate::document::DocumentRegistry;
use crate::jobs::Job;
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
use arrival::enter;
pub(crate) use arrival::{arrive, finish_load};
use leave::{hide_lessons, leave_builder, leave_cad, leave_inspect, leave_place, leave_robot, show_lessons};
use prepare::{Prepared, leaving_blockers, missing_document, prepare};
use super::picker::{self, Picker};
use crate::ui_kit::text::TextFieldApp;

/// The document a mode opens with when it has none of its own yet.
#[derive(Clone, Debug, PartialEq)]
pub enum Document {
    /// A file or directory (by mode: `*.description.json`, `*.system.json`,
    /// a lessons folder, `*.simrobot.json`, a place directory).
    Path(PathBuf),
    /// A robot preset id (`robot_presets`).
    Preset(String),
    /// A running RoboCAD service to attach to (CAD mode only; loopback).
    Url(String),
}
impl Document {
    fn describe(&self) -> String {
        match self {
            Document::Path(p) => p.display().to_string(),
            Document::Preset(id) => format!("preset {id}"),
            Document::Url(url) => format!("RoboCAD at {url}"),
        }
    }
    /// The `viewer_mode` argument that names it: `{path}`, `{preset}` or `{url}`.
    pub(crate) fn json(&self) -> Value {
        match self {
            Document::Path(p) => json!({"path": p}),
            Document::Preset(id) => json!({"preset": id}),
            Document::Url(url) => json!({"url": url}),
        }
    }
}

/// A request to change the window's mode (the one request type).
#[derive(Clone, Debug, PartialEq)]
pub struct ModeSwitch {
    pub mode: ViewerMode,
    pub document: Option<Document>,
}
impl ModeSwitch {
    /// `viewer_mode {mode, path?, preset?, url?}`.
    pub(crate) fn from_args(args: Map<String, Value>) -> Result<Self, String> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Args {
            mode: String,
            #[serde(default)]
            path: Option<PathBuf>,
            #[serde(default)]
            preset: Option<String>,
            #[serde(default)]
            url: Option<String>,
        }
        let args: Args = serde_json::from_value(Value::Object(args)).map_err(|e| format!("viewer_mode: {e}"))?;
        let mode = ViewerMode::parse(&args.mode)?;
        let document = match (args.path, args.preset, args.url) {
            (Some(p), None, None) => Some(Document::Path(p)),
            (None, Some(id), None) => Some(Document::Preset(id)),
            (None, None, Some(url)) => Some(Document::Url(url)),
            (None, None, None) => None,
            _ => return Err("viewer_mode takes one of path, preset or url".into()),
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
            spec("viewer_mode", actions::ALL, json!({"mode":"robot","preset":"robot-measured-400hz"}), "Switch this window's mode, or with no mode (args {}) report it: active, modes, pending (a document still loading), entering, message (the switcher's last line) and documents (what each mode reopens: inspect description/spatial, lessons dir and lesson, robot path or preset, place, cad path or url, phenomena exhibit; library, models, presets; registry, the document registry's entries: id, kind, source, revision, mode, presence open | parked | remembered, parked). mode inspect | build | lessons | robot | place | cad | phenomena; optional path (inspect: a *.description.json with its *.spatial.json beside it; build: a *.system.json; lessons: a folder of <slug>/lesson.md; robot: a *.simrobot.json; place: a sim-place build directory; cad: a *.rcad file, on which this window starts RoboCAD's headless service and stops it when the document closes), preset (robot: an id listed by robot_presets) or url (cad: a running loopback RoboCAD service such as http://127.0.0.1:8420, attached to and never stopped). Without one, a mode reopens what it showed before in this window or at launch; inspect falls back to the example assembly and cad to RoboCAD at http://127.0.0.1:8420. Phenomena takes no document: it opens the built-in exhibits (sim_phenomena::exhibits) on the exhibit it last showed, else --exhibit's or $PHENOMENA_EXHIBIT's, else the first; select another with phenomena_select. Entering cad mode does not wait for RoboCAD: the connection (or the service's start) runs on its own job and shows in cad_state.connection and the mode's header. The same handler as the window's mode switcher, the builder's Lessons button and system_ui mode:<mode> (every mode). In the window, a mode chosen in the switcher (or with system_ui mode:<mode>) that has no document opens a document picker instead of being refused; its entries are the system_ui controls picker:<mode>:<n> (each listed with picker_revision, the picker's own revision, separate from the answer's ui_revision; an activation giving another picker_revision is refused, naming both: list the controls again; ui_revision is not checked for picker:* controls), picker:path (activate with an optional text, the path to open) and picker:close. Refused, naming the reason, with the current mode kept: no document for the target mode; leaving build or lessons, entering lessons from build (by any entry point: the switcher, the Lessons button, system_ui mode:lessons, viewer_mode, lesson_open or lesson_screen) or replacing the builder with a new lesson's, while a text or discussion draft, placement drag, study, replay, Codex answer or open is in progress (the builder's system_open blockers), or leaving build or lessons while a lesson draft or contact sheet is, and a new lesson replacing a build-mode builder with a live run; leaving robot mode while a recording is being written or a replay runs; leaving cad mode while a CAD edit is in flight or while a RoboCAD service this window started holds unsaved edits, or may hold edits whose saved state can't be confirmed because the window is not connected to it (save first; leaving an attached RoboCAD with unsaved edits is allowed and the result's message says it keeps them); another switch in progress; a path of the wrong kind; a document that fails to load (named). Documents load off the UI thread; poll the job. Build and lessons share the builder: switching between them keeps it and the lesson (the lesson screen is drawn over the builder), and entering lessons pauses a live build run and keeps it (Run resumes it in build mode). The builder stays in the window across every switch: leaving build/lessons pauses a live run and parks its scene; build with a path while it has another file open is refused (use system_open in build mode). Leaving lessons closes the lesson (its recordings and narration stop; lessons reopens it); leaving robot mode stops its run, gait and playback threads; leaving cad mode stops its poll and a RoboCAD service it started (never an attached one); leaving phenomena mode stops its exhibits' run thread (dropped off the UI thread: joined within 200 ms, else detached and ending at its next check) and remembers the exhibit; leaving inspect parks its scene and selection link. Commands of another mode are refused naming the active mode. Result: mode, previous, document, load_seconds (unchanged=true when the mode was already active)."),
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
        &["mode:inspect", "mode:build", "mode:lessons", "mode:robot", "mode:place", "mode:cad", "mode:phenomena", "picker:<mode>:<n>", "picker:path", "picker:close"]
    }
}
impl WindowAction {
    /// `system_ui` in modes without controls of their own.
    pub(crate) fn switcher_commands() -> Vec<Spec> {
        vec![spec("system_ui", actions::SWITCHER_ONLY, json!({"action":{"operation":"controls"}}), "Inspect and place mode: the mode switcher's controls mode:inspect | mode:build | mode:lessons | mode:robot | mode:place | mode:cad | mode:phenomena (controls; activate {id, ui_revision}, the same handler as viewer_mode and the switcher's buttons; ui_revision is not checked for mode:* controls). In build, lessons, robot, cad and phenomena mode system_ui is the mode's own (its controls list ends with these mode:* controls). While the document picker is open, every mode's controls list ends with its picker:<mode>:<n>, picker:path and picker:close controls, and activating them works in every mode; each picker:<mode>:<n> carries picker_revision, the picker's own revision, and an activation giving another one is refused (list the controls again); ui_revision is not checked for picker:* controls.")]
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
            _ => Err(format!("system_ui in {} mode has only the mode switcher's controls: operation controls, or activate with a mode:<mode> id (or a picker:* id while the document picker is open)", mode.name())),
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


/// The launch facts the loaders need. Shared: it survives every switch.
/// What each mode opens without being told is the document registry's
/// (`crate::document::DocumentRegistry`, [`sources`]).
#[derive(Resource)]
pub struct Documents {
    /// The palette library (`--library`, else `<workspace>/library/systems`).
    pub library: Result<PathBuf, String>,
    /// The display-model catalog.
    pub models: PathBuf,
    /// `--robot-presets` (None: `<workspace>/web/viewer/presets.json`).
    pub presets: Option<PathBuf>,
    /// The hardware servers given at launch (`--hardware`, `--motor-bench`), for robot mode's Leg calibration panel.
    pub hardware: crate::robot::hardware::HardwareConfig,
}
impl Default for Documents {
    fn default() -> Self {
        Self {
            library: crate::workspace::path("library/systems"),
            models: crate::workspace::path("library/models").unwrap_or_else(|_| PathBuf::from("models")),
            presets: None,
            hardware: Default::default(),
        }
    }
}

/// The one switch in progress and the latest outcome (REST callers wait on
/// their reply token, `actions::Replies`).
#[derive(Resource, Default)]
pub struct Switcher {
    pending: Option<Pending>,
    /// Loaded documents waiting for the target mode's OnEnter ([`arrive`]).
    arrival: Option<Box<Arrival>>,
    /// A switch whose `NextState` is set: (who asked, target, result once
    /// entered, the document the request named, recorded once entered).
    entering: Option<(Origin, ViewerMode, Value, Option<Document>)>,
    /// The latest outcome as the switcher shows it (refusals name the reason).
    pub message: Option<Result<String, String>>,
    /// Bumped whenever an outcome or a pending load changes.
    pub revision: u64,
}

struct Pending {
    origin: Origin,
    mode: ViewerMode,
    /// The document the request named (recorded once entered).
    document: Option<Document>,
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
    cad: Option<CadDocument>,
    /// The mode whose parked scene comes back (Inspect, or Build for the builder's).
    unpark: Option<ViewerMode>,
    /// The inspected assembly a load brought (description, spatial).
    inspect: Option<(PathBuf, PathBuf)>,
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
    pub fn json(&self, mode: ViewerMode, documents: &Documents, registry: &DocumentRegistry) -> Value {
        json!({
            "active": mode,
            "modes": ViewerMode::ALL,
            "pending": self.pending.as_ref().map(|p| json!({"mode": p.mode, "loading": p.what, "seconds": p.started.elapsed().as_secs_f64()})),
            "entering": self.entering.as_ref().map(|e| e.1),
            "message": self.message.as_ref().map(|m| match m { Ok(t) => json!({"ok": true, "text": t}), Err(e) => json!({"ok": false, "text": e}) }),
            "documents": documents.json(registry),
            "revision": self.revision,
        })
    }
}

/// Registers the handler, the teardown, the lesson screen requests and the
/// document picker's windowless parts (its state, keys, clicks and job
/// results; `picker::draw` is the core's, with the fonts). No window
/// needed: the state-transition tests run this on MinimalPlugins.
pub(crate) fn build(app: &mut App) {
    actions::register::<WindowAction>(app);
    app.init_resource::<Switcher>()
        .init_resource::<Replies>()
        .init_resource::<Documents>()
        .init_resource::<Picker>()
        // The picker's path field (the kit's; `UiKitPlugin` adds the input
        // system, the windowless tests only its message).
        .add_message::<crate::ui_kit::text::FieldMsg>()
        .add_text_field(picker::PATH, picker::path_text_field())
        // Modal keys: after Bevy's input systems and the kit's (which types
        // into the path field first), before anything reads them; the
        // picker's own draft changes reach the field before the kit types.
        .add_systems(PreUpdate, picker::sync.before(crate::ui_kit::text::input::keys))
        .add_systems(PreUpdate, picker::keys.after(bevy::input::InputSystems).after(crate::ui_kit::text::input::keys))
        .add_systems(Update, picker::clicks.in_set(ViewerSet::Input))
        .add_systems(Update, picker::receive.in_set(ViewerSet::JobResults))
        .add_systems(Update, (actions::serve, lesson_screen_requests).chain().in_set(ViewerSet::Input))
        .add_systems(Update, handle.in_set(ViewerSet::Actions))
        .add_systems(Update, finish_load.in_set(ViewerSet::JobResults))
        .add_systems(OnExit(ModeScope::Inspect), leave_inspect)
        .add_systems(OnExit(ModeScope::Builder), leave_builder)
        .add_systems(OnExit(ModeScope::Robot), leave_robot)
        .add_systems(OnExit(ModeScope::Place), leave_place)
        .add_systems(OnExit(ModeScope::Cad), leave_cad)
        .add_systems(OnExit(ModeScope::Phenomena), crate::phenomena::leave)
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
        if let Some((origin, target, summary, document)) = switch.entering.take() {
            if current == target {
                // A document the request named goes to the recent documents
                // (on Pool::Io; the handle completes on drop).
                if let Some(document) = document {
                    drop(super::recent::record_job(target, document));
                }
                switch.finish(world, origin, Ok(summary));
            } else {
                switch.entering = Some((origin, target, summary, document));
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
            // Interactive: a click in the window, or a `system_ui`
            // activation (a person's control, driven remotely). Only these
            // open the document picker for a mode with no document.
            let (request, interactive) = match action {
                WindowAction::Switch(request) => (Ok(request), origin == Origin::Ui),
                WindowAction::ViewerMode(args) if args.is_empty() => {
                    let mut status = switch.json(current, world.resource::<Documents>(), world.resource::<DocumentRegistry>());
                    status["picker"] = world.get_resource::<Picker>().map_or(Value::Null, Picker::json);
                    answer(world, origin, Ok(status));
                    continue;
                }
                WindowAction::ViewerMode(args) => (ModeSwitch::from_args(args), false),
                WindowAction::SystemUi(args) => {
                    if let Some((id, text, ui_revision)) = picker::activation(&args) {
                        match picker::activate(world, &id, text.as_deref(), ui_revision) {
                            Ok(picker::Activation::Switch(request)) => (Ok(request), true),
                            Ok(picker::Activation::Closed(mode)) => {
                                answer(world, origin, Ok(json!({"closed": mode, "message": format!("The {} document picker is closed.", mode.label())})));
                                continue;
                            }
                            Err(e) => {
                                answer(world, origin, Err(e));
                                continue;
                            }
                        }
                    } else {
                        match WindowAction::mode_control(&args) {
                            Some(request) => (request, true),
                            None => {
                                let result = WindowAction::switcher_ui(current, &args);
                                answer(world, origin, result);
                                continue;
                            }
                        }
                    }
                }
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
            if let Err(e) = start(world, &mut switch, current, origin, request, interactive) {
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

/// How a person opens `doc` in `mode` once it is active, in the window
/// (no REST payloads or command names: the switcher shows it too).
fn open_hint(mode: ViewerMode, doc: &str) -> String {
    match mode {
        ViewerMode::Build => format!("open {doc} from the Systems tab's Open"),
        ViewerMode::Lessons => format!("use the Lessons list; another lessons folder ({doc}) cannot be opened from within Lessons mode"),
        ViewerMode::Cad => format!("open {doc} with CAD mode's Open…"),
        ViewerMode::Phenomena => "phenomena mode takes no document; choose an exhibit in its gallery".into(),
        ViewerMode::Robot | ViewerMode::Inspect | ViewerMode::Place => format!("{} mode shows one document per visit and has no Open of its own yet, so {doc} is not opened", mode.label()),
    }
}

/// Validate and begin one switch. `interactive` (a click, or a `system_ui`
/// activation): a target with no document and nothing to reopen opens the
/// document picker instead of being refused.
fn start(world: &mut World, switch: &mut Switcher, current: ViewerMode, origin: Origin, request: ModeSwitch, interactive: bool) -> Result<(), String> {
    let target = request.mode;
    if let Some(p) = &switch.pending {
        return Err(refusal(target, current, &format!("the switch to {} mode is still loading {}", p.mode.label(), p.what)));
    }
    if let Some((_, m, ..)) = &switch.entering {
        return Err(refusal(target, current, &format!("the switch to {} mode is still being applied", m.label())));
    }
    if target == current {
        if let Some(d) = &request.document {
            return Err(refusal(target, current, &format!("{} mode is already active: {}", target.label(), open_hint(target, &d.describe()))));
        }
        switch.finish(world, origin, Ok(json!({"mode": target, "previous": current, "unchanged": true, "message": format!("{} mode is already active.", target.label())})));
        return Ok(());
    }
    let blockers = leaving_blockers(world, current, target);
    if !blockers.is_empty() {
        return Err(refusal(target, current, &blockers.join("; ")));
    }
    // Checked after the blockers, so a person is not asked to choose a
    // document for a switch that would be refused anyway.
    if interactive
        && request.document.is_none()
        && let Some(reason) = missing_document(world, target)
        && world.contains_resource::<Picker>()
    {
        // The picker is modal (it covers the window and takes the keys), so
        // the Leg calibration panel's STOP button and its Z/Escape keys
        // would be out of reach while it is open.
        if current == ViewerMode::Robot && world.get_resource::<crate::robot::hardware::Hardware>().is_some_and(|hw| hw.open) {
            return Err(refusal(target, current, "close the Leg calibration panel first (its STOP must stay reachable), then choose the mode again"));
        }
        // Already open for this mode: keep what was typed and found.
        if world.resource::<Picker>().open == Some(target) {
            let controls = world.resource::<Picker>().controls();
            let message = format!("Choose {} for {} mode in the picker.", picker::noun(target), target.label());
            answer(world, origin, Ok(json!({"picker": target, "message": message, "controls": controls})));
            return Ok(());
        }
        let message = format!("Choose {} for {} mode in the picker.", picker::noun(target), target.label());
        switch.message = Some(Ok(message.clone()));
        switch.revision += 1;
        let revision = switch.revision;
        let presets = world.resource::<Documents>().presets.clone();
        let mut open = world.resource_mut::<Picker>();
        open.open_for(target, current, reason, revision, presets);
        let controls = open.controls();
        answer(world, origin, Ok(json!({"picker": target, "message": message, "controls": controls})));
        return Ok(());
    }
    match prepare(world, current, &request).map_err(|e| refusal(target, current, &e))? {
        Prepared::Now(arrival) => enter(world, switch, origin, current, target, arrival, Instant::now(), request.document),
        Prepared::Load(what, work) => {
            switch.message = Some(Ok(format!("Switching to {} mode: loading {what} off the UI thread…", target.label())));
            switch.revision += 1;
            switch.pending = Some(Pending { origin, mode: target, document: request.document, what, work, started: Instant::now() });
        }
    }
    Ok(())
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

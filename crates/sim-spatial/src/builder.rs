//! Build mode: edit a `sim.system/1` file from the physical assembly.
//!
//! Every edit is a shared `sim_system::Command` applied through the file
//! store, so this window, the schematic, REST clients and the CLI share one
//! validation path and one undo history. The window watches the file and
//! recompiles through the shared runtime after any editor changes it.
//! Physics runs in a background thread on the same `SystemSession` the
//! schematic uses; this window never integrates anything itself.
use super::*;
use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use sim_core::BehaviorRegistry;
use sim_runtime::run_history::Fidelity;
use sim_runtime::system_builder;
use sim_system::library::{self, Alternative};
use sim_system::{Command as SystemCommand, InstanceKind, InstanceSpec, ReferenceView, Resolver, SystemDocument, SystemStore, Terminal};
use crate::document::DocumentRegistry;
use crate::selection::Selection;
use picked::Picked;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};

const PALETTE_ROWS: usize = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all="snake_case")]
pub enum Tab {
    Library,
    Discussions,
    Studies,
    Outline,
    References,
    /// Open another system file (build mode).
    Systems,
    /// Read-only accepted actuator registry and consumer staleness checks.
    Actuators,
    /// Read-only gait-lab results browser (reports and journal times).
    GaitLab,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all="snake_case")]
pub enum Mode {
    Annotate,
    Select,
    Connect,
}

#[derive(Clone, Debug)]
pub struct PaletteItem {
    pub label: String,
    pub detail: String,
    pub kind: InstanceKind,
    /// Library file to import before placing, for library definitions.
    pub library_path: Option<String>,
    pub domain: String,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all="snake_case")]
enum Purpose {
    Filter,
    Comment, CommentAuthor, ThreadTitle,
    GridSpacing,
    GridOrigin,
    Position,
    Parameter { name: String, parameter: String },
    Rename(String),
    ImportImage,
    Distance { id: String, first: [f32; 3], second: [f32; 3] },
    ReferenceWidth(String),
    /// "from to count" for a sweep of (instance, parameter).
    Sweep { name: String, parameter: String, observe: Vec<String> },
    /// Path of a system file to open in this window.
    OpenSystem,
    /// Path of an actuator registry to load (read-only).
    ActuatorRegistry,
    /// Path of a consumer file to check against the registry.
    ActuatorConsumer,
    /// Path of a gait-lab results folder to read (read-only).
    GaitResults,
}

#[derive(Clone, Debug)]
struct TextInput {
    purpose: Purpose,
    buffer: String,
}

#[derive(Default)]
struct RunShared {
    snapshot: Option<sim_inspect::live::LiveSnapshot>,
    running: bool,
    speed: f64,
    /// Recent (time, value) samples per subscribed observable, for graphs.
    history: BTreeMap<String, std::collections::VecDeque<[f64; 2]>>,
    /// Set by `Builder::run_reset` (with `history` cleared) and cleared by the
    /// run thread once `Command::Reset` has run. While set, the thread drops
    /// pre-reset samples instead of flushing them and `save_run` refuses, so
    /// nothing from before the reset reaches the graphs or a saved run.
    reset_pending: bool,
    /// A robot system's run (`robot_run`): the loaded system and the drive
    /// status, written by its run thread only. None for every other run.
    drive: Option<robot_run::RobotDrive>,
}

/// Simulated seconds of history kept for graphs.
pub(super) const HISTORY_SECONDS: f64 = 20.0;
const HISTORY_POINTS: usize = 4000;
const RUNNING_STATUS: &str = "Running on the shared runtime (background thread, paced to real time at most).";

enum RunControl {
    Start,
    Pause,
    /// Replace the set of observables the run records.
    Observe(Vec<String>),
    /// The document changed: swap the running model (state kept when the
    /// structure is the same). Carries the description id the viewer shows.
    Swap(Box<SystemDocument>, String),
    /// Advance one timestep (`Command::Step`; the session must be paused).
    Step,
    /// Rebuild at t = 0, paused (`Command::Reset`); clears the history.
    Reset,
    /// A robot system's twist request (`Builder::drive`), interpreted against
    /// its profile on the UI thread; the run thread applies every queued one
    /// in order before the next seam period (`robot_run`).
    Twist { request: sim_domain_control::drive::kinematics::BodyTwist, halt: bool },
}

/// A hand pushing on a running system through one of its load elements.
#[derive(Clone, Debug)]
pub(crate) struct Grab {
    /// `instance/path.parameter` of the load being varied.
    parameter: String,
    original: f64,
    /// Load change per pixel of horizontal drag.
    per_pixel: f64,
    dragged: f32,
    sent: std::time::Instant,
    applied: f64,
}

struct LiveRun {
    /// The run session: `run_thread` on its own thread; dropping it stops
    /// the session (closed channel) and joins within `jobs::JOIN_BOUND`.
    worker: crate::jobs::RunThread<RunControl, RunShared>,
    description_id: String,
    /// Fidelity chosen when the run started (the toggle may change mid-run).
    fidelity: Fidelity,
    /// Document the run simulates (the realtime profile for realtime runs),
    /// and whether it was edited live.
    document: SystemDocument,
    edited: bool,
    /// Hosted instances run on the shared drive host (`robot_run`), not the system session.
    robot: bool,
    /// The last Run (true) or Pause/Reset (false) sent: a nonzero drive
    /// request is refused unless it is true (commands are ordered, so the
    /// run thread sees that Run before the request).
    requested_running: bool,
    /// The last drive request sent, and the last one refused (`Builder::drive`).
    drive_requested: Option<(sim_domain_control::drive::kinematics::BodyTwist, bool)>,
    drive_refusal: Option<String>,
}

#[derive(Resource)]
pub struct Builder {
    reference: reference::Reference,
    agent: agent::Agent,
    discussion: discussion::Editor,
    ui_api: ui_api::Controls,
    action_error: Option<String>,
    drag: Option<placement::DragState>,
    store: SystemStore,
    registry: BehaviorRegistry,
    pub document: SystemDocument,
    stamp: Option<std::time::SystemTime>,
    checked: f64,
    /// Instance path whose definition is being edited ("" = top level).
    pub level: String,
    /// `Selection::changed` when the panels and highlight last followed it
    /// (`picked::track`); the selection itself is the shared one (`picked`).
    seen_selection: Option<u64>,
    palette: Vec<PaletteItem>,
    filter: String,
    page: usize,
    pub status: String,
    alternatives: Option<(String, Vec<Alternative>)>,
    /// First terminal of a connection being made.
    connect_from: Option<Terminal>,
    /// Instance whose ports are listed for connecting.
    port_menu: Option<String>,
    input: Option<TextInput>,
    calibrating: Option<(String, Vec<[f32; 3]>)>,
    library_dir: PathBuf,
    scene_dirty: bool,
    panel_dirty: bool,
    /// Sidebar scroll offset requested through system_ui, applied on the next rebuild.
    sidebar_scroll: Option<f32>,
    run: Option<LiveRun>,
    compile_error: Option<String>,
    findings: Vec<sim_system::Finding>,
    /// Subsystem instance paths -> world placement, for ghosting and drill-in framing.
    subsystems: BTreeMap<String, sim_system::flatten::WorldPlacement>,
    textures: BTreeMap<String, Handle<Image>>,
    /// Registry elements never change while running; computed once.
    elements: Vec<library::ElementEntry>,
    /// Background compile of the latest document (generation: its revision).
    job: Option<crate::jobs::Job<CompileResult>>,
    tab: Tab,
    category: Option<&'static str>,
    pub(super) mode: Mode,
    live_refresh: f64,
    fitted: bool,
    level_of_snaps: String,
    /// Library item whose card the inspector shows.
    preview: Option<PaletteItem>,
    /// What can attach to each port of the one selected instance, computed
    /// when the selection or document changes: (instance, revision, result).
    snaps: Option<(String, u64, Result<Vec<sim_system::snap::PortSuggestions>, String>)>,
    /// Ports whose full suggestion list is expanded.
    snap_expanded: BTreeSet<String>,
    /// Show the long explanation, equations and trade-offs in the inspector.
    show_notes: bool,
    /// Graph dock.
    graphs: graphs::Graphs,
    /// Background study and the latest result.
    pub(super) study: StudyState,
    /// Authored parts (`library/parts/*.part`), hot-reloaded into `registry`.
    parts: sim_parts::PartLibrary,
    /// Datasheet of the previewed element (`library/datasheets`), if any.
    pub(super) preview_sheet: Option<sim_runtime::bench::Datasheet>,
    /// Cached file scans (never on the UI thread's per-frame path).
    pub(super) updates: Vec<library::Stale>,
    pub(super) runs: Vec<(PathBuf, sim_runtime::run_history::RunSummary)>,
    pub(super) run_picks: BTreeSet<String>,
    /// Background replay of a saved run and the outcome per run id.
    pub(super) replay: ReplayState,
    /// Description of the last compiled scene (for labelling saved runs).
    last_description: Option<SystemDescription>,
    /// Frame-time measurement: (worst over the window, frames over 50 ms, window start).
    pub(super) frames: (f64, usize, f64),
    /// Run the realtime profile instead of the detailed model.
    pub(super) realtime: bool,
    pub(super) used_in: Option<(String, Vec<(String, usize)>)>,
    /// Set when a lesson opened this builder: the lesson's title (the
    /// toolbar then offers the way back).
    pub(crate) lesson: Option<String>,
    /// Alt-drag on a running system: the load being pushed and its own value.
    pub(crate) grab: Option<Grab>,
    /// Opening another system file (build mode only).
    open: open::OpenState,
    /// Read-only actuator registry inspector (Actuators tab).
    actuators: actuators::ActuatorState,
    /// Read-only gait-lab results browser (Gait lab tab).
    gait_lab: gait_lab::GaitLabState,
    /// Read-only identification archive review (Actuators tab → Measured evidence).
    calibration: calibration::CalibrationState,
    /// Which part of the Actuators tab is shown.
    actuator_view: calibration::ActuatorView,
    /// Read-only schematic pane of the current level (presentation only).
    pub(crate) schematic: schematic::Schematic,
}

#[derive(Default)]
pub(super) struct StudyState {
    job: Option<StudyJob>,
    pub result: Option<sim_runtime::system_study::StudyResult>,
    pub error: Option<String>,
}

/// A study in progress (progress: variants done); dropping it cancels the run.
struct StudyJob {
    name: String,
    work: crate::jobs::Job<sim_runtime::system_study::StudyResult>,
    total: usize,
}

#[derive(Default)]
pub(super) struct ReplayState {
    job: Option<ReplayJob>,
    /// Latest outcome per run id (including the running one).
    pub outcomes: BTreeMap<String, ReplayOutcome>,
}

/// A replay in progress; dropping it stops the rerun between steps.
struct ReplayJob {
    id: String,
    work: crate::jobs::Job<sim_runtime::run_history::ReplayReport>,
    started: std::time::Instant,
}

/// One replay of a saved run: headless, from time 0, with the record's
/// document and config, on the shared runtime.
#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct ReplayOutcome {
    pub id: String,
    /// running, done, cancelled or error.
    pub status: &'static str,
    pub max_rel_diff: Option<f64>,
    /// Recorded samples compared.
    pub samples: Option<usize>,
    pub error: Option<String>,
    /// Wall-clock seconds the replay took (so far, while running).
    pub wall_seconds: f64,
    /// Simulated seconds the record covers.
    pub duration: f64,
    pub seed: u64,
    /// Fidelity of the recorded run ("detailed", "realtime", or not recorded for sim.run/1).
    pub fidelity: String,
    /// The recorded document changed mid-run: the record holds the final
    /// document, so no single document produced its series.
    pub edited_while_running: bool,
}

/// Everything the scene needs from one compile, built off the UI thread.
struct CompileResult {
    revision: u64,
    result: Result<CompileOutput, String>,
    /// What the resolver can say about an unfinished system when it does not compile.
    findings: Vec<sim_system::Finding>,
}
struct CompileOutput {
    description: SystemDescription,
    spatial: SpatialDescription,
    animation: Option<sim_inspect::animation::AnimationDescription>,
    findings: Vec<sim_system::Finding>,
    subsystems: BTreeMap<String, sim_system::flatten::WorldPlacement>,
    runtime_error: Option<String>,
}

impl Builder {
    pub fn open(path: PathBuf, library_dir: PathBuf, mut registry: BehaviorRegistry) -> Result<Self, String> {
        // Authored parts live next to the saved subsystems: library/parts.
        let library_dir_parts = library_dir.parent().map(|p| p.join("parts")).unwrap_or_else(|| library_dir.join("parts"));
        // Load them before validating the document, which may place them.
        let mut first = sim_parts::PartLibrary::new(library_dir_parts.clone());
        first.refresh(&mut registry);
        let store = SystemStore::new(path);
        let document = store.load_valid(&registry).map_err(|e| e.to_string())?;
        let mut builder = Self {
            reference:Default::default(),
            agent: agent::Agent::open(&store.path, &registry),
            stamp: store.stamp(),
            discussion: discussion::Editor { author: "User".into(), open_only:true, ..Default::default() },
            ui_api:Default::default(),
            action_error:None,
            drag: None,
            store,
            registry,
            document,
            checked: 0.,
            level: String::new(),
            seen_selection: None,
            palette: Vec::new(),
            filter: String::new(),
            page: 0,
            status: "Build mode: pick a component from the palette to place it.".into(),
            alternatives: None,
            connect_from: None,
            port_menu: None,
            input: None,
            calibrating: None,
            library_dir,
            scene_dirty: true,
            panel_dirty: true,
            sidebar_scroll: None,
            run: None,
            compile_error: None,
            findings: Vec::new(),
            subsystems: BTreeMap::new(),
            textures: BTreeMap::new(),
            elements: Vec::new(),
            job: None,
            tab: Tab::Library,
            category: None,
            mode: Mode::Select,
            live_refresh: 0.,
            fitted: false,
            level_of_snaps: String::new(),
            preview: None,
            snaps: None,
            snap_expanded: BTreeSet::new(),
            show_notes: true,
            graphs: graphs::Graphs::default(),
            study: StudyState::default(),
            parts: first,
            preview_sheet: None,
            updates: Vec::new(),
            used_in: None,
            runs: Vec::new(),
            run_picks: BTreeSet::new(),
            replay: ReplayState::default(),
            last_description: None,
            frames: (0., 0, 0.),
            realtime: false,
            lesson: None,
            grab: None,
            open: Default::default(),
            actuators: Default::default(),
            gait_lab: Default::default(),
            calibration: Default::default(),
            actuator_view: Default::default(),
            schematic: Default::default(),
        };
        builder.runs = sim_runtime::run_history::list(&sim_runtime::run_history::dir_for(&builder.store.path));
        builder.updates = builder.library_updates();
        let errors: Vec<String> = builder.parts.last.iter().filter_map(|l| l.error.clone()).collect();
        if !errors.is_empty() {
            builder.status = format!("Part error: {}", errors.join(" | "));
        }
        builder.elements = library::elements(&builder.registry);
        builder.refresh_palette();
        Ok(builder)
    }

    /// Load changed `.part` files; on any change refresh the palette and
    /// recompile. Load errors (with file and line) go to the status bar.
    pub fn reload_parts(&mut self) -> bool {
        let Some(changed) = self.parts.refresh(&mut self.registry) else { return false };
        let errors: Vec<String> = changed.iter().filter_map(|l| l.error.clone()).collect();
        let ok: Vec<String> = changed.iter().filter_map(|l| l.type_id.clone()).collect();
        self.elements = library::elements(&self.registry);
        self.refresh_palette();
        self.snaps = None;
        self.scene_dirty = true;
        self.panel_dirty = true;
        self.status = if errors.is_empty() { format!("Loaded parts: {}", ok.join(", ")) } else { format!("Part error: {}", errors.join(" | ")) };
        true
    }

    /// Authored part files and their load results.
    pub fn parts_json(&self) -> serde_json::Value {
        serde_json::json!({"dir": self.parts.dir, "parts": self.parts.last})
    }

    /// The system file this builder edits.
    pub fn path(&self) -> &std::path::Path {
        &self.store.path
    }
    /// The latest document has been compiled into the scene (or failed).
    pub(crate) fn compile_settled(&self) -> bool {
        self.job.is_none() && !self.scene_dirty
    }
    pub(crate) fn compile_error(&self) -> Option<&str> {
        self.compile_error.as_deref()
    }
    pub(crate) fn registry(&self) -> &BehaviorRegistry {
        &self.registry
    }
    /// Leaving Build and Lessons for another mode: the builder stays in the
    /// window, paused (no physics runs unseen). On return its chrome is
    /// rebuilt and its scene recompiled and respawned (parts, nets and the
    /// reference images only `rebuild_scene` draws; the scope's entities
    /// were despawned on exit).
    pub(crate) fn leave_scope(&mut self) {
        if self.running() {
            self.run_pause();
        }
        self.panel_dirty = true;
        self.scene_dirty = true;
    }

    /// Entering Lessons: the lesson screen is drawn over the builder and plays
    /// its own recorded runs. A live run is paused and kept, not dropped (Run
    /// resumes it back in build mode; `sync_run` leaves the scene to the
    /// lesson meanwhile); a connect or annotate mode ends. No draft or drag
    /// can be open: the mode switch refuses to enter Lessons from Build on
    /// them (`switch_blockers`).
    pub(crate) fn pause_for_learn(&mut self) {
        if self.running() {
            self.run_pause();
        }
        // A drag begun after the switch was validated (the state changes a
        // frame later) ends here, as it did before. A draft is kept: its kit
        // field is refocused (`drafts::sync_field`, build mode only) when build mode returns.
        self.drag = None;
        self.connect_from = None;
        self.mode = Mode::Select;
        self.panel_dirty = true;
    }

    /// `system_state`; `selected` is the builder's selection (`picked::names`;
    /// a REST answer uses `Picked::state`).
    pub fn state_json(&self, selected: &BTreeSet<String>) -> serde_json::Value {
        serde_json::json!({
            "workspace":crate::workspace::json(),
            "reference":self.reference.json(),
            "agent":self.agent.state.public(),
            "ui":self.ui_state(),
            "display_semantics": sim_system::display::SEMANTICS,
            "overlap_policy":sim_system::display_overlap::POLICY,
            "display_frame": {"unit":"m","up":"+Y","relative_to":"enclosing_definition","level":self.level},
            "grid": self.grid(),
            "display_preview":self.drag.as_ref().map(|d|serde_json::json!({"target_m":d.target.to_array(),"names":d.names,"level":d.level,"revision":d.revision,"released":d.released})),
            "annotation_markers":self.discussion.markers,
            "annotation_tool":self.mode==Mode::Annotate,
            "discussion_selected":self.discussion.selected,
            "discussion_draft":self.input.as_ref().filter(|i|matches!(i.purpose,Purpose::Comment|Purpose::ThreadTitle)).map(|i|serde_json::json!({"thread":self.discussion.selected,"body":i.buffer})),
            "placements": self.definition().map(|d|d.instances.into_iter().map(|(n,i)|(n,i.placement)).collect::<BTreeMap<_,_>>()),
            "discussions": self.document.discussions,
            "path": self.store.path,
            "title": self.document.title,
            "revision": self.document.revision,
            "level": self.level,
            "definition": self.definition_id(),
            "selected": selected,
            "status": self.status,
            "findings": self.findings,
            "compile_error": self.compile_error,
            "compiling": self.job.is_some() || self.scene_dirty,
            "running": self.running(),
            "realtime": self.realtime,
            "live_run": self.live_run_json(),
            "frame_ms": {"worst_in_window": 1e3 * self.frames.0, "over_50ms_in_window": self.frames.1},
            "runs": self.runs.iter().map(|(_, s)| s).collect::<Vec<_>>(),
            "replay": self.replay_json(),
            "open": self.open_json(),
            "actuators": self.actuators_json(),
            "gait_reports": self.gait_reports_json(),
            "actuator_view": self.actuator_view,
            "calibration_review": self.calibration_json(),
            "schematic": self.schematic.json(self.document.revision, &self.level, selected),
            "history": self.store.history(),
        })
    }
}

#[derive(Component)]
pub(super) struct BuilderPanel;
#[derive(Component)]
pub(super) struct ReferenceQuad(String);

/// The System Builder: Build mode, and the builder under the lesson screen
/// (Lessons). Its systems run in the Builder scope (Build and Lessons), in
/// their original order; those that draw or edit the builder's own chrome
/// run in Build only (the lesson screen is shown over it in Lessons).
pub struct BuilderPlugin;
impl Plugin for BuilderPlugin {
    fn build(&self, app: &mut App) {
        let building = in_state(ViewerMode::Build);
        app.add_plugins(calibration::study::StudyPlugin);
        crate::app::actions::register::<system_actions::SystemAction>(app);
        drafts::add_fields(app);
        // Buttons and keys write the builder's actions (after REST's, as the old chain
        // applied them); its one handler applies them and REST's in Actions.
        app.add_systems(Update, (actions::buttons, actions::keys.run_if(building.clone()).run_if(not(crate::ui_kit::text::typing))).chain().in_set(crate::app::InputSet::Window).run_if(in_state(ModeScope::Builder)))
            .add_systems(Update, system_actions::apply.in_set(ViewerSet::Actions).run_if(in_state(ModeScope::Builder)));
        // Drive devices (`crate::drive_input`'s one poller, InputSet::Window): Build mode's target
        // is written before it, and its requests for Build mode go through the one drive apply
        // after the builder's apply; nothing is offered once Build mode is left.
        app.add_systems(Update, (robot_run::drive_target.in_set(ViewerSet::Input).before(crate::app::InputSet::Window), actions::drive_devices.in_set(ViewerSet::Actions).after(system_actions::apply)).run_if(building.clone()))
            .add_systems(OnExit(ViewerMode::Build), actions::leave_drive);
        app.add_systems(
            Update,
            (frame_timing, watch, agent::tick, reference::tick, sync_field.run_if(building.clone()), drops.run_if(building.clone()), grab_push.run_if(building.clone()), open_system, picked::track, (finish_actuators, finish_gait_reports, finish_calibration, calibration::update_chart.run_if(building.clone())).chain(), rebuild_scene, sync_run, graphs::update.run_if(building.clone()), schematic::update.run_if(building.clone()), ui::rebuild_panel.run_if(building.clone()), ui::scroll_panels.run_if(building.clone()), clear_for_learn.run_if(in_state(ViewerMode::Lessons)))
                .chain()
                // Docks and home requests reach the shared camera before it is placed.
                .before(crate::inspect_view::InspectViewSet::Camera)
                .before(crate::inspect_view::InspectViewSet::Parts)
                .in_set(ViewerSet::SimSync)
                .run_if(in_state(ModeScope::Builder)),
        )
        // Picks and handles read the placed camera.
        .add_systems(Update, placement::update.after(crate::inspect_view::InspectViewSet::Parts).after(crate::camera::CameraSet::Place).in_set(ViewerSet::SimSync).run_if(building.clone()))
        .add_systems(Update, (placement::apply_preview, placement::draw_handles).chain().after(placement::update).in_set(ViewerSet::SimSync).run_if(building.clone()))
        .add_systems(Update, discussion::hover.after(crate::inspect_view::InspectViewSet::Notes).in_set(ViewerSet::SimSync).run_if(building.clone()))
        .add_systems(Update, markers::sync.after(placement::apply_preview).after(discussion::hover).in_set(ViewerSet::SimSync).run_if(building.clone()))
        .add_systems(Update, ui_api::collect.after(markers::sync).after(ui::rebuild_panel).in_set(ViewerSet::SimSync).run_if(building))
        .add_observer(placement::end_drag);
    }
}

mod reference;
pub(crate) mod actions;
pub(crate) mod system_actions;
pub(crate) use actions::BuildAction;
use actions::dispatch;
pub(crate) mod agent;
pub(crate) mod graphs;
mod markers;
pub(crate) mod ui_api;
pub(crate) mod placement;
mod placement_worker;
pub(crate) mod discussion;
pub(crate) mod ui;
pub mod open;
pub mod actuators;
pub mod gait_lab;
pub mod calibration;
pub(crate) mod schematic;
mod background;
mod drafts;
mod editing;
pub(crate) mod picked;
mod live_run;
mod rebuild;
mod robot_run;
mod studies;
use background::{finish_actuators, finish_calibration, finish_gait_reports, frame_timing, open_system, watch};
use drafts::{drops, sync_field};
use live_run::{grab_push, sync_run};
pub use rebuild::compiled_scene;
pub(super) use rebuild::click_part;
use rebuild::{clear_for_learn, compile_now, rebuild_scene};

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod replay_tests;

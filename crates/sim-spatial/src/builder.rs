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
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use sim_core::BehaviorRegistry;
use sim_runtime::run_history::Fidelity;
use sim_runtime::system_builder;
use sim_system::library::{self, Alternative};
use sim_system::{Command as SystemCommand, InstanceKind, InstanceSpec, ReferenceView, Resolver, SystemDocument, SystemStore, Terminal};
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
    /// Path of an identification archive folder to review (read-only).
    CalibrationArchive,
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
}

impl LiveRun {
    /// The one way a run session starts: `run_thread` on a `RunThread`.
    fn spawn(document: SystemDocument, registry: BehaviorRegistry, observed: Vec<String>, description_id: String, fidelity: Fidelity) -> Self {
        let initial = RunShared { snapshot: None, running: true, speed: 1.0, history: BTreeMap::new(), reset_pending: false };
        let (simulated, source_id) = (document.clone(), description_id.clone());
        let worker = crate::jobs::RunThread::spawn("builder-run", initial, move |commands, shared| run_thread(simulated, registry, observed, source_id, commands, shared));
        Self { worker, description_id, fidelity, document, edited: false }
    }
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
    /// Selected instance names at `level`.
    pub selected: BTreeSet<String>,
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

impl ReplayOutcome {
    /// One-line result for the Studies tab.
    pub fn headline(&self) -> String {
        match (self.status, self.max_rel_diff, &self.error) {
            ("running", ..) => format!("Replaying… {:.1} s", self.wall_seconds),
            ("cancelled", ..) => "Cancelled · replay stopped, no result".into(),
            (_, Some(d), _) if d == 0. => format!("Reproduced exactly · max rel diff 0 · {} samples · {}", self.samples.unwrap_or(0), self.fidelity),
            (_, Some(d), _) => format!("Differs · max rel diff {d:.2e} · {} samples · {}", self.samples.unwrap_or(0), self.fidelity),
            (_, _, Some(e)) => format!("Replay failed: {e}"),
            _ => String::new(),
        }
    }
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

fn compile_job(document: SystemDocument, registry: BehaviorRegistry) -> crate::jobs::Job<CompileResult> {
    crate::jobs::Job::spawn(crate::jobs::Pool::Compute, document.revision, "the compile", move |_| Ok(compile_now(document, registry)))
}

/// One compile of `document` for the scene (call off the UI thread).
fn compile_now(document: SystemDocument, registry: BehaviorRegistry) -> CompileResult {
    let config = system_builder::config_for(&document);
    let result = system_builder::compile(&document, &registry, config.clone()).map(|compiled| {
        let runtime_error = sim_compile::Runtime::new(compiled.flat.model.clone(), &registry, config.integrator).err().map(|e| system_builder::locate(&compiled.flat, e.to_string()));
        CompileOutput {
            spatial: compiled.spatial.clone().unwrap_or_else(|| compiled.flat.spatial(&compiled.description.id, &document.title)),
            description: compiled.description,
            animation: compiled.animation,
            findings: compiled.flat.findings.clone(),
            subsystems: compiled.flat.subsystems.clone(),
            runtime_error,
        }
    });
    // A system that is still being wired fails to compile on its first
    // loose port; say which ports are loose instead of the solver's id.
    let findings = if result.is_err() { sim_system::Resolver::new(&document, &registry).findings() } else { Vec::new() };
    let loose: Vec<&str> = findings.iter().filter(|f| f.code == "unconnected_port").map(|f| f.message.trim_end_matches(" is not connected")).collect();
    let result = result.map_err(|e| {
        if loose.is_empty() || !e.contains("is not connected") {
            e
        } else {
            format!("{} unconnected port{}: {}{}", loose.len(), if loose.len() == 1 { "" } else { "s" }, loose.iter().take(4).copied().collect::<Vec<_>>().join(", "), if loose.len() > 4 { ", …" } else { "" })
        }
    });
    CompileResult { revision: document.revision, result, findings }
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
            selected: BTreeSet::new(),
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

    pub fn typing(&self) -> bool {
        self.input.is_some()
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
        // frame later) ends here, as it did before. A draft is kept: its keys
        // stop (text_input runs in build mode only) until build mode returns.
        self.drag = None;
        self.connect_from = None;
        self.mode = Mode::Select;
        self.panel_dirty = true;
    }

    fn refresh_palette(&mut self) {
        let mut items = Vec::new();
        for id in self.document.definitions.keys() {
            if *id == self.document.root || self.definition_id().as_deref() == Some(id.as_str()) {
                continue;
            }
            let d = &self.document.definitions[id];
            items.push(PaletteItem { label: d.label.clone(), detail: format!("subsystem - {id}"), kind: InstanceKind::Subsystem { definition: id.clone() }, library_path: None, domain: ui::interface_category(d.interface.as_deref()).into() });
        }
        if let Ok(entries) = library::list(&self.library_dir, &self.registry) {
            for e in entries {
                if self.document.definitions.contains_key(&e.id) {
                    continue;
                }
                items.push(PaletteItem { label: e.label, detail: format!("library - {}", e.id), kind: InstanceKind::Subsystem { definition: e.id }, library_path: Some(e.path), domain: ui::interface_category(e.interface.as_deref()).into() });
            }
        }
        for e in &self.elements {
            items.push(PaletteItem { label: e.display_name.clone(), detail: e.component_type.clone(), kind: InstanceKind::Element { component_type: e.component_type.clone() }, library_path: None, domain: e.domain.clone() });
        }
        self.palette = items;
    }

    fn filtered(&self) -> Vec<&PaletteItem> {
        let f = self.filter.to_lowercase();
        self.palette
            .iter()
            .filter(|p| self.category.is_none_or(|c| ui::category(&p.domain) == c || (c == "Subsystems" && matches!(p.kind, InstanceKind::Subsystem { .. }))))
            .filter(|p| f.is_empty() || p.label.to_lowercase().contains(&f) || p.detail.to_lowercase().contains(&f))
            .collect()
    }

    fn definition(&self) -> Option<sim_system::Definition> {
        self.document.definitions.get(&self.definition_id()?).cloned()
    }

    fn definition_id(&self) -> Option<String> {
        Resolver::new(&self.document, &self.registry).definition_id_at(&self.level).ok()
    }

    /// Apply shared commands through the file store (with undo).
    pub fn apply(&mut self, label: &str, commands: Vec<SystemCommand>) -> Result<sim_system::store::Applied, String> {
        let result = self.store.apply(&self.registry, label, &commands, Some(self.document.revision));
        match result {
            Ok(applied) => {
                self.status = applied.outcomes.iter().map(|o| {
                    if o.shared_by > 1 { format!("{} (shared by {} placements)", o.message, o.shared_by) } else { o.message.clone() }
                }).collect::<Vec<_>>().join("; ");
                self.reload();
                Ok(applied)
            }
            Err(sim_system::SystemError::Stale { .. }) => {
                self.reload();
                Err("Another editor changed the system; reloaded. Try again.".into())
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn report(&mut self, result: Result<impl Sized, String>) {
        if let Err(e) = result {
            self.action_error=Some(e.clone());
            self.status = e;
            self.panel_dirty = true;
        }
    }

    pub fn reload(&mut self) {
        match self.store.load_valid(&self.registry) {
            Ok(document) => {
                let needs_scene=sim_system::display::scene_hash(&document)!=sim_system::display::scene_hash(&self.document);
                let previous = self.document.revision;
                self.document = document;
                if self.input.is_none() && self.discussion.selected.as_ref().is_some_and(|id|!self.document.discussions.threads.contains_key(id)){self.discussion.selected=None;}
                self.stamp = self.store.stamp();
                if self.definition_id().is_none() {
                    self.level.clear();
                }
                let names = self.definition_id().and_then(|id| self.document.definitions.get(&id).map(|d| d.instances.keys().cloned().collect::<BTreeSet<_>>())).unwrap_or_default();
                self.selected.retain(|s| names.contains(s));
                self.alternatives = None;
                self.refresh_palette();
                self.updates = self.library_updates();
                self.used_in = None;
                self.scene_dirty |= needs_scene;
                // No compile follows a scene-neutral edit: carry the schematic's compiled source forward.
                if !self.scene_dirty && self.job.is_none() { self.schematic.advance_revision(previous, self.document.revision); }
                self.panel_dirty = true;
            }
            Err(e) => self.status = format!("Could not load the system: {e}"),
        }
    }

    /// Shared-history step; the outcome is shown in the status bar and
    /// returned so REST callers see refusals ("nothing to undo", stale file).
    pub fn undo(&mut self) -> Result<sim_system::store::Applied, String> {
        let result = self.store.undo().map_err(|e| e.to_string());
        match &result {
            Ok(applied) => {
                self.status = applied.outcomes[0].message.clone();
                self.reload();
            }
            Err(e) => self.status = e.clone(),
        }
        self.panel_dirty = true;
        result
    }

    /// Shared-history step; the outcome is shown in the status bar and
    /// returned so REST callers see refusals ("nothing to redo", stale file).
    pub fn redo(&mut self) -> Result<sim_system::store::Applied, String> {
        let result = self.store.redo().map_err(|e| e.to_string());
        match &result {
            Ok(applied) => {
                self.status = applied.outcomes[0].message.clone();
                self.reload();
            }
            Err(e) => self.status = e.clone(),
        }
        self.panel_dirty = true;
        result
    }

    pub fn set_level(&mut self, path: &str) -> Result<(), String> {
        Resolver::new(&self.document, &self.registry).definition_id_at(path).map_err(|e| e.to_string())?;
        self.level = path.trim_matches('/').to_string();
        self.selected.clear();
        self.alternatives = None;
        self.port_menu = None;
        self.connect_from = None;
        self.refresh_palette();
        self.scene_dirty = true;
        self.panel_dirty = true;
        Ok(())
    }

    fn full_path(&self, name: &str) -> String {
        sim_system::join_path(&self.level, name)
    }

    /// The instance at this level containing a flattened component path.
    pub fn instance_for_component(&self, component: &str) -> Option<String> {
        schematic::instance_at(&self.level, component)
    }

    fn unique_name(&self, base: &str) -> String {
        let taken: BTreeSet<String> = self.definition_id().and_then(|id| self.document.definitions.get(&id)).map(|d| d.instances.keys().cloned().collect()).unwrap_or_default();
        let base: String = base.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
        let base = base.trim_matches('_').chars().take(24).collect::<String>();
        let base = if base.is_empty() { "part".to_string() } else { base };
        (1..).map(|n| format!("{base}{n}")).find(|c| !taken.contains(c)).unwrap()
    }

    /// Propose the next slot on this level's grid; shared validation checks clearance.
    fn place(&mut self, item: PaletteItem) {
        let count=self.definition().map(|d|d.instances.len()).unwrap_or(0);
        let grid=self.grid(); let (a,b,_)=grid.plane.axes(); let mut p=grid.origin_m;
        p[a]+=(count%6) as f32*grid.spacing_m*3.;p[b]+=(count/6) as f32*grid.spacing_m*3.;
        self.place_at(item,p);
    }
    fn placement_commands(&self, item: &PaletteItem, position: [f32;3]) -> Result<(String, Vec<SystemCommand>), String> {
        let mut commands = Vec::new();
        if let Some(path) = &item.library_path {
            let definitions=library::import(std::path::Path::new(path)).map_err(|e|e.to_string())?;
            commands.push(SystemCommand::AddDefinitions { definitions });
        }
        let base = match &item.kind {
            InstanceKind::Element { component_type } => component_type.rsplit('.').next().unwrap_or("part"),
            InstanceKind::Subsystem { definition } => definition.rsplit('.').next().unwrap_or("sub"),
        };
        let name = self.unique_name(base);
        let spec = sim_system::snap::starter(&self.registry, &item.kind, &item.label).at(position);
        commands.push(SystemCommand::AddInstance { at: self.level.clone(), name: name.clone(), instance: spec });
        Ok((name,commands))
    }
    fn place_at(&mut self, item: PaletteItem, position: [f32;3]) {
        let result = self.placement_commands(&item, position).and_then(|(name,commands)| {
            self.apply(&format!("Place {}", item.label), commands).map(|applied| {
                self.selected = BTreeSet::from([name]);
                applied
            })
        });
        self.report(result);
    }

    /// Port suggestions for `name` at this level, cached per revision.
    pub(super) fn suggestions(&mut self, name: &str) -> Result<Vec<sim_system::snap::PortSuggestions>, String> {
        let fresh = self.snaps.as_ref().is_some_and(|(n, r, _)| n == name && *r == self.document.revision) && self.level_of_snaps == self.level;
        if let Some(InstanceKind::Subsystem { definition }) = self.spec(name).map(|s| s.kind) {
            if self.used_in.as_ref().is_none_or(|(d, _)| *d != definition) {
                self.used_in = Some((definition.clone(), self.where_used(&definition)));
            }
        }
        if !fresh {
            let result = sim_system::snap::suggestions(&self.document, &self.registry, Some(&self.library_dir), &self.level, name).map_err(|e| e.to_string());
            self.snaps = Some((name.to_string(), self.document.revision, result));
            self.level_of_snaps = self.level.clone();
        }
        self.snaps.as_ref().unwrap().2.clone()
    }

    fn cached_suggestions(&self, name: &str) -> Option<&Vec<sim_system::snap::PortSuggestions>> {
        self.snaps.as_ref().filter(|(n, r, _)| n == name && *r == self.document.revision).and_then(|(_, _, s)| s.as_ref().ok())
    }

    /// Attach a suggested candidate to `name.port` as one undoable edit.
    pub fn snap(&mut self, name: &str, port: &str, candidate: &sim_system::snap::Candidate) -> Result<String, String> {
        if let Some(conflict) = &candidate.conflict {
            return Err(conflict.clone());
        }
        let base = match &candidate.kind {
            InstanceKind::Element { component_type } => component_type.rsplit('.').next().unwrap_or("part").to_string(),
            InstanceKind::Subsystem { definition } => definition.rsplit('.').next().unwrap_or("sub").to_string(),
        };
        let new_name = self.unique_name(&base);
        let commands = sim_system::snap::snap(&self.document, &self.registry, &self.level, name, port, candidate, &new_name).map_err(|e| e.to_string())?;
        self.apply(&format!("Snap {} to {name}.{port}", candidate.label), commands)?;
        self.selected = BTreeSet::from([new_name.clone()]);
        self.preview = None;
        self.scene_dirty = true;
        self.panel_dirty = true;
        Ok(new_name)
    }

    /// The palette entry for a registry element or definition, for cards.
    fn icon(&self, kind: &InstanceKind) -> String {
        match kind {
            InstanceKind::Element { component_type } => self.elements.iter().find(|e|&e.component_type==component_type).map(|e|e.icon.clone()).unwrap_or_else(||sim_core::icons::for_type(component_type).into()),
            InstanceKind::Subsystem { definition } => sim_core::icons::resolve(self.document.definitions.get(definition).map(|d|d.icon.as_str()).unwrap_or(""), definition).into(),
        }
    }
    fn palette_item(&self, kind: &InstanceKind) -> Option<PaletteItem> {
        self.palette.iter().find(|p| p.kind == *kind).cloned()
    }

    fn datasheet(&self, component_type: &str) -> Option<sim_runtime::bench::Datasheet> {
        let dir = self.library_dir.parent().map(|p| p.join("datasheets"))?;
        serde_json::from_slice(&std::fs::read(sim_runtime::bench::path(&dir, component_type)).ok()?).ok()
    }

    fn load_preview_sheet(&mut self) {
        self.preview_sheet = match self.preview.as_ref().map(|p| p.kind.clone()) {
            Some(InstanceKind::Element { component_type }) => self.datasheet(&component_type),
            _ => None,
        };
    }

    pub(crate) fn element_entry(&self, component_type: &str) -> Option<&library::ElementEntry> {
        self.elements.iter().find(|e| e.component_type == component_type)
    }

    fn only_selected(&self) -> Option<String> {
        (self.selected.len() == 1).then(|| self.selected.iter().next().unwrap().clone())
    }

    fn spec(&self, name: &str) -> Option<InstanceSpec> {
        let id = self.definition_id()?;
        self.document.definitions.get(&id)?.instances.get(name).cloned()
    }

    fn nudge(&mut self, delta: [f32; 3]) {
        let names: Vec<String> = self.selected.iter().cloned().collect();
        let mut commands = Vec::new();
        for name in names {
            if let Some(spec) = self.spec(&name) {
                let mut placement = spec.placement.clone();
                for k in 0..3 {
                    placement.position[k] += delta[k];
                }
                commands.push(SystemCommand::MoveInstance { at: self.level.clone(), name, placement });
            }
        }
        if !commands.is_empty() {
            let r = self.apply("Move", commands);
            self.report(r);
        }
    }

    fn group_selected(&mut self) {
        if self.selected.is_empty() {
            self.status = "Select instances to group (shift-click to add).".into();
            return;
        }
        let name = self.unique_name("group");
        let prefix = self.document.root.clone();
        let mut definition = format!("{prefix}.{name}");
        let mut n = 2;
        while self.document.definitions.contains_key(&definition) {
            definition = format!("{prefix}.{name}_{n}");
            n += 1;
        }
        let instances: Vec<String> = self.selected.iter().cloned().collect();
        let result = self.apply("Group", vec![SystemCommand::Group { at: self.level.clone(), instances, name: name.clone(), definition, label: format!("Group {name}") }]);
        if result.is_ok() {
            self.selected = BTreeSet::from([name]);
        }
        self.report(result);
    }

    fn remove_selected(&mut self) {
        let commands: Vec<_> = self.selected.iter().map(|n| SystemCommand::RemoveInstance { at: self.level.clone(), name: n.clone() }).collect();
        if commands.is_empty() {
            return;
        }
        let r = self.apply("Delete", commands);
        if r.is_ok() {
            self.selected.clear();
        }
        self.report(r);
    }

    fn start_input(&mut self, purpose: Purpose, initial: String) {
        if self.input.is_some(){self.status="Finish or cancel the current draft first.".into();self.action_error=Some(self.status.clone());self.panel_dirty=true;return;}
        self.input = Some(TextInput { purpose, buffer: initial });
        self.panel_dirty = true;
    }

    fn commit_input(&mut self) {
        let original=self.input.clone();
        let Some(TextInput { purpose, buffer }) = self.input.take() else { return };
        self.panel_dirty = true;
        let text = buffer.trim().to_string();
        let result: Result<(), String> = match purpose {
            Purpose::CommentAuthor=>{if text.is_empty(){Err("Enter an author name".into())}else{self.discussion.author=text;Ok(())}},
            Purpose::Comment|Purpose::ThreadTitle=>Err("Use the discussion editor to submit".into()),
            Purpose::GridSpacing=>text.parse::<f32>().map_err(|_|"Enter spacing in metres".into()).and_then(|spacing_m|{let mut grid=self.grid();grid.spacing_m=spacing_m;self.set_grid(grid)}),
            Purpose::GridOrigin|Purpose::Position=>{
                let p: Result<Vec<f32>,_>=text.split_whitespace().map(str::parse).collect();
                match p {Ok(p) if p.len()==3=>{let position=[p[0],p[1],p[2]];if purpose==Purpose::GridOrigin {let mut grid=self.grid();grid.origin_m=position;self.set_grid(grid)}else{self.display_move(self.selected.iter().cloned().collect(),position,false,false,None).map(|_|())}},_=>Err("Enter three coordinates in metres: x y z".into())}
            }

            Purpose::Filter => {
                self.filter = text;
                self.page = 0;
                Ok(())
            }
            Purpose::Parameter { name, parameter } => {
                if text.is_empty() {
                    self.apply("Clear parameter", vec![SystemCommand::SetParameter { at: self.level.clone(), name, parameter, binding: None }]).map(|_| ())
                } else if let Some(from) = text.strip_prefix('$') {
                    self.apply("Inherit parameter", vec![SystemCommand::SetParameter { at: self.level.clone(), name, parameter, binding: Some(sim_system::ParameterBinding::Parameter { parameter: from.to_string() }) }]).map(|_| ())
                } else {
                    match text.parse::<f64>() {
                        Ok(v) => self.apply("Set parameter", vec![SystemCommand::SetParameter { at: self.level.clone(), name, parameter, binding: Some(sim_system::ParameterBinding::value(v)) }]).map(|_| ()),
                        Err(_) => Err(format!("`{text}` is not a number (or $parameter to inherit)")),
                    }
                }
            }
            Purpose::Rename(name) => {
                let r = self.apply("Rename", vec![SystemCommand::RenameInstance { at: self.level.clone(), name, new_name: text.clone() }]).map(|_| ());
                if r.is_ok() {
                    self.selected = BTreeSet::from([text]);
                }
                r
            }
            Purpose::ImportImage => self.import_image(PathBuf::from(text)),
            Purpose::OpenSystem => self.open_system(PathBuf::from(text)).map(|_| ()),
            Purpose::ActuatorRegistry if text.is_empty() => Err("Type the path of an actuator registry.json".into()),
            Purpose::ActuatorRegistry => self.actuators_request(Some(PathBuf::from(text)), None).map(|_| ()),
            Purpose::ActuatorConsumer if text.is_empty() => Err("Type the path of a file that embeds a robot model".into()),
            Purpose::ActuatorConsumer => self.actuators_request(None, Some(vec![PathBuf::from(text)])).map(|_| ()),
            Purpose::GaitResults if text.is_empty() => Err("Type the path of a gait-lab results folder".into()),
            Purpose::GaitResults => self.gait_reports_request(Some(PathBuf::from(text))).map(|_| ()),
            Purpose::CalibrationArchive if text.is_empty() => Err("Type the path of an identification archive folder".into()),
            Purpose::CalibrationArchive => self.calibration_request(Some(PathBuf::from(text))).map(|_| ()),
            Purpose::Distance { id, first, second } => match text.parse::<f32>() {
                Ok(d) => self.apply("Calibrate reference", vec![SystemCommand::CalibrateReference { at: self.level.clone(), id, first, second, distance: d }]).map(|_| ()),
                Err(_) => Err("Enter the real distance between the two points in meters".into()),
            },
            Purpose::Sweep { name, parameter, observe } => {
                let parts: Vec<f64> = text.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                match parts[..] {
                    [from, to, count] if count >= 2. && count <= 64. => {
                        let n = count as usize;
                        let values = (0..n).map(|i| from + (to - from) * i as f64 / (n - 1) as f64).collect();
                        let study = self.default_study(observe, &name, sim_system::StudyKind::Sweep { parameter: parameter.clone(), values });
                        self.save_and_run_study(&format!("sweep_{name}_{}", parameter.replace('/', "_")), study)
                    }
                    _ => Err("Type: from to count (for example 1 4 4)".into()),
                }
            }
            Purpose::ReferenceWidth(id) => match (text.parse::<f32>(), self.reference(&id)) {
                (Ok(w), Some(mut r)) => {
                    r.width = w;
                    self.apply("Resize reference", vec![SystemCommand::SetReference { at: self.level.clone(), id, reference: r }]).map(|_| ())
                }
                _ => Err("Enter a width in meters".into()),
            },
        };
        if result.is_err(){self.input=original;}
        self.report(result);
    }

    fn reference(&self, id: &str) -> Option<sim_system::ReferenceImage> {
        self.document.definitions.get(&self.definition_id()?)?.references.get(id).cloned()
    }

    pub fn import_image(&mut self, path: PathBuf) -> Result<(), String> {
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "image".into());
        let taken: BTreeSet<String> = self.definition_id().and_then(|id| self.document.definitions.get(&id)).map(|d| d.references.keys().cloned().collect()).unwrap_or_default();
        let base: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).take(24).collect();
        let id = (1..).map(|n| if n == 1 { base.clone() } else { format!("{base}_{n}") }).find(|c| !taken.contains(c) && sim_system::valid_name(c)).unwrap_or_else(|| "image".into());
        let origin = self.subsystems.get(&self.level).map(|w| w.position).unwrap_or([0.; 3]);
        let applied = self.store.import_reference(&self.registry, &self.level, &id, &path, ReferenceView::Spatial, [origin[0], origin[1] - 0.002, origin[2]], 0.1).map_err(|e| e.to_string())?;
        self.status = format!("{} - adjust width, opacity or calibration in the References tab", applied.outcomes.last().map(|o| o.message.clone()).unwrap_or_default());
        self.reload();
        Ok(())
    }

    fn stop_run(&mut self) {
        // Keep every run that got anywhere.
        if self.run.as_ref().is_some_and(|r| r.worker.shared().lock().ok().and_then(|s| s.snapshot.as_ref().and_then(|x| x.status.as_ref()).map(|x| x.time)).unwrap_or(0.) >= 0.1) {
            let _ = self.save_run("");
        }
        self.run = None;
    }

    /// Keep the current run: its document, seed, settings and recorded history.
    pub fn save_run(&mut self, note: &str) -> Result<PathBuf, String> {
        let run = self.run.as_ref().ok_or("nothing is running")?;
        let (duration, history) = {
            let s = run.worker.shared().lock().map_err(|_| "run state unavailable")?;
            if s.reset_pending {
                return Err("reset in progress; save again in a moment".into());
            }
            (s.snapshot.as_ref().and_then(|x| x.status.as_ref()).map(|x| x.time).unwrap_or(0.), s.history.clone())
        };
        // The model the run simulated last: every swap (edit or grab) goes
        // through `hot_swap`, which keeps `run.document` equal to what it sent.
        let document = run.document.clone();
        let description = self.last_description.clone();
        let series = history
            .into_iter()
            .map(|(id, points)| {
                let (label, unit) = description.as_ref().map(|d| (system_builder::observable_key(d, &id), d.observables.get(&id).map(|o| sim_inspect::plot::unit(d, o).to_string()).unwrap_or_default())).unwrap_or((id.clone(), String::new()));
                system_builder::Series { observable: id, label, unit, times: points.iter().map(|p| p[0]).collect(), values: points.iter().map(|p| p[1]).collect() }
            })
            .collect();
        let note = if run.edited { format!("{note}{}{}", if note.is_empty() { "" } else { "; " }, sim_runtime::run_history::EDITED_WHILE_RUNNING) } else { note.to_string() };
        let record = sim_runtime::run_history::RunRecord::new(&document, system_builder::config_for(&document), duration, series, &note).with_provenance(run.fidelity, run.edited);
        let path = sim_runtime::run_history::save(&sim_runtime::run_history::dir_for(&self.store.path), &record)?;
        self.runs = sim_runtime::run_history::list(&sim_runtime::run_history::dir_for(&self.store.path));
        self.status = format!("Saved run {} ({:.2} s)", record.id, duration);
        self.panel_dirty = true;
        Ok(path)
    }

    /// Overlay saved runs in the graph dock and the Studies tab.
    pub fn compare_runs(&mut self, ids: &[String]) -> Result<(), String> {
        let records: Vec<_> = self.runs.iter().filter(|(_, s)| ids.contains(&s.id)).map(|(p, _)| sim_runtime::run_history::load(p)).collect::<Result<_, _>>()?;
        if records.len() < 2 {
            return Err("pick at least two runs to compare".into());
        }
        let metrics: Vec<sim_system::Metric> = records[0].series.iter().take(4).map(|s| sim_system::Metric { label: format!("{} (final)", s.label), observable: s.label.clone(), reduce: sim_system::Reduce::Final, window: None }).collect();
        self.study.result = Some(sim_runtime::run_history::compare(&records, &metrics));
        self.graphs.visible = true;
        self.graphs.force_refresh();
        self.tab = Tab::Studies;
        self.panel_dirty = true;
        Ok(())
    }

    /// Rerun saved run `id` headlessly on a worker thread and compare it with
    /// its record; the outcome arrives in `poll_replay`. Replaces (and stops)
    /// a replay already running.
    pub fn replay_run(&mut self, id: &str) -> Result<(), String> {
        let (path, summary) = self.runs.iter().find(|(_, s)| s.id == id).cloned().ok_or_else(|| format!("no saved run `{id}`"))?;
        let record = sim_runtime::run_history::load(&path)?;
        self.cancel_replay();
        let registry = self.registry.clone();
        let (edited, fidelity) = (record.edited_while_running(), record.fidelity_label());
        let work = crate::jobs::Job::spawn(crate::jobs::Pool::Dedicated, 0, "replay thread", move |ctx| sim_runtime::run_history::replay_with_cancel(&record, &registry, Some(ctx.cancel_flag())));
        self.replay.outcomes.insert(
            id.to_string(),
            ReplayOutcome { id: id.to_string(), status: "running", max_rel_diff: None, samples: None, error: None, wall_seconds: 0., duration: summary.duration, seed: summary.seed, fidelity, edited_while_running: edited },
        );
        self.replay.job = Some(ReplayJob { id: id.to_string(), work, started: std::time::Instant::now() });
        self.tab = Tab::Studies;
        self.status = format!("Replaying run {id} headlessly from t = 0 on the shared runtime (background).");
        self.panel_dirty = true;
        Ok(())
    }

    /// Stop the running replay between simulation steps; it reports no result.
    pub fn cancel_replay(&mut self) -> bool {
        // Dropping the job at the end of this function stops the rerun.
        let Some(job) = self.replay.job.take() else { return false };
        if let Some(o) = self.replay.outcomes.get_mut(&job.id) {
            o.status = "cancelled";
            o.wall_seconds = job.started.elapsed().as_secs_f64();
        }
        self.status = format!("Replay of {} cancelled.", job.id);
        self.panel_dirty = true;
        true
    }

    fn poll_replay(&mut self) {
        let Some(job) = &self.replay.job else { return };
        let polled = job.work.poll();
        let wall = job.started.elapsed().as_secs_f64();
        let id = job.id.clone();
        let Some(outcome) = self.replay.outcomes.get_mut(&id) else { return };
        outcome.wall_seconds = wall;
        let Some(result) = polled else { return };
        match result {
            Ok(report) => {
                outcome.status = "done";
                outcome.max_rel_diff = Some(report.max_rel_diff);
                outcome.samples = Some(report.samples);
            }
            Err(e) => {
                outcome.status = "error";
                outcome.error = Some(e);
            }
        }
        self.status = format!("Run {id}: {}", outcome.headline());
        self.replay.job = None;
        self.panel_dirty = true;
    }

    pub fn replay_json(&self) -> serde_json::Value {
        serde_json::json!({
            "running": self.replay.job.as_ref().map(|j| &j.id),
            "method": "headless rerun from t = 0 with the record's document, seed and config on the shared runtime; compared at every recorded sample time",
            "outcomes": self.replay.outcomes.values().map(|o| {
                let mut v = serde_json::json!(o);
                v["headline"] = serde_json::json!(o.headline());
                v
            }).collect::<Vec<_>>(),
        })
    }

    fn start_run(&mut self, scene: &SpatialScene) {
        if let Some(run) = &self.run {
            if run.description_id == scene.description.id {
                let _ = run.worker.send(RunControl::Start);
                self.status = RUNNING_STATUS.into();
                return;
            }
        }
        let fidelity = if self.realtime { Fidelity::Realtime } else { Fidelity::Detailed };
        let document = match fidelity.document(&self.document, &self.registry) {
            Ok(d) => d,
            Err(e) => {
                self.status = format!("Realtime profile: {}", e.trim_start_matches("realtime profile: "));
                return;
            }
        };
        let registry = self.registry.clone();
        let mut observed: Vec<String> = scene
            .animation
            .as_ref()
            .map(|a| a.observables().into_iter().collect())
            .unwrap_or_default();
        observed.extend(graphs::recordable(scene, &self.graphs.pinned));
        observed.sort();
        observed.dedup();
        self.run = Some(LiveRun::spawn(document, registry, observed, scene.description.id.clone(), fidelity));
        self.status = RUNNING_STATUS.into();
    }

    fn pause_run(&mut self) {
        if let Some(run) = &self.run {
            let _ = run.worker.send(RunControl::Pause);
            // No time: the last published snapshot may be a frame behind the pause.
            self.status = "Paused (Step advances one timestep; Run resumes).".into();
        }
    }

    /// Swap the live run onto `source` (a detailed document) through the run's
    /// fidelity profile, so a realtime run never receives the detailed model.
    /// `run.document` becomes exactly what was sent and the run counts as
    /// edited. If the profile fails the run is stopped (kept up to the swap)
    /// and the status says why; returns false then. Never touches
    /// `self.document` or the file.
    fn hot_swap(&mut self, source: &SystemDocument, description_id: String) -> bool {
        let Some(fidelity) = self.run.as_ref().map(|r| r.fidelity) else { return false };
        match fidelity.document(source, &self.registry) {
            Ok(document) => {
                let run = self.run.as_mut().expect("checked above");
                let _ = run.worker.send(RunControl::Swap(Box::new(document.clone()), description_id.clone()));
                run.description_id = description_id;
                run.document = document;
                run.edited = true;
                true
            }
            Err(e) => {
                // Never continue a realtime run on the detailed model: stop it
                // (kept up to the edit, on the model it ran) and say why.
                self.stop_run();
                self.status = format!("The {} run was stopped (kept up to the edit): the edited system has no valid {e}", fidelity.label());
                false
            }
        }
    }

    /// Ask the running session to record `ids` as well (newly pinned plots).
    pub(super) fn observe(&mut self, scene: &SpatialScene) {
        if let Some(run) = &self.run {
            let mut ids = graphs::recordable(scene, &self.graphs.pinned);
            if let Some(a) = &scene.animation {
                ids.extend(a.observables());
            }
            ids.sort();
            ids.dedup();
            let _ = run.worker.send(RunControl::Observe(ids));
        }
    }

    /// Recorded history of one observable (empty when not running).
    pub(super) fn history(&self, id: &str) -> Vec<[f64; 2]> {
        self.run.as_ref().and_then(|r| r.worker.shared().lock().ok().and_then(|s| s.history.get(id).map(|h| h.iter().copied().collect()))).unwrap_or_default()
    }

    /// Save a study (shared command, undoable) and start running it.
    pub fn save_and_run_study(&mut self, name: &str, study: sim_system::Study) -> Result<(), String> {
        self.apply(&format!("Study {name}"), vec![SystemCommand::SetStudy { name: name.to_string(), study: Some(study) }])?;
        self.run_study(name)
    }

    /// Run a saved study on worker threads; results arrive in `poll_study`.
    pub fn run_study(&mut self, name: &str) -> Result<(), String> {
        let study = self.document.studies.get(name).cloned().ok_or_else(|| format!("no study `{name}`"))?;
        // Replacing the job below cancels a study already running.
        let (document, registry, n) = (self.document.clone(), self.registry.clone(), name.to_string());
        let total = sim_runtime::system_study::variant_count(&study);
        let work = crate::jobs::Job::spawn(crate::jobs::Pool::Dedicated, 0, format!("study {name}"), move |ctx| {
            let threads = std::thread::available_parallelism().map(|n| n.get().saturating_sub(1).max(1)).unwrap_or(2);
            sim_runtime::system_study::run(&document, &registry, &n, &study, threads, Some(ctx.cancel_flag()), &|done, total| ctx.steps(done as u64, total as u64))
        });
        self.study.job = Some(StudyJob { name: name.to_string(), work, total });
        self.study.error = None;
        self.tab = Tab::Studies;
        self.graphs.visible = true;
        self.status = format!("Running study {name}: {total} variants on the shared runtime (background).");
        self.panel_dirty = true;
        Ok(())
    }

    pub fn study_json(&self) -> serde_json::Value {
        serde_json::json!({
            "running": self.study_progress().map(|(n, d, t)| serde_json::json!({"name": n, "done": d, "total": t})),
            "error": self.study.error,
            "result": self.study.result.as_ref().map(|r| serde_json::json!({
                "name": r.name, "kind": r.kind, "parameter": r.parameter, "source_hash": r.source_hash,
                "table": sim_runtime::system_study::table(r),
                "variants": r.variants.iter().map(|v| serde_json::json!({"label": v.label, "value": v.value, "metrics": v.metrics, "derived": v.derived, "error": v.error, "wall_seconds": v.wall_seconds})).collect::<Vec<_>>(),
            })),
        })
    }

    pub(super) fn study_progress(&self) -> Option<(String, usize, usize)> {
        self.study.job.as_ref().map(|j| (j.name.clone(), j.work.progress().steps.map_or(0, |(done, _)| done as usize), j.total))
    }

    fn poll_study(&mut self) {
        let Some(job) = &self.study.job else { return };
        match job.work.poll() {
            None => {}
            Some(result) => {
                self.study.job = None;
                match result {
                    Ok(r) => {
                        self.status = format!("Study {} finished: {} variants", r.name, r.variants.len());
                        self.study.result = Some(r);
                    }
                    Err(e) => {
                        self.status = format!("Study failed: {e}");
                        self.study.error = Some(e);
                    }
                }
                self.panel_dirty = true;
                self.graphs.force_refresh();
            }
        }
    }

    /// A comparison of `name` against the alternatives that fit its ports
    /// (same interface first, at most three), with default metrics.
    pub fn compare_alternatives(&mut self, scene: &SpatialScene, name: &str) -> Result<(), String> {
        let list = library::alternatives(&self.document, &self.registry, Some(&self.library_dir), &self.level, name).map_err(|e| e.to_string())?;
        let chosen: Vec<library::Alternative> = list.into_iter().filter(|a| a.same_interface).take(3).collect();
        if chosen.is_empty() {
            return Err(format!("nothing with the same interface fits {name}; use Show alternatives and Swap instead"));
        }
        let mut imports = Vec::new();
        for a in &chosen {
            if let Some(path) = &a.library_path {
                imports.push(SystemCommand::AddDefinitions { definitions: library::import(std::path::Path::new(path)).map_err(|e| e.to_string())? });
            }
        }
        if !imports.is_empty() {
            self.apply("Import alternatives", imports)?;
        }
        let observe = self.default_observe(scene, name);
        let study = self.default_study(observe, name, sim_system::StudyKind::Compare { alternatives: chosen.into_iter().map(|a| a.kind).collect() });
        self.save_and_run_study(&format!("compare_{name}"), study)
    }

    /// Default observables and metrics: the run's readouts plus what the
    /// selected part itself exposes, meaned over the last quarter.
    fn default_observe(&self, scene: &SpatialScene, name: &str) -> Vec<String> {
        let mut observe: Vec<String> = scene.animation.as_ref().map(|a| a.readouts.iter().take(2).map(|r| system_builder::observable_key(&scene.description, &r.observable)).collect()).unwrap_or_default();
        for (id, _) in graphs::candidates(scene, &self.full_path(name)).into_iter().take(3) {
            observe.push(system_builder::observable_key(&scene.description, &id));
        }
        for pinned in &self.graphs.pinned {
            observe.push(system_builder::observable_key(&scene.description, pinned));
        }
        observe.sort();
        observe.dedup();
        observe
    }

    fn default_study(&self, observe: Vec<String>, name: &str, kind: sim_system::StudyKind) -> sim_system::Study {
        let duration = self.document.studies.values().map(|s| s.duration).fold(2.0, f64::max);
        let metrics = observe.iter().map(|o| sim_system::Metric { label: format!("{o} (mean, last 25 %)"), observable: o.clone(), reduce: sim_system::Reduce::Mean, window: Some([0.75 * duration, duration]) }).collect();
        sim_system::Study { at: self.level.clone(), instance: name.to_string(), kind, duration, observe, metrics }
    }

    /// Directory recorded library paths resolve against: the workspace root
    /// (`crate::workspace`), or the error naming what was searched.
    fn base_dir(&self) -> Result<PathBuf, String> {
        crate::workspace::root().map(std::path::Path::to_path_buf)
    }

    /// Publish a definition (and what it places) as a new library version,
    /// refreshing library files that bundle it.
    pub fn publish(&mut self, definition: &str) -> Result<Vec<library::Published>, String> {
        let published = library::publish(&self.document, definition, &self.library_dir).map_err(|e| e.to_string())?;
        let changed: Vec<String> = published.iter().filter(|p| p.changed).map(|p| format!("{} v{}", p.id, p.version)).collect();
        self.status = if changed.is_empty() { format!("{definition} is already published with these contents") } else { format!("Published {}", changed.join(", ")) };
        self.updates = self.library_updates();
        self.used_in = None;
        self.refresh_palette();
        self.panel_dirty = true;
        Ok(published)
    }

    /// Imported definitions whose library file has changed.
    pub fn library_updates(&self) -> Vec<library::Stale> {
        // Without a workspace root no recorded library path resolves (reported in system_state.workspace).
        self.base_dir().map(|base| library::stale(&self.document, &base)).unwrap_or_default()
    }

    /// Bring every stale import up to date as one undoable edit.
    pub fn sync_library(&mut self) -> Result<sim_system::store::Applied, String> {
        let commands = library::sync(&self.document, &self.base_dir()?).map_err(|e| e.to_string())?;
        if commands.is_empty() {
            return Err("Library imports are up to date".into());
        }
        self.apply("Update from library", commands)
    }

    /// Where a definition is placed in the system files under the workspace's examples/ and next to this file.
    pub fn where_used(&self, definition: &str) -> Vec<(String, usize)> {
        let mut files = self.base_dir().map(|base| library::system_files(&base.join("examples"))).unwrap_or_default();
        if let Some(dir) = self.store.path.parent() {
            files.extend(library::system_files(dir));
        }
        files.sort();
        files.dedup();
        library::where_used(&files, definition)
    }

    /// Make `instance.parameter` of this level a parameter of the level's definition.
    pub fn expose(&mut self, instance: &str, parameter: &str) -> Result<sim_system::store::Applied, String> {
        let definition = self.definition_id().ok_or("no level")?;
        if definition == self.document.root {
            return Err("Expose works inside a subsystem: open one first".into());
        }
        let taken = self.document.definitions[&definition].parameters.keys().cloned().collect::<BTreeSet<_>>();
        let name = std::iter::once(parameter.to_string()).chain((2..).map(|n| format!("{parameter}_{n}"))).find(|n| !taken.contains(n)).unwrap();
        self.apply("Expose parameter", vec![SystemCommand::ExposeParameter { definition, instance: instance.into(), inner: parameter.into(), parameter: name, description: String::new() }])
    }

    /// Library entry for a registry element, with its notes.
    pub fn component_json(&self, component_type: &str) -> Result<serde_json::Value, String> {
        let entry = self.element_entry(component_type).ok_or_else(|| format!("no palette element `{component_type}`"))?;
        Ok(serde_json::json!({"entry": entry, "datasheet": self.datasheet(component_type)}))
    }

    /// Pin observables (IDs or readable keys) to the graph dock.
    pub fn set_plots(&mut self, scene: &SpatialScene, pin: Option<Vec<String>>, visible: Option<bool>) -> Result<serde_json::Value, String> {
        if let Some(pin) = pin {
            let mut ids = Vec::new();
            for p in pin {
                let id = if scene.description.observables.contains_key(&p) {
                    p
                } else {
                    scene.description.observables.keys().find(|id| system_builder::observable_key(&scene.description, id) == p).cloned().ok_or_else(|| format!("no observable `{p}`"))?
                };
                ids.push(id);
            }
            ids.truncate(graphs::MAX_CHARTS);
            self.graphs.pinned = ids;
            self.graphs.visible = true;
            self.observe(scene);
        }
        if let Some(v) = visible {
            self.graphs.visible = v;
        }
        self.panel_dirty = true;
        Ok(serde_json::json!({"visible": self.graphs.visible, "pinned": self.graphs.pinned, "charts": self.graphs.charts.iter().map(|c| serde_json::json!({"id": c.id, "title": c.title, "unit": c.unit, "latest": c.latest, "range": [c.range.0, c.range.1], "window": [c.window.0, c.window.1]})).collect::<Vec<_>>()}))
    }

    pub fn run_start(&mut self, scene: &SpatialScene) {
        self.start_run(scene);
        self.panel_dirty = true;
    }
    pub fn run_pause(&mut self) {
        self.pause_run();
        self.panel_dirty = true;
    }

    /// Advance a paused run by exactly one timestep on its run thread
    /// (`Command::Step`). Refused, never ignored, without a paused run.
    pub fn run_step(&mut self) -> Result<(), String> {
        let run = self.run.as_ref().ok_or("nothing is running: start a run, pause it, then step")?;
        {
            let s = run.worker.shared().lock().map_err(|_| "run state unavailable")?;
            if s.reset_pending {
                return Err("reset in progress; step again in a moment".into());
            }
            if s.running {
                return Err("pause the run before stepping".into());
            }
        }
        run.worker.send(RunControl::Step).map_err(|_| "the run has ended; start a new run".to_string())?;
        self.panel_dirty = true;
        Ok(())
    }

    /// Return the live run to t = 0, paused (`Command::Reset` on its run
    /// thread, which rebuilds the model it currently simulates).
    ///
    /// Recorded work is kept, not lost: a run that reached t >= 0.1 s is saved
    /// first (the `stop_run` rule) and its id is named in the status. After the
    /// reset the run simulates `run.document` (the last model swapped in) from
    /// t = 0 with no live edit, so `edited` becomes false; the fidelity is kept.
    /// The graphs are cleared at once and stay closed to pre-reset samples.
    pub fn run_reset(&mut self) -> Result<(), String> {
        let run = self.run.as_ref().ok_or("nothing is running")?;
        if run.worker.shared().lock().map_err(|_| "run state unavailable")?.reset_pending {
            return Err("reset in progress".into());
        }
        let reached = run.worker.shared().lock().ok().and_then(|s| s.snapshot.as_ref().and_then(|x| x.status.as_ref()).map(|x| x.time)).unwrap_or(0.);
        let kept = if reached >= 0.1 { Some(self.save_run("")?) } else { None };
        let run = self.run.as_mut().expect("checked above");
        {
            let mut s = run.worker.shared().lock().map_err(|_| "run state unavailable")?;
            s.reset_pending = true;
            s.history.clear();
        }
        if run.worker.send(RunControl::Reset).is_err() {
            if let Ok(mut s) = run.worker.shared().lock() {
                s.reset_pending = false;
            }
            return Err("the run has ended; start a new run".into());
        }
        run.edited = false;
        let kept = kept.and_then(|p| self.runs.iter().find(|(q, _)| *q == p).map(|(_, s)| s.id.clone()));
        self.status = match kept {
            Some(id) => format!("Reset to t = 0 (paused); kept the previous run as {id}."),
            None => "Reset to t = 0 (paused); the previous run was under 0.1 s and was not kept.".into(),
        };
        self.panel_dirty = true;
        Ok(())
    }
    pub fn select(&mut self, names: Vec<String>) {
        self.selected = names.into_iter().collect();
        self.alternatives = None;
        self.scene_dirty = true;
        self.panel_dirty = true;
    }
    pub fn state_json(&self) -> serde_json::Value {
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
            "selected": self.selected,
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
            "schematic": self.schematic.json(self.document.revision, &self.level, &self.selected),
            "history": self.store.history(),
        })
    }

    /// Latest run-thread status: null without a run.
    fn live_run_json(&self) -> serde_json::Value {
        let Some(run) = &self.run else { return serde_json::Value::Null };
        let s = run.worker.shared().lock().ok();
        let status = s.as_ref().and_then(|s| s.snapshot.as_ref().and_then(|x| x.status.clone()));
        serde_json::json!({
            "time": status.as_ref().map(|x| x.time),
            "phase": status.as_ref().map(|x| x.phase),
            "step": status.as_ref().map(|x| x.step),
            "generation": status.as_ref().map(|x| x.generation),
            "interval": system_builder::config_for(&run.document).interval,
            "reset_pending": s.as_ref().is_some_and(|s| s.reset_pending),
            "error": s.as_ref().and_then(|s| s.snapshot.as_ref().and_then(|x| x.error.clone())),
            "fidelity": run.fidelity.label(),
            "edited": run.edited,
        })
    }

    fn running(&self) -> bool {
        self.run.as_ref().is_some_and(|r| r.worker.shared().lock().map(|s| s.running).unwrap_or(false))
    }
}

fn run_thread(document: SystemDocument, registry: BehaviorRegistry, observed: Vec<String>, source_id: String, commands: mpsc::Receiver<RunControl>, shared: Arc<Mutex<RunShared>>) {
    let source_id = std::cell::RefCell::new(source_id);
    let publish = |status: Option<sim_inspect::live::SessionStatus>, frame: Option<sim_inspect::SampleFrame>, error: Option<String>, running: bool| {
        if let Ok(mut s) = shared.lock() {
            s.snapshot = Some(sim_inspect::live::LiveSnapshot { version: 1, source_description_id: source_id.borrow().clone(), description: None, status, frame, error });
            s.running = running;
        }
    };
    let config = system_builder::config_for(&document);
    let compiled = match system_builder::compile(&document, &registry, config.clone()) {
        Ok(c) => c,
        Err(e) => return publish(None, None, Some(e), false),
    };
    let source = sim_runtime::system_session::ModelSource {
        model: compiled.flat.model.clone(),
        registry: registry.clone(),
        identities: compiled.flat.identities.clone(),
        source_hash: compiled.flat.source_hash.clone(),
        revision: document.revision.max(1),
    };
    let mut session = match sim_runtime::system_session::SystemSession::new(compiled.launch.run_id.clone(), config.clone(), move |c| source.build(c)) {
        Ok(s) => s,
        Err(e) => return publish(None, None, Some(system_builder::locate(&compiled.flat, e)), false),
    };
    if let Err(e) = session.subscribe(observed) {
        return publish(None, None, Some(e), false);
    }
    let _ = session.execute(sim_runtime::system_session::Command::Start);
    let mut running = true;
    let mut pending: Vec<(f64, Vec<(String, f64)>)> = Vec::new();
    let mut last_sample = f64::NEG_INFINITY;
    let mut wall = std::time::Instant::now();
    let mut sim_at_wall = 0.0;
    let mut last_publish = std::time::Instant::now() - std::time::Duration::from_secs(1);
    // A refused step stays visible until the next command.
    let mut step_error: Option<String> = None;
    let sample = |frame: &sim_inspect::SampleFrame| -> (f64, Vec<(String, f64)>) {
        (frame.time, frame.values.keys().filter_map(|id| sim_inspect::animation::scalar(Some(frame), id).map(|v| (id.clone(), v.value))).collect())
    };
    let flush = |pending: &mut Vec<(f64, Vec<(String, f64)>)>| {
        if let Ok(mut s) = shared.lock() {
            if s.reset_pending {
                // Samples from before a requested reset never reach the graphs.
                pending.clear();
                return;
            }
            for (t, values) in pending.drain(..) {
                for (id, v) in values {
                    let h = s.history.entry(id).or_default();
                    h.push_back([t, v]);
                    while h.len() > HISTORY_POINTS || h.front().is_some_and(|f| f[0] < t - HISTORY_SECONDS) {
                        h.pop_front();
                    }
                }
            }
        }
    };
    loop {
        loop {
            let command = commands.try_recv();
            if matches!(command, Ok(RunControl::Start | RunControl::Pause | RunControl::Step | RunControl::Reset)) {
                step_error = None;
            }
            match command {
                Ok(RunControl::Start) => {
                    running = true;
                    let _ = session.execute(sim_runtime::system_session::Command::Start);
                    wall = std::time::Instant::now();
                    sim_at_wall = session.status().time;
                }
                Ok(RunControl::Pause) => {
                    running = false;
                    let _ = session.execute(sim_runtime::system_session::Command::Pause);
                }
                Ok(RunControl::Swap(next, description_id)) => {
                    let config = system_builder::config_for(&next);
                    match system_builder::compile(&next, &registry, config) {
                        Ok(compiled) => {
                            let source = sim_runtime::system_session::ModelSource {
                                model: compiled.flat.model.clone(),
                                registry: registry.clone(),
                                identities: compiled.flat.identities.clone(),
                                source_hash: compiled.flat.source_hash.clone(),
                                revision: next.revision.max(1),
                            };
                            match session.hot_swap(move |c| source.build(c)) {
                                Ok(preserved) => {
                                    *source_id.borrow_mut() = description_id;
                                    if !preserved {
                                        pending.clear();
                                        last_sample = f64::NEG_INFINITY;
                                        if let Ok(mut s) = shared.lock() {
                                            s.history.clear();
                                        }
                                    }
                                    wall = std::time::Instant::now();
                                    sim_at_wall = session.status().time;
                                    if running {
                                        let _ = session.execute(sim_runtime::system_session::Command::Start);
                                    }
                                    publish(Some(session.status().clone()), Some(session.latest().clone()), None, running);
                                }
                                Err(e) => publish(Some(session.status().clone()), Some(session.latest().clone()), Some(e), false),
                            }
                        }
                        Err(e) => publish(Some(session.status().clone()), Some(session.latest().clone()), Some(e), running),
                    }
                }
                Ok(RunControl::Step) => match session.execute(sim_runtime::system_session::Command::Step) {
                    Ok(_) => {
                        // The stepped point joins the trace like a tick sample.
                        let frame = session.latest();
                        last_sample = frame.time;
                        pending.push(sample(frame));
                        flush(&mut pending);
                        publish(Some(session.status().clone()), Some(session.latest().clone()), None, running);
                    }
                    Err(e) => {
                        step_error = Some(format!("step refused: {e}"));
                        publish(Some(session.status().clone()), Some(session.latest().clone()), step_error.clone(), running);
                    }
                },
                Ok(RunControl::Reset) => {
                    let result = session.execute(sim_runtime::system_session::Command::Reset);
                    // Paused at t = 0 (the runtime's reset); later Resume paces from here.
                    running = false;
                    pending.clear();
                    last_sample = f64::NEG_INFINITY;
                    wall = std::time::Instant::now();
                    sim_at_wall = session.status().time;
                    if let Ok(mut s) = shared.lock() {
                        s.history.clear();
                        s.reset_pending = false;
                    }
                    let error = result.err().map(|e| format!("reset failed: {e}"));
                    step_error = error.clone();
                    publish(Some(session.status().clone()), Some(session.latest().clone()), error, false);
                }
                Ok(RunControl::Observe(ids)) => {
                    if let Err(e) = session.subscribe(ids) {
                        publish(Some(session.status().clone()), Some(session.latest().clone()), Some(e), running);
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }
        if running {
            // Never faster than real time; slower systems run as fast as they can.
            let ahead = session.status().time - sim_at_wall - wall.elapsed().as_secs_f64();
            if ahead > 0. {
                std::thread::sleep(std::time::Duration::from_secs_f64(ahead.min(0.02)));
            } else if let Err(e) = session.tick() {
                publish(Some(session.status().clone()), Some(session.latest().clone()), Some(system_builder::locate(&compiled.flat, e)), false);
                running = false;
            } else {
                let frame = session.latest();
                // About 1500 points across the kept window.
                if frame.time - last_sample >= HISTORY_SECONDS / 1500. {
                    last_sample = frame.time;
                    pending.push(sample(frame));
                }
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if last_publish.elapsed().as_millis() >= 33 {
            let speed = if wall.elapsed().as_secs_f64() > 0. { (session.status().time - sim_at_wall) / wall.elapsed().as_secs_f64() } else { 0. };
            publish(Some(session.status().clone()), Some(session.latest().clone()), step_error.clone(), running);
            if let Ok(mut s) = shared.lock() {
                s.speed = speed;
            }
            flush(&mut pending);
            last_publish = std::time::Instant::now();
        }
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
        crate::app::actions::register::<system_actions::SystemAction>(app);
        // Buttons and keys write the builder's actions (after REST's, as the old chain
        // applied them); its one handler applies them and REST's in Actions.
        app.add_systems(Update, (actions::buttons, actions::keys.run_if(building.clone())).chain().after(crate::app::actions::serve).in_set(ViewerSet::Input).run_if(in_state(ModeScope::Builder)))
            .add_systems(Update, system_actions::apply.in_set(ViewerSet::Actions).run_if(in_state(ModeScope::Builder)));
        app.add_systems(
            Update,
            (frame_timing, watch, agent::tick, reference::tick, text_input.run_if(building.clone()), drops.run_if(building.clone()), grab_push.run_if(building.clone()), open_system, (finish_actuators, finish_gait_reports, finish_calibration, calibration::update_chart.run_if(building.clone())).chain(), rebuild_scene, sync_run, graphs::update.run_if(building.clone()), schematic::update.run_if(building.clone()), ui::rebuild_panel.run_if(building.clone()), ui::scroll_panels.run_if(building.clone()), ui::hover, clear_for_learn.run_if(in_state(ViewerMode::Lessons)))
                .chain()
                .before(update_parts)
                .in_set(ViewerSet::SimSync)
                .run_if(in_state(ModeScope::Builder)),
        )
        .add_systems(Update, placement::update.after(update_parts).in_set(ViewerSet::SimSync).run_if(building.clone()))
        .add_systems(Update, (placement::apply_preview, placement::draw_handles).chain().after(placement::update).in_set(ViewerSet::SimSync).run_if(building.clone()))
        .add_systems(Update, discussion::hover.after(notes::update).in_set(ViewerSet::SimSync).run_if(building.clone()))
        .add_systems(Update, markers::sync.after(placement::apply_preview).after(discussion::hover).in_set(ViewerSet::SimSync).run_if(building.clone()))
        .add_systems(Update, ui_api::collect.after(markers::sync).after(ui::rebuild_panel).in_set(ViewerSet::SimSync).run_if(building))
        .add_observer(placement::end_drag);
    }
}

/// Build mode's scene of `builder`'s document, compiled with the shared
/// runtime: the launch, a lessons launch and a switch to build or lessons
/// mode all start from it.
pub fn compiled_scene(builder: &Builder) -> Result<SpatialScene, String> {
    let compiled = system_builder::compile(&builder.document, builder.registry(), system_builder::config_for(&builder.document)).map_err(|e| e.to_string())?;
    let spatial = compiled.spatial.clone().unwrap_or_else(|| compiled.flat.spatial(&compiled.description.id, &builder.document.title));
    let mut scene = SpatialScene::for_builder(compiled.description.clone(), spatial).map_err(|e| e.to_string())?;
    if let Some(animation) = compiled.animation.clone() {
        scene.set_animation(animation).map_err(|e| e.to_string())?;
    }
    Ok(scene)
}

/// While a lesson is shown, the builder's chrome and pins are removed; they
/// are rebuilt when the builder is shown again.
fn clear_for_learn(mut commands: Commands, mut builder: ResMut<Builder>, chrome: Query<Entity, Or<(With<BuilderPanel>, With<markers::Marker>, With<markers::Leader>)>>) {
    for e in &chrome {
        commands.entity(e).try_despawn();
    }
    if !builder.panel_dirty {
        builder.panel_dirty = true;
    }
    if builder.drag.is_some() {
        builder.drag = None;
    }
}

/// Install a system opened in the background (scene, annotations, models follow it).
fn open_system(mut builder: ResMut<Builder>, mut scene: ResMut<SpatialScene>, models: Option<ResMut<crate::models::ModelLibrary>>) {
    if builder.open.job.is_some() {
        builder.finish_open(&mut scene, models.map(|m| m.into_inner()));
    }
}

/// Install a finished actuator registry load and check.
fn finish_actuators(mut builder: ResMut<Builder>) {
    if builder.actuators.pending().is_some() {
        builder.finish_actuators();
    }
}

/// Install a finished identification archive load.
fn finish_calibration(mut builder: ResMut<Builder>) {
    if builder.calibration.pending().is_some() {
        builder.finish_calibration();
    }
}

/// Install a finished gait-lab results scan.
fn finish_gait_reports(mut builder: ResMut<Builder>) {
    if builder.gait_lab.pending().is_some() {
        builder.finish_gait_reports();
    }
}

fn watch(time: Res<Time>, mut builder: ResMut<Builder>) {
    // Defer directory scans and disk reloads during direct manipulation. The
    // worker's expected-revision transaction still rejects external edits.
    if builder.drag.is_some() { return; }
    let now = time.elapsed_secs_f64();
    if now - builder.checked < 0.4 {
        return;
    }
    builder.checked = now;
    builder.reload_parts();
    if (now / 2.0).floor() != ((now - 0.4) / 2.0).floor() {
        let updates = builder.library_updates();
        if updates != builder.updates {
            builder.updates = updates;
            builder.panel_dirty = true;
        }
    }
    if builder.store.stamp() != builder.stamp {
        builder.reload();
        builder.status = "Reloaded: the system file changed in another editor.".into();
    }
}

/// Typing into the open draft (input editing); Enter and Escape are the
/// draft's submit and drop actions, applied by the builder's handler.
/// Keys from the frame a draft opens are dropped: the key that opened it
/// ("/" for the filter) was pressed before the draft existed.
fn text_input(mut events: MessageReader<KeyboardInput>, mut builder: ResMut<Builder>, keys: Res<ButtonInput<KeyCode>>, mode: Option<Res<State<ViewerMode>>>, mut was_open: Local<bool>, mut out: MessageWriter<crate::app::actions::Act<system_actions::SystemAction>>) {
    let mut send = |action: BuildAction| {
        out.write(crate::app::actions::Act::ui(system_actions::SystemAction::Ui(action)));
    };
    let opened_now = builder.input.is_some() && !*was_open;
    *was_open = builder.input.is_some();
    if builder.input.is_none() || opened_now {
        events.clear();
        return;
    }
    for e in events.read() {
        if e.state != ButtonState::Pressed {
            continue;
        }
        match &e.logical_key {
            Key::Enter => {
                if builder.input.as_ref().is_some_and(|i|matches!(i.purpose,Purpose::Comment|Purpose::ThreadTitle)) && (keys.pressed(KeyCode::ShiftLeft)||keys.pressed(KeyCode::ShiftRight)) {
                    builder.input.as_mut().unwrap().buffer.push('\n');builder.panel_dirty=true;continue;
                }
                send(BuildAction::SubmitDraft);
                return;
            }
            Key::Escape => {
                send(BuildAction::DropDraft);
                // Escape also leaves Connect/Annotate, as build mode's Escape key does.
                if builder.drag.is_none() && mode.is_some_and(|m| *m.get() == ViewerMode::Build) {
                    send(BuildAction::SetMode(Mode::Select));
                }
                return;
            }
            Key::Backspace => {
                if let Some(i) = builder.input.as_mut() {
                    i.buffer.pop();
                }
            }
            Key::Space => {
                if let Some(i) = builder.input.as_mut() {
                    i.buffer.push(' ');
                }
            }
            Key::Character(c) => {
                if let Some(i) = builder.input.as_mut() {
                    i.buffer.push_str(c.as_str());
                }
            }
            _ => {}
        }
        builder.panel_dirty = true;
    }
}

fn drops(mut events: MessageReader<FileDragAndDrop>, mut builder: ResMut<Builder>) {
    for e in events.read() {
        if let FileDragAndDrop::DroppedFile { path_buf, .. } = e {
            let name = path_buf.to_string_lossy().to_lowercase();
            let result = if name.ends_with(".definition.json") {
                match library::import(path_buf) {
                    Ok(definitions) => {
                        let id = library::read(path_buf).map(|(f, _)| f.definition).unwrap_or_default();
                        let r = builder.apply("Import library definition", vec![SystemCommand::AddDefinitions { definitions }]);
                        r.map(|_| builder.status = format!("Imported {id}; it is now in the palette"))
                    }
                    Err(e) => Err(e.to_string()),
                }
            } else {
                builder.import_image(path_buf.clone())
            };
            builder.report(result);
        }
    }
}

/// Recompile after edits and respawn the parts, nets and reference images.
#[allow(clippy::too_many_arguments)]
fn rebuild_scene(
    mut commands: Commands,
    mut builder: ResMut<Builder>,
    mut scene: ResMut<SpatialScene>,
    content: Query<Entity, With<SceneContent>>,
    ui_roots: Query<Entity, With<UiRoot>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut orbit: Single<&mut Orbit>,
    mut models: Option<ResMut<crate::models::ModelLibrary>>,
    mode: Option<Res<State<ViewerMode>>>,
) {
    let learning = mode.is_some_and(|m| *m.get() == ViewerMode::Lessons);
    // Do not replace picked entities while a pointer owns them. A saved drop
    // explicitly allows the replacement while its preview remains visible.
    if builder.drag.as_ref().is_some_and(|d| !d.awaiting_scene()) { return; }
    // Compile off the UI thread; apply the newest finished result.
    if builder.scene_dirty && builder.job.is_none() {
        builder.scene_dirty = false;
        builder.job = Some(compile_job(builder.document.clone(), builder.registry.clone()));
    }
    let Some(polled) = builder.job.as_ref().and_then(crate::jobs::Job::poll) else { return };
    builder.job = None;
    let finished = match polled {
        Ok(r) => r,
        Err(e) => {
            // The compile panicked: say so instead of leaving the scene silently stale.
            builder.compile_error = Some(e.clone());
            builder.status = e;
            builder.panel_dirty = true;
            return;
        }
    };
    if finished.revision != builder.document.revision {
        // An edit arrived while compiling; compile again before redrawing.
        builder.scene_dirty = true;
        return;
    }
    let compiled = match finished.result {
        Ok(c) => c,
        Err(e) => {
            builder.compile_error = Some(e.clone());
            builder.findings = finished.findings;
            builder.status = format!("Does not compile yet: {e}");
            builder.panel_dirty = true;
            return;
        }
    };
    let first = scene.description.components.is_empty() && scene.spatial.parts.is_empty();
    builder.findings = compiled.findings;
    builder.subsystems = compiled.subsystems;
    builder.compile_error = compiled.runtime_error;
    // A running system takes the edit live: parameters keep its state,
    // structural edits restart it at t = 0 (the session decides).
    let new_id = compiled.description.id.clone();
    // The run keeps its fidelity: a realtime run takes the edit's realtime profile.
    if builder.run.as_ref().is_some_and(|r| r.description_id != new_id) {
        let document = builder.document.clone();
        builder.hot_swap(&document, new_id);
        scene.live.snapshot = None;
    }
    builder.last_description = Some(compiled.description.clone());
    builder.schematic.set_source(&compiled.description, finished.revision);
    scene.replace(compiled.description, compiled.spatial, compiled.animation);
    let level = builder.level.clone();
    scene.ghost = scene
        .spatial
        .parts
        .iter()
        .filter(|p| !(level.is_empty() || p.component == level || p.component.starts_with(&format!("{level}/"))))
        .map(|p| p.component.clone())
        .collect();
    let names: BTreeSet<String> = builder.selected.iter().map(|n| builder.full_path(n)).collect();
    let chosen: BTreeSet<String> = scene.spatial.parts.iter().map(|p| p.component.clone()).filter(|c| names.iter().any(|n| c == n || c.starts_with(&format!("{n}/")))).collect();
    let _ = scene.set_selection(if chosen.is_empty() { SelectionTarget::None } else { SelectionTarget::Components { ids: chosen } });
    for e in &content {
        commands.entity(e).despawn();
    }
    for e in &ui_roots {
        commands.entity(e).despawn();
    }
    spawn_parts(&mut commands, &scene, &mut meshes, &mut materials, models.as_deref_mut());
    linked::spawn_nets(&mut commands, &scene, &mut meshes, &mut materials);
    // Reference images of the current level, as textured planes.
    let definition = builder.definition_id();
    let references: Vec<(String, sim_system::ReferenceImage)> = definition
        .and_then(|id| builder.document.definitions.get(&id))
        .map(|d| d.references.iter().filter(|(_, r)| r.view == ReferenceView::Spatial && r.visible).map(|(k, r)| (k.clone(), r.clone())).collect())
        .unwrap_or_default();
    let frame = builder.subsystems.get(&builder.level).copied().unwrap_or(sim_system::flatten::WorldPlacement::IDENTITY);
    for (id, reference) in references {
        let Some(asset) = builder.document.assets.get(&reference.asset).cloned() else { continue };
        let texture = match builder.textures.get(&reference.asset) {
            Some(h) => h.clone(),
            None => {
                let path = sim_system::assets::resolve(&builder.store.path, &asset);
                let loaded = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| {
                    let extension = if asset.media_type == "image/png" { "png" } else { "jpg" };
                    Image::from_buffer(&bytes, ImageType::Extension(extension), CompressedImageFormats::NONE, true, ImageSampler::default(), RenderAssetUsages::default()).map_err(|e| e.to_string())
                });
                match loaded {
                    Ok(image) => {
                        let handle = images.add(image);
                        builder.textures.insert(reference.asset.clone(), handle.clone());
                        handle
                    }
                    Err(e) => {
                        builder.status = format!("Could not load reference {id}: {e}");
                        continue;
                    }
                }
            }
        };
        let height = reference.height(&asset);
        let n = Vec3::from_array(reference.normal).normalize_or(Vec3::Y);
        let x = Vec3::from_array(reference.x_axis).reject_from(n).normalize_or(Vec3::X);
        let y = n.cross(x);
        let local = Transform::from_translation(Vec3::from_array(reference.origin)).with_rotation(Quat::from_mat3(&Mat3::from_cols(x, y, n)));
        let parent = Transform::from_translation(Vec3::from_array(frame.position)).with_rotation(Quat::from_array(frame.rotation_xyzw));
        let mut entity = commands.spawn((
            Mesh3d(meshes.add(Rectangle::new(reference.width, height))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgba(1., 1., 1., reference.opacity),
                base_color_texture: Some(texture),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                double_sided: true,
                cull_mode: None,
                ..default()
            })),
            parent * local,
            ReferenceQuad(id.clone()),
            SceneContent,
        ));
        if !reference.locked {
            entity.insert(Pickable::default()).observe(pick_reference);
        }
    }
    spawn_ui(&mut commands, &scene);
    builder.panel_dirty = true;
    if first || !builder.fitted {
        builder.fitted = true;
        // A lesson frames its scene with the scene's own camera.
        if !learning {
            orbit.home = true;
        }
    }
}

/// A click on a reference image, as the builder's `ReferencePoint` action.
fn pick_reference(click: On<Pointer<Click>>, quads: Query<&ReferenceQuad>, mut out: MessageWriter<crate::app::actions::Act<system_actions::SystemAction>>) {
    let Ok(quad) = quads.get(click.entity) else { return };
    let Some(position) = click.hit.position else { return };
    out.write(crate::app::actions::Act::ui(system_actions::SystemAction::Ui(BuildAction::ReferencePoint { id: quad.0.clone(), world: position.to_array() })));
}

/// Part clicks select instances at the current level.
pub(super) fn click_part(builder: &mut Builder, component: &str, shift: bool) {
    if builder.drag.is_some(){return;}
    let Some(name) = builder.instance_for_component(component) else {
        builder.status = "That part is outside this level; press Up (U) to leave the subsystem.".into();
        builder.panel_dirty = true;
        return;
    };
    if shift {
        if !builder.selected.remove(&name) {
            builder.selected.insert(name);
        }
    } else if builder.connect_from.is_some() || builder.mode == Mode::Connect {
        builder.selected = BTreeSet::from([name.clone()]);
        builder.port_menu = Some(name);
    } else {
        builder.selected = BTreeSet::from([name]);
        builder.port_menu = None;
    }
    if let Some(n) = builder.only_selected() {
        let _ = builder.suggestions(&n);
    }
    builder.alternatives = None;
    builder.scene_dirty = true;
    builder.panel_dirty = true;
}

/// Measure UI-thread frame times: the worst frame and the count over 50 ms
/// in each 10 s window (reported by `system_state`).
fn frame_timing(time: Res<Time>, mut builder: ResMut<Builder>) {
    let dt = time.delta_secs_f64();
    let now = time.elapsed_secs_f64();
    let frames = builder.bypass_change_detection();
    if now - frames.frames.2 > 10. {
        frames.frames = (0., 0, now);
    }
    frames.frames.0 = frames.frames.0.max(dt);
    if dt > 0.05 {
        frames.frames.1 += 1;
    }
}

fn sync_run(time: Res<Time>, mut builder: ResMut<Builder>, mut scene: ResMut<SpatialScene>, mode: Res<State<ViewerMode>>) {
    builder.poll_study();
    builder.poll_replay();
    if (builder.study.job.is_some() || builder.replay.job.is_some()) && time.elapsed_secs_f64() - builder.live_refresh > 0.25 {
        builder.live_refresh = time.elapsed_secs_f64();
        builder.panel_dirty = true;
    }
    if builder.run.is_some() && time.elapsed_secs_f64() - builder.live_refresh > 0.25 {
        builder.live_refresh = time.elapsed_secs_f64();
        builder.panel_dirty = true;
    }
    // Under the lesson screen the scene shows the lesson's recorded runs; a
    // kept (paused) builder run shows again back in build mode.
    if *mode.get() == ViewerMode::Lessons {
        return;
    }
    let Some(run) = &builder.run else { return };
    let Ok(shared) = run.worker.shared().lock() else { return };
    if let Some(snapshot) = &shared.snapshot {
        if snapshot.source_description_id == scene.description.id {
            let changed = scene.live.snapshot.as_ref().is_none_or(|s| s.frame != snapshot.frame || s.status != snapshot.status || s.error != snapshot.error);
            if changed {
                scene.live.snapshot = Some(Arc::new(snapshot.clone()));
            }
        }
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
pub use ui::{TOPBAR, STATUSBAR, LEFT_WIDTH, RIGHT_WIDTH};

/// Grab and push: with a run going, Alt-drag on a part changes the load
/// acting on it (a load torque on its shaft, a load force on its slide) in
/// the running model only; letting go restores it. The file is untouched.
fn grab_push(buttons: Res<ButtonInput<MouseButton>>, keys: Res<ButtonInput<KeyCode>>, mut motion: MessageReader<bevy::input::mouse::MouseMotion>, pointed: Res<crate::view::PartHover>, scene: Res<SpatialScene>, mut builder: ResMut<Builder>) {
    let drag: f32 = motion.read().map(|e| e.delta.x).sum();
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    let running = builder.run.is_some();
    if builder.grab.is_none() {
        if !(running && alt && buttons.just_pressed(MouseButton::Left)) {
            return;
        }
        let Some(component) = pointed.0.clone() else { return };
        let Some((parameter, original)) = load_for(&scene, &component) else {
            builder.status = "Nothing to push on here: this part has no load torque or load force on its shaft or slide.".into();
            builder.panel_dirty = true;
            return;
        };
        let per_pixel = original.abs().max(0.01) / 150.;
        builder.grab = Some(Grab { parameter, original, per_pixel, dragged: 0., sent: std::time::Instant::now(), applied: original });
        return;
    }
    let mut grab = builder.grab.clone().unwrap();
    if !buttons.pressed(MouseButton::Left) {
        // Let go: the load returns to the file's value.
        builder.grab = None;
        if swap_with(&mut builder, &grab.parameter, grab.original) {
            builder.status = "Released: the load is back to its value in the file.".into();
        }
        builder.panel_dirty = true;
        return;
    }
    grab.dragged += drag;
    let value = grab.original + grab.dragged as f64 * grab.per_pixel;
    if (value - grab.applied).abs() > 1e-12 && grab.sent.elapsed() > std::time::Duration::from_millis(120) {
        grab.applied = value;
        grab.sent = std::time::Instant::now();
        builder.panel_dirty = true;
        if !swap_with(&mut builder, &grab.parameter, value) {
            // The run was stopped and the status says why; the gesture ends.
            builder.grab = None;
            return;
        }
        builder.status = format!("Pushing: {} = {} (was {}). Let go to release.", grab.parameter, crate::builder::ui::num(value), crate::builder::ui::num(grab.original));
    }
    builder.grab = Some(grab);
}

/// The load element acting on a part's shaft or slide, and its value now.
fn load_for(scene: &SpatialScene, component: &str) -> Option<(String, f64)> {
    let d = &scene.description;
    let nets: Vec<&sim_inspect::NetDescription> = d.nets.values().filter(|n| n.ports.iter().any(|p| d.ports.get(p).is_some_and(|q| q.component == component))).collect();
    for net in nets {
        for p in &net.ports {
            let c = &d.components.get(&d.ports.get(p)?.component)?;
            let parameter = match c.component_type.as_str() {
                "rotational.load_torque" => "torque",
                "translational.load_force" => "force",
                _ => continue,
            };
            let value = c.parameters.get(parameter).map(|v| v.value)?;
            return Some((format!("{}.{parameter}", c.id), value));
        }
    }
    None
}

/// Swap the running model for the document with one parameter changed,
/// through the run's fidelity (`Builder::hot_swap`); the file's document is
/// untouched. False only when the swap stopped the run (status explains).
fn swap_with(builder: &mut Builder, parameter: &str, value: f64) -> bool {
    let Some(id) = builder.run.as_ref().map(|r| r.description_id.clone()) else { return true };
    let mut doc = builder.document.clone();
    let Ok(command) = sim_runtime::lesson::set_command(parameter, value) else { return true };
    if sim_system::apply(&mut doc, &builder.registry, &[command]).is_ok() {
        return builder.hot_swap(&doc, id);
    }
    true
}

/// Test hooks for code outside the builder (the mode-switch test).
#[cfg(test)]
impl Builder {
    /// Open a text-field draft: one of the `system_open` blockers.
    pub(crate) fn test_open_draft(&mut self, text: &str) {
        self.start_input(Purpose::Filter, text.into());
    }
    pub(crate) fn test_drop_draft(&mut self) {
        self.input = None;
    }
    /// A live run is kept: whether its thread reports it running (None: no run).
    pub(crate) fn test_run(&self) -> Option<bool> {
        self.run.as_ref().map(|_| self.running())
    }
}

#[cfg(test)]
mod replay_tests {
    use super::*;

    fn wait(b: &mut Builder) -> ReplayOutcome {
        let id = b.replay.job.as_ref().unwrap().id.clone();
        for _ in 0..600 {
            b.poll_replay();
            if b.replay.job.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        b.replay.outcomes[&id].clone()
    }

    #[test]
    fn saved_runs_replay_in_the_background_and_cancel() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = std::env::temp_dir().join(format!("builder-replay-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("winch.system.json");
        std::fs::copy(root.join("examples/systems-builder/worm-drive/winch.system.json"), &path).unwrap();
        let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
        let mut b = Builder::open(path.clone(), root.join("library/systems"), registry.clone()).unwrap();
        let runs = sim_runtime::run_history::dir_for(&path);
        let select = vec!["drum.shaft.speed".to_string()];
        let clean = sim_runtime::run_history::record(&b.document, &registry, 0.5, system_builder::config_for(&b.document), &select, "clean").unwrap();
        let mut edited = sim_runtime::run_history::record(&b.document, &registry, 0.5, system_builder::config_for(&b.document), &select, sim_runtime::run_history::EDITED_WHILE_RUNNING).unwrap().with_provenance(Fidelity::Detailed, true);
        edited.id.push_str("-edited");
        let long = sim_runtime::run_history::record(&b.document, &registry, 30., system_builder::config_for(&b.document), &select, "long").unwrap();
        let mut long = long;
        long.id.push_str("-long");
        for r in [&clean, &edited, &long] {
            sim_runtime::run_history::save(&runs, r).unwrap();
        }
        b.runs = sim_runtime::run_history::list(&runs);

        let e = b.replay_run("does-not-exist").unwrap_err();
        assert!(e.contains("does-not-exist"), "{e}");
        assert!(b.replay.job.is_none());

        b.replay_run(&clean.id).unwrap();
        assert_eq!(b.replay.outcomes[&clean.id].status, "running");
        let o = wait(&mut b);
        assert_eq!((o.status, o.max_rel_diff, o.edited_while_running), ("done", Some(0.), false), "{o:?}");
        assert!(o.headline().starts_with("Reproduced exactly"), "{}", o.headline());
        assert!(o.samples.unwrap() > 0);

        b.replay_run(&edited.id).unwrap();
        let o = wait(&mut b);
        assert_eq!((o.status, o.max_rel_diff, o.edited_while_running), ("done", Some(0.), true));

        // Cancel stops the worker between steps; it reports no result.
        b.replay_run(&long.id).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(b.cancel_replay());
        let o = b.replay.outcomes[&long.id].clone();
        assert_eq!((o.status, o.max_rel_diff), ("cancelled", None));
        assert!(!b.cancel_replay(), "nothing left to cancel");
        // The shared replay really stops: a raised flag ends a 30 s rerun at once.
        let started = std::time::Instant::now();
        let stopped = sim_runtime::run_history::replay_with_cancel(&long, &registry, Some(&std::sync::atomic::AtomicBool::new(true)));
        assert_eq!(stopped.unwrap_err(), system_builder::CANCELLED);
        assert!(started.elapsed().as_secs_f64() < 2., "{:?}", started.elapsed());
        let state = b.state_json();
        assert_eq!(state["replay"]["outcomes"].as_array().unwrap().len(), 3);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Step and Reset on a live run go through `run_step`/`run_reset` (the
    /// buttons' and REST's path) to `Command::Step`/`Command::Reset` on the
    /// real run thread: one step is exactly one interval, stepping a running
    /// run is refused, reset keeps the old run and clears the history, and a
    /// run saved after the reset holds only post-reset samples and replays
    /// exactly. Starting/resuming uses the control channel (no Bevy scene).
    #[test]
    fn live_run_step_and_reset_share_the_session_and_keep_work() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = std::env::temp_dir().join(format!("builder-step-reset-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("winch.system.json");
        std::fs::copy(root.join("examples/systems-builder/worm-drive/winch.system.json"), &path).unwrap();
        let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
        let mut b = Builder::open(path.clone(), root.join("library/systems"), registry.clone()).unwrap();
        let document = b.document.clone();
        let interval = system_builder::config_for(&document).interval;
        let compiled = system_builder::compile(&document, &registry, system_builder::config_for(&document)).unwrap();
        let observed: Vec<String> = compiled.description.observables.keys().filter(|id| system_builder::observable_key(&compiled.description, id).contains("drum.shaft.speed")).cloned().collect();
        let key = observed[0].clone();
        b.last_description = Some(compiled.description.clone());
        assert!(b.run_step().is_err() && b.run_reset().is_err(), "no run: refused");
        assert_eq!(b.state_json()["live_run"], serde_json::Value::Null);
        b.run = Some(LiveRun::spawn(document.clone(), registry.clone(), observed, compiled.description.id.clone(), Fidelity::Detailed));
        let shared = b.run.as_ref().unwrap().worker.shared().clone();
        let status = || shared.lock().unwrap().snapshot.as_ref().and_then(|x| x.status.clone());
        let time = || status().map(|x| x.time).unwrap_or(0.);
        let started = std::time::Instant::now();
        let until = |done: &dyn Fn() -> bool| {
            while !done() && started.elapsed().as_secs() < 60 {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(done(), "timed out");
        };
        // Stepping a running run is refused, never ignored.
        assert!(shared.lock().unwrap().running);
        assert_eq!(b.run_step(), Err("pause the run before stepping".to_string()));
        until(&|| time() >= 0.3);
        b.run_pause();
        until(&|| !shared.lock().unwrap().running && status().is_some_and(|x| x.phase == sim_inspect::live::Phase::Paused));
        let before = status().unwrap();
        b.run_step().unwrap();
        until(&|| status().is_some_and(|x| x.step == before.step + 1));
        let after = status().unwrap();
        assert!((after.time - before.time - interval).abs() < 1e-9, "{} -> {} with dt {interval}", before.time, after.time);
        assert_eq!(b.history(&key).last().map(|p| p[0]), Some(after.time), "the stepped point is on the trace");
        let state = b.state_json();
        assert_eq!((state["realtime"].as_bool(), state["live_run"]["step"].as_u64(), state["live_run"]["phase"].as_str(), state["live_run"]["fidelity"].as_str()), (Some(false), Some(after.step), Some("paused"), Some("detailed")));
        // Reset keeps the run so far, then returns to t = 0 with no history.
        let runs = b.runs.len();
        b.run_reset().unwrap();
        assert_eq!(b.runs.len(), runs + 1, "{}", b.status);
        assert!(b.status.contains(&format!("kept the previous run as {}", b.runs[0].1.id)), "{}", b.status);
        assert!(b.runs[0].1.duration >= 0.3);
        assert!(b.history(&key).is_empty());
        until(&|| !shared.lock().unwrap().reset_pending);
        let reset = status().unwrap();
        assert_eq!((reset.time, reset.step, reset.phase, reset.generation), (0., 0, sim_inspect::live::Phase::Paused, before.generation + 1));
        assert!(b.history(&key).is_empty());
        assert_eq!(b.state_json()["live_run"]["time"].as_f64(), Some(0.));
        // Step once from t = 0, resume briefly, pause and save.
        b.run_step().unwrap();
        until(&|| status().is_some_and(|x| x.step == 1));
        b.run.as_ref().unwrap().worker.send(RunControl::Start).unwrap();
        until(&|| time() >= 0.12);
        b.run_pause();
        until(&|| !shared.lock().unwrap().running);
        let duration = time();
        assert!(duration < 0.3);
        let saved = b.save_run("after reset").unwrap();
        let record = sim_runtime::run_history::load(&saved).unwrap();
        assert_eq!((record.duration, record.fidelity, record.edited_while_running()), (duration, Some(Fidelity::Detailed), false));
        let times = &record.series.iter().find(|s| s.observable == key).unwrap().times;
        assert_eq!(times.first().copied(), Some(interval), "starts at the post-reset step");
        assert!(times.windows(2).all(|w| w[0] < w[1]) && times.iter().all(|t| *t <= duration), "only post-reset samples: {times:?}");
        b.replay_run(&record.id).unwrap();
        let o = wait(&mut b);
        assert_eq!((o.status, o.max_rel_diff), ("done", Some(0.)), "{o:?}");
        assert!(o.headline().starts_with("Reproduced exactly"), "{}", o.headline());
        b.run = None;
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The Alt-drag grab swaps through `hot_swap` like an edit: a realtime run
    /// gets the realtime profile of the pushed document (never the detailed
    /// one), `run.document` is what was sent, the file's document is
    /// untouched, and a save records the pushed model as edited. Release goes
    /// back to the file value and the run stays edited. Pause says so.
    #[test]
    fn grab_swap_keeps_the_run_fidelity_and_marks_it_edited() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = std::env::temp_dir().join(format!("builder-grab-swap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("winch.system.json");
        std::fs::copy(root.join("examples/systems-builder/worm-drive/winch.system.json"), &path).unwrap();
        let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
        let mut b = Builder::open(path.clone(), root.join("library/systems"), registry.clone()).unwrap();
        let file = b.document.clone();
        let profile = Fidelity::Realtime.document(&file, &registry).unwrap();
        let compiled = system_builder::compile(&profile, &registry, system_builder::config_for(&profile)).unwrap();
        let (parameter, original) = compiled.description.components.values().find_map(|c| match c.component_type.as_str() {
            "rotational.load_torque" => Some((format!("{}.torque", c.id), c.parameters["torque"].value)),
            "translational.load_force" => Some((format!("{}.force", c.id), c.parameters["force"].value)),
            _ => None,
        }).expect("the winch has a load the grab pushes on");
        b.last_description = Some(compiled.description.clone());
        b.run = Some(LiveRun::spawn(profile.clone(), registry.clone(), Vec::new(), compiled.description.id.clone(), Fidelity::Realtime));
        let shared = b.run.as_ref().unwrap().worker.shared().clone();
        let time = || shared.lock().unwrap().snapshot.as_ref().and_then(|x| x.status.as_ref()).map(|x| x.time).unwrap_or(0.);
        let started = std::time::Instant::now();
        while time() < 0.15 && started.elapsed().as_secs() < 60 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        // Push: the same edit the gesture makes, on the detailed document.
        let pushed = original * 0.5;
        let mut detailed = file.clone();
        sim_system::apply(&mut detailed, &registry, &[sim_runtime::lesson::set_command(&parameter, pushed).unwrap()]).unwrap();
        let expected = Fidelity::Realtime.document(&detailed, &registry).unwrap();
        assert_ne!(expected.content_hash(), detailed.content_hash());
        assert!(swap_with(&mut b, &parameter, pushed));
        let run = b.run.as_ref().unwrap();
        assert_eq!(run.document.content_hash(), expected.content_hash(), "the realtime profile of the pushed document was sent");
        assert_ne!(run.document.content_hash(), detailed.content_hash(), "never the detailed document");
        assert_eq!((run.edited, run.fidelity, run.description_id.as_str()), (true, Fidelity::Realtime, compiled.description.id.as_str()));
        assert_eq!(b.document.content_hash(), file.content_hash(), "the file's document is untouched");
        // Pause is truthful and a save records the pushed model as edited.
        b.run_pause();
        assert!(b.status.starts_with("Paused"), "{}", b.status);
        while shared.lock().unwrap().running && started.elapsed().as_secs() < 60 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(b.state_json()["live_run"]["phase"].as_str(), Some("paused"));
        let record = sim_runtime::run_history::load(&b.save_run("grab").unwrap()).unwrap();
        assert_eq!((record.fidelity, record.edited_while_running(), record.content_hash.clone()), (Some(Fidelity::Realtime), true, expected.content_hash()));
        // Release: back to the file value, still edited.
        assert!(swap_with(&mut b, &parameter, original));
        let run = b.run.as_ref().unwrap();
        let mut back = file.clone();
        sim_system::apply(&mut back, &registry, &[sim_runtime::lesson::set_command(&parameter, original).unwrap()]).unwrap();
        assert_eq!((run.document.content_hash(), run.edited), (Fidelity::Realtime.document(&back, &registry).unwrap().content_hash(), true));
        assert_eq!(b.document.content_hash(), file.content_hash());
        b.run = None;
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A run started with Realtime on is saved with the realtime profile it
    /// simulated (document, config, fidelity) and replays exactly. Drives the
    /// real live thread and `save_run`; the Bevy system that hot-swaps edits
    /// is not exercised here.
    #[test]
    fn realtime_runs_save_the_profile_they_ran_and_replay_exactly() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = std::env::temp_dir().join(format!("builder-realtime-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("winch.system.json");
        std::fs::copy(root.join("examples/systems-builder/worm-drive/winch.system.json"), &path).unwrap();
        let registry = sim_runtime::registry_with_parts(&root.join("library/parts")).0;
        let mut b = Builder::open(path.clone(), root.join("library/systems"), registry.clone()).unwrap();
        let profile = Fidelity::Realtime.document(&b.document, &registry).unwrap();
        assert_ne!(profile.content_hash(), b.document.content_hash(), "the winch has a realtime profile");
        let compiled = system_builder::compile(&profile, &registry, system_builder::config_for(&profile)).unwrap();
        let observed: Vec<String> = compiled.description.observables.keys().filter(|id| system_builder::observable_key(&compiled.description, id).contains("drum.shaft.speed")).cloned().collect();
        assert!(!observed.is_empty());
        b.last_description = Some(compiled.description.clone());
        // What start_run launches with Realtime on.
        b.realtime = true;
        b.run = Some(LiveRun::spawn(profile.clone(), registry.clone(), observed, compiled.description.id.clone(), Fidelity::Realtime));
        let shared = b.run.as_ref().unwrap().worker.shared().clone();
        let time = || shared.lock().unwrap().snapshot.as_ref().and_then(|x| x.status.as_ref()).map(|x| x.time).unwrap_or(0.);
        let started = std::time::Instant::now();
        while time() < 0.3 && started.elapsed().as_secs() < 60 {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        b.run.as_ref().unwrap().worker.send(RunControl::Pause).unwrap();
        while shared.lock().unwrap().running && started.elapsed().as_secs() < 60 {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(time() >= 0.3, "run advanced to {}", time());
        // Turning the toggle off mid-run does not relabel the run.
        b.realtime = false;
        let saved = b.save_run("rt").unwrap();
        let record = sim_runtime::run_history::load(&saved).unwrap();
        assert_eq!((record.schema.as_str(), record.fidelity, record.edited_while_running()), (sim_runtime::run_history::SCHEMA, Some(Fidelity::Realtime), false));
        assert_eq!(record.content_hash, profile.content_hash());
        assert_eq!(serde_json::to_value(&record.config).unwrap(), serde_json::to_value(system_builder::config_for(&profile)).unwrap());
        assert_eq!(b.runs[0].1.fidelity, "realtime");
        b.replay_run(&record.id).unwrap();
        let o = wait(&mut b);
        assert_eq!((o.status, o.max_rel_diff, o.fidelity.as_str()), ("done", Some(0.), "realtime"), "{o:?}");
        assert!(o.headline().starts_with("Reproduced exactly") && o.headline().ends_with("realtime"), "{}", o.headline());
        let state = b.state_json();
        assert_eq!(state["replay"]["outcomes"][0]["fidelity"], "realtime");
        // The detailed document (what was recorded before) does not reproduce it.
        let mut detailed = record.clone();
        detailed.config = system_builder::config_for(&b.document);
        detailed.document = b.document.clone();
        let wrong = sim_runtime::run_history::replay(&detailed, &registry);
        assert!(wrong.as_ref().map(|d| *d > 0.).unwrap_or(true), "{wrong:?}");
        b.run = None;
        std::fs::remove_dir_all(&dir).ok();
    }
}

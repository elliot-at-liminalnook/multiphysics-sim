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
use sim_runtime::system_builder;
use sim_system::library::{self, Alternative};
use sim_system::{Command as SystemCommand, InstanceKind, InstanceSpec, ReferenceView, Resolver, SystemDocument, SystemStore, Terminal};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};

const PALETTE_ROWS: usize = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Library,
    Outline,
    References,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
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

#[derive(Clone, Debug, PartialEq)]
enum Purpose {
    Filter,
    Parameter { name: String, parameter: String },
    Rename(String),
    ImportImage,
    Distance { id: String, first: [f32; 3], second: [f32; 3] },
    ReferenceWidth(String),
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
}

enum RunControl {
    Start,
    Pause,
}

struct LiveRun {
    control: mpsc::Sender<RunControl>,
    shared: Arc<Mutex<RunShared>>,
    description_id: String,
}

#[derive(Resource)]
pub struct Builder {
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
    run: Option<LiveRun>,
    compile_error: Option<String>,
    findings: Vec<sim_system::Finding>,
    /// Subsystem instance paths -> world placement, for ghosting and drill-in framing.
    subsystems: BTreeMap<String, sim_system::flatten::WorldPlacement>,
    textures: BTreeMap<String, Handle<Image>>,
    /// Registry elements never change while running; computed once.
    elements: Vec<library::ElementEntry>,
    /// Background compile of the latest document.
    job: Option<Mutex<mpsc::Receiver<CompileResult>>>,
    tab: Tab,
    category: Option<&'static str>,
    mode: Mode,
    live_refresh: f64,
    fitted: bool,
}

/// Everything the scene needs from one compile, built off the UI thread.
struct CompileResult {
    revision: u64,
    result: Result<CompileOutput, String>,
}
struct CompileOutput {
    description: SystemDescription,
    spatial: SpatialDescription,
    animation: Option<sim_inspect::animation::AnimationDescription>,
    findings: Vec<sim_system::Finding>,
    subsystems: BTreeMap<String, sim_system::flatten::WorldPlacement>,
    runtime_error: Option<String>,
}

fn compile_job(document: SystemDocument, registry: BehaviorRegistry) -> mpsc::Receiver<CompileResult> {
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
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
        let _ = send.send(CompileResult { revision: document.revision, result });
    });
    receive
}

impl Builder {
    pub fn open(path: PathBuf, library_dir: PathBuf, registry: BehaviorRegistry) -> Result<Self, String> {
        let store = SystemStore::new(path);
        let document = store.load_valid(&registry).map_err(|e| e.to_string())?;
        let mut builder = Self {
            stamp: store.stamp(),
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
        };
        builder.elements = library::elements(&builder.registry);
        builder.refresh_palette();
        Ok(builder)
    }

    pub fn typing(&self) -> bool {
        self.input.is_some()
    }

    fn refresh_palette(&mut self) {
        let mut items = Vec::new();
        for id in self.document.definitions.keys() {
            if *id == self.document.root || self.definition_id().as_deref() == Some(id.as_str()) {
                continue;
            }
            let d = &self.document.definitions[id];
            items.push(PaletteItem { label: d.label.clone(), detail: format!("subsystem - {id}"), kind: InstanceKind::Subsystem { definition: id.clone() }, library_path: None, domain: "system".into() });
        }
        if let Ok(entries) = library::list(&self.library_dir, &self.registry) {
            for e in entries {
                if self.document.definitions.contains_key(&e.id) {
                    continue;
                }
                items.push(PaletteItem { label: e.label, detail: format!("library - {}", e.id), kind: InstanceKind::Subsystem { definition: e.id }, library_path: Some(e.path), domain: "library".into() });
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
            .filter(|p| self.category.is_none_or(|c| ui::category(&p.domain) == c))
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
            self.status = e;
            self.panel_dirty = true;
        }
    }

    pub fn reload(&mut self) {
        match self.store.load_valid(&self.registry) {
            Ok(document) => {
                self.document = document;
                self.stamp = self.store.stamp();
                if self.definition_id().is_none() {
                    self.level.clear();
                }
                let names = self.definition_id().and_then(|id| self.document.definitions.get(&id).map(|d| d.instances.keys().cloned().collect::<BTreeSet<_>>())).unwrap_or_default();
                self.selected.retain(|s| names.contains(s));
                self.alternatives = None;
                self.refresh_palette();
                self.scene_dirty = true;
                self.panel_dirty = true;
            }
            Err(e) => self.status = format!("Could not load the system: {e}"),
        }
    }

    pub fn undo(&mut self) {
        let result = self.store.undo().map(|a| a.outcomes[0].message.clone()).map_err(|e| e.to_string());
        match result {
            Ok(m) => {
                self.status = m;
                self.reload();
            }
            Err(e) => self.status = e,
        }
        self.panel_dirty = true;
    }

    pub fn redo(&mut self) {
        let result = self.store.redo().map(|a| a.outcomes[0].message.clone()).map_err(|e| e.to_string());
        match result {
            Ok(m) => {
                self.status = m;
                self.reload();
            }
            Err(e) => self.status = e,
        }
        self.panel_dirty = true;
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
        let rest = if self.level.is_empty() { component } else { component.strip_prefix(&format!("{}/", self.level))? };
        rest.split('/').next().map(str::to_string)
    }

    fn unique_name(&self, base: &str) -> String {
        let taken: BTreeSet<String> = self.definition_id().and_then(|id| self.document.definitions.get(&id)).map(|d| d.instances.keys().cloned().collect()).unwrap_or_default();
        let base: String = base.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
        let base = base.trim_matches('_').chars().take(24).collect::<String>();
        let base = if base.is_empty() { "part".to_string() } else { base };
        (1..).map(|n| format!("{base}{n}")).find(|c| !taken.contains(c)).unwrap()
    }

    /// Place a palette item at the next free spot on this level's grid.
    fn place(&mut self, item: PaletteItem) {
        let mut commands = Vec::new();
        if let Some(path) = &item.library_path {
            match library::import(std::path::Path::new(path)) {
                Ok(definitions) => commands.push(SystemCommand::AddDefinitions { definitions }),
                Err(e) => {
                    self.status = e.to_string();
                    return;
                }
            }
        }
        let count = self.definition_id().and_then(|id| self.document.definitions.get(&id)).map(|d| d.instances.len()).unwrap_or(0);
        let position = [(count % 6) as f32 * 0.03, 0., (count / 6) as f32 * 0.03];
        let base = match &item.kind {
            InstanceKind::Element { component_type } => component_type.rsplit('.').next().unwrap_or("part").to_string(),
            InstanceKind::Subsystem { definition } => definition.rsplit('.').next().unwrap_or("sub").to_string(),
        };
        let name = self.unique_name(&base);
        let mut spec = InstanceSpec::new(item.kind.clone()).at(position);
        spec.label = item.label.clone();
        commands.push(SystemCommand::AddInstance { at: self.level.clone(), name: name.clone(), instance: spec });
        let result = self.apply(&format!("Place {}", item.label), commands);
        if result.is_ok() {
            self.selected = BTreeSet::from([name]);
        }
        self.report(result);
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
        self.input = Some(TextInput { purpose, buffer: initial });
        self.panel_dirty = true;
    }

    fn commit_input(&mut self) {
        let Some(TextInput { purpose, buffer }) = self.input.take() else { return };
        self.panel_dirty = true;
        let text = buffer.trim().to_string();
        let result: Result<(), String> = match purpose {
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
            Purpose::Distance { id, first, second } => match text.parse::<f32>() {
                Ok(d) => self.apply("Calibrate reference", vec![SystemCommand::CalibrateReference { at: self.level.clone(), id, first, second, distance: d }]).map(|_| ()),
                Err(_) => Err("Enter the real distance between the two points in meters".into()),
            },
            Purpose::ReferenceWidth(id) => match (text.parse::<f32>(), self.reference(&id)) {
                (Ok(w), Some(mut r)) => {
                    r.width = w;
                    self.apply("Resize reference", vec![SystemCommand::SetReference { at: self.level.clone(), id, reference: r }]).map(|_| ())
                }
                _ => Err("Enter a width in meters".into()),
            },
        };
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
        self.run = None;
    }

    fn start_run(&mut self, scene: &SpatialScene) {
        if let Some(run) = &self.run {
            if run.description_id == scene.description.id {
                let _ = run.control.send(RunControl::Start);
                return;
            }
        }
        let document = self.document.clone();
        let registry = self.registry.clone();
        let observed: Vec<String> = scene
            .animation
            .as_ref()
            .map(|a| a.colors.iter().map(|c| c.observable.clone()).chain(a.rotations.iter().map(|r| r.observable.clone())).chain(a.readouts.iter().map(|r| r.observable.clone())).collect())
            .unwrap_or_default();
        let (control, commands) = mpsc::channel();
        let shared = Arc::new(Mutex::new(RunShared { snapshot: None, running: true, speed: 1.0 }));
        let thread_shared = shared.clone();
        let description_id = scene.description.id.clone();
        let source_id = description_id.clone();
        std::thread::spawn(move || run_thread(document, registry, observed, source_id, commands, thread_shared));
        self.run = Some(LiveRun { control, shared, description_id });
        self.status = "Running on the shared runtime (background thread, paced to real time at most).".into();
    }

    fn pause_run(&mut self) {
        if let Some(run) = &self.run {
            let _ = run.control.send(RunControl::Pause);
        }
    }

    pub fn run_start(&mut self, scene: &SpatialScene) {
        self.start_run(scene);
        self.panel_dirty = true;
    }
    pub fn run_pause(&mut self) {
        self.pause_run();
        self.panel_dirty = true;
    }
    pub fn select(&mut self, names: Vec<String>) {
        self.selected = names.into_iter().collect();
        self.alternatives = None;
        self.scene_dirty = true;
        self.panel_dirty = true;
    }
    pub fn state_json(&self) -> serde_json::Value {
        serde_json::json!({
            "path": self.store.path,
            "title": self.document.title,
            "revision": self.document.revision,
            "level": self.level,
            "definition": self.definition_id(),
            "selected": self.selected,
            "status": self.status,
            "findings": self.findings,
            "compile_error": self.compile_error,
            "running": self.running(),
            "history": self.store.history(),
        })
    }

    fn running(&self) -> bool {
        self.run.as_ref().is_some_and(|r| r.shared.lock().map(|s| s.running).unwrap_or(false))
    }
}

fn run_thread(document: SystemDocument, registry: BehaviorRegistry, observed: Vec<String>, source_id: String, commands: mpsc::Receiver<RunControl>, shared: Arc<Mutex<RunShared>>) {
    let publish = |status: Option<sim_inspect::live::SessionStatus>, frame: Option<sim_inspect::SampleFrame>, error: Option<String>, running: bool| {
        if let Ok(mut s) = shared.lock() {
            s.snapshot = Some(sim_inspect::live::LiveSnapshot { version: 1, source_description_id: source_id.clone(), description: None, status, frame, error });
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
    let mut wall = std::time::Instant::now();
    let mut sim_at_wall = 0.0;
    let mut last_publish = std::time::Instant::now() - std::time::Duration::from_secs(1);
    loop {
        loop {
            match commands.try_recv() {
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
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if last_publish.elapsed().as_millis() >= 33 {
            let speed = if wall.elapsed().as_secs_f64() > 0. { (session.status().time - sim_at_wall) / wall.elapsed().as_secs_f64() } else { 0. };
            publish(Some(session.status().clone()), Some(session.latest().clone()), None, running);
            if let Ok(mut s) = shared.lock() {
                s.speed = speed;
            }
            last_publish = std::time::Instant::now();
        }
    }
}

#[derive(Component)]
pub(super) struct BuilderPanel;
#[derive(Component)]
pub(super) struct ReferenceQuad(String);

#[derive(Component, Clone, Debug)]
enum BuildAction {
    Tab(Tab),
    Category(Option<&'static str>),
    SetMode(Mode),
    Reset,
    Up,
    Level(String),
    Select(String),
    Open(String),
    Place(usize),
    Filter,
    Group,
    Ungroup,
    Swap,
    SwapTo(usize),
    MakeUnique,
    Delete,
    Rename,
    Terminal(Terminal),
    CancelConnect,
    Disconnect(Terminal),
    Parameter(String, String),
    Undo,
    Redo,
    Run,
    Pause,
    ImportImage,
    Opacity(String, f32),
    Lock(String),
    Calibrate(String),
    Width(String),
    RemoveReference(String),
    SaveToLibrary,
}

pub struct BuilderPlugin;
impl Plugin for BuilderPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (watch, text_input, drops, builder_buttons, builder_keys, rebuild_scene, sync_run, ui::rebuild_panel, ui::scroll_panels, ui::hover).chain().before(update_parts),
        )
        .add_systems(Startup, ui::load_fonts);
    }
}

fn watch(time: Res<Time>, mut builder: ResMut<Builder>) {
    let now = time.elapsed_secs_f64();
    if now - builder.checked < 0.4 {
        return;
    }
    builder.checked = now;
    if builder.store.stamp() != builder.stamp {
        builder.reload();
        builder.status = "Reloaded: the system file changed in another editor.".into();
    }
}

fn text_input(mut events: EventReader<KeyboardInput>, mut builder: ResMut<Builder>) {
    if builder.input.is_none() {
        events.clear();
        return;
    }
    for e in events.read() {
        if e.state != ButtonState::Pressed {
            continue;
        }
        match &e.logical_key {
            Key::Enter => {
                builder.commit_input();
                return;
            }
            Key::Escape => {
                builder.input = None;
                builder.panel_dirty = true;
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

fn drops(mut events: EventReader<FileDragAndDrop>, mut builder: ResMut<Builder>) {
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

fn builder_buttons(
    actions: Query<(&Interaction, &BuildAction), (Changed<Interaction>, With<Button>)>,
    mut builder: ResMut<Builder>,
    scene: Res<SpatialScene>,
    mut orbit: Single<&mut Orbit>,
) {
    let pressed: Vec<BuildAction> = actions.iter().filter(|(i, _)| **i == Interaction::Pressed).map(|(_, a)| a.clone()).collect();
    for action in pressed {
        dispatch(&mut builder, &scene, &mut orbit, action);
    }
}

fn dispatch(builder: &mut Builder, scene: &SpatialScene, orbit: &mut Orbit, action: BuildAction) {
    builder.panel_dirty = true;
    match action {
        BuildAction::Tab(tab) => builder.tab = tab,
        BuildAction::Category(category) => {
            builder.category = category;
            builder.page = 0;
        }
        BuildAction::SetMode(mode) => {
            builder.mode = mode;
            builder.connect_from = None;
            builder.status = match mode {
                Mode::Connect => "Connect: select a part, pick a port in the inspector, then pick the other port.".into(),
                Mode::Select => "Select: click parts; shift-click adds to the selection.".into(),
            };
            if let Some(name) = builder.only_selected() {
                builder.port_menu = (mode == Mode::Connect).then_some(name);
            }
        }
        BuildAction::Reset => {
            builder.stop_run();
            builder.start_run(scene);
        }
        BuildAction::Up => {
            let parent = builder.level.rsplit_once('/').map(|(p, _)| p.to_string()).unwrap_or_default();
            let child = builder.level.rsplit('/').next().unwrap_or("").to_string();
            let r = builder.set_level(&parent);
            if !child.is_empty() {
                builder.selected = BTreeSet::from([child]);
            }
            orbit.home = true;
            builder.report(r);
        }
        BuildAction::Level(path) => {
            let r = builder.set_level(&path);
            orbit.home = true;
            builder.report(r);
        }
        BuildAction::Select(name) => {
            builder.selected = BTreeSet::from([name]);
            builder.alternatives = None;
            builder.scene_dirty = true;
        }
        BuildAction::Open(name) => {
            let path = builder.full_path(&name);
            let r = builder.set_level(&path);
            orbit.home = true;
            builder.report(r);
        }
        BuildAction::Place(index) => {
            let item = builder.filtered().get(index).cloned().cloned();
            if let Some(item) = item {
                builder.place(item);
            }
        }
        BuildAction::Filter => {
            let initial = builder.filter.clone();
            builder.start_input(Purpose::Filter, initial);
        }
        BuildAction::Group => builder.group_selected(),
        BuildAction::Ungroup => {
            if let Some(name) = builder.only_selected() {
                let r = builder.apply("Ungroup", vec![SystemCommand::Ungroup { at: builder.level.clone(), name }]);
                if r.is_ok() {
                    builder.selected.clear();
                }
                builder.report(r);
            }
        }
        BuildAction::Swap => {
            if let Some(name) = builder.only_selected() {
                match library::alternatives(&builder.document, &builder.registry, Some(&builder.library_dir), &builder.level, &name) {
                    Ok(list) => {
                        builder.status = format!("{} implementations fit {name}'s connected ports", list.len());
                        builder.alternatives = Some((name, list));
                    }
                    Err(e) => builder.status = e.to_string(),
                }
            }
        }
        BuildAction::SwapTo(index) => {
            if let Some((name, list)) = builder.alternatives.clone() {
                if let Some(alt) = list.get(index) {
                    let mut commands = Vec::new();
                    if let Some(path) = &alt.library_path {
                        match library::import(std::path::Path::new(path)) {
                            Ok(definitions) => commands.push(SystemCommand::AddDefinitions { definitions }),
                            Err(e) => {
                                builder.status = e.to_string();
                                return;
                            }
                        }
                    }
                    commands.push(SystemCommand::Swap { at: builder.level.clone(), name, kind: alt.kind.clone(), keep_parameters: true });
                    let r = builder.apply(&format!("Swap to {}", alt.label), commands);
                    builder.report(r);
                }
            }
        }
        BuildAction::MakeUnique => {
            if let Some(name) = builder.only_selected() {
                if let Some(InstanceKind::Subsystem { definition }) = builder.spec(&name).map(|s| s.kind) {
                    let mut id = format!("{definition}_{name}");
                    let mut n = 2;
                    while builder.document.definitions.contains_key(&id) {
                        id = format!("{definition}_{name}_{n}");
                        n += 1;
                    }
                    let r = builder.apply("Make unique", vec![SystemCommand::MakeUnique { at: builder.level.clone(), name, definition: id }]);
                    builder.report(r);
                }
            }
        }
        BuildAction::Delete => builder.remove_selected(),
        BuildAction::Rename => {
            if let Some(name) = builder.only_selected() {
                builder.start_input(Purpose::Rename(name.clone()), name);
            }
        }
        BuildAction::Terminal(t) => match builder.connect_from.take() {
            None => {
                builder.status = format!("Connecting from {t}: pick the other terminal (select another part, then its port).");
                builder.connect_from = Some(t);
            }
            Some(from) if from == t => builder.status = "Connection cancelled.".into(),
            Some(from) => {
                let r = builder.apply("Connect", vec![SystemCommand::Connect { at: builder.level.clone(), terminals: vec![from, t], label: String::new() }]);
                builder.report(r);
            }
        },
        BuildAction::CancelConnect => {
            builder.connect_from = None;
            builder.status = "Connection cancelled.".into();
        }
        BuildAction::Disconnect(t) => {
            let r = builder.apply("Disconnect", vec![SystemCommand::Disconnect { at: builder.level.clone(), terminal: t }]);
            builder.report(r);
        }
        BuildAction::Parameter(name, parameter) => {
            let current = builder.spec(&name).and_then(|s| s.parameters.get(&parameter).cloned()).map(|b| match b {
                sim_system::ParameterBinding::Value { value, .. } => value.to_string(),
                sim_system::ParameterBinding::Parameter { parameter } => format!("${parameter}"),
            });
            builder.start_input(Purpose::Parameter { name, parameter }, current.unwrap_or_default());
        }
        BuildAction::Undo => builder.undo(),
        BuildAction::Redo => builder.redo(),
        BuildAction::Run => builder.start_run(scene),
        BuildAction::Pause => builder.pause_run(),
        BuildAction::ImportImage => builder.start_input(Purpose::ImportImage, String::new()),
        BuildAction::Opacity(id, delta) => {
            if let Some(mut r) = builder.reference(&id) {
                r.opacity = (r.opacity + delta).clamp(0.05, 1.0);
                let result = builder.apply("Reference opacity", vec![SystemCommand::SetReference { at: builder.level.clone(), id, reference: r }]);
                builder.report(result);
            }
        }
        BuildAction::Lock(id) => {
            if let Some(mut r) = builder.reference(&id) {
                r.locked = !r.locked;
                let result = builder.apply(if r.locked { "Lock reference" } else { "Unlock reference" }, vec![SystemCommand::SetReference { at: builder.level.clone(), id, reference: r }]);
                builder.report(result);
            }
        }
        BuildAction::Calibrate(id) => {
            builder.calibrating = Some((id, Vec::new()));
            builder.status = "Calibrate: click two points on the image a known distance apart.".into();
        }
        BuildAction::Width(id) => {
            let width = builder.reference(&id).map(|r| r.width.to_string()).unwrap_or_default();
            builder.start_input(Purpose::ReferenceWidth(id), width);
        }
        BuildAction::RemoveReference(id) => {
            let r = builder.apply("Remove reference", vec![SystemCommand::RemoveReference { at: builder.level.clone(), id }]);
            builder.report(r);
        }
        BuildAction::SaveToLibrary => {
            if let Some(name) = builder.only_selected() {
                if let Some(InstanceKind::Subsystem { definition }) = builder.spec(&name).map(|s| s.kind) {
                    let result = library::save(&builder.document, &definition, &builder.library_dir).map(|p| format!("Saved {definition} to {}", p.display())).map_err(|e| e.to_string());
                    match result {
                        Ok(m) => {
                            builder.status = m;
                            builder.refresh_palette();
                        }
                        Err(e) => builder.status = e,
                    }
                }
            }
        }
    }
}

fn builder_keys(keys: Res<ButtonInput<KeyCode>>, mut builder: ResMut<Builder>, scene: Res<SpatialScene>, mut orbit: Single<&mut Orbit>) {
    if builder.typing() {
        return;
    }
    let command = keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight) || keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if command && keys.just_pressed(KeyCode::KeyZ) {
        if shift { builder.redo() } else { builder.undo() }
        return;
    }
    let step = if shift { 0.001 } else { 0.005 };
    for (key, delta) in [
        (KeyCode::ArrowLeft, [-step, 0., 0.]),
        (KeyCode::ArrowRight, [step, 0., 0.]),
        (KeyCode::ArrowUp, [0., 0., -step]),
        (KeyCode::ArrowDown, [0., 0., step]),
        (KeyCode::PageUp, [0., step, 0.]),
        (KeyCode::PageDown, [0., -step, 0.]),
    ] {
        if keys.just_pressed(key) {
            builder.nudge(delta);
        }
    }
    let action = if keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Backspace) {
        Some(BuildAction::Delete)
    } else if keys.just_pressed(KeyCode::KeyG) {
        Some(BuildAction::Group)
    } else if keys.just_pressed(KeyCode::KeyU) {
        Some(BuildAction::Up)
    } else if keys.just_pressed(KeyCode::Enter) {
        builder.only_selected().filter(|n| matches!(builder.spec(n).map(|s| s.kind), Some(InstanceKind::Subsystem { .. }))).map(BuildAction::Open)
    } else if keys.just_pressed(KeyCode::Slash) {
        Some(BuildAction::Filter)
    } else if keys.just_pressed(KeyCode::KeyR) {
        Some(if builder.running() { BuildAction::Pause } else { BuildAction::Run })
    } else {
        None
    };
    if let Some(action) = action {
        dispatch(&mut builder, &scene, &mut orbit, action);
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
) {
    // Compile off the UI thread; apply the newest finished result.
    if builder.scene_dirty && builder.job.is_none() {
        builder.scene_dirty = false;
        builder.job = Some(Mutex::new(compile_job(builder.document.clone(), builder.registry.clone())));
    }
    let Some(job) = &builder.job else { return };
    let polled = job.lock().map(|j| j.try_recv()).unwrap_or(Err(mpsc::TryRecvError::Disconnected));
    let finished = match polled {
        Ok(r) => r,
        Err(mpsc::TryRecvError::Empty) => return,
        Err(mpsc::TryRecvError::Disconnected) => {
            builder.job = None;
            return;
        }
    };
    builder.job = None;
    if finished.revision != builder.document.revision {
        // An edit arrived while compiling; compile again before redrawing.
        builder.scene_dirty = true;
        return;
    }
    let compiled = match finished.result {
        Ok(c) => c,
        Err(e) => {
            builder.compile_error = Some(e.clone());
            builder.status = format!("Does not compile yet: {e}");
            builder.panel_dirty = true;
            return;
        }
    };
    let first = scene.description.components.is_empty() && scene.spatial.parts.is_empty();
    builder.findings = compiled.findings;
    builder.subsystems = compiled.subsystems;
    builder.compile_error = compiled.runtime_error;
    if builder.run.as_ref().is_some_and(|r| r.description_id != compiled.description.id) {
        builder.stop_run();
        scene.live.snapshot = None;
    }
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
        orbit.home = true;
    }
}

fn pick_reference(click: Trigger<Pointer<Click>>, quads: Query<&ReferenceQuad>, mut builder: ResMut<Builder>) {
    let Ok(quad) = quads.get(click.target()) else { return };
    let Some(position) = click.hit.position else { return };
    let frame = builder.subsystems.get(&builder.level).copied().unwrap_or(sim_system::flatten::WorldPlacement::IDENTITY);
    // Points are recorded in the level's frame, like the reference origin.
    let inverse = Transform::from_translation(Vec3::from_array(frame.position)).with_rotation(Quat::from_array(frame.rotation_xyzw)).compute_matrix().inverse();
    let local = inverse.transform_point3(position).to_array();
    let Some((id, points)) = builder.calibrating.as_mut() else {
        builder.status = format!("Reference {} (click Calibrate to scale it from two points)", quad.0);
        builder.panel_dirty = true;
        return;
    };
    if *id != quad.0 {
        return;
    }
    points.push(local);
    if points.len() == 2 {
        let (id, points) = builder.calibrating.take().unwrap();
        builder.start_input(Purpose::Distance { id, first: points[0], second: points[1] }, String::new());
        builder.status = "Type the real distance between the two points (m) and press Enter.".into();
    } else {
        builder.status = "Now click the second point.".into();
    }
    builder.panel_dirty = true;
}

/// Part clicks select instances at the current level.
pub(super) fn click_part(builder: &mut Builder, component: &str, shift: bool) {
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
    builder.alternatives = None;
    builder.scene_dirty = true;
    builder.panel_dirty = true;
}

fn sync_run(time: Res<Time>, mut builder: ResMut<Builder>, mut scene: ResMut<SpatialScene>) {
    if builder.run.is_some() && time.elapsed_secs_f64() - builder.live_refresh > 0.25 {
        builder.live_refresh = time.elapsed_secs_f64();
        builder.panel_dirty = true;
    }
    let Some(run) = &builder.run else { return };
    let Ok(shared) = run.shared.lock() else { return };
    if let Some(snapshot) = &shared.snapshot {
        if snapshot.source_description_id == scene.description.id {
            let changed = scene.live.snapshot.as_ref().is_none_or(|s| s.frame != snapshot.frame || s.status != snapshot.status || s.error != snapshot.error);
            if changed {
                scene.live.snapshot = Some(Arc::new(snapshot.clone()));
            }
        }
    }
}

mod ui;
pub use ui::{TOPBAR, STATUSBAR, LEFT_WIDTH, RIGHT_WIDTH};

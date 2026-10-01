//! The one native viewer app (docs/architecture/native-viewer.md §1–§2).
//!
//! - **One `App`**, built by [`run`] and nowhere else. Launch flags only
//!   choose the initial [`ViewerMode`] and its documents ([`Launch`]).
//! - **Modes are states.** [`ViewerMode`] (Inspect, Build, Lessons, Robot,
//!   Place, Cad, Phenomena) is the Bevy state; two computed
//!   states follow it: [`ModeScope`] (what a mode's entities and resources
//!   live for: Build and Lessons share one scope, the lesson screen being
//!   drawn over the builder's scene) and [`SpatialScreen`] (the spatial
//!   assembly view is drawn: Inspect, Build and Lessons).
//! - **One pipeline of sets** in `Update`: [`ViewerSet`] Input → Actions →
//!   JobResults → SimSync → Present, configured once here.
//! - **The core** ([`CorePlugin`]): window, winit, clear colour and light per
//!   mode, fonts, mesh picking, occlusion, the UI kit (`ui_kit::UiKitPlugin`), the REST wake,
//!   the mode switcher and the one REST server (`rest::Rest`, bound once by
//!   `rest::bind`).
//! - **Actions** ([`actions`], native-viewer.md §3): every intent is a typed
//!   action (one enum per mode or feature), written as a Bevy Message by
//!   buttons, keys, `system_ui` and REST (Input) and applied by one system
//!   per action type (Actions). REST capabilities are generated from the
//!   action registry; one REST poll ([`actions::serve`]) goes through the
//!   one dispatch ([`route`]).
//! - **Switching** lives in [`switch`]: one request type, one validating
//!   handler, teardown on exit. Its entry points: [`switcher`] (the buttons),
//!   `system_ui` `mode:*`, `viewer_mode`, the builder's Lessons button and
//!   the lesson screen's toggles.
pub mod actions;
pub mod route;
pub mod switch;
pub mod switcher;
#[cfg(test)]
mod tests;

use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowResizeConstraints};
use bevy::winit::{UpdateMode, WinitSettings};
use serde::{Deserialize, Serialize};

/// The window's mode. One is active at a time; the user switches between
/// them in the running window (`switch`).
#[derive(States, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewerMode {
    /// A source-bound assembly (`--description`/`--spatial`, or the example).
    Inspect,
    /// The System Builder on a `*.system.json` file.
    Build,
    /// Lessons, drawn over the builder's scene.
    Lessons,
    /// A CAD-exported `*.simrobot.json` or a robot preset.
    Robot,
    /// A scanned place (`sim-place build` directory).
    Place,
    /// A RoboCAD document (`*.rcad`), shown and edited through RoboCAD's
    /// REST service (native-viewer.md §9 phase 1; `crate::cad`).
    Cad,
    /// The phenomena gallery: the built-in exhibits of
    /// `sim_phenomena::exhibits`, run live (`crate::phenomena`; it was
    /// sim-app's default scene).
    Phenomena,
}

impl ViewerMode {
    pub const ALL: [ViewerMode; 7] = [ViewerMode::Inspect, ViewerMode::Build, ViewerMode::Lessons, ViewerMode::Robot, ViewerMode::Place, ViewerMode::Cad, ViewerMode::Phenomena];
    /// The REST and `system_ui` name (`mode:<name>`).
    pub fn name(self) -> &'static str {
        match self {
            ViewerMode::Inspect => "inspect",
            ViewerMode::Build => "build",
            ViewerMode::Lessons => "lessons",
            ViewerMode::Robot => "robot",
            ViewerMode::Place => "place",
            ViewerMode::Cad => "cad",
            ViewerMode::Phenomena => "phenomena",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            ViewerMode::Inspect => "Inspect",
            ViewerMode::Build => "Build",
            ViewerMode::Lessons => "Lessons",
            ViewerMode::Robot => "Robot",
            ViewerMode::Place => "Place",
            ViewerMode::Cad => "CAD",
            ViewerMode::Phenomena => "Phenomena",
        }
    }
    pub fn parse(name: &str) -> Result<Self, String> {
        Self::ALL.into_iter().find(|m| m.name() == name).ok_or_else(|| format!("unknown mode `{name}`; expected one of {}", Self::ALL.map(|m| m.name()).join(", ")))
    }
    /// Build and Lessons share the builder (the lesson screen is drawn over it).
    pub fn builder_family(self) -> bool {
        matches!(self, ViewerMode::Build | ViewerMode::Lessons)
    }
}

/// What a mode's entities and resources live for. Build and Lessons are one
/// scope: switching between them keeps the builder, its scene and the lesson
/// (the lesson screen is shown over the builder, as the lessons launch always
/// did). Mode entities carry `DespawnOnExit<ModeScope>` (see
/// [`scope_new_entities`]); mode resources are removed by the scope's OnExit
/// (`switch`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ModeScope {
    Inspect,
    Builder,
    Robot,
    Place,
    Cad,
    Phenomena,
}
impl ComputedStates for ModeScope {
    type SourceStates = ViewerMode;
    // Build ↔ Lessons computes Builder → Builder: no exit, no enter, nothing despawned.
    const ALLOW_SAME_STATE_TRANSITIONS: bool = false;
    fn compute(mode: ViewerMode) -> Option<Self> {
        Some(match mode {
            ViewerMode::Inspect => ModeScope::Inspect,
            ViewerMode::Build | ViewerMode::Lessons => ModeScope::Builder,
            ViewerMode::Robot => ModeScope::Robot,
            ViewerMode::Place => ModeScope::Place,
            ViewerMode::Cad => ModeScope::Cad,
            ViewerMode::Phenomena => ModeScope::Phenomena,
        })
    }
}

/// The spatial assembly view is drawn (Inspect, Build, Lessons): the run
/// condition of `SpatialViewerPlugin`'s systems.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SpatialScreen;
impl ComputedStates for SpatialScreen {
    type SourceStates = ViewerMode;
    const ALLOW_SAME_STATE_TRANSITIONS: bool = false;
    fn compute(mode: ViewerMode) -> Option<Self> {
        matches!(mode, ViewerMode::Inspect | ViewerMode::Build | ViewerMode::Lessons).then_some(SpatialScreen)
    }
}

/// The shared pipeline (native-viewer.md §2), in this order every frame.
/// A feature adds its systems to these sets.
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ViewerSet {
    /// Buttons, keys and the one REST poll write actions (`actions::Act`);
    /// nothing here changes a mode's state.
    Input,
    /// One handler per action type validates and applies this frame's
    /// actions (and those still in flight), answering REST replies.
    Actions,
    /// Finished background work is applied (a mode switch's document load).
    JobResults,
    /// Each mode's frame: jobs, continuous gestures (orbit, fly, drags, text
    /// entry) and the scene synced to the model.
    SimSync,
    /// Drawing-only work: panels, overlays, gizmos, the switcher's highlight,
    /// REST snapshots.
    Present,
}

/// Core entities that live across every mode (the mode switcher).
#[derive(Component)]
pub struct Persistent;

/// What `main` resolved from the command line: the initial mode, its
/// documents and the launch facts later switches need.
pub struct Launch {
    pub mode: ViewerMode,
    /// The one REST server (`rest::bind`).
    pub api: sim_api::Server,
    pub documents: switch::Documents,
    pub models: crate::models::ModelLibrary,
    pub scene: Option<crate::SpatialScene>,
    pub link: Option<crate::SelectionLink>,
    pub builder: Option<crate::builder::Builder>,
    pub learn: Option<crate::lesson::Learn>,
    pub robot: Option<crate::robot::RobotView>,
    pub place: Option<crate::place_view::PlaceView>,
    /// CAD mode's document (the RoboCAD service to attach to or start).
    pub cad: Option<crate::cad::CadDocument>,
}

/// The one app: every mode's plugin under its state, the core, and the
/// initial mode's documents. Returns when the window closes.
pub fn run(launch: Launch) {
    let Launch { mode, api, documents, models, scene, link, builder, learn, robot, place, cad } = launch;
    let compact = scene.as_ref().is_some_and(|s| s.compact);
    let mut app = App::new();
    app.add_plugins(CorePlugin { initial: mode, compact })
        .insert_resource(crate::rest::Rest(api, None))
        .insert_resource(models)
        .insert_resource(documents);
    if let Some(scene) = scene {
        app.insert_resource(scene);
    }
    if let Some(link) = link {
        app.insert_resource(link);
    }
    if let Some(builder) = builder {
        app.insert_resource(builder);
    }
    if let Some(learn) = learn {
        app.insert_resource(learn);
    }
    if let Some(robot) = robot {
        app.insert_resource(robot);
    }
    if let Some(place) = place {
        app.insert_resource(place);
    }
    if let Some(cad) = cad {
        app.insert_resource(cad);
    }
    app.add_plugins((
        ModesPlugin { initial: mode },
        // The one orbit/fly camera every mode's 3D view uses.
        crate::camera::CameraPlugin,
        crate::SpatialViewerPlugin,
        crate::builder::BuilderPlugin,
        crate::lesson::LearnPlugin,
        crate::robot::RobotPlugin,
        crate::place_view::PlacePlugin,
        crate::cad::CadPlugin,
        crate::phenomena::PhenomenaPlugin,
    ))
    .run();
}

/// States, the pipeline sets and mode switching: everything about modes that
/// needs no window (so the state-transition test runs it on MinimalPlugins).
pub struct ModesPlugin {
    pub initial: ViewerMode,
}
impl Plugin for ModesPlugin {
    fn build(&self, app: &mut App) {
        app.insert_state(self.initial)
            .add_computed_state::<ModeScope>()
            .add_computed_state::<SpatialScreen>()
            .configure_sets(Update, (ViewerSet::Input, ViewerSet::Actions, ViewerSet::JobResults, ViewerSet::SimSync, ViewerSet::Present).chain())
            .add_systems(Last, scope_new_entities);
        switch::build(app);
    }
}

/// Every root entity a mode spawns (a `Node` or `Transform` without a
/// parent, not `Persistent`) is scoped to the active [`ModeScope`], so it is
/// despawned (with its children) when the scope exits. One sweep at the end
/// of the frame instead of a marker at each of the modes' spawn sites; the
/// state cannot change between a spawn in `Update` and this `Last` sweep
/// (transitions run after `PreUpdate`).
pub(crate) fn scope_new_entities(mut commands: Commands, scope: Option<Res<State<ModeScope>>>, roots: Query<Entity, (Or<(With<Node>, With<Transform>)>, Without<ChildOf>, Without<DespawnOnExit<ModeScope>>, Without<Persistent>)>) {
    let Some(scope) = scope else { return };
    for entity in &roots {
        commands.entity(entity).try_insert(DespawnOnExit(*scope.get()));
    }
}

const CLEAR: Color = Color::srgb(0.10, 0.125, 0.155);
const AMBIENT: Color = Color::srgb(0.85, 0.90, 1.0);

/// A mode's window: what its own launch used before the modes were one app.
struct Look {
    title: &'static str,
    clear: Color,
    ambient: (Color, f32),
    /// Initial size when the window opens in this mode.
    size: (u32, u32),
    min: Option<(f32, f32)>,
    winit: WinitSettings,
}

/// Reactive at 60 Hz focused, low power unfocused: every mode but Place.
/// Pipelined rendering needs a follow-up update after input; long
/// desktop-app sleeps leave the inspector one frame behind a click.
fn reactive() -> WinitSettings {
    WinitSettings { focused_mode: UpdateMode::reactive(std::time::Duration::from_secs_f64(1.0 / 60.0)), unfocused_mode: UpdateMode::reactive_low_power(std::time::Duration::from_millis(40)) }
}

fn look(mode: ViewerMode, compact: bool) -> Look {
    match mode {
        ViewerMode::Inspect => Look { title: "Systems — Physical assembly", clear: CLEAR, ambient: (AMBIENT, 420.0), size: if compact { (880, 850) } else { (1440, 900) }, min: Some((780.0, 720.0)), winit: reactive() },
        ViewerMode::Build => Look { title: "Systems — Physical assembly (build)", clear: CLEAR, ambient: (AMBIENT, 420.0), size: (1500, 940), min: Some((980.0, 720.0)), winit: reactive() },
        ViewerMode::Lessons => Look { title: "Systems — Lessons", clear: CLEAR, ambient: (AMBIENT, 420.0), size: (1560, 980), min: Some((1100.0, 720.0)), winit: reactive() },
        ViewerMode::Robot => Look { title: "Systems — Robot (file read-only)", clear: CLEAR, ambient: (AMBIENT, 420.0), size: (1500, 940), min: Some((980.0, 720.0)), winit: reactive() },
        // A walkthrough is flown continuously while focused (W/A/S/D), so it
        // updates every frame; the setting is scoped to this mode.
        ViewerMode::Place => Look {
            title: "Place walkthrough",
            clear: Color::srgb(0.07, 0.08, 0.1),
            ambient: (Color::WHITE, 900.0),
            size: (1440, 900),
            min: None,
            winit: WinitSettings { focused_mode: UpdateMode::Continuous, unfocused_mode: UpdateMode::reactive_low_power(std::time::Duration::from_millis(100)) },
        },
        ViewerMode::Cad => Look { title: "Systems — CAD (RoboCAD document)", clear: CLEAR, ambient: (AMBIENT, 420.0), size: (1500, 940), min: Some((980.0, 720.0)), winit: reactive() },
        // The exhibit moves on its own run thread, so the window redraws
        // every frame while focused to show its latest frame; the setting is
        // scoped to this mode (as Place's). The exhibits paint with
        // `sim_phenomena::exhibit::paint` (dark ink for a light board), so
        // the scene keeps sim-app's light background and light; the panels
        // are kit docks with their own surfaces.
        ViewerMode::Phenomena => Look {
            title: "Systems — Phenomena",
            clear: Color::srgb(0.94, 0.95, 0.965),
            ambient: (Color::srgb(0.85, 0.88, 0.95), 650.0),
            size: (1500, 940),
            min: Some((980.0, 720.0)),
            winit: WinitSettings { focused_mode: UpdateMode::Continuous, unfocused_mode: UpdateMode::reactive_low_power(std::time::Duration::from_millis(100)) },
        },
    }
}

fn constraints(min: Option<(f32, f32)>) -> WindowResizeConstraints {
    match min {
        Some((min_width, min_height)) => WindowResizeConstraints { min_width, min_height, ..default() },
        None => WindowResizeConstraints::default(),
    }
}

/// The window, winit, colours, fonts, picking, occlusion, the UI kit's
/// systems (repaint, labels, slider value, scroll clamp), the REST wake and
/// the mode switcher: shared by every mode.
pub struct CorePlugin {
    pub initial: ViewerMode,
    /// Inspect `--compact` (or `--schematic`): the smaller first window.
    pub compact: bool,
}
impl Plugin for CorePlugin {
    fn build(&self, app: &mut App) {
        let first = look(self.initial, self.compact);
        app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { title: first.title.into(), resolution: first.size.into(), resize_constraints: constraints(first.min), ..default() }),
            ..default()
        }))
        .insert_resource(ClearColor(first.clear))
        .insert_resource(GlobalAmbientLight { color: first.ambient.0, brightness: first.ambient.1, affects_lightmapped_meshes: true })
        .insert_resource(first.winit)
        .add_plugins(MeshPickingPlugin)
        .insert_resource(bevy::picking::mesh_picking::MeshPickingSettings { require_markers: true, ..default() })
        .init_resource::<crate::rest::Occlusion>()
        .add_systems(PreUpdate, crate::rest::track_occlusion)
        .add_plugins(crate::ui_kit::UiKitPlugin)
        .add_systems(Startup, (crate::rest::wake_on_request, switcher::spawn_switcher))
        .add_systems(Update, switcher::switcher_clicks.in_set(ViewerSet::Input))
        .add_systems(Update, (switcher::update_switcher, switcher::publish).in_set(ViewerSet::Present));
        for mode in ViewerMode::ALL {
            app.add_systems(OnEnter(mode), apply_look);
        }
        // The first mode's OnEnter runs before Startup, so the fonts it
        // spawns text with are loaded now, while the app is built.
        let fonts = crate::ui_kit::UiFonts::load(app.world_mut());
        app.insert_resource(fonts);
    }
}

/// OnEnter of every mode: its title, clear colour, light, minimum size and
/// winit setting (the window keeps the size the user gave it).
fn apply_look(mode: Res<State<ViewerMode>>, mut commands: Commands, window: Option<Single<&mut Window, With<PrimaryWindow>>>) {
    let look = look(*mode.get(), false);
    commands.insert_resource(ClearColor(look.clear));
    commands.insert_resource(GlobalAmbientLight { color: look.ambient.0, brightness: look.ambient.1, affects_lightmapped_meshes: true });
    commands.insert_resource(look.winit);
    if let Some(mut window) = window {
        if window.title != look.title {
            window.title = look.title.into();
        }
        window.resize_constraints = constraints(look.min);
    }
}

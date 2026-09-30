//! Reusable Bevy presentation of shared inspection contracts. Inspection mode
//! never advances physics. Build mode (`--system`) edits a system file through
//! the shared `sim-system` commands and runs it on the shared runtime in a
//! background thread; it contains no physics of its own.
mod animation;
pub(crate) mod annotate;
pub(crate) mod chart;
pub mod builder;
pub mod jobs;
pub mod launch;
pub mod lesson;
mod linked;
pub mod models;
pub mod place_view;
pub mod robot;
pub mod robot_gait;
pub mod robot_graphs;
pub mod robot_motion;
pub mod robot_playback;
pub mod robot_preset;
pub mod robot_recording;
pub mod robot_run;
pub mod robot_source;
pub mod robot_stress;
pub mod markdown;
pub(crate) mod physics_view;
pub(crate) mod view;
mod notes;
pub mod rest;
pub mod workspace;
use bevy::{
    core_pipeline::tonemapping::Tonemapping,
    input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    picking::mesh_picking::{MeshPickingCamera, MeshPickingSettings},
    prelude::*,
    camera::Viewport,
    winit::{UpdateMode, WinitSettings},
};
pub use builder::{Builder, BuilderPlugin};
pub use linked::SelectionLink;
use sim_inspect::selection::{SelectionDetails, SelectionTarget};
use sim_inspect::{
    InspectionError, ObservationLocation, SystemDescription,
    spatial::{SpatialCommand, SpatialDescription, SpatialShape, SpatialViewState},
};
use std::collections::BTreeMap;

const LEFT: f32 = 230.0;
const RIGHT: f32 = 320.0;
const TOP: f32 = 116.0;
const BOTTOM: f32 = 66.0;
const INK: Color = Color::srgb(0.87, 0.90, 0.94);
const MUTED: Color = Color::srgb(0.56, 0.64, 0.72);
const PANEL: Color = Color::srgb(0.075, 0.093, 0.12);
const ACCENT: Color = Color::srgb(0.30, 0.83, 0.75);

#[derive(Resource)]
pub struct SpatialScene {
    pub description: SystemDescription,
    pub spatial: SpatialDescription,
    pub state: SpatialViewState,
    pub selection: SelectionTarget,
    details: SelectionDetails,
    pub animation: Option<sim_inspect::animation::AnimationDescription>,
    live: animation::LivePresentation,
    pub compact: bool,
    parts_visible: bool,
    annotations: Option<sim_inspect::annotations::native::Store>,
    note_navigation: u64,
    note_hover: SelectionTarget,
    note_pointer_hover: SelectionTarget,
    note_error: Option<String>,
    /// Components drawn translucent (outside the level being built).
    pub ghost: std::collections::BTreeSet<String>,
    /// Build mode replaces the parts list with the builder panel.
    pub builder_mode: bool,
    /// Height of the build-mode graph dock above the status bar (0 = hidden).
    pub builder_dock: f32,
    /// Width of the build-mode schematic pane beside the inspector.
    pub builder_side: f32,
    /// Learn mode: the 3D view is drawn inside a lesson's scene card.
    pub learn_view: Option<LearnView>,
    /// Display directives from lesson scripts and narration (spotlight,
    /// pins, inset, X-ray, explode), or set by the learner.
    pub directives: sim_script::presentation::ViewState,
    /// How far the exploded view has opened, 0–1 (eased).
    pub explode_t: f32,
    /// Sliding parts are drawn this many times further from their start
    /// (a lesson scene's `magnify`; display only).
    pub motion_scale: f32,
    /// Reader preference: no glides, orbiting, explode easing or motion blur.
    pub reduced_motion: bool,
    /// Parts a lesson script is pointing at: everything else dims a little.
    pub soft_focus: Vec<String>,
    /// A second run shown beside (split) or over (ghost) this one.
    pub companion: Option<view::CompanionView>,
}

/// Where the 3D view is embedded in a lesson page, in physical pixels: the
/// card's whole viewport and the part of it currently visible (scrolled).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct LearnView {
    pub full: Rect,
    pub visible: Rect,
}

/// The catalog colour of one piece of a display model.
#[derive(Component)]
pub struct ModelColor(pub Color);

/// Parts, net hubs and reference images; respawned when the system changes.
#[derive(Component)]
pub struct SceneContent;
/// Root nodes of the inspection panels; rebuilt when the description changes.
#[derive(Component)]
pub struct UiRoot;

impl SpatialScene {
    /// Build mode: the system may be empty or incomplete, so an empty
    /// presentation is allowed here; everything else is validated as usual.
    pub fn for_builder(description: SystemDescription, spatial: SpatialDescription) -> Result<Self, InspectionError> {
        if !spatial.parts.is_empty() {
            spatial.validate(&description)?;
        }
        let mut scene = Self::unchecked(description, spatial);
        scene.builder_mode = true;
        Ok(scene)
    }
    /// Swap in a recompiled system, keeping display preferences.
    pub fn replace(&mut self, description: SystemDescription, spatial: SpatialDescription, animation: Option<sim_inspect::animation::AnimationDescription>) {
        self.description = description;
        self.spatial = spatial;
        self.animation = animation;
        self.state.hidden.clear();
        self.state.selected = None;
        self.selection = SelectionTarget::None;
        self.details = SelectionDetails::default();
    }
    fn unchecked(description: SystemDescription, spatial: SpatialDescription) -> Self {
        Self {
            description,
            spatial,
            state: SpatialViewState { overlays: sim_inspect::spatial::Overlay::defaults(), ..Default::default() },
            selection: SelectionTarget::None,
            details: SelectionDetails::default(),
            animation: None,
            live: animation::LivePresentation::default(),
            compact: false,
            parts_visible: true,
            annotations: None,
            note_navigation: 0,
            note_hover: SelectionTarget::None,
            note_pointer_hover: SelectionTarget::None,
            note_error: None,
            ghost: Default::default(),
            builder_mode: false,
            builder_dock: 0.,
            builder_side: 0.,
            learn_view: None,
            directives: Default::default(),
            explode_t: 0.,
            motion_scale: 1.,
            reduced_motion: false,
            soft_focus: Vec::new(),
            companion: None,
        }
    }
    pub fn new(
        description: SystemDescription,
        spatial: SpatialDescription,
    ) -> Result<Self, InspectionError> {
        spatial.validate(&description)?;
        Ok(Self::unchecked(description, spatial))
    }
    pub fn apply(&mut self, command: SpatialCommand) -> Result<(), InspectionError> {
        if matches!(command, SpatialCommand::HideSelected) {
            self.state.hidden.extend(
                self.spatial
                    .parts
                    .iter()
                    .filter(|p| self.details.components.contains(&p.component))
                    .map(|p| p.component.clone()),
            );
            return Ok(());
        }
        let select = match &command {
            SpatialCommand::Select { component } => {
                Some(SelectionTarget::component(component.clone()))
            }
            SpatialCommand::ClearSelection => Some(SelectionTarget::None),
            _ => None,
        };
        self.state.apply(&self.spatial, command)?;
        if let Some(target) = select {
            self.set_selection(target)?;
        }
        Ok(())
    }
    pub fn set_selection(&mut self, target: SelectionTarget) -> Result<(), InspectionError> {
        let details = target.resolve(&self.description)?;
        self.state.selected = match &target {
            SelectionTarget::Components { ids } if ids.len() == 1 => ids.first().cloned(),
            _ => None,
        };
        for id in &details.components {
            self.state.hidden.remove(id);
        }
        if matches!(
            target,
            SelectionTarget::Ports { .. } | SelectionTarget::Nets { .. }
        ) {
            self.state.connections = true;
        }
        self.details = details;
        self.selection = target;
        Ok(())
    }
    fn left(&self) -> f32 {
        if self.builder_mode {
            return builder::LEFT_WIDTH;
        }
        if self.parts_visible {
            if self.compact { 180. } else { LEFT }
        } else {
            0.
        }
    }
    fn right(&self) -> f32 {
        if self.builder_mode {
            return builder::RIGHT_WIDTH + self.builder_side;
        }
        if self.compact { 260. } else { RIGHT }
    }
    fn top(&self) -> f32 {
        if self.builder_mode { builder::TOPBAR } else { TOP }
    }
    fn bottom(&self) -> f32 {
        if self.builder_mode { builder::STATUSBAR + self.builder_dock } else { BOTTOM }
    }
    fn select(&mut self, component: String) {
        if let Err(e) = self.apply(SpatialCommand::Select { component }) {
            error!("{e}");
        }
    }
    fn representatives(&self) -> Vec<(String, String)> {
        let mut seen = std::collections::BTreeSet::new();
        self.spatial
            .parts
            .iter()
            .filter(|p| seen.insert(p.component.clone()))
            .map(|p| (p.component.clone(), p.label.clone()))
            .collect()
    }
    fn positions(&self) -> BTreeMap<String, Vec3> {
        let mut result = BTreeMap::new();
        for p in &self.spatial.parts {
            result.entry(p.component.clone()).or_insert_with(|| {
                Vec3::from_array(p.position)
                    + if self.state.exploded {
                        Vec3::from_array(p.exploded_offset)
                    } else {
                        Vec3::ZERO
                    }
            });
        }
        result
    }
    fn bounds(&self) -> (Vec3, f32) {
        self.bounds_of(None)
    }
    /// Centre and radius of the parts of one instance path (and everything
    /// inside it), or of every part.
    pub(crate) fn bounds_of(&self, prefix: Option<&str>) -> (Vec3, f32) {
        let inside = |c: &str| prefix.is_none_or(|p| p.is_empty() || c == p || c.starts_with(&format!("{p}/")));
        if !self.spatial.parts.iter().any(|p| inside(&p.component)) {
            if prefix.is_some() {
                return self.bounds_of(None);
            }
            return (Vec3::ZERO, 0.1);
        }
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for p in self.spatial.parts.iter().filter(|p| inside(&p.component)) {
            let center = Vec3::from_array(p.position)
                + if self.state.exploded {
                    Vec3::from_array(p.exploded_offset)
                } else {
                    Vec3::ZERO
                };
            let radius = match p.shape {
                SpatialShape::Box { size } => Vec3::from_array(size).length() * 0.5,
                SpatialShape::Cylinder { radius, length } => radius.hypot(length * 0.5),
                SpatialShape::Sphere { radius } => radius,
            };
            lo = lo.min(center - radius);
            hi = hi.max(center + radius);
        }
        // Pieces drawn inside a part (a link's arm, a gear train) can reach
        // well beyond its housing: frame them too.
        if let Some(a) = &self.animation {
            for b in &a.internals {
                let Some(p) = self.spatial.parts.iter().find(|p| p.id == b.part && inside(&p.component)) else { continue };
                let offset = if self.state.exploded { Vec3::from_array(p.exploded_offset) } else { Vec3::ZERO };
                let reach = match b.element {
                    sim_inspect::animation::InternalElement::Arm => b.length + b.radius,
                    _ => b.radius.hypot(b.length * 0.5),
                };
                let center = Vec3::from_array(b.center) + offset;
                lo = lo.min(center - reach);
                hi = hi.max(center + reach);
            }
        }
        ((lo + hi) * 0.5, (hi - lo).length() * 0.5)
    }
}

pub struct SpatialViewerPlugin;
impl Plugin for SpatialViewerPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MeshPickingPlugin)
            .insert_resource(MeshPickingSettings {
                require_markers: true,
                ..default()
            })
            .init_resource::<physics_view::Labels>()
            .init_resource::<view::PartHover>()
            .add_systems(Startup, (setup_scene, setup_ui, rest::wake_on_request))
            .add_systems(PostUpdate, view::clamp_scroll_positions.after(bevy::ui::UiSystems::Layout))
            .init_resource::<rest::Occlusion>()
            .add_systems(PreUpdate, rest::track_occlusion)
            .add_systems(
                Update,
                (
                    notes::update,
                    rest::poll,
                    buttons,
                    keyboard,
                    linked::sync_link,
                    animation::sync_live,
                    update_layout,
                    camera_viewport,
                    orbit,
                    scroll_inspector,
                    update_parts,
                    linked::update_nets,
                    update_ui,
                    draw_guides,
                    notes::guides,
                    animation::draw_markers,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (view::animate, physics_view::overlay_clicks, physics_view::update_internals, physics_view::draw, view::draw_pins, view::draw_ghost, view::split, view::inset, physics_view::labels, physics_view::overlay_bar, view::caption_fonts)
                    .chain()
                    .after(animation::draw_markers),
            );
    }
}

/// Build mode: the physical assembly plus the system builder panel.
pub fn run_builder(scene: SpatialScene, builder: builder::Builder, api: sim_api::Server, models: models::ModelLibrary) {
    let mut app = App::new();
    app.insert_resource(models).insert_resource(rest::Rest(api, None))
        .insert_resource(builder)
        .insert_resource(scene)
        .insert_resource(ClearColor(Color::srgb(0.10, 0.125, 0.155)))
        .insert_resource(GlobalAmbientLight { color: Color::srgb(0.85, 0.90, 1.0), brightness: 420.0, affects_lightmapped_meshes: true })
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::reactive(std::time::Duration::from_secs_f64(1.0 / 60.0)),
            unfocused_mode: UpdateMode::reactive_low_power(std::time::Duration::from_millis(40)),
        })
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Systems — Physical assembly (build)".into(),
                resolution: (1500_u32, 940_u32).into(),
                resize_constraints: bevy::window::WindowResizeConstraints { min_width: 980.0, min_height: 720.0, ..default() },
                ..default()
            }),
            ..default()
        }))
        .add_plugins(SpatialViewerPlugin)
        .add_plugins(builder::BuilderPlugin)
        .run();
}

/// Lesson mode: lessons around the builder's scene (Learn screen first).
pub fn run_lessons(scene: SpatialScene, builder: builder::Builder, learn: lesson::Learn, api: sim_api::Server, models: models::ModelLibrary) {
    let mut app = App::new();
    app.insert_resource(models).insert_resource(rest::Rest(api, None))
        .insert_resource(builder)
        .insert_resource(scene)
        .insert_resource(learn)
        .insert_resource(ClearColor(Color::srgb(0.10, 0.125, 0.155)))
        .insert_resource(GlobalAmbientLight { color: Color::srgb(0.85, 0.90, 1.0), brightness: 420.0, affects_lightmapped_meshes: true })
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::reactive(std::time::Duration::from_secs_f64(1.0 / 60.0)),
            unfocused_mode: UpdateMode::reactive_low_power(std::time::Duration::from_millis(40)),
        })
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Systems — Lessons".into(),
                resolution: (1560_u32, 980_u32).into(),
                resize_constraints: bevy::window::WindowResizeConstraints { min_width: 1100.0, min_height: 720.0, ..default() },
                ..default()
            }),
            ..default()
        }))
        .add_plugins(SpatialViewerPlugin)
        .add_plugins(builder::BuilderPlugin)
        .add_plugins(lesson::LearnPlugin)
        .run();
}

pub fn run(scene: SpatialScene, link: Option<SelectionLink>) {
    run_with_api(scene, link, None, None);
}
pub fn run_with_api(
    mut scene: SpatialScene,
    link: Option<SelectionLink>,
    api: Option<sim_api::Server>,
    models: Option<models::ModelLibrary>,
) {
    let compact = scene.compact;
    if compact {
        scene.parts_visible = false;
    }
    let mut app = App::new();
    if let Some(api) = api {
        app.insert_resource(rest::Rest(api, None));
    }
    if let Some(models) = models {
        app.insert_resource(models);
    }
    if let Some(link) = link {
        app.insert_resource(link);
    }
    app.insert_resource(scene)
        .insert_resource(ClearColor(Color::srgb(0.10, 0.125, 0.155)))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.85, 0.90, 1.0),
            brightness: 420.0,
            affects_lightmapped_meshes: true,
        })
        .insert_resource(WinitSettings {
            // Pipelined rendering needs a follow-up update after input. Long
            // desktop-app sleeps leave the inspector one frame behind a click.
            focused_mode: UpdateMode::reactive(std::time::Duration::from_secs_f64(1.0 / 60.0)),
            unfocused_mode: UpdateMode::reactive_low_power(std::time::Duration::from_millis(40)),
        })
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Systems — Physical assembly".into(),
                resolution: if compact {
                    (880_u32, 850_u32).into()
                } else {
                    (1440_u32, 900_u32).into()
                },
                resize_constraints: bevy::window::WindowResizeConstraints {
                    min_width: 780.0,
                    min_height: 720.0,
                    ..default()
                },
                ..default()
            }),
            ..default()
        }))
        .add_plugins(SpatialViewerPlugin)
        .run();
}

#[derive(Component)]
struct Part {
    index: usize,
}
#[derive(Component, Default)]
struct Orbit {
    focus: Vec3,
    radius: f32,
    yaw: f32,
    pitch: f32,
    home: bool,
    /// An eased camera move in progress (see `view.rs`).
    glide: Option<view::Glide>,
    /// Slow circling, rad/s; any learner input stops it.
    spin: f32,
}
#[derive(Component, Clone)]
enum Action {
    Select(String),
    Net(String),
    Clear,
    Parts,
    Explode,
    Connections,
    Home,
    Hide,
    ShowAll,
}
#[derive(Component)]
struct PartsPanel;
#[derive(Component)]
struct Inspector;
#[derive(Component)]
struct InspectorScroll;
#[derive(Component)]
struct Status;
#[derive(Component)]
struct ActionLabel;

fn setup_scene(
    mut commands: Commands,
    scene: Res<SpatialScene>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut models: Option<ResMut<models::ModelLibrary>>,
) {
    let (focus, radius) = scene.bounds();
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            near: 0.001,
            ..default()
        }),
        MeshPickingCamera,
        Tonemapping::None,
        Transform::from_xyz(0.5, 0.6, 0.8).looking_at(focus, Vec3::Y),
        Orbit {
            focus,
            radius: radius * 3.5,
            yaw: 0.35,
            pitch: 0.6,
            home: true,
            ..Default::default()
        },
    ));
    // Split view: a second camera on the companion run's copy.
    commands.spawn((
        Camera3d::default(),
        Camera { order: 1, is_active: false, ..default() },
        Projection::Perspective(PerspectiveProjection { near: 0.001, ..default() }),
        Tonemapping::None,
        Transform::default(),
        view::SplitCamera,
    ));
    for right in [false, true] {
        commands.spawn((
            view::SplitLabel(right),
            Node { border_radius: BorderRadius::all(Val::Px(4.)), position_type: PositionType::Absolute, padding: UiRect::axes(Val::Px(8.), Val::Px(3.)), ..default() },
            BackgroundColor(Color::srgba(0.04, 0.06, 0.08, 0.8)),
            Visibility::Hidden,
            GlobalZIndex(23),
            Pickable::IGNORE,
            children![(Text::new(""), TextFont { font_size: FontSize::Px(12.), ..default() }, TextColor(if right { Color::srgb(0.72, 0.58, 1.0) } else { ACCENT }), view::ViewCaption, Pickable::IGNORE)],
        ));
    }
    // Picture-in-picture close-up: drawn after the main view, before the UI.
    commands.spawn((
        Camera3d::default(),
        Camera { order: 2, is_active: false, ..default() },
        Projection::Perspective(PerspectiveProjection { near: 0.0005, ..default() }),
        Tonemapping::None,
        Transform::default(),
        view::InsetCamera,
    ));
    commands.spawn((
        view::InsetFrame,
        Node { border_radius: BorderRadius::all(Val::Px(4.)), position_type: PositionType::Absolute, border: UiRect::all(Val::Px(2.)), justify_content: JustifyContent::FlexStart, align_items: AlignItems::FlexStart, ..default() },
        BorderColor::all(ACCENT),
        Visibility::Hidden,
        GlobalZIndex(22),
        Pickable::IGNORE,
        children![(Text::new(""), TextFont { font_size: FontSize::Px(11.), ..default() }, TextColor(ACCENT), BackgroundColor(Color::srgba(0.04, 0.06, 0.08, 0.8)), Node { padding: UiRect::axes(Val::Px(6.), Val::Px(2.)), ..default() }, view::ViewCaption, Pickable::IGNORE)],
    ));
    commands.spawn((
        Camera2d,
        Camera {
            order: 3,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        IsDefaultUiCamera,
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 5500.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.95, -0.7, 0.0)),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 2000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.4, 2.4, 0.0)),
    ));
    linked::spawn_nets(&mut commands, &scene, &mut meshes, &mut materials);
    spawn_parts(&mut commands, &scene, &mut meshes, &mut materials, models.as_deref_mut());
}

pub(crate) fn spawn_parts(
    commands: &mut Commands,
    scene: &SpatialScene,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    mut models: Option<&mut models::ModelLibrary>,
) {
    for (index, part) in scene.spatial.parts.iter().enumerate() {
        let transform = Transform::from_translation(Vec3::from_array(part.position)).with_rotation(Quat::from_array(part.rotation_xyzw));
        let component_type = scene.description.components.get(&part.component).map(|c| c.component_type.as_str()).unwrap_or("");
        let pieces = models.as_deref_mut().and_then(|library| {
            let id = library.model_for(part.model.as_deref(), component_type)?;
            library.pieces(&id, meshes)
        });
        if let Some(pieces) = pieces {
            for piece in pieces {
                let mut entity = commands.spawn((
                    Mesh3d(piece.mesh.clone()),
                    MeshMaterial3d(materials.add(StandardMaterial { base_color: piece.color, metallic: piece.metallic, perceptual_roughness: if piece.metallic > 0.5 { 0.32 } else { 0.55 }, ..default() })),
                    transform,
                    Part { index },
                    ModelColor(piece.color),
                    SceneContent,
                ));
                if !scene.ghost.contains(&part.component) {
                    entity.insert(Pickable::default()).observe(pick_part).observe(builder::placement::start_part).observe(view::part_over).observe(view::part_out);
                }
            }
            continue;
        }
        let mesh = match part.shape {
            SpatialShape::Box { size } => meshes.add(Cuboid::from_size(Vec3::from_array(size))),
            SpatialShape::Cylinder { radius, length } => meshes.add(Cylinder::new(radius, length)),
            SpatialShape::Sphere { radius } => meshes.add(Sphere::new(radius)),
        };
        let [r, g, b] = part.color_srgb;
        let mut entity = commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(r, g, b),
                metallic: 0.25,
                perceptual_roughness: 0.42,
                ..default()
            })),
            Transform::from_translation(Vec3::from_array(part.position))
                .with_rotation(Quat::from_array(part.rotation_xyzw)),
            Part { index },
            SceneContent,
        ));
        if !scene.ghost.contains(&part.component) {
            entity.insert(Pickable::default()).observe(pick_part).observe(builder::placement::start_part).observe(view::part_over).observe(view::part_out);
        }
    }
    physics_view::spawn_internals(commands, scene, meshes, materials);
}

fn pick_part(
    click: On<Pointer<Click>>,
    parts: Query<&Part>,
    mut scene: ResMut<SpatialScene>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    builder: Option<ResMut<builder::Builder>>,
    learn: Option<ResMut<lesson::Learn>>,
) {
    if click.button != bevy::picking::pointer::PointerButton::Primary {
        return;
    }
    if let Ok(part) = parts.get(click.entity) {
        let id = scene.spatial.parts[part.index].component.clone();
        if let Some(mut learn) = learn.filter(|l| l.active) {
            learn.pick(&mut scene, &id);
            return;
        }
        if let Some(mut builder) = builder {
            if builder.mode == builder::Mode::Annotate {
                if let Some(world)=click.hit.position {builder::discussion::begin_surface(&mut builder,&scene,part.index,world);}
                return;
            }
            let shift = keys.as_ref().is_some_and(|k| k.pressed(KeyCode::ShiftLeft) || k.pressed(KeyCode::ShiftRight));
            builder::click_part(&mut builder, &id, shift);
            return;
        }
        if keys
            .as_ref()
            .is_some_and(|k| k.pressed(KeyCode::ShiftLeft) || k.pressed(KeyCode::ShiftRight))
        {
            let mut ids = scene.details.components.clone();
            if !ids.remove(&id) {
                ids.insert(id);
            }
            let target = if ids.is_empty() {
                SelectionTarget::None
            } else {
                SelectionTarget::Components { ids }
            };
            let _ = scene.set_selection(target);
        } else {
            scene.select(id);
        }
    }
}

fn text(value: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(value),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
        TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter),
    )
}
fn action_button(label: &str, action: Action) -> impl Bundle {
    (
        Button,
        action,
        Node { border_radius: BorderRadius::all(Val::Px(5.0)),
            padding: UiRect::axes(Val::Px(12.0), Val::Px(9.0)),
            flex_shrink: 0.0,
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BorderColor::all(Color::srgb(0.21, 0.27, 0.33)),
        BackgroundColor(Color::srgb(0.115, 0.15, 0.19)),
        children![(text(label, 14.0, INK), ActionLabel)],
    )
}
fn setup_ui(mut commands: Commands, scene: Res<SpatialScene>) {
    spawn_ui(&mut commands, &scene);
}
pub(crate) fn spawn_ui(commands: &mut Commands, scene: &SpatialScene) {
    if scene.builder_mode {
        return; // Build mode draws its own chrome.
    }
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                height: Val::Px(TOP),
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(Val::Px(22.0), Val::Px(14.0)),
                row_gap: Val::Px(13.0),
                ..default()
            },
            BackgroundColor(PANEL),
            UiRoot,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    ..default()
                },
                children![
                    text(
                        format!("ASSEMBLY / {}", scene.spatial.title),
                        if scene.compact { 18. } else { 24. },
                        INK
                    ),
                    (
                        text("Standalone assembly", 13.0, ACCENT),
                        linked::LinkStatus
                    )
                ],
            ));
            root.spawn(Node {
                column_gap: Val::Px(6.0),
                flex_wrap: FlexWrap::Wrap,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|row| {
                row.spawn(action_button("Parts", Action::Parts));
                row.spawn(action_button("Clear", Action::Clear));
                row.spawn(action_button("Explode", Action::Explode));
                row.spawn(action_button("Connections", Action::Connections));
                row.spawn(action_button("Fit view", Action::Home));
                row.spawn(action_button("Hide selected", Action::Hide));
                row.spawn(action_button("Show all", Action::ShowAll));
            });
        });
    commands.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(TOP), bottom: Val::Px(BOTTOM), width: Val::Px(scene.left()), display: if scene.parts_visible && !scene.builder_mode { Display::Flex } else { Display::None }, padding: UiRect::all(Val::Px(18.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(9.0), ..default() }, BackgroundColor(PANEL), PartsPanel, UiRoot))
        .with_children(|column| {
            column.spawn(text("COMPONENTS", 12.0, MUTED));
            for (i, (id, label)) in scene.representatives().iter().enumerate() {
                column.spawn(action_button(&format!("{}  {}", i+1, label), Action::Select(id.clone())));
            }
            column.spawn((text("Select a part in the assembly or here.\n\nGold marks the selection.\nLive temperature colors retain their scale when selected.", 13.0, MUTED), Node { margin: UiRect::top(Val::Px(16.0)), ..default() }));
        });
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(0.0),
                top: Val::Px(TOP),
                bottom: Val::Px(BOTTOM),
                width: Val::Px(scene.right()),
                padding: UiRect::all(Val::Px(20.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(14.0),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            BackgroundColor(PANEL),
            InspectorScroll,
            UiRoot,
        ))
        .with_children(|column| {
            column.spawn((
                notes::NotesPanel,
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(8.),
                    flex_shrink: 0.,
                    ..default()
                },
            ));
            column.spawn(text("INSPECTOR", 12.0, ACCENT));
            column.spawn((
                text("", 13.0, INK),
                animation::LiveReadouts,
                Node {
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
            column.spawn(text("Select a connection:", 12.0, MUTED));
            for (i, id) in scene.description.nets.keys().enumerate() {
                column.spawn(action_button(
                    &format!(
                        "{} {}",
                        i + 1,
                        scene.description.nets[id]
                            .ports
                            .iter()
                            .map(|p| scene.description.components
                                [&scene.description.ports[p].component]
                                .label
                                .as_str())
                            .collect::<Vec<_>>()
                            .join(" / ")
                    ),
                    Action::Net(id.clone()),
                ));
            }
            column.spawn((
                text("", 14.0, INK),
                Inspector,
                Node {
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
        });
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(0.0),
                height: Val::Px(BOTTOM),
                padding: UiRect::axes(Val::Px(22.0), Val::Px(10.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(5.0),
                ..default()
            },
            BackgroundColor(PANEL),
            UiRoot,
        ))
        .with_children(|column| {
            column.spawn(text(
                format!(
                    "Right-drag: orbit | Shift-drag: pan | Scroll: zoom | F: fit | 1-{}: select",
                    scene.representatives().len().min(9)
                ),
                13.0,
                INK,
            ));
            column.spawn((text("", 12.0, MUTED), Status));
        });
}

fn buttons(
    mut interactions: Query<(&Interaction, &Action, &mut BackgroundColor), With<Button>>,
    scene: Res<SpatialScene>,
) {
    for (interaction, action, mut bg) in &mut interactions {
        let active = match action {
            Action::Select(id) => scene.details.components.contains(id),
            Action::Net(id) => {
                matches!(&scene.selection, SelectionTarget::Nets {ids} if ids.contains(id))
            }
            Action::Parts => scene.parts_visible,
            Action::Explode => scene.state.exploded,
            Action::Connections => scene.state.connections,
            _ => false,
        };
        bg.0 = if active {
            Color::srgb(0.12, 0.32, 0.31)
        } else if *interaction == Interaction::Hovered {
            Color::srgb(0.20, 0.26, 0.32)
        } else {
            Color::srgb(0.115, 0.15, 0.19)
        };
    }
    // Press dispatch is handled by change detection below, to avoid repeat while held.
}

fn dispatch(action: &Action, scene: &mut SpatialScene, orbit: &mut Orbit) {
    let command = match action {
        Action::Net(id) => {
            if let Err(e) = scene.set_selection(SelectionTarget::net(id.clone())) {
                error!("{e}");
            }
            return;
        }
        Action::Clear => {
            let _ = scene.set_selection(SelectionTarget::None);
            return;
        }
        Action::Parts => {
            scene.parts_visible = !scene.parts_visible;
            orbit.home = true;
            return;
        }
        Action::Select(id) => SpatialCommand::Select {
            component: id.clone(),
        },
        Action::Explode => {
            orbit.home = true;
            SpatialCommand::SetExploded {
                enabled: !scene.state.exploded,
            }
        }
        Action::Connections => SpatialCommand::SetConnections {
            enabled: !scene.state.connections,
        },
        Action::Home => {
            orbit.home = true;
            return;
        }
        Action::Hide => SpatialCommand::HideSelected,
        Action::ShowAll => SpatialCommand::ShowAll,
    };
    if let Err(e) = scene.apply(command) {
        error!("{e}");
    }
}

fn keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    actions: Query<(&Interaction, &Action), (Changed<Interaction>, With<Button>)>,
    mut scene: ResMut<SpatialScene>,
    mut camera: Single<&mut Orbit>,
    window: Option<Single<&Window>>,
    builder: Option<Res<builder::Builder>>,
    learn: Option<Res<lesson::Learn>>,
) {
    if builder.as_ref().is_some_and(|b| b.typing()) || learn.as_ref().is_some_and(|l| l.active) {
        return;
    }
    // F: fly to the selected part (the whole system when nothing is selected).
    if let Some(window) = window.filter(|_| keys.just_pressed(KeyCode::KeyF)) {
        let focus = scene.details.components.iter().next().cloned().or_else(|| scene.state.selected.clone());
        view::zoom_to(&scene, &mut camera, &window, focus.as_deref(), 1.0, view::GLIDE_S);
    }
    for (interaction, action) in &actions {
        if *interaction == Interaction::Pressed {
            dispatch(action, &mut scene, &mut camera);
        }
    }
    for (key, action) in [
        (KeyCode::Escape, Action::Clear),
        (KeyCode::KeyH, Action::Home),
        (KeyCode::KeyE, Action::Explode),
        (KeyCode::KeyC, Action::Connections),
    ] {
        if keys.just_pressed(key) {
            dispatch(&action, &mut scene, &mut camera);
        }
    }
    let ids = scene.representatives();
    for (i, key) in [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ]
    .iter()
    .enumerate()
    {
        if keys.just_pressed(*key) {
            if let Some((id, _)) = ids.get(i) {
                scene.select(id.clone());
            }
        }
    }
}

fn update_layout(scene: Res<SpatialScene>, mut panels: Query<&mut Node, With<PartsPanel>>) {
    if !scene.is_changed() {
        return;
    }
    for mut panel in &mut panels {
        panel.display = if scene.parts_visible && !scene.builder_mode {
            Display::Flex
        } else {
            Display::None
        };
        panel.width = Val::Px(scene.left());
    }
}
fn camera_viewport(
    scene: Res<SpatialScene>,
    window: Single<&Window>,
    mut camera: Single<&mut Camera, With<Orbit>>,
) {
    if let Some(view) = scene.learn_view {
        // Embedded in a lesson card: render only the visible part of the
        // card, with the projection of the whole card (no squash when scrolled).
        let (viewport, sub) = if view.visible.width() < 1.0 || view.visible.height() < 1.0 {
            (Viewport { physical_position: UVec2::ZERO, physical_size: UVec2::ONE, ..default() }, None)
        } else {
            let sub = bevy::camera::SubCameraView {
                full_size: view.full.size().max(Vec2::ONE).as_uvec2(),
                offset: view.visible.min - view.full.min,
                size: view.visible.size().max(Vec2::ONE).as_uvec2(),
            };
            (Viewport { physical_position: view.visible.min.max(Vec2::ZERO).as_uvec2(), physical_size: view.visible.size().max(Vec2::ONE).as_uvec2(), ..default() }, Some(sub))
        };
        if camera.viewport.as_ref().is_none_or(|old| old.physical_size != viewport.physical_size || old.physical_position != viewport.physical_position) {
            camera.viewport = Some(viewport);
        }
        if camera.sub_camera_view != sub {
            camera.sub_camera_view = sub;
        }
        return;
    }
    if camera.sub_camera_view.is_some() {
        camera.sub_camera_view = None;
    }
    let scale = window.scale_factor();
    let width = (window.width() - scene.left() - scene.right()).max(1.0);
    let height = (window.height() - scene.top() - scene.bottom()).max(1.0);
    let viewport = Viewport {
        physical_position: UVec2::new((scene.left() * scale) as u32, (scene.top() * scale) as u32),
        physical_size: UVec2::new((width * scale) as u32, (height * scale) as u32),
        ..default()
    };
    if camera.viewport.as_ref().is_none_or(|old| {
        old.physical_size != viewport.physical_size
            || old.physical_position != viewport.physical_position
    }) {
        camera.viewport = Some(viewport);
    }
}
fn orbit(
    time: Res<Time>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    scene: Res<SpatialScene>,
    window: Single<&Window>,
    camera: Single<(&mut Transform, &mut Orbit)>,
) {
    let drag = motion.read().fold(Vec2::ZERO, |sum, e| sum + e.delta);
    let zoom = wheel.read().fold(0.0, |sum, e| {
        sum + match e.unit {
            MouseScrollUnit::Line => e.y,
            MouseScrollUnit::Pixel => e.y * 0.02,
        }
    });
    let (mut transform, mut orbit) = camera.into_inner();
    let learn = scene.learn_view;
    let in_scene = window.cursor_position().is_some_and(|p| match learn {
        Some(v) => v.visible.contains(p * window.scale_factor()),
        None => {
            p.x > scene.left()
                && p.x < window.width() - scene.right()
                && p.y > scene.top()
                && p.y < window.height() - scene.bottom()
        }
    });
    // In a lesson the wheel scrolls the page; zoom needs Ctrl or Cmd.
    let zoom = if learn.is_some() && !(keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) || keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight)) { 0.0 } else { zoom };
    let (_, extent) = scene.bounds();
    if orbit.home {
        // The overview: a glide once the view has been placed, a cut the first time.
        let pose = view::frame_pose(&scene, None, 1.0, 0.35, 0.60, view::aspect(&scene, &window));
        let seconds = if orbit.radius > 0. && orbit.focus != Vec3::ZERO { view::GLIDE_S } else { 0. };
        orbit.glide_to(pose, seconds);
    }
    if scene.reduced_motion {
        // Cuts instead of glides; no circling.
        orbit.finish_glide();
        orbit.spin = 0.;
    }
    orbit.step(time.delta_secs().min(0.1));
    if in_scene {
        let dragging = (buttons.pressed(MouseButton::Right) || buttons.pressed(MouseButton::Middle)) && drag != Vec2::ZERO;
        if dragging || zoom != 0.0 {
            orbit.interrupt();
        }
        let pan_modifier = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        if buttons.pressed(MouseButton::Right) && !pan_modifier {
            orbit.yaw -= drag.x * 0.007;
            orbit.pitch = (orbit.pitch + drag.y * 0.007).clamp(-1.4, 1.4);
        }
        if buttons.pressed(MouseButton::Middle)
            || (buttons.pressed(MouseButton::Right) && pan_modifier)
        {
            let shift =
                (transform.right() * -drag.x + transform.up() * drag.y) * orbit.radius * 0.0015;
            orbit.focus += shift;
        }
        orbit.radius = (orbit.radius * (-zoom * 0.12).exp()).clamp(extent * 0.3, extent * 20.0);
    }
    let horizontal = orbit.pitch.cos() * orbit.radius;
    transform.translation = orbit.focus
        + Vec3::new(
            orbit.yaw.sin() * horizontal,
            orbit.pitch.sin() * orbit.radius,
            orbit.yaw.cos() * horizontal,
        );
    transform.look_at(orbit.focus, Vec3::Y);
}

fn update_parts(
    scene: Res<SpatialScene>,
    mut parts: Query<(
        &Part,
        &mut Transform,
        &mut Visibility,
        &MeshMaterial3d<StandardMaterial>,
        Option<&ModelColor>,
    ), Without<view::CompanionPart>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !scene.is_changed() {
        return;
    }
    for (part, mut transform, mut visibility, handle, model) in &mut parts {
        let p = &scene.spatial.parts[part.index];
        *transform = animation::part_transform(&scene, part.index);
        *visibility = if scene.state.hidden.contains(&p.component) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if let Some(mut material) = materials.get_mut(&handle.0) {
            let thermal = animation::part_color(&scene, part.index);
            let selected = scene.details.components.contains(&p.component);
            let [r, g, b] = thermal.unwrap_or(p.color_srgb);
            if let Some(ModelColor(color)) = model {
                // Models keep their colours; temperature tints them, selection glows.
                material.base_color = if thermal.is_some() { Color::srgb(r, g, b).mix(color, 0.35) } else { *color };
            } else {
                material.base_color = if selected && thermal.is_none() {
                    Color::srgb(0.96, 0.70, 0.26)
                } else {
                    Color::srgb(r, g, b)
                };
            }
            if scene.ghost.contains(&p.component) {
                material.base_color = material.base_color.with_alpha(0.13);
                material.alpha_mode = AlphaMode::Blend;
            } else if let Some(alpha) = view::emphasis(&scene, &p.component) {
                // Spotlight and X-ray: everything else goes see-through.
                material.base_color = material.base_color.with_alpha(alpha);
                material.alpha_mode = AlphaMode::Blend;
            } else if physics_view::has_internals(&scene, &p.id) {
                // See-through housing: the moving pieces inside are drawn.
                material.base_color = material.base_color.with_alpha(0.28);
                material.alpha_mode = AlphaMode::Blend;
            } else {
                material.alpha_mode = AlphaMode::Opaque;
            }
            let glow = animation::part_temperature(&scene, part.index).filter(|_| scene.state.overlays.contains(&sim_inspect::spatial::Overlay::Heat)).map(|k| ((k - 293.15) / 60.).clamp(0., 1.) as f32).unwrap_or(0.);
            material.emissive = if glow > 0.01 {
                LinearRgba::new(1.4 * glow, 0.38 * glow, 0.08 * glow, 1.)
            } else if selected && model.is_some() {
                LinearRgba::new(0.30, 0.19, 0.03, 1.)
            } else if selected && thermal.is_some() {
                LinearRgba::new(0.12, 0.07, 0.01, 1.)
            } else {
                LinearRgba::BLACK
            };
        }
    }
}

fn scroll_inspector(
    scene: Res<SpatialScene>,
    mut wheel: MessageReader<MouseWheel>,
    window: Single<&Window>,
    mut panel: Single<&mut ScrollPosition, With<InspectorScroll>>,
) {
    let delta = wheel.read().fold(0.0, |sum, e| {
        sum + match e.unit {
            MouseScrollUnit::Line => e.y * 24.0,
            MouseScrollUnit::Pixel => e.y,
        }
    });
    if window.cursor_position().is_some_and(|p| {
        p.x >= window.width() - scene.right() && p.y > TOP && p.y < window.height() - BOTTOM
    }) {
        panel.y = (panel.y - delta).max(0.0);
    }
}

fn inspector(scene: &SpatialScene) -> String {
    if let Some(text) = linked::selection_inspector(scene) {
        return text;
    }
    let Some(id) = &scene.state.selected else {
        return format!(
            "{}\n\n{} components / {} physical nets\n\nClick a part to inspect its shared model identity, parameters and connections.\n\nGEOMETRY\nIllustrative assembly study. Shapes and placement are not CAD geometry.\n\nSIMULATION\n{}",
            scene.spatial.title,
            scene.description.components.len(),
            scene.description.nets.len(),
            scene.live_status()
        );
    };
    let c = &scene.description.components[id];
    let label = scene
        .spatial
        .parts
        .iter()
        .find(|p| &p.component == id)
        .map(|p| p.label.as_str())
        .unwrap_or(&c.label);
    let mut s = format!("{label}\n{}\n\nMODEL PARAMETERS\n", c.component_type);
    if c.parameters.is_empty() {
        s.push_str("No parameters declared.\n");
    }
    for (name, p) in &c.parameters {
        s.push_str(&format!(
            "{}: {} {}\n",
            name.replace('_', " "),
            p.value,
            p.unit
                .as_deref()
                .unwrap_or("[unit undeclared]")
                .replace('Ω', "ohm")
                .replace('·', " ")
                .replace('²', "^2")
                .replace('³', "^3")
        ));
    }
    s.push_str("\nCONNECTIONS\n");
    let ports: Vec<_> = scene
        .description
        .ports
        .values()
        .filter(|p| &p.component == id && p.composite_parent.is_none())
        .collect();
    // Include physical leaves of composite connectors for actual terminal membership.
    let leaves: Vec<_> = scene
        .description
        .ports
        .values()
        .filter(|p| {
            &p.component == id
                && !scene
                    .description
                    .ports
                    .values()
                    .any(|child| child.composite_parent.as_ref() == Some(&p.id))
        })
        .collect();
    for port in &leaves {
        let others: Vec<_> = scene
            .description
            .nets
            .values()
            .filter(|n| n.ports.contains(&port.id))
            .flat_map(|n| n.ports.iter())
            .filter_map(|pid| scene.description.ports.get(pid))
            .filter(|p| &p.component != id)
            .map(|p| {
                format!(
                    "{}.{}",
                    scene.description.components[&p.component].label, p.name
                )
            })
            .collect();
        s.push_str(&format!(
            "{}: {}\n",
            port.name,
            if others.is_empty() {
                "open".into()
            } else {
                others.join(", ")
            }
        ));
    }
    let observations = scene
        .description
        .observables
        .values()
        .filter(|o| match &o.location {
            ObservationLocation::State { component, .. }
            | ObservationLocation::Diagnostic {
                component: Some(component),
                ..
            } => component == id,
            ObservationLocation::Across { port, .. }
            | ObservationLocation::Through { port, .. }
            | ObservationLocation::Signal { port } => scene
                .description
                .ports
                .get(port)
                .is_some_and(|p| &p.component == id),
            _ => false,
        })
        .count();
    s.push_str(&format!("\nOBSERVATIONS\n{observations} authored quantities; live readouts above\n{} top-level ports\n\nSOURCE ID\n{id}\n\nPROVENANCE\nParameters: provenance / uncertainty unspecified.\nGeometry: illustrative, display only.\n", ports.len()));
    if !scene.spatial.parts.iter().any(|p| &p.component == id) {
        s.push_str("\nNo geometry for this component.");
    }
    if scene.state.hidden.contains(id) {
        s.push_str("\nHidden in assembly. Select to reveal.");
    }
    s
}

fn update_ui(
    scene: Res<SpatialScene>,
    mut texts: ParamSet<(
        Query<&mut Text, With<Inspector>>,
        Query<&mut Text, With<Status>>,
        Query<&mut Text, With<animation::LiveReadouts>>,
    )>,
) {
    if !scene.is_changed() {
        return;
    }
    for mut t in &mut texts.p0() {
        t.0 = inspector(&scene);
    }
    for mut t in &mut texts.p1() {
        t.0 = format!(
            "Illustrative geometry | {} hidden | {}",
            scene.state.hidden.len(),
            scene.live_status().replace('\n', " | ")
        );
    }
    for mut t in &mut texts.p2() {
        t.0 = scene.live_readouts();
    }
}

fn draw_guides(scene: Res<SpatialScene>, mut gizmos: Gizmos) {
    let (center, extent) = scene.bounds();
    // A presentation grid, deliberately without implying source CAD dimensions.
    let step = extent / 8.0;
    for i in -12..=12 {
        let v = i as f32 * step;
        let col = Color::srgb(0.18, 0.22, 0.26);
        gizmos.line(
            Vec3::new(center.x - extent * 1.5, -0.004, center.z + v),
            Vec3::new(center.x + extent * 1.5, -0.004, center.z + v),
            col,
        );
        gizmos.line(
            Vec3::new(center.x + v, -0.004, center.z - extent * 1.5),
            Vec3::new(center.x + v, -0.004, center.z + extent * 1.5),
            col,
        );
    }
    if !scene.state.connections {
        return;
    }
    for net in scene.description.nets.values() {
        let Some((hub, endpoints)) = linked::net_geometry(&scene, &net.id) else {
            continue;
        };
        let color = if scene.details.nets.contains(&net.id) {
            Color::srgb(1.0, 0.76, 0.32)
        } else {
            Color::srgb(0.40, 0.65, 0.73)
        };
        for p in endpoints {
            gizmos.line(p, hub, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::picking::{
        backend::HitData,
        pointer::{Location, PointerButton, PointerId},
    };
    use bevy::camera::RenderTarget;

    pub(super) fn fixture() -> SpatialScene {
        let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/systems-viewer/spatial");
        SpatialScene::new(
            serde_json::from_slice(
                &std::fs::read(base.join("motor-thermal.description.json")).unwrap(),
            )
            .unwrap(),
            serde_json::from_slice(
                &std::fs::read(base.join("motor-thermal.spatial.json")).unwrap(),
            )
            .unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn linked_net_and_port_selection_reveal_geometry_without_editing_sources() {
        let mut scene = fixture();
        let physical = serde_json::to_vec(&scene.description).unwrap();
        let geometry = serde_json::to_vec(&scene.spatial).unwrap();
        scene.state.hidden = scene.description.components.keys().cloned().collect();
        let net = scene
            .description
            .nets
            .values()
            .find(|n| n.ports.len() > 2)
            .unwrap()
            .clone();
        scene.set_selection(SelectionTarget::net(&net.id)).unwrap();
        assert!(scene.state.connections);
        assert_eq!(scene.details.nets.len(), 1);
        assert_eq!(scene.details.ports.len(), net.ports.len());
        assert!(
            scene
                .details
                .components
                .iter()
                .all(|id| !scene.state.hidden.contains(id))
        );
        assert!(linked::net_geometry(&scene, &net.id).is_some());
        assert!(inspector(&scene).contains("CONNECTION"));
        scene.apply(SpatialCommand::HideSelected).unwrap();
        assert!(
            scene
                .details
                .components
                .iter()
                .all(|id| scene.state.hidden.contains(id))
        );
        assert!(linked::net_geometry(&scene, &net.id).is_none());
        scene.set_selection(SelectionTarget::None).unwrap();
        scene.set_selection(SelectionTarget::net(&net.id)).unwrap();
        assert!(linked::net_geometry(&scene, &net.id).is_some());
        let port = net.ports[0].clone();
        scene
            .set_selection(SelectionTarget::Ports {
                ids: std::collections::BTreeSet::from([port.clone()]),
            })
            .unwrap();
        assert!(
            scene
                .details
                .components
                .contains(&scene.description.ports[&port].component)
        );
        assert!(inspector(&scene).contains("PORT"));
        assert_eq!(serde_json::to_vec(&scene.description).unwrap(), physical);
        assert_eq!(serde_json::to_vec(&scene.spatial).unwrap(), geometry);
        scene.set_selection(SelectionTarget::None).unwrap();
        assert_eq!(scene.details, SelectionDetails::default());
    }
    #[test]
    fn mesh_click_and_list_button_resolve_the_same_source_identity() {
        let scene = fixture();
        let expected = scene.spatial.parts[1].component.clone();
        let mut app = App::new();
        app.insert_resource(scene)
            .insert_resource(ButtonInput::<KeyCode>::default());
        app.add_systems(Update, keyboard);
        let camera = app
            .world_mut()
            .spawn(Orbit {
                focus: Vec3::ZERO,
                radius: 1.0,
                yaw: 0.0,
                pitch: 0.0,
                home: false,
                ..Default::default()
            })
            .id();
        let part = app
            .world_mut()
            .spawn(Part { index: 1 })
            .observe(pick_part)
            .id();
        let window = app.world_mut().spawn_empty().id();
        let click = Pointer::new(
            PointerId::Mouse,
            Location {
                target: RenderTarget::Window(bevy::window::WindowRef::Entity(window))
                    .normalize(Some(window))
                    .unwrap(),
                position: Vec2::ZERO,
            },
            Click {
                button: PointerButton::Primary,
                hit: HitData::new(camera, 0.1, None, None),
                duration: std::time::Duration::from_millis(50),
                count: 1,
            },
            part,
        );
        app.world_mut().trigger(click);
        app.update();
        assert_eq!(
            app.world()
                .resource::<SpatialScene>()
                .state
                .selected
                .as_ref(),
            Some(&expected)
        );
        assert!(inspector(app.world().resource::<SpatialScene>()).contains("resistance: 2"));
        app.world_mut()
            .resource_mut::<SpatialScene>()
            .apply(SpatialCommand::ClearSelection)
            .unwrap();
        let button = app
            .world_mut()
            .spawn((
                Button,
                Interaction::Pressed,
                Action::Select(expected.clone()),
            ))
            .id();
        app.update();
        assert_eq!(
            app.world()
                .resource::<SpatialScene>()
                .state
                .selected
                .as_ref(),
            Some(&expected)
        );
        *app.world_mut().get_mut::<Action>(button).unwrap() = Action::Explode;
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::None;
        app.update();
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;
        app.update();
        assert!(app.world().resource::<SpatialScene>().state.exploded);
        app.update();
        assert!(
            app.world().resource::<SpatialScene>().state.exploded,
            "holding a button must not repeatedly toggle"
        );
    }
}

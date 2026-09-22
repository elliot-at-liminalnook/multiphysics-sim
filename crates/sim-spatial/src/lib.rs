//! Reusable Bevy presentation of shared inspection contracts. This crate does
//! not depend on a runtime or solver and cannot advance physics.
mod animation;
mod linked;
mod notes;
pub mod rest;
use bevy::{
    core_pipeline::tonemapping::Tonemapping,
    input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    picking::mesh_picking::{MeshPickingCamera, MeshPickingSettings},
    prelude::*,
    render::camera::Viewport,
    winit::{UpdateMode, WinitSettings},
};
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
}
impl SpatialScene {
    pub fn new(
        description: SystemDescription,
        spatial: SpatialDescription,
    ) -> Result<Self, InspectionError> {
        spatial.validate(&description)?;
        Ok(Self {
            description,
            spatial,
            state: SpatialViewState::default(),
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
        })
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
        if self.parts_visible {
            if self.compact { 180. } else { LEFT }
        } else {
            0.
        }
    }
    fn right(&self) -> f32 {
        if self.compact { 260. } else { RIGHT }
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
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for p in &self.spatial.parts {
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
            .add_systems(Startup, (setup_scene, setup_ui))
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
            );
    }
}

pub fn run(scene: SpatialScene, link: Option<SelectionLink>) {
    run_with_api(scene, link, None);
}
pub fn run_with_api(
    mut scene: SpatialScene,
    link: Option<SelectionLink>,
    api: Option<sim_api::Server>,
) {
    let compact = scene.compact;
    if compact {
        scene.parts_visible = false;
    }
    let mut app = App::new();
    if let Some(api) = api {
        app.insert_resource(rest::Rest(api, None));
    }
    if let Some(link) = link {
        app.insert_resource(link);
    }
    app.insert_resource(scene)
        .insert_resource(ClearColor(Color::srgb(0.10, 0.125, 0.155)))
        .insert_resource(AmbientLight {
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
                    (880.0_f32, 850.0_f32).into()
                } else {
                    (1440.0_f32, 900.0_f32).into()
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
#[derive(Component)]
struct Orbit {
    focus: Vec3,
    radius: f32,
    yaw: f32,
    pitch: f32,
    home: bool,
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
        },
    ));
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        IsDefaultUiCamera,
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 5500.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.95, -0.7, 0.0)),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 2000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.4, 2.4, 0.0)),
    ));
    linked::spawn_nets(&mut commands, &scene, &mut meshes, &mut materials);
    for (index, part) in scene.spatial.parts.iter().enumerate() {
        let mesh = match part.shape {
            SpatialShape::Box { size } => meshes.add(Cuboid::from_size(Vec3::from_array(size))),
            SpatialShape::Cylinder { radius, length } => meshes.add(Cylinder::new(radius, length)),
            SpatialShape::Sphere { radius } => meshes.add(Sphere::new(radius)),
        };
        let [r, g, b] = part.color_srgb;
        commands
            .spawn((
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
                Pickable::default(),
            ))
            .observe(pick_part);
    }
}

fn pick_part(
    click: Trigger<Pointer<Click>>,
    parts: Query<&Part>,
    mut scene: ResMut<SpatialScene>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
) {
    if click.button != bevy::picking::pointer::PointerButton::Primary {
        return;
    }
    if let Ok(part) = parts.get(click.target()) {
        let id = scene.spatial.parts[part.index].component.clone();
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
            font_size: size,
            ..default()
        },
        TextColor(color),
        TextLayout::new_with_linebreak(bevy::text::LineBreak::WordOrCharacter),
    )
}
fn action_button(label: &str, action: Action) -> impl Bundle {
    (
        Button,
        action,
        Node {
            padding: UiRect::axes(Val::Px(12.0), Val::Px(9.0)),
            flex_shrink: 0.0,
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BorderColor(Color::srgb(0.21, 0.27, 0.33)),
        BorderRadius::all(Val::Px(5.0)),
        BackgroundColor(Color::srgb(0.115, 0.15, 0.19)),
        children![(text(label, 14.0, INK), ActionLabel)],
    )
}
fn setup_ui(mut commands: Commands, scene: Res<SpatialScene>) {
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
    commands.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(TOP), bottom: Val::Px(BOTTOM), width: Val::Px(scene.left()), display: if scene.parts_visible { Display::Flex } else { Display::None }, padding: UiRect::all(Val::Px(18.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(9.0), ..default() }, BackgroundColor(PANEL), PartsPanel))
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
) {
    for (interaction, action) in &actions {
        if *interaction == Interaction::Pressed {
            dispatch(action, &mut scene, &mut camera);
        }
    }
    for (key, action) in [
        (KeyCode::Escape, Action::Clear),
        (KeyCode::KeyF, Action::Home),
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
        panel.display = if scene.parts_visible {
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
    let scale = window.scale_factor();
    let width = (window.width() - scene.left() - scene.right()).max(1.0);
    let height = (window.height() - TOP - BOTTOM).max(1.0);
    let viewport = Viewport {
        physical_position: UVec2::new((scene.left() * scale) as u32, (TOP * scale) as u32),
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
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut motion: EventReader<MouseMotion>,
    mut wheel: EventReader<MouseWheel>,
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
    let in_scene = window.cursor_position().is_some_and(|p| {
        p.x > scene.left()
            && p.x < window.width() - scene.right()
            && p.y > TOP
            && p.y < window.height() - BOTTOM
    });
    let (_, extent) = scene.bounds();
    if orbit.home {
        let (center, radius) = scene.bounds();
        orbit.focus = center;
        let aspect = ((window.width() - scene.left() - scene.right())
            / (window.height() - TOP - BOTTOM))
            .max(0.1);
        orbit.radius = radius * 2.9 / aspect.min(1.0);
        orbit.yaw = 0.35;
        orbit.pitch = 0.60;
        orbit.home = false;
    }
    if in_scene {
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
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !scene.is_changed() {
        return;
    }
    for (part, mut transform, mut visibility, handle) in &mut parts {
        let p = &scene.spatial.parts[part.index];
        *transform = animation::part_transform(&scene, part.index);
        *visibility = if scene.state.hidden.contains(&p.component) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if let Some(material) = materials.get_mut(&handle.0) {
            let thermal = animation::part_color(&scene, part.index);
            let selected = scene.details.components.contains(&p.component);
            let [r, g, b] = thermal.unwrap_or(p.color_srgb);
            material.base_color = if selected && thermal.is_none() {
                Color::srgb(0.96, 0.70, 0.26)
            } else {
                Color::srgb(r, g, b)
            };
            material.emissive = if selected && thermal.is_some() {
                LinearRgba::new(0.12, 0.07, 0.01, 1.)
            } else {
                LinearRgba::BLACK
            };
        }
    }
}

fn scroll_inspector(
    scene: Res<SpatialScene>,
    mut wheel: EventReader<MouseWheel>,
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
        panel.offset_y = (panel.offset_y - delta).max(0.0);
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
    use bevy::render::camera::RenderTarget;

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
            part,
            Click {
                button: PointerButton::Primary,
                hit: HitData::new(camera, 0.1, None, None),
                duration: std::time::Duration::from_millis(50),
            },
        );
        app.world_mut().trigger_targets(click, part);
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

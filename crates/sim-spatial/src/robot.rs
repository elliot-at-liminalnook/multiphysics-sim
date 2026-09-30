//! Robot mode (`--robot FILE`): a CAD-exported `.simrobot.json` opened
//! read-only. The shared `PhysicalModel` loader runs on a worker thread; each
//! link's collision geometry is drawn at the exported assembly pose. Nothing
//! is stepped, edited or written; running a robot still needs sim-app.
use super::{ACCENT, INK, MUTED, PANEL};
use crate::builder::ui::UiFonts;
use bevy::{
    asset::RenderAssetUsages,
    core_pipeline::tonemapping::Tonemapping,
    input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    picking::mesh_picking::{MeshPickingCamera, MeshPickingSettings},
    prelude::*,
    render::{
        camera::Viewport,
        mesh::{Indices, PrimitiveTopology},
    },
    winit::{UpdateMode, WinitSettings},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_domain_robot::PhysicalModel;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, mpsc};

const LEFT: f32 = 280.0;
const RIGHT: f32 = 330.0;
const TOP: f32 = 64.0;
/// Exported link frames: origin at the stored `com`, axes aligned with the
/// model frame (the solver's zero-angle pose, `articulated.rs` `com0`).
pub const POSE: &str = "exported assembly pose: link frames at the stored com, axes aligned with the model frame (Z up); no joint motion, not stepped";

/// One link's display triangles in its own frame (flat-shaded).
pub struct LinkGeometry {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
}
impl LinkGeometry {
    pub fn triangles(&self) -> usize {
        self.positions.len() / 3
    }
}
/// A robot file as loaded on the worker thread.
pub struct Loaded {
    pub model: PhysicalModel,
    /// Per link, `None` when it has no collision geometry to draw.
    pub geometry: Vec<Option<LinkGeometry>>,
}

/// The shared loader plus triangulation; called on the worker thread (and by
/// `--validate-only`). Errors name the path.
pub fn load(path: &Path) -> Result<Loaded, String> {
    let model = PhysicalModel::load(&path.to_string_lossy())?;
    let geometry = model
        .links
        .iter()
        .map(|l| {
            let mut g = LinkGeometry { positions: Vec::new(), normals: Vec::new() };
            for tri in l.collision.display_triangles() {
                let n = sim_domain_robot::model::triangle_normal(tri);
                for p in tri {
                    g.positions.push([p[0] as f32, p[1] as f32, p[2] as f32]);
                    g.normals.push([n[0] as f32, n[1] as f32, n[2] as f32]);
                }
            }
            (!g.positions.is_empty()).then_some(g)
        })
        .collect();
    Ok(Loaded { model, geometry })
}

enum Status {
    Loading(std::time::Instant),
    Loaded { seconds: f64 },
    Error(String),
}

#[derive(Resource)]
pub struct RobotView {
    pub path: PathBuf,
    status: Status,
    model: Option<PhysicalModel>,
    triangles: Vec<usize>,
    /// The one link selection shared by the list, the 3D view and REST.
    pub selected: Option<usize>,
    rx: Option<Mutex<mpsc::Receiver<Result<Loaded, String>>>>,
    ui_revision: u64,
    panels_ready: bool,
}
impl RobotView {
    /// Starts the worker load; the window opens without waiting for it.
    pub fn open(path: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        let worker = path.clone();
        std::thread::spawn(move || {
            let _ = tx.send(load(&worker));
        });
        Self {
            path,
            status: Status::Loading(std::time::Instant::now()),
            model: None,
            triangles: Vec::new(),
            selected: None,
            rx: Some(Mutex::new(rx)),
            ui_revision: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_micros() as u64,
            panels_ready: false,
        }
    }
    fn link_name(&self, i: usize) -> Option<&str> {
        self.model.as_ref()?.links.get(i).map(|l| l.name.as_str())
    }
    pub fn state_json(&self) -> Value {
        let (status, error, seconds) = match &self.status {
            Status::Loading(_) => ("loading", None, None),
            Status::Loaded { seconds } => ("loaded", None, Some(*seconds)),
            Status::Error(e) => ("error", Some(e.clone()), None),
        };
        let links: Vec<Value> = self
            .model
            .iter()
            .flat_map(|m| m.links.iter().enumerate())
            .map(|(i, l)| json!({"index": i, "name": l.name, "has_mesh": self.triangles.get(i).is_some_and(|t| *t > 0), "triangles": self.triangles.get(i).copied().unwrap_or(0)}))
            .collect();
        let selected = self.selected.and_then(|i| {
            let l = self.model.as_ref()?.links.get(i)?;
            Some(json!({"index": i, "name": l.name, "mass": l.mass, "com": l.com, "material": l.material}))
        });
        json!({"file": self.path, "status": status, "error": error, "load_seconds": seconds,
            "link_count": self.model.as_ref().map(|m| m.links.len()), "links": links, "selected": selected,
            "pose": POSE, "read_only": true, "stepped": false, "ui_revision": self.ui_revision, "controls_ready": self.panels_ready})
    }
}

/// The handlers behind a click and `system_ui` activation.
#[derive(Component, Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
enum RobotAction {
    SelectLink { index: usize, name: String },
    ClearSelection,
    Fit,
}
fn dispatch(view: &mut RobotView, orbit: &mut RobotOrbit, action: RobotAction) {
    match action {
        RobotAction::SelectLink { index, .. } => view.selected = Some(index),
        RobotAction::ClearSelection => view.selected = None,
        RobotAction::Fit => orbit.home = true,
    }
}
fn controls(view: &RobotView) -> Vec<(String, String, RobotAction)> {
    let mut out: Vec<(String, String, RobotAction)> = view
        .model
        .iter()
        .flat_map(|m| m.links.iter().enumerate())
        .map(|(index, l)| (format!("link:{index}"), l.name.clone(), RobotAction::SelectLink { index, name: l.name.clone() }))
        .collect();
    if view.panels_ready {
        out.push(("clear_selection".into(), "Clear selection".into(), RobotAction::ClearSelection));
        out.push(("fit".into(), "Fit".into(), RobotAction::Fit));
    }
    out
}
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum UiRequest {
    Controls,
    Activate { id: String, ui_revision: u64 },
}
#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    RobotState,
    SystemUi { action: UiRequest },
    Camera { focus: [f32; 3], radius: f32, yaw: f32, pitch: f32 },
    Fit,
}
fn execute(view: &mut RobotView, orbit: &mut RobotOrbit, command: &sim_api::Command) -> sim_api::Result {
    match sim_api::decode::<Request>(command)? {
        Request::RobotState => {}
        Request::SystemUi { action: UiRequest::Controls } => {
            let items: Vec<Value> = controls(view).into_iter().map(|(id, label, action)| json!({"id": id, "label": label, "enabled": true, "action": action})).collect();
            return Ok(json!({"ui_revision": view.ui_revision, "ready": view.panels_ready, "controls": items, "state": view.state_json()}));
        }
        Request::SystemUi { action: UiRequest::Activate { id, ui_revision } } => {
            if !view.panels_ready || ui_revision != view.ui_revision {
                return Err("UI changed; request controls again before activating".into());
            }
            let (_, _, action) = controls(view).into_iter().find(|(i, _, _)| *i == id).ok_or("unknown control; request controls")?;
            dispatch(view, orbit, action);
        }
        Request::Camera { focus, radius, yaw, pitch } => {
            if !focus.iter().chain([radius, yaw, pitch].iter()).all(|x| x.is_finite()) || radius <= 0. || pitch.abs() > 1.5 {
                return Err("finite camera required; radius > 0 and pitch within ±1.5 radians".into());
            }
            *orbit = RobotOrbit { focus: Vec3::from_array(focus), radius, yaw, pitch, home: false, ..*orbit };
        }
        Request::Fit => orbit.home = true,
    }
    Ok(view.state_json())
}

/// The loopback REST server for robot mode.
pub fn server(port: u16) -> std::io::Result<sim_api::Server> {
    sim_api::Server::bind(port, "robot", capabilities())
}
fn capabilities() -> Vec<Value> {
    use sim_api::capability as c;
    vec![
        c("robot_state", json!({}), "Read-only robot mode: file, status (loading | loaded | error, with the error naming the path), link_count, links [{index, name, has_mesh, triangles}], the selected link (index, name, mass, com, material exactly as stored) and the pose convention. Nothing is stepped or written."),
        c("system_ui", json!({"action":{"operation":"controls"}}), "Discover the link list and view controls (controls) and activate one by id with the current ui_revision (activate), through the same handler as a click. Selection is shared by the list, the 3D view and robot_state."),
        c("screenshot", json!({"path":"/tmp/view.png"}), "Save the window exactly as drawn to a PNG after the next frame"),
        c("camera", json!({"focus":[0,0,0],"radius":0.5,"yaw":0.7,"pitch":0.4}), "Absolute orbit in the display frame (Y up); SI metres and radians"),
        c("fit", json!({}), "Fit the robot"),
    ]
}

#[derive(Component, Clone, Copy)]
struct RobotOrbit {
    focus: Vec3,
    radius: f32,
    yaw: f32,
    pitch: f32,
    home: bool,
    extent: f32,
}
#[derive(Component)]
struct RobotRoot;
#[derive(Component)]
struct LinkMesh(usize);
#[derive(Component)]
struct LinkRow(usize);
#[derive(Component)]
struct StatusText;
#[derive(Component)]
struct Inspector;
#[derive(Component)]
struct ListRoot;
#[derive(Resource)]
struct Materials {
    normal: Handle<StandardMaterial>,
    selected: Handle<StandardMaterial>,
}

/// The window: worker load, posed link meshes, link list, inspector and REST.
pub fn run_robot(view: RobotView, api: sim_api::Server) {
    App::new()
        .insert_resource(crate::rest::Rest(api, None))
        .insert_resource(view)
        .insert_resource(ClearColor(Color::srgb(0.10, 0.125, 0.155)))
        .insert_resource(AmbientLight { color: Color::srgb(0.85, 0.90, 1.0), brightness: 420.0, affects_lightmapped_meshes: true })
        .insert_resource(MeshPickingSettings { require_markers: true, ..default() })
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::reactive(std::time::Duration::from_secs_f64(1.0 / 60.0)),
            unfocused_mode: UpdateMode::reactive_low_power(std::time::Duration::from_millis(40)),
        })
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Systems — Robot (read-only)".into(),
                resolution: (1500.0_f32, 940.0_f32).into(),
                resize_constraints: bevy::window::WindowResizeConstraints { min_width: 980.0, min_height: 720.0, ..default() },
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, ((crate::builder::ui::load_fonts, setup).chain(), crate::rest::wake_on_request))
        .add_systems(Update, (receive, poll_rest, buttons, orbit, viewport, highlight, panels, draw).chain())
        .run();
}

fn setup(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>, view: Res<RobotView>, fonts: Res<UiFonts>) {
    let text = |value: &str, size: f32, color: Color| label(&fonts, value, size, color);
    commands.insert_resource(Materials {
        normal: materials.add(StandardMaterial { base_color: Color::srgb(0.62, 0.68, 0.76), perceptual_roughness: 0.7, metallic: 0.05, cull_mode: None, ..default() }),
        selected: materials.add(StandardMaterial { base_color: Color::srgb(0.98, 0.62, 0.22), emissive: LinearRgba::rgb(0.35, 0.16, 0.02), perceptual_roughness: 0.6, cull_mode: None, ..default() }),
    });
    commands.spawn((
        Camera3d::default(),
        MeshPickingCamera,
        Tonemapping::None,
        Transform::from_xyz(0.5, 0.6, 0.8).looking_at(Vec3::ZERO, Vec3::Y),
        RobotOrbit { focus: Vec3::ZERO, radius: 1.0, yaw: 0.7, pitch: 0.45, home: false, extent: 0.3 },
    ));
    // UI over the whole window; the 3D camera only draws the middle viewport.
    commands.spawn((Camera2d, Camera { order: 3, clear_color: ClearColorConfig::None, ..default() }, IsDefaultUiCamera));
    commands.spawn((DirectionalLight { illuminance: 9000.0, shadows_enabled: false, ..default() }, Transform::from_xyz(1.0, 2.0, 1.5).looking_at(Vec3::ZERO, Vec3::Y)));
    // Z-up model frame shown in Bevy's Y-up frame (as sim-app does).
    commands.spawn((Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)), Visibility::default(), RobotRoot));
    let file = view.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    commands.spawn((
        Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(0.0), top: Val::Px(0.0), height: Val::Px(TOP), padding: UiRect::axes(Val::Px(18.0), Val::Px(8.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), ..default() },
        BackgroundColor(PANEL),
        children![text(&format!("Robot — {file}  ·  read-only, not stepped"), 18.0, INK), (text("Loading…", 13.0, MUTED), StatusText)],
    ));
    commands.spawn((
        Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(TOP), bottom: Val::Px(0.0), width: Val::Px(LEFT), padding: UiRect::all(Val::Px(14.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), overflow: Overflow::clip_y(), ..default() },
        BackgroundColor(PANEL),
        ListRoot,
        children![text("Links", 15.0, INK)],
    ));
    commands.spawn((
        Node { position_type: PositionType::Absolute, right: Val::Px(0.0), top: Val::Px(TOP), bottom: Val::Px(0.0), width: Val::Px(RIGHT), padding: UiRect::all(Val::Px(16.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), ..default() },
        BackgroundColor(PANEL),
        children![text("Link", 15.0, INK), (text("Select a link in the list or the 3D view.", 13.0, MUTED), Inspector)],
    ));
}

fn label(fonts: &UiFonts, value: &str, size: f32, color: Color) -> (Text, TextFont, TextColor, TextLayout) {
    (Text::new(value), TextFont { font: fonts.regular.clone(), font_size: size, ..default() }, TextColor(color), TextLayout::new_with_linebreak(bevy::text::LineBreak::WordOrCharacter))
}

/// Takes the worker's result; spawns meshes and the link list on success.
fn receive(
    mut commands: Commands,
    mut view: ResMut<RobotView>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<Materials>,
    root: Single<Entity, With<RobotRoot>>,
    list: Single<Entity, With<ListRoot>>,
    mut orbit: Single<&mut RobotOrbit>,
    mut redraw: EventWriter<bevy::window::RequestRedraw>,
    fonts: Res<UiFonts>,
) {
    let result = match view.rx.as_ref().map(|rx| rx.lock().unwrap_or_else(|p| p.into_inner()).try_recv()) {
        Some(Ok(result)) => result,
        Some(Err(mpsc::TryRecvError::Empty)) => {
            redraw.write(bevy::window::RequestRedraw);
            return;
        }
        Some(Err(mpsc::TryRecvError::Disconnected)) => Err(format!("{}: the loader stopped without a result", view.path.display())),
        None => return,
    };
    view.rx = None;
    let started = match view.status {
        Status::Loading(t) => t,
        _ => std::time::Instant::now(),
    };
    let loaded = match result {
        Ok(l) => l,
        Err(e) => {
            view.status = Status::Error(e);
            return;
        }
    };
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    let to_display = |p: Vec3| Vec3::new(p.x, p.z, -p.y);
    for (i, (link, geometry)) in loaded.model.links.iter().zip(&loaded.geometry).enumerate() {
        let com = Vec3::new(link.com[0] as f32, link.com[1] as f32, link.com[2] as f32);
        let Some(g) = geometry else { continue };
        for p in &g.positions {
            let w = to_display(com + Vec3::from_array(*p));
            lo = lo.min(w);
            hi = hi.max(w);
        }
        let count = g.positions.len() as u32;
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, g.positions.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, g.normals.clone());
        mesh.insert_indices(Indices::U32((0..count).collect()));
        let entity = commands
            .spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(materials.normal.clone()), Transform::from_translation(com), Visibility::default(), LinkMesh(i), Pickable::default()))
            .observe(pick_link)
            .id();
        commands.entity(*root).add_child(entity);
    }
    if lo.x.is_finite() {
        orbit.focus = (lo + hi) / 2.0;
        orbit.extent = ((hi - lo).length() / 2.0).max(0.02);
    }
    orbit.home = true;
    view.triangles = loaded.geometry.iter().map(|g| g.as_ref().map_or(0, |g| g.triangles())).collect();
    let rows: Vec<Entity> = loaded
        .model
        .links
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let name = if view.triangles[i] > 0 { l.name.clone() } else { format!("{}  (no collision geometry)", l.name) };
            commands
                .spawn((
                    Button,
                    RobotAction::SelectLink { index: i, name: l.name.clone() },
                    LinkRow(i),
                    Node { padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)), flex_shrink: 0.0, ..default() },
                    BorderRadius::all(Val::Px(4.0)),
                    BackgroundColor(Color::NONE),
                    children![label(&fonts, &name, 13.0, INK)],
                ))
                .id()
        })
        .collect();
    commands.entity(*list).add_children(&rows);
    view.model = Some(loaded.model);
    view.status = Status::Loaded { seconds: started.elapsed().as_secs_f64() };
    view.ui_revision += 1;
    view.panels_ready = true;
}

fn pick_link(click: Trigger<Pointer<Click>>, links: Query<&LinkMesh>, mut view: ResMut<RobotView>, mut orbit: Single<&mut RobotOrbit>) {
    if click.button != bevy::picking::pointer::PointerButton::Primary {
        return;
    }
    if let Ok(link) = links.get(click.target()) {
        let name = view.link_name(link.0).unwrap_or_default().to_string();
        dispatch(&mut view, &mut orbit, RobotAction::SelectLink { index: link.0, name });
    }
}

fn buttons(clicks: Query<(&Interaction, &RobotAction), Changed<Interaction>>, mut view: ResMut<RobotView>, mut orbit: Single<&mut RobotOrbit>) {
    for (interaction, action) in &clicks {
        if *interaction == Interaction::Pressed {
            dispatch(&mut view, &mut orbit, action.clone());
        }
    }
}

fn poll_rest(mut commands: Commands, mut redraw: EventWriter<bevy::window::RequestRedraw>, mut rest: ResMut<crate::rest::Rest>, mut view: ResMut<RobotView>, mut orbit: Single<&mut RobotOrbit>) {
    let server = &mut rest.0;
    let mut shots = Vec::new();
    server.poll(|command, _, _| {
        if command.command == "screenshot" {
            let path = command.args.get("path").and_then(|p| p.as_str()).map(PathBuf::from);
            return sim_api::Outcome::Done(match path.filter(|p| p.extension().is_some_and(|e| e == "png")) {
                Some(p) => {
                    shots.push(p.clone());
                    Ok(json!({"path": p, "note": "saved once the next frame renders"}))
                }
                None => Err("screenshot needs {\"path\": \"…/file.png\"}".into()),
            });
        }
        sim_api::Outcome::Done(execute(&mut view, &mut orbit, command))
    });
    if server.snapshot_due() {
        server.publish("robot_state", view.state_json());
    }
    if server.busy() || !shots.is_empty() {
        redraw.write(bevy::window::RequestRedraw);
    }
    for path in shots {
        use bevy::render::view::screenshot::{Screenshot, save_to_disk};
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
}

fn orbit(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut motion: EventReader<MouseMotion>,
    mut wheel: EventReader<MouseWheel>,
    window: Single<&Window>,
    camera: Single<(&mut Transform, &mut RobotOrbit)>,
) {
    let drag = motion.read().fold(Vec2::ZERO, |sum, e| sum + e.delta);
    let zoom = wheel.read().fold(0.0, |sum, e| sum + match e.unit {
        MouseScrollUnit::Line => e.y,
        MouseScrollUnit::Pixel => e.y * 0.02,
    });
    let (mut transform, mut orbit) = camera.into_inner();
    if orbit.home {
        orbit.radius = orbit.extent * 3.2;
        orbit.home = false;
    }
    let in_scene = window.cursor_position().is_some_and(|p| p.x > LEFT && p.x < window.width() - RIGHT && p.y > TOP);
    if in_scene {
        let pan = buttons.pressed(MouseButton::Middle) || (buttons.pressed(MouseButton::Right) && (keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight)));
        if pan {
            let shift = (transform.right() * -drag.x + transform.up() * drag.y) * orbit.radius * 0.0015;
            orbit.focus += shift;
        } else if buttons.pressed(MouseButton::Right) {
            orbit.yaw -= drag.x * 0.007;
            orbit.pitch = (orbit.pitch + drag.y * 0.007).clamp(-1.4, 1.4);
        }
        orbit.radius = (orbit.radius * (-zoom * 0.12).exp()).clamp(orbit.extent * 0.3, orbit.extent * 20.0);
    }
    let horizontal = orbit.pitch.cos() * orbit.radius;
    let eye = orbit.focus + Vec3::new(orbit.yaw.sin() * horizontal, orbit.pitch.sin() * orbit.radius, orbit.yaw.cos() * horizontal);
    let target = Transform::from_translation(eye).looking_at(orbit.focus, Vec3::Y);
    if *transform != target {
        *transform = target;
    }
}

fn viewport(window: Single<&Window>, mut camera: Single<&mut Camera, With<RobotOrbit>>) {
    let scale = window.scale_factor();
    let width = (window.width() - LEFT - RIGHT).max(1.0);
    let height = (window.height() - TOP).max(1.0);
    let viewport = Viewport { physical_position: UVec2::new((LEFT * scale) as u32, (TOP * scale) as u32), physical_size: UVec2::new((width * scale) as u32, (height * scale) as u32), ..default() };
    if camera.viewport.as_ref().is_none_or(|old| old.physical_size != viewport.physical_size || old.physical_position != viewport.physical_position) {
        camera.viewport = Some(viewport);
    }
}

/// The one selection, shown in 3D and in the list.
fn highlight(view: Res<RobotView>, materials: Res<Materials>, mut meshes: Query<(&LinkMesh, &mut MeshMaterial3d<StandardMaterial>)>, mut rows: Query<(&LinkRow, &Interaction, &mut BackgroundColor)>) {
    for (link, mut material) in &mut meshes {
        let want = if view.selected == Some(link.0) { &materials.selected } else { &materials.normal };
        if material.0 != *want {
            material.0 = want.clone();
        }
    }
    for (row, interaction, mut background) in &mut rows {
        let color = if view.selected == Some(row.0) {
            ACCENT.with_alpha(0.28)
        } else if *interaction == Interaction::Hovered {
            Color::srgb(0.13, 0.17, 0.21)
        } else {
            Color::NONE
        };
        if background.0 != color {
            background.0 = color;
        }
    }
}

/// Status line and a minimal inspector (the full physical inspector is T9.2).
fn panels(view: Res<RobotView>, mut status: Single<&mut Text, (With<StatusText>, Without<Inspector>)>, mut inspector: Single<&mut Text, (With<Inspector>, Without<StatusText>)>) {
    let line = match (&view.status, &view.model) {
        (Status::Loading(t), _) => format!("Loading {} on a worker thread… {:.1} s", view.path.display(), t.elapsed().as_secs_f64()),
        (Status::Error(e), _) => format!("Could not open the robot: {e}"),
        (Status::Loaded { seconds }, Some(m)) => {
            let without = view.triangles.iter().filter(|t| **t == 0).count();
            let missing = if without > 0 { format!(" · {without} without collision geometry (listed, not drawn)") } else { String::new() };
            format!("{} · {} links{missing} · loaded in {seconds:.2} s · {}", view.path.display(), m.links.len(), "exported assembly pose")
        }
        (Status::Loaded { .. }, None) => String::new(),
    };
    if status.0 != line {
        status.0 = line;
    }
    let body = match view.selected.and_then(|i| Some((i, view.model.as_ref()?.links.get(i)?))) {
        Some((i, l)) => {
            let geometry = match view.triangles.get(i).copied().unwrap_or(0) {
                0 => "no collision geometry (not drawn)".to_string(),
                n => format!("{n} collision triangles"),
            };
            let material = if l.material.is_empty() { "(none)" } else { l.material.as_str() };
            format!("{}\n\nmass: {:?} kg\ncom: [{:?}, {:?}, {:?}] m\nmaterial: {material}\n{geometry}", l.name, l.mass, l.com[0], l.com[1], l.com[2])
        }
        None => "Select a link in the list or the 3D view.".to_string(),
    };
    if inspector.0 != body {
        inspector.0 = body;
    }
}

/// Floor grid and the selected link's centre of mass.
fn draw(view: Res<RobotView>, mut gizmos: Gizmos) {
    let Some(model) = &view.model else { return };
    let floor = model.world.floor_z as f32;
    gizmos.grid(Isometry3d::new(Vec3::new(0.0, floor, 0.0), Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)), UVec2::splat(20), Vec2::splat(0.05), Color::srgba(0.45, 0.50, 0.58, 0.35));
    if let Some(l) = view.selected.and_then(|i| model.links.get(i)) {
        let com = Vec3::new(l.com[0] as f32, l.com[2] as f32, -l.com[1] as f32);
        gizmos.sphere(Isometry3d::from_translation(com), 0.006, ACCENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn robot_mode_loads_wheeled_baseline_and_names_bad_paths() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let loaded = load(&root.join("examples/wheeled-robot/baseline/robot.simrobot.json")).unwrap();
        assert_eq!(loaded.model.links.len(), 4);
        assert_eq!(loaded.geometry.len(), 4);
        assert!(loaded.geometry.iter().all(|g| g.as_ref().is_some_and(|g| g.triangles() > 0)));
        let missing = root.join("examples/wheeled-robot/baseline/no-such.simrobot.json");
        let err = load(&missing).err().unwrap();
        assert!(err.contains(&*missing.to_string_lossy()), "{err}");
    }
}

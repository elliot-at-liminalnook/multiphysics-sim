//! Walkthrough of a scanned place (`sim-spatial --place DIR`): the fused,
//! coloured mesh from `sim-place build`, with photo and station markers and
//! fly-through controls. Presentation only; the place model is not changed.
//!
//! Controls: W/A/S/D move, Q/E down/up, Shift faster, right-drag (or
//! left-drag) to look around, wheel to change speed, P to toggle photo
//! markers, 1–9 to jump to a scan station's view, H to toggle help.
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use crate::app::actions::{self, Act, InFlight, Replies, Spec, spec};
use crate::app::{ModeScope, ViewerMode, ViewerSet};
use crate::camera::fly::{Fly, angles_toward, orientation};
use bevy::ecs::message::Messages;
use serde::Deserialize;
use sim_api::Outcome;
use serde_json::json;
use std::path::{Path, PathBuf};

struct PlaceInfo {
    stations: Vec<Vec3>,
    views: Vec<(Vec3, Vec3)>,
    start: Vec3,
    description: String,
}

#[derive(Component)]
struct PhotoMarker;
#[derive(Component)]
struct Help;

/// Place frame (+Z up) to viewer frame (+Y up).
fn to_view(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[2] as f32, -p[1] as f32)
}

/// Read the binary PLY that `sim-place build` writes (position, normal, colour, triangles).
fn read_mesh(path: &Path) -> Result<Mesh, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let end = bytes.windows(11).position(|w| w == b"end_header\n").ok_or("not a PLY file")? + 11;
    let header = std::str::from_utf8(&bytes[..end]).map_err(|e| e.to_string())?;
    let count = |element: &str| header.lines().find_map(|l| l.strip_prefix(&format!("element {element} ")).and_then(|n| n.trim().parse::<usize>().ok())).ok_or(format!("PLY has no {element} count"));
    let (nv, nf) = (count("vertex")?, count("face")?);
    let mut o = end;
    let f32_at = |o: usize| f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
    let (mut positions, mut normals, mut colors) = (Vec::with_capacity(nv), Vec::with_capacity(nv), Vec::with_capacity(nv));
    for _ in 0..nv {
        let v: Vec<f32> = (0..6).map(|k| f32_at(o + 4 * k)).collect();
        positions.push([v[0], v[2], -v[1]]);
        normals.push([v[3], v[5], -v[4]]);
        let c = &bytes[o + 24..o + 27];
        let lin = |x: u8| (x as f32 / 255.0).powf(2.2);
        colors.push([lin(c[0]), lin(c[1]), lin(c[2]), 1.0]);
        o += 27;
    }
    let mut indices = Vec::with_capacity(3 * nf);
    for _ in 0..nf {
        let n = bytes[o] as usize;
        o += 1;
        for k in 0..n {
            let i = i32::from_le_bytes([bytes[o + 4 * k], bytes[o + 4 * k + 1], bytes[o + 4 * k + 2], bytes[o + 4 * k + 3]]);
            if k < 3 {
                indices.push(i as u32);
            }
        }
        o += 4 * n;
    }
    Ok(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(indices)))
}

/// Parse `dir/place.json` and its mesh as place mode does, without a
/// window; a one-line summary or the error naming the path.
pub fn validate_place(dir: &std::path::Path) -> Result<String, String> {
    let (info, mesh) = load_place(dir)?;
    Ok(format!("Validated place {} with {} stations, {} views and {} mesh vertices.", dir.display(), info.stations.len(), info.views.len(), mesh.count_vertices()))
}

fn load_place(dir: &std::path::Path) -> Result<(PlaceInfo, Mesh), String> {
    let text = std::fs::read_to_string(dir.join("place.json")).map_err(|e| format!("{}: {e}", dir.join("place.json").display()))?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if v["schema"] != "sim.place/1" {
        return Err(format!("{}: not a sim.place/1 place", dir.display()));
    }
    let point = |p: &serde_json::Value| -> [f64; 3] { [p[0].as_f64().unwrap_or(0.0), p[1].as_f64().unwrap_or(0.0), p[2].as_f64().unwrap_or(0.0)] };
    let views: Vec<(Vec3, Vec3)> = v["views"].as_array().map(|a| a.iter().map(|w| {
        let r = &w["pose"]["rotation"];
        let forward = [r[0][2].as_f64().unwrap_or(0.0), r[1][2].as_f64().unwrap_or(0.0), r[2][2].as_f64().unwrap_or(1.0)];
        (to_view(point(&w["pose"]["center"])), to_view(forward))
    }).collect()).unwrap_or_default();
    let stations: Vec<Vec3> = v["stations"].as_array().map(|a| a.iter().map(|s| to_view(point(&s["placement"]["position"]))).collect()).unwrap_or_default();
    let eye_height = views.first().map(|w| w.0.y).unwrap_or(0.1);
    // Start above and behind the first station (outside the ring of photo markers), looking across the room.
    let start = stations.first().copied().unwrap_or(Vec3::ZERO) + Vec3::Y * (eye_height + 0.6) - Vec3::X * 0.8;
    let mesh = read_mesh(&dir.join(v["mesh_file"].as_str().unwrap_or("mesh.ply")))?;
    let info = PlaceInfo { stations, views, start, description: v["description"].as_str().unwrap_or("").to_string() };
    Ok((info, mesh))
}

/// A scanned place in place mode: the parsed `place.json`, its mesh until
/// the scene is spawned, and the walkthrough's state (REST `state`).
#[derive(Resource)]
pub struct PlaceView {
    pub(crate) dir: PathBuf,
    info: PlaceInfo,
    mesh: Option<Mesh>,
    /// The station last jumped to (keys 1–9, REST `camera {station}`).
    station: Option<usize>,
    help: bool,
    markers: bool,
}
impl PlaceView {
    /// Parse `dir/place.json` and its mesh (the launch reads it before the
    /// window opens; a switch to place mode reads it on a worker).
    pub fn open(dir: PathBuf) -> Result<Self, String> {
        let (info, mesh) = load_place(&dir)?;
        Ok(Self { dir, info, mesh: Some(mesh), station: None, help: true, markers: true })
    }
}

/// Place mode: the walkthrough's scene is spawned on entering the Place
/// scope; keys write its actions (Input), [`apply`] applies them and REST's
/// (Actions), flying is the camera module's (`camera::fly`, SimSync's
/// `CameraSet::Place`) and the REST snapshot is in Present. Its
/// continuous update while focused is set on entering place mode and
/// replaced on entering any other (`app::CorePlugin`).
pub struct PlacePlugin;
impl Plugin for PlacePlugin {
    fn build(&self, app: &mut App) {
        actions::register::<PlaceAction>(app);
        app.add_systems(OnEnter(ModeScope::Place), setup).add_systems(
            Update,
            (
                keys.in_set(crate::app::InputSet::Window).run_if(not(crate::ui_kit::text::typing)),
                apply.in_set(ViewerSet::Actions),
                publish.in_set(ViewerSet::Present),
                viewport.in_set(ViewerSet::Present),
            )
                .run_if(in_state(ViewerMode::Place)),
        );
    }
}

/// Every intent of place mode. REST keeps each command's JSON shape; keys
/// 1–9 are `camera {station}`; P and H are the skipped toggles. (Flying with
/// W/A/S/D/Q/E, dragging and the wheel are continuous camera motion, not
/// actions: `camera::fly`.)
#[derive(Deserialize, Clone)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum PlaceAction {
    /// How Place mode works, for an agent starting cold (`place_guide`).
    PlaceGuide {
        #[serde(default)]
        topic: Option<String>,
    },
    State,
    Camera {
        #[serde(default)]
        position: Option<[f32; 3]>,
        #[serde(default)]
        yaw: Option<f32>,
        #[serde(default)]
        pitch: Option<f32>,
        #[serde(default)]
        speed: Option<f32>,
        #[serde(default)]
        station: Option<usize>,
        /// A photo's viewpoint: index into `state.photo_views`.
        #[serde(default)]
        view: Option<usize>,
    },
    /// Key P: the photo and station markers.
    #[serde(skip)]
    ToggleMarkers,
    /// Key H: the help line.
    #[serde(skip)]
    ToggleHelp,
}

impl actions::Action for PlaceAction {
    fn commands() -> Vec<Spec> {
        vec![
            spec("place_guide", actions::PLACE, json!({}), "Start here: how Place mode works (concepts, workflows in order, every command with an example, rules) for an agent starting cold; topic narrows it (about, how_to_call, concepts, workflows, commands, rules). Also GET /v1/place_guide."),
            spec("state", actions::PLACE, json!({}), "Place mode: dir, description, stations (positions in the viewer frame: metres, Y up; the place's +Z up maps to +Y), views (photo count), photo_views (each photo's position and unit viewing direction, in the viewer frame), station (the station last jumped to with keys 1-9 or camera {station}, null before any), camera {position, yaw, pitch (radians), speed (m/s)}, help_visible, markers_visible and viewer_mode. Presentation only: the place model is not changed."),
            spec("camera", actions::PLACE, json!({"position":[0.5,0.9,0.2],"yaw":0.6,"pitch":-0.25}), "Place mode: set the fly camera, any subset of position [x, y, z] (viewer frame, metres), yaw and pitch (radians; pitch within ±1.5), speed (m/s, 0.05-8), station (index into state.stations: the same jump as keys 1-9) or view (index into state.photo_views: stand where that photo was taken, looking where it looked); a station or view jump is applied first, then position, yaw and pitch override it. Yaw 0 looks along +X, positive yaw turns toward -Z; pitch tilts the view up (positive) or down. Refused naming the field: a non-finite value, pitch or speed out of range, an unknown station or view, or both a station and a view. Returns state."),
        ]
    }
}

fn state(view: &PlaceView, t: &Transform, fly: &Fly) -> serde_json::Value {
    json!({
        "dir": view.dir, "description": view.info.description,
        "stations": view.info.stations.iter().map(|s| s.to_array()).collect::<Vec<_>>(),
        "views": view.info.views.len(), "station": view.station,
        "photo_views": view.info.views.iter().map(|(p, f)| json!({"position": p.to_array(), "direction": f.normalize_or_zero().to_array()})).collect::<Vec<_>>(),
        "camera": {"position": t.translation.to_array(), "yaw": fly.yaw, "pitch": fly.pitch, "speed": fly.speed},
        "help_visible": view.help, "markers_visible": view.markers,
        "frame": "viewer frame: metres, Y up (the place's +Z up maps to +Y)",
    })
}

/// The walkthrough's first heading: along +X (across the room), tilted
/// down a little; the camera starts with exactly this rotation, so the
/// first mouse look continues from what is shown.
const START: Fly = Fly { yaw: 0.0, pitch: -0.25, speed: 0.8 };

/// The camera over the station `index` (as keys 1-9 place it).
fn station_pose(info: &PlaceInfo, index: usize) -> Option<Vec3> {
    info.stations.get(index).map(|s| *s + Vec3::Y * info.start.y - Vec3::X * 0.8)
}

/// The one handler of place mode's actions; REST gets `state` back.
fn execute(view: &mut PlaceView, t: &mut Transform, fly: &mut Fly, markers: &mut Query<&mut Visibility, (With<PhotoMarker>, Without<Help>)>, help: &mut Query<&mut Visibility, With<Help>>, action: &PlaceAction) -> sim_api::Result {
    match action {
        PlaceAction::PlaceGuide { topic } => return crate::place_guide::guide(topic.as_deref()),
        PlaceAction::State => {}
        PlaceAction::Camera { position, yaw, pitch, speed, station, view: photo } => {
            let (position, yaw, pitch, speed, station, photo) = (*position, *yaw, *pitch, *speed, *station, *photo);
            if station.is_some() && photo.is_some() {
                return Err("camera: give a station or a view, not both".into());
            }
            let finite = |name: &str, v: Option<f32>| match v {
                Some(x) if !x.is_finite() => Err(format!("camera {name} must be finite")),
                _ => Ok(()),
            };
            finite("yaw", yaw)?;
            finite("pitch", pitch)?;
            finite("speed", speed)?;
            if position.is_some_and(|p| !p.iter().all(|x| x.is_finite())) {
                return Err("camera position must be finite".into());
            }
            if pitch.is_some_and(|p| p.abs() > 1.5) {
                return Err("camera pitch must be within ±1.5 radians".into());
            }
            if speed.is_some_and(|s| !(0.05..=8.0).contains(&s)) {
                return Err("camera speed must be within 0.05-8 m/s".into());
            }
            let jump = match station {
                Some(i) => Some(station_pose(&view.info, i).ok_or_else(|| format!("camera station {i}: no such station (the place has {})", view.info.stations.len()))?),
                None => None,
            };
            // A photo's viewpoint: its position, looking along its direction.
            let shot = match photo {
                Some(i) => {
                    let (p, f) = *view.info.views.get(i).ok_or_else(|| format!("camera view {i}: no such photo view (the place has {})", view.info.views.len()))?;
                    Some((p, angles_toward(f).ok_or_else(|| format!("camera view {i}: the photo has no viewing direction"))?))
                }
                None => None,
            };
            if let Some(p) = jump {
                t.translation = p;
                view.station = station;
            }
            if let Some((p, (y, pi))) = shot {
                t.translation = p;
                fly.yaw = y;
                fly.pitch = pi;
            }
            if let Some(p) = position {
                t.translation = Vec3::from_array(p);
            }
            fly.yaw = yaw.unwrap_or(fly.yaw);
            fly.pitch = pitch.unwrap_or(fly.pitch);
            fly.speed = speed.unwrap_or(fly.speed);
            t.rotation = orientation(fly);
        }
        PlaceAction::ToggleMarkers => {
            view.markers = !view.markers;
            for mut v in markers.iter_mut() {
                *v = if view.markers { Visibility::Inherited } else { Visibility::Hidden };
            }
        }
        PlaceAction::ToggleHelp => {
            view.help = !view.help;
            for mut v in help.iter_mut() {
                *v = if view.help { Visibility::Inherited } else { Visibility::Hidden };
            }
        }
    }
    Ok(state(view, t, fly))
}

/// Actions: place mode's one apply system (keys and REST; a key's refusal,
/// such as a station the place does not have, is dropped as before).
fn apply(
    mut messages: ResMut<Messages<Act<PlaceAction>>>,
    mut in_flight: ResMut<InFlight<PlaceAction>>,
    mut replies: ResMut<Replies>,
    view: Option<ResMut<PlaceView>>,
    camera: Option<Single<(&mut Transform, &mut Fly)>>,
    mut markers: Query<&mut Visibility, (With<PhotoMarker>, Without<Help>)>,
    mut help: Query<&mut Visibility, With<Help>>,
) {
    let (Some(mut view), Some(camera)) = (view, camera) else {
        // The guide answers without a place: it is how an agent learns to open one.
        actions::apply(&mut messages, &mut in_flight, &mut replies, |action, _| match action {
            PlaceAction::PlaceGuide { topic } => Outcome::Done(crate::place_guide::guide(topic.as_deref())),
            _ => Outcome::Done(Err("the place is not open".into())),
        });
        return;
    };
    let (mut t, mut fly) = camera.into_inner();
    actions::apply(&mut messages, &mut in_flight, &mut replies, |action, _| Outcome::Done(execute(&mut view, &mut t, &mut fly, &mut markers, &mut help, action)));
}

/// Input: keys 1–9 jump to a station (`camera {station}`), P and H toggle;
/// not while a kit text field has the keyboard (`ui_kit::text::typing`).
fn keys(keys: Res<ButtonInput<KeyCode>>, mut out: MessageWriter<Act<PlaceAction>>) {
    let digits = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9];
    for (i, k) in digits.iter().enumerate() {
        if keys.just_pressed(*k) {
            out.write(Act::ui(PlaceAction::Camera { position: None, yaw: None, pitch: None, speed: None, station: Some(i), view: None }));
        }
    }
    if keys.just_pressed(KeyCode::KeyP) {
        out.write(Act::ui(PlaceAction::ToggleMarkers));
    }
    if keys.just_pressed(KeyCode::KeyH) {
        out.write(Act::ui(PlaceAction::ToggleHelp));
    }
}

/// Present: the walkthrough draws above the switcher strip
/// (`ui_kit::SWITCHER_STRIP`), as every mode's 3D view ends where its
/// docks do; written only on a change (`Viewport` has no `PartialEq`).
fn viewport(window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>, mut camera: Query<&mut Camera, With<Fly>>) {
    let Some(window) = window else { return };
    let Ok(mut camera) = camera.single_mut() else { return };
    let size = window.physical_size();
    let strip = (crate::ui_kit::SWITCHER_STRIP * window.scale_factor()).round() as u32;
    let wanted = (size.x >= 1 && size.y > strip + 1).then(|| bevy::camera::Viewport { physical_position: UVec2::ZERO, physical_size: UVec2::new(size.x, size.y - strip), ..default() });
    let same = match (&camera.viewport, &wanted) {
        (Some(a), Some(b)) => a.physical_position == b.physical_position && a.physical_size == b.physical_size,
        (None, None) => true,
        _ => false,
    };
    if !same {
        camera.viewport = wanted;
    }
}

/// Present: `/v1/state`, at most every 100 ms.
fn publish(rest: Option<ResMut<crate::rest::Rest>>, view: Res<PlaceView>, camera: Single<(&Transform, &Fly)>) {
    let Some(mut rest) = rest else { return };
    let (t, fly) = *camera;
    if rest.0.snapshot_due() {
        let mut shown = state(&view, t, fly);
        shown["viewer_mode"] = json!(ViewerMode::Place.name());
        rest.0.publish("state", shown);
        rest.0.publish_changed("place_guide", "1", || crate::place_guide::guide(None).unwrap_or_default());
    }
}

fn setup(mut commands: Commands, fonts: Res<crate::ui_kit::UiFonts>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, mut view: ResMut<PlaceView>, window: Option<Single<&mut Window, With<bevy::window::PrimaryWindow>>>) {
    if let Some(mut window) = window {
        window.title = format!("Place walkthrough — {}", view.dir.display());
    }
    if let Some(mesh) = view.mesh.take() {
        let material = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.95, double_sided: true, cull_mode: None, ..default() });
        commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(material)));
    }
    let info = &view.info;
    commands.spawn((DirectionalLight { illuminance: 2500.0, shadow_maps_enabled: false, ..default() }, Transform::from_xyz(1.0, 3.0, 2.0).looking_at(Vec3::ZERO, Vec3::Y)));
    let dot = meshes.add(Sphere::new(0.005));
    let red = materials.add(StandardMaterial { base_color: Color::srgb(0.95, 0.15, 0.2), unlit: true, ..default() });
    let blue = materials.add(StandardMaterial { base_color: Color::srgb(0.2, 0.4, 0.95), unlit: true, ..default() });
    let stick = meshes.add(Cuboid::new(0.002, 0.002, 0.03));
    for (p, f) in &info.views {
        commands.spawn((Mesh3d(dot.clone()), MeshMaterial3d(red.clone()), Transform::from_translation(*p), PhotoMarker));
        commands.spawn((Mesh3d(stick.clone()), MeshMaterial3d(red.clone()), Transform::from_translation(*p + *f * 0.015).looking_to(*f, Vec3::Y), PhotoMarker));
    }
    let post = meshes.add(Cylinder::new(0.02, 0.2));
    for s in &info.stations {
        commands.spawn((Mesh3d(post.clone()), MeshMaterial3d(blue.clone()), Transform::from_translation(*s + Vec3::Y * 0.1), PhotoMarker));
    }
    commands.spawn((Camera3d::default(), bevy::core_pipeline::tonemapping::Tonemapping::None, Projection::Perspective(PerspectiveProjection { fov: 75f32.to_radians(), near: 0.01, ..default() }), Transform::from_translation(info.start).with_rotation(orientation(&START)), START));
    // UI over the whole window (the help line, the switcher strip); the
    // walkthrough's camera draws above the strip (`viewport`).
    commands.spawn((Camera2d, Camera { order: 3, clear_color: ClearColorConfig::None, ..default() }, IsDefaultUiCamera));
    // The help line floats over the top-left of the walkthrough.
    let k = crate::ui_kit::Kit::new(&fonts);
    commands.spawn((
        k.text(format!("{}\nW/A/S/D move | Q/E down/up | Shift faster | drag to look | wheel: speed | 1-9: stations | P: photo markers | H: help", info.description), 14.0, crate::ui_kit::TEXT, 0),
        Node { position_type: PositionType::Absolute, left: Val::Px(12.0), top: Val::Px(10.0), ..default() },
        Help,
    ));
    // A view shown again keeps its toggles.
    let (help, markers) = (view.help, view.markers);
    if !help || !markers {
        commands.queue(move |world: &mut World| {
            let mut q = world.query::<(&mut Visibility, Has<Help>, Has<PhotoMarker>)>();
            for (mut v, is_help, is_marker) in q.iter_mut(world) {
                if (is_help && !help) || (is_marker && !markers) {
                    *v = Visibility::Hidden;
                }
            }
        });
    }
}

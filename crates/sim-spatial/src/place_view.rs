//! Walkthrough of a scanned place (`sim-spatial --place DIR`): the fused,
//! coloured mesh from `sim-place build`, with photo and station markers and
//! fly-through controls. Presentation only; the place model is not changed.
//!
//! Controls: W/A/S/D move, Q/E down/up, Shift faster, right-drag (or
//! left-drag) to look around, wheel to change speed, P to toggle photo
//! markers, 1–9 to jump to a scan station's view, H to toggle help.
use bevy::asset::RenderAssetUsages;
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::winit::{UpdateMode, WinitSettings};
use std::path::{Path, PathBuf};

#[derive(Resource)]
struct PlaceInfo {
    stations: Vec<Vec3>,
    views: Vec<(Vec3, Vec3)>,
    start: Vec3,
    look: Vec3,
    description: String,
}

#[derive(Component)]
struct Fly {
    yaw: f32,
    pitch: f32,
    speed: f32,
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

/// Parse `dir/place.json` and its mesh as [`run_place`] does, without a
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
    let info = PlaceInfo { stations, views, start, look: Vec3::X, description: v["description"].as_str().unwrap_or("").to_string() };
    Ok((info, mesh))
}

pub fn run_place(dir: PathBuf) -> Result<(), String> {
    let (info, mesh) = load_place(&dir)?;
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.07, 0.08, 0.1)))
        .insert_resource(AmbientLight { color: Color::WHITE, brightness: 900.0, affects_lightmapped_meshes: true })
        .insert_resource(WinitSettings { focused_mode: UpdateMode::Continuous, unfocused_mode: UpdateMode::reactive_low_power(std::time::Duration::from_millis(100)) })
        .insert_resource(info)
        .insert_resource(MeshSource(Some(mesh)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { title: format!("Place walkthrough — {}", dir.display()), resolution: (1440.0_f32, 900.0_f32).into(), ..default() }),
            ..default()
        }))
        .add_systems(Startup, setup)
        .add_systems(Update, (fly, toggles))
        .run();
    Ok(())
}

#[derive(Resource)]
struct MeshSource(Option<Mesh>);

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, mut source: ResMut<MeshSource>, info: Res<PlaceInfo>) {
    if let Some(mesh) = source.0.take() {
        let material = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.95, double_sided: true, cull_mode: None, ..default() });
        commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(material)));
    }
    commands.spawn((DirectionalLight { illuminance: 2500.0, shadows_enabled: false, ..default() }, Transform::from_xyz(1.0, 3.0, 2.0).looking_at(Vec3::ZERO, Vec3::Y)));
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
    commands.spawn((Camera3d::default(), bevy::core_pipeline::tonemapping::Tonemapping::None, Projection::Perspective(PerspectiveProjection { fov: 75f32.to_radians(), near: 0.01, ..default() }), Transform::from_translation(info.start).looking_to(info.look, Vec3::Y), Fly { yaw: 0.0, pitch: -0.25, speed: 0.8 }));
    commands.spawn((
        Text::new(format!("{}\nW/A/S/D move | Q/E down/up | Shift faster | drag to look | wheel: speed | 1-9: stations | P: photo markers | H: help", info.description)),
        TextFont { font_size: 14.0, ..default() },
        TextColor(Color::srgb(0.9, 0.92, 0.95)),
        Node { position_type: PositionType::Absolute, left: Val::Px(12.0), top: Val::Px(10.0), ..default() },
        Help,
    ));
}

fn fly(time: Res<Time>, keys: Res<ButtonInput<KeyCode>>, buttons: Res<ButtonInput<MouseButton>>, mut motion: EventReader<MouseMotion>, mut wheel: EventReader<MouseWheel>, info: Res<PlaceInfo>, mut q: Query<(&mut Transform, &mut Fly)>) {
    let Ok((mut t, mut fly)) = q.single_mut() else { return };
    if buttons.pressed(MouseButton::Right) || buttons.pressed(MouseButton::Left) {
        for m in motion.read() {
            fly.yaw -= m.delta.x * 0.004;
            fly.pitch = (fly.pitch - m.delta.y * 0.004).clamp(-1.5, 1.5);
        }
    } else {
        motion.clear();
    }
    for w in wheel.read() {
        fly.speed = (fly.speed * if w.y > 0.0 { 1.15 } else { 1.0 / 1.15 }).clamp(0.05, 8.0);
    }
    let digits = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9];
    for (i, k) in digits.iter().enumerate() {
        if keys.just_pressed(*k) {
            if let Some(s) = info.stations.get(i) {
                t.translation = *s + Vec3::Y * info.start.y - Vec3::X * 0.8;
            }
        }
    }
    t.rotation = Quat::from_euler(EulerRot::YXZ, fly.yaw, fly.pitch, 0.0) * Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2);
    let (forward, right) = (*t.forward(), *t.right());
    let flat = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
    let mut d = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) { d += flat; }
    if keys.pressed(KeyCode::KeyS) { d -= flat; }
    if keys.pressed(KeyCode::KeyD) { d += right; }
    if keys.pressed(KeyCode::KeyA) { d -= right; }
    if keys.pressed(KeyCode::KeyE) { d += Vec3::Y; }
    if keys.pressed(KeyCode::KeyQ) { d -= Vec3::Y; }
    let boost = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) { 3.0 } else { 1.0 };
    t.translation += d.normalize_or_zero() * fly.speed * boost * time.delta_secs();
}

fn toggles(keys: Res<ButtonInput<KeyCode>>, mut markers: Query<&mut Visibility, (With<PhotoMarker>, Without<Help>)>, mut help: Query<&mut Visibility, With<Help>>) {
    if keys.just_pressed(KeyCode::KeyP) {
        for mut v in &mut markers {
            *v = if *v == Visibility::Hidden { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
    if keys.just_pressed(KeyCode::KeyH) {
        for mut v in &mut help {
            *v = if *v == Visibility::Hidden { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
}

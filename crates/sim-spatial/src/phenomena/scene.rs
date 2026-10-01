//! Phenomena mode's 3D view: sim-app's camera pose and shadowed light, the
//! UI camera over the whole window, one entity pool for the exhibit's
//! spheres, rods and blocks and gizmos for its lines, arrows and polylines
//! (phenomena_app.rs `render`, :238–323), drawn from the shown frame.
//!
//! The orbit follows this viewer's convention, not sim-app's: right-drag
//! rotates, middle-drag or Shift+right-drag pans, the wheel zooms, only for
//! gestures that start over the 3D area between the docks. sim-app orbited
//! on a left drag; here the left button is the panels' (list rows, buttons,
//! the knob slider), as in robot and CAD mode.
use super::gallery::Gallery;
use super::panel::CHART_HEIGHT;
use crate::app::ModeScope;
use crate::ui_kit::{LEFT_WIDTH, RIGHT_WIDTH, TOPBAR};
use bevy::camera::Viewport;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use sim_phenomena::exhibit::Shape;

/// The orbit camera's state (phenomena_app.rs `OrbitCamera`).
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct PhenomenaOrbit {
    focus: Vec3,
    radius: f32,
    yaw: f32,
    pitch: f32,
}

/// A pool entity: which mesh it shows (0 sphere, 1 rod, 2 block) and the
/// colour last written to its material.
#[derive(Component)]
pub(super) struct Pooled {
    kind: usize,
    color: Option<[f32; 3]>,
}

/// The pool's meshes and entities (each with its own material).
#[derive(Resource)]
pub(super) struct Pool {
    meshes: [Handle<Mesh>; 3],
    entities: Vec<(Entity, Handle<StandardMaterial>)>,
}

/// OnEnter(ModeScope::Phenomena): the 3D camera at sim-app's pose (focus
/// (0, 0.3, 0), eye (3, 2.6, 9)), the UI camera, sim-app's light and the
/// pool's meshes. `app::scope_new_entities` would scope these roots anyway;
/// the explicit `DespawnOnExit` shows the intent.
pub(super) fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    let focus = Vec3::new(0.0, 0.3, 0.0);
    let position = Vec3::new(3.0, 2.6, 9.0);
    let offset = position - focus;
    let radius = offset.length();
    commands.spawn((
        Camera3d::default(),
        // The viewer is built without `tonemapping_luts`: the default
        // TonyMcMapface would sample a placeholder table (as robot and place
        // mode, tone mapping is off).
        Tonemapping::None,
        Transform::from_translation(position).looking_at(focus, Vec3::Y),
        PhenomenaOrbit { focus, radius, yaw: offset.x.atan2(offset.z), pitch: (offset.y / radius).asin() },
        DespawnOnExit(ModeScope::Phenomena),
    ));
    // UI over the whole window, drawn after the 3D view.
    commands.spawn((Camera2d, Camera { order: 3, clear_color: ClearColorConfig::None, ..default() }, IsDefaultUiCamera, DespawnOnExit(ModeScope::Phenomena)));
    commands.spawn((
        DirectionalLight { illuminance: 9_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, -0.5, 0.0)),
        DespawnOnExit(ModeScope::Phenomena),
    ));
    commands.insert_resource(Pool { meshes: [meshes.add(Sphere::new(1.0)), meshes.add(Cylinder::new(1.0, 1.0)), meshes.add(Cuboid::new(2.0, 2.0, 2.0))], entities: Vec::new() });
}

/// SimSync: the 3D view draws between the docks (header, exhibit list,
/// inspector, chart strip).
pub(super) fn viewport(window: Option<Single<&Window, With<PrimaryWindow>>>, camera: Option<Single<&mut Camera, With<PhenomenaOrbit>>>) {
    let (Some(window), Some(mut camera)) = (window, camera) else { return };
    let want = scene_viewport(&window);
    let same = match (&camera.viewport, &want) {
        (Some(a), Some(b)) => a.physical_position == b.physical_position && a.physical_size == b.physical_size,
        (None, None) => true,
        _ => false,
    };
    if !same {
        camera.viewport = want;
    }
}

/// The 3D view between the docks; None for a window narrower than the
/// docks, where it draws over the whole window rather than outside it.
fn scene_viewport(window: &Window) -> Option<Viewport> {
    let scale = window.scale_factor();
    let width = (window.width() - LEFT_WIDTH - RIGHT_WIDTH).max(1.0);
    let height = (window.height() - TOPBAR - CHART_HEIGHT).max(1.0);
    let viewport = Viewport { physical_position: UVec2::new((LEFT_WIDTH * scale) as u32, (TOPBAR * scale) as u32), physical_size: UVec2::new((width * scale) as u32, (height * scale) as u32), ..default() };
    (viewport.physical_position + viewport.physical_size).cmple(window.physical_size()).all().then_some(viewport)
}

/// Whether the cursor is over the 3D area: between the docks, or anywhere
/// when the view covers the whole window.
fn over_scene(window: &Window) -> bool {
    let whole = scene_viewport(window).is_none();
    window.cursor_position().is_some_and(|p| whole || (p.x > LEFT_WIDTH && p.x < window.width() - RIGHT_WIDTH && p.y > TOPBAR && p.y < window.height() - CHART_HEIGHT))
}

/// SimSync: the orbit (navigation only; nothing in the exhibit changes).
/// sim-app's rates and limits: 0.008 rad per pixel, pitch within ±1.35,
/// radius within 3…30.
pub(super) fn orbit(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    camera: Option<Single<(&mut Transform, &mut PhenomenaOrbit)>>,
    mut dragging: Local<bool>,
) {
    let drag = motion.read().fold(Vec2::ZERO, |sum, e| sum + e.delta);
    let zoom = wheel.read().fold(0.0, |sum, e| {
        sum + match e.unit {
            MouseScrollUnit::Line => e.y,
            MouseScrollUnit::Pixel => e.y * 0.02,
        }
    });
    let (Some(window), Some(camera)) = (window, camera) else { return };
    let (mut transform, mut orbit) = camera.into_inner();
    let in_scene = over_scene(&window);
    // A drag that starts over the 3D view keeps going over a panel; one that starts over a panel is the panel's.
    if buttons.any_just_pressed([MouseButton::Right, MouseButton::Middle]) {
        *dragging = in_scene;
    }
    if !buttons.any_pressed([MouseButton::Right, MouseButton::Middle]) {
        *dragging = false;
    }
    if *dragging && drag != Vec2::ZERO {
        let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        if buttons.pressed(MouseButton::Middle) || (buttons.pressed(MouseButton::Right) && shift) {
            let pan = (transform.right() * -drag.x + transform.up() * drag.y) * orbit.radius * 0.0015;
            orbit.focus += pan;
        } else {
            orbit.yaw -= drag.x * 0.008;
            orbit.pitch = (orbit.pitch + drag.y * 0.008).clamp(-1.35, 1.35);
        }
    }
    if zoom != 0.0 && in_scene {
        orbit.radius = (orbit.radius * (-zoom * 0.12).exp()).clamp(3.0, 30.0);
    }
    let horizontal = orbit.pitch.cos() * orbit.radius;
    let eye = orbit.focus + Vec3::new(orbit.yaw.sin() * horizontal, orbit.pitch.sin() * orbit.radius, orbit.yaw.cos() * horizontal);
    let target = Transform::from_translation(eye).looking_at(orbit.focus, Vec3::Y);
    if *transform != target {
        *transform = target;
    }
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

fn color(c: [f32; 3]) -> Color {
    Color::srgb(c[0], c[1], c[2])
}

/// Present: the shown frame's shapes, exactly as sim-app's `render`: lines,
/// arrows and polylines as gizmos; spheres, rods and blocks on pooled
/// entities (grown as needed, extra ones hidden), each with its own
/// material, whose colour is written only when it changed. sim-app's gizmo
/// chart board behind the scene is the chart strip under the view now.
pub(super) fn render(
    mut commands: Commands,
    gallery: Option<Res<Gallery>>,
    pool: Option<ResMut<Pool>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut pooled: Query<(&mut Transform, &mut Visibility, &mut Mesh3d, &mut Pooled)>,
    mut gizmos: Gizmos,
) {
    let Some(mut pool) = pool else { return };
    let shapes: &[Shape] = match gallery.as_deref().and_then(Gallery::ready) {
        Some(frame) => &frame.shapes,
        None => &[],
    };
    let mut mesh_shapes = Vec::new();
    for shape in shapes {
        match shape {
            Shape::Line { from, to, color: c } => gizmos.line(v3(*from), v3(*to), color(*c)),
            Shape::Arrow { from, to, color: c } => {
                gizmos.arrow(v3(*from), v3(*to), color(*c));
            }
            Shape::Polyline { points, color: c } => gizmos.linestrip(points.iter().map(|p| v3(*p)), color(*c)),
            other => mesh_shapes.push(other),
        }
    }
    while pool.entities.len() < mesh_shapes.len() {
        let material = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.6, ..default() });
        let entity = commands.spawn((Mesh3d(pool.meshes[0].clone()), MeshMaterial3d(material.clone()), Transform::default(), Visibility::Hidden, Pooled { kind: 0, color: None }, DespawnOnExit(ModeScope::Phenomena))).id();
        pool.entities.push((entity, material));
    }
    for (index, (entity, material)) in pool.entities.iter().enumerate() {
        let Ok((mut transform, mut visibility, mut mesh, mut pooled)) = pooled.get_mut(*entity) else { continue };
        let Some(shape) = mesh_shapes.get(index) else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        visibility.set_if_neq(Visibility::Visible);
        let (kind, t, c) = match shape {
            Shape::Sphere { center, radius, color } => (0, Transform::from_translation(v3(*center)).with_scale(Vec3::splat(*radius as f32)), *color),
            Shape::Rod { from, to, radius, color } => {
                let (a, b) = (v3(*from), v3(*to));
                let d = b - a;
                let len = d.length().max(1.0e-6);
                (1, Transform::from_translation((a + b) * 0.5).with_rotation(Quat::from_rotation_arc(Vec3::Y, d / len)).with_scale(Vec3::new(*radius as f32, len, *radius as f32)), *color)
            }
            Shape::Block { center, half, rotation, color } => (
                2,
                // `rotation` is (w, x, y, z).
                Transform::from_translation(v3(*center)).with_rotation(Quat::from_xyzw(rotation[1] as f32, rotation[2] as f32, rotation[3] as f32, rotation[0] as f32)).with_scale(v3(*half)),
                *color,
            ),
            // Only spheres, rods and blocks were kept for the pool.
            Shape::Line { .. } | Shape::Arrow { .. } | Shape::Polyline { .. } => continue,
        };
        if pooled.kind != kind {
            pooled.kind = kind;
            mesh.0 = pool.meshes[kind].clone();
        }
        transform.set_if_neq(t);
        if pooled.color != Some(c) {
            if let Some(mut m) = materials.get_mut(material) {
                m.base_color = color(c);
            }
            pooled.color = Some(c);
        }
    }
}

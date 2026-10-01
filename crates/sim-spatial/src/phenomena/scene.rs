//! Phenomena mode's 3D view: sim-app's camera pose and shadowed light, the
//! UI camera over the whole window, one entity pool for the exhibit's
//! spheres, rods and blocks and gizmos for its lines, arrows and polylines
//! (phenomena_app.rs `render`, :238–323), drawn from the shown frame.
//!
//! The orbit is the shared camera (`crate::camera`) with sim-app's rates
//! and limits, in this viewer's convention, not sim-app's: right-drag
//! rotates, middle-drag or Shift+right-drag pans, the wheel zooms, only for
//! gestures that start over the 3D area between the docks. sim-app orbited
//! on a left drag; here the left button is the panels' (list rows, buttons,
//! the knob slider), as in robot and CAD mode.
use super::gallery::Gallery;
use super::panel::CHART_HEIGHT;
use crate::app::ModeScope;
use crate::ui_kit::{LEFT_WIDTH, RIGHT_WIDTH, SWITCHER_STRIP, TOPBAR};
use crate::camera::{Framing, Orbit, OrbitRules, Pose, RadiusLimits, ViewArea};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use sim_phenomena::exhibit::Shape;

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
    // sim-app's pose (phenomena_app.rs `OrbitCamera`), which is also the home view.
    let home = Pose { focus, radius, yaw: offset.x.atan2(offset.z), pitch: (offset.y / radius).asin() };
    let orbit = Orbit { focus, radius, yaw: home.yaw, pitch: home.pitch, ..default() };
    commands.spawn((
        Camera3d::default(),
        // The viewer is built without `tonemapping_luts`: the default
        // TonyMcMapface would sample a placeholder table (as robot and place
        // mode, tone mapping is off).
        Tonemapping::None,
        // Camera3d's required components insert the perspective Projection.
        orbit.transform(),
        orbit,
        // sim-app's rates and limits: 0.008 rad per pixel, pitch within
        // ±1.35, radius within 3…30 m; home is its camera pose.
        OrbitRules {
            rate: 0.008,
            pitch_limit: 1.35,
            radius: RadiusLimits::Absolute { min: 3.0, max: 30.0 },
            framing: Framing::Fixed(home),
            glide_home: false,
            zoom_to_cursor: false,
            yield_to_ui: false,
            keys: true,
            ..default()
        },
        // Between the header, exhibit list, inspector and chart strip (the
        // whole window when they leave no room).
        ViewArea::Docks { left: LEFT_WIDTH, right: RIGHT_WIDTH, top: TOPBAR, bottom: CHART_HEIGHT + SWITCHER_STRIP },
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

//! CAD mode's 3D view: the camera (mesh picking on), the UI camera over the
//! whole window, a light that follows the view, the Z-up millimetre root the
//! bodies hang from (`mesh`), an orbit (right-drag rotates, middle-drag or
//! shift+right-drag pans, the wheel zooms; the pointer over a panel is the
//! panel's) and the framing `cad_fit` and the first meshes ask for.
use super::mesh::{CadMaterials, CadMeshes, CadRoot, root_transform};
use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::picking::hover::HoverMap;
use bevy::picking::mesh_picking::MeshPickingCamera;
use bevy::picking::pointer::PointerId;
use bevy::camera::Viewport;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use crate::ui_kit::{LEFT_WIDTH, RIGHT_WIDTH, STATUSBAR, TOPBAR};

/// The orbit camera's state.
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct CadOrbit {
    pub(super) focus: Vec3,
    pub(super) radius: f32,
    pub(super) yaw: f32,
    pub(super) pitch: f32,
    /// Half the framed bounds' diagonal (zoom limits scale with it).
    pub(super) extent: f32,
}

/// OnEnter(ModeScope::Cad): cameras, light, root and display materials.
/// Every root entity is scoped to the mode by `app::scope_new_entities`.
pub(super) fn setup(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(CadMaterials::new(&mut materials));
    commands
        .spawn((
            Camera3d::default(),
            // Parts are millimetres shown in metres: the default 0.1 m near plane would cut them.
            Projection::Perspective(PerspectiveProjection { near: 0.001, near_clip_plane: Vec4::new(0.0, 0.0, -1.0, -0.001), ..default() }),
            MeshPickingCamera,
            Transform::from_xyz(0.3, 0.25, 0.3).looking_at(Vec3::ZERO, Vec3::Y),
            CadOrbit { focus: Vec3::ZERO, radius: 0.5, yaw: 0.7, pitch: 0.45, extent: 0.15 },
        ))
        // A headlight: the light follows the view, so every face the camera sees is lit.
        .with_children(|camera| {
            camera.spawn((DirectionalLight { illuminance: 6000.0, shadow_maps_enabled: false, ..default() }, Transform::from_xyz(0.2, 0.3, 0.0).looking_at(Vec3::new(0.0, 0.0, -1.0), Vec3::Y)));
        });
    // UI over the whole window, drawn after the 3D view.
    commands.spawn((Camera2d, Camera { order: 3, clear_color: ClearColorConfig::None, ..default() }, IsDefaultUiCamera));
    commands.spawn((root_transform(), Visibility::default(), CadRoot));
}

/// SimSync: the 3D view draws between the panel's docks (top bar, model
/// tree, inspector, status bar: `panel`'s layout tokens), so framing centres
/// the bodies where they can be seen.
pub(super) fn viewport(window: Option<Single<&Window, With<PrimaryWindow>>>, camera: Option<Single<&mut Camera, With<CadOrbit>>>) {
    let (Some(window), Some(mut camera)) = (window, camera) else { return };
    let scale = window.scale_factor();
    let width = (window.width() - LEFT_WIDTH - RIGHT_WIDTH).max(1.0);
    let height = (window.height() - TOPBAR - STATUSBAR).max(1.0);
    let viewport = Viewport { physical_position: UVec2::new((LEFT_WIDTH * scale) as u32, (TOPBAR * scale) as u32), physical_size: UVec2::new((width * scale) as u32, (height * scale) as u32), ..default() };
    // A window narrower than the docks: draw over the whole window rather than outside it.
    let fits = (viewport.physical_position + viewport.physical_size).cmple(window.physical_size()).all();
    let want = fits.then_some(viewport);
    let same = match (&camera.viewport, &want) {
        (Some(a), Some(b)) => a.physical_position == b.physical_position && a.physical_size == b.physical_size,
        (None, None) => true,
        _ => false,
    };
    if !same {
        camera.viewport = want;
    }
}

/// OnExit(ModeScope::Cad): the display materials (entities go by `DespawnOnExit`).
pub(super) fn teardown(mut commands: Commands) {
    commands.remove_resource::<CadMaterials>();
}

/// SimSync: apply a framing (`cad_fit`, the first meshes).
pub(super) fn fit(meshes: Option<ResMut<CadMeshes>>, orbit: Option<Single<&mut CadOrbit>>) {
    let (Some(mut meshes), Some(mut orbit)) = (meshes, orbit) else { return };
    let Some((centre, extent)) = meshes.fit.take() else { return };
    orbit.focus = centre;
    orbit.extent = extent;
    orbit.radius = extent * 3.2;
}

/// Whether the mouse is over a UI node (a panel): the pointer is the panel's then.
fn over_ui(hover: Option<&HoverMap>, nodes: &Query<(), With<Node>>) -> bool {
    hover.and_then(|h| h.get(&PointerId::Mouse)).is_some_and(|hits| hits.keys().any(|e| nodes.contains(*e)))
}

/// SimSync: the orbit (navigation only; nothing is selected or changed).
#[allow(clippy::too_many_arguments)]
pub(super) fn orbit(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    hover: Option<Res<HoverMap>>,
    nodes: Query<(), With<Node>>,
    camera: Option<Single<(&mut Transform, &mut CadOrbit)>>,
    mut dragging: Local<bool>,
) {
    let drag = motion.read().fold(Vec2::ZERO, |sum, e| sum + e.delta);
    let zoom = wheel.read().fold(0.0, |sum, e| {
        sum + match e.unit {
            MouseScrollUnit::Line => e.y,
            MouseScrollUnit::Pixel => e.y * 0.02,
        }
    });
    let Some(camera) = camera else { return };
    let (mut transform, mut orbit) = camera.into_inner();
    let held = buttons.pressed(MouseButton::Right) || buttons.pressed(MouseButton::Middle);
    // A drag that starts over the 3D view keeps going over a panel; one that starts over a panel is the panel's.
    let over_panel = over_ui(hover.as_deref(), &nodes);
    if buttons.any_just_pressed([MouseButton::Right, MouseButton::Middle]) {
        *dragging = !over_panel;
    }
    if !held {
        *dragging = false;
    }
    if *dragging {
        let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        if buttons.pressed(MouseButton::Middle) || (buttons.pressed(MouseButton::Right) && shift) {
            let pan = (transform.right() * -drag.x + transform.up() * drag.y) * orbit.radius * 0.0015;
            orbit.focus += pan;
        } else {
            orbit.yaw -= drag.x * 0.007;
            orbit.pitch = (orbit.pitch + drag.y * 0.007).clamp(-1.5, 1.5);
        }
    }
    if zoom != 0.0 && !over_panel {
        orbit.radius = (orbit.radius * (-zoom * 0.12).exp()).clamp(orbit.extent * 0.05, orbit.extent * 40.0);
    }
    let horizontal = orbit.pitch.cos() * orbit.radius;
    let eye = orbit.focus + Vec3::new(orbit.yaw.sin() * horizontal, orbit.pitch.sin() * orbit.radius, orbit.yaw.cos() * horizontal);
    let target = Transform::from_translation(eye).looking_at(orbit.focus, Vec3::Y);
    if *transform != target {
        *transform = target;
    }
}

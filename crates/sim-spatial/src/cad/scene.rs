//! CAD mode's 3D view: the camera (mesh picking on), the UI camera over the
//! whole window, a light that follows the view, the Z-up millimetre root the
//! bodies hang from (`mesh`) and the framing `cad_fit` and the first meshes
//! ask for.
//!
//! The camera is the shared one (`crate::camera`): it carries an
//! [`Orbit`], CAD's [`OrbitRules`] and a [`ViewArea`] between the panel's
//! docks, and the camera module does the navigation (right-drag orbits,
//! middle or Shift+right-drag pans, the wheel zooms toward the point under
//! the cursor; a gesture that starts over a panel is the panel's), the
//! viewport and the placing. CAD's own tools (selection, the gizmo, the
//! plane and sketch tools) use the left button only, so the rules stay
//! enabled while they work. Differences from CAD's former own orbit, on
//! purpose: the pitch limit is RoboCAD's 89.5° (was 1.5 rad ≈ 85.9°), and
//! the wheel zooms toward the cursor as RoboCAD's `Camera.zoom(factor,
//! anchor)` (was toward the focus).
use super::mesh::{CadMaterials, CadMeshes, CadRoot, root_transform};
use crate::camera::{Framing, Orbit, OrbitRules, RadiusLimits, ViewArea};
use crate::ui_kit::{LEFT_WIDTH, RIGHT_WIDTH, STATUSBAR, TOPBAR};
use bevy::picking::hover::HoverMap;
use bevy::picking::mesh_picking::MeshPickingCamera;
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;

/// CAD's camera feel: RoboCAD's (drag rates as before, its 89.5° pitch
/// limit, zoom 0.05–40 × the framed extent toward the cursor, a fit at 3.2 ×
/// the extent from the current heading, cut not glided). CAD binds its own
/// keymap (RoboCAD's command table), so the shared camera keys are off.
pub(super) fn rules() -> OrbitRules {
    OrbitRules {
        rate: 0.007,
        pan_rate: 0.0015,
        pitch_limit: 89.5f32.to_radians(),
        radius: RadiusLimits::Extent { min: 0.05, max: 40.0 },
        framing: Framing::Bounds { scale: 3.2, aspect: false, view: None },
        glide_home: false,
        zoom_to_cursor: true,
        yield_to_ui: true,
        keys: false,
        ..Default::default()
    }
}

/// OnEnter(ModeScope::Cad): cameras, light, root and display materials.
/// Every root entity is scoped to the mode by `app::scope_new_entities`.
pub(super) fn setup(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(CadMaterials::new(&mut materials));
    let orbit = Orbit { focus: Vec3::ZERO, radius: 0.5, yaw: 0.7, pitch: 0.45, extent: 0.15, ..Default::default() };
    commands
        .spawn((
            Camera3d::default(),
            // As every other mode: this crate is built without `tonemapping_luts`, so the
            // default TonyMcMapface would sample Bevy's placeholder LUT.
            bevy::core_pipeline::tonemapping::Tonemapping::None,
            // Parts are millimetres shown in metres: the default 0.1 m near plane would cut them.
            Projection::Perspective(PerspectiveProjection { near: 0.001, near_clip_plane: Vec4::new(0.0, 0.0, -1.0, -0.001), ..default() }),
            MeshPickingCamera,
            orbit.transform(),
            orbit,
            rules(),
            // The 3D view draws between the panel's docks (top bar, model tree,
            // inspector, status bar), so framing centres the bodies where they can be seen.
            ViewArea::Docks { left: LEFT_WIDTH, right: RIGHT_WIDTH, top: TOPBAR, bottom: STATUSBAR },
        ))
        // A headlight: the light follows the view, so every face the camera sees is lit.
        .with_children(|camera| {
            camera.spawn((DirectionalLight { illuminance: 6000.0, shadow_maps_enabled: false, ..default() }, Transform::from_xyz(0.2, 0.3, 0.0).looking_at(Vec3::new(0.0, 0.0, -1.0), Vec3::Y)));
        });
    // UI over the whole window, drawn after the 3D view.
    commands.spawn((Camera2d, Camera { order: 3, clear_color: ClearColorConfig::None, ..default() }, IsDefaultUiCamera));
    commands.spawn((root_transform(), Visibility::default(), CadRoot));
}

/// OnExit(ModeScope::Cad): the display materials (entities go by `DespawnOnExit`).
pub(super) fn teardown(mut commands: Commands) {
    commands.remove_resource::<CadMaterials>();
}

/// SimSync, before `CameraSet::Place`: a framing (`cad_fit`, the first
/// meshes) becomes the orbit's bounds and a home request; the place step
/// then frames the centre at 3.2 × the extent from the current heading.
pub(super) fn fit(meshes: Option<ResMut<CadMeshes>>, orbit: Option<Single<&mut Orbit>>) {
    let (Some(mut meshes), Some(mut orbit)) = (meshes, orbit) else { return };
    let Some((centre, extent)) = meshes.fit.take() else { return };
    orbit.centre = centre;
    orbit.extent = extent;
    orbit.home = true;
}

/// Whether the mouse is over a UI node (a panel): the pointer is the panel's
/// then. CAD's tools test it before taking a press in the 3D view.
pub(super) fn over_ui(hover: Option<&HoverMap>, nodes: &Query<(), With<Node>>) -> bool {
    hover.and_then(|h| h.get(&PointerId::Mouse)).is_some_and(|hits| hits.keys().any(|e| nodes.contains(*e)))
}

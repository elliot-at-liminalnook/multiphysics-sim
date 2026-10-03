//! CAD mode's 3D view: the camera (mesh picking on), the UI camera over the
//! whole window, a light that follows the view, the Z-up millimetre root the
//! bodies hang from (`mesh`), and the camera's per-frame data ([`fit`]): the
//! bounds of every drawn body, the framing `cad_fit` and the first meshes
//! ask for, and the input gate.
//!
//! The camera is the shared one (`crate::camera`): it carries an
//! [`Orbit`], CAD's [`OrbitRules`] and a [`ViewArea`] between the panel's
//! docks, and the camera module does the navigation (right-drag orbits,
//! middle or Shift+right-drag pans, the wheel zooms toward the point under
//! the cursor; a gesture that starts over a panel is the panel's), the
//! viewport and the placing, with RoboCAD's own gestures on
//! (`robocad_gestures`: Shift+middle orbits, Alt+right snaps to an axis
//! view, Alt+left-drag orbits, the arrow keys orbit and pan). CAD's own
//! tools (selection, the gizmo, the plane and sketch tools) use the left
//! button, so the rules stay enabled while they work; only Alt+left-drag
//! is gated (`alt_left`: the Select tool with no catalogue interaction or
//! command surface open, where a drag past the slop is the camera's and a
//! click stays the Alt menu's), and the arrow keys stop while a text field
//! has the keyboard (the camera reads the kit's `typing` itself,
//! `ui_kit::text`). Differences from
//! CAD's former own orbit, on purpose: the pitch limit is RoboCAD's 89.5°
//! (was 1.5 rad ≈ 85.9°), and the wheel zooms toward the cursor as
//! RoboCAD's `Camera.zoom(factor, anchor)` (was toward the focus).
use super::document::{CadDocument, CadTool};
use super::mesh::{CadMaterials, CadMeshes, CadRoot, root_transform};
use crate::camera::{Framing, Orbit, OrbitRules, RadiusLimits, ViewArea};
use bevy::window::PrimaryWindow;
use crate::ui_kit::{LEFT_WIDTH, RIGHT_WIDTH, STATUSBAR, SWITCHER_STRIP, TOPBAR};
use bevy::picking::hover::HoverMap;
use bevy::picking::mesh_picking::MeshPickingCamera;
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;

/// CAD's camera feel: RoboCAD's (drag rates as before, its 89.5° pitch
/// limit, zoom 0.05–40 × every drawn body's extent toward the cursor, a fit
/// at 3.2 × the extent from the current heading, cut not glided, RoboCAD's
/// extra gestures and arrow keys). CAD binds its own keymap (RoboCAD's
/// command table), so the shared numpad camera keys are off.
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
        robocad_gestures: true,
        ..Default::default()
    }
}

/// RoboCAD's default field of view (`ui/viewport.py` `Camera.fov`), degrees.
pub(super) const ROBOCAD_FOV_DEG: f32 = 40.0;

/// OnEnter(ModeScope::Cad): cameras, light, root and display materials.
/// Every root entity is scoped to the mode by `app::scope_new_entities`.
pub(super) fn setup(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(CadMaterials::new(&mut materials));
    // RoboCAD's starting camera (`ui/viewport.py` `Camera`: yaw −35°, pitch
    // 28°, which is its iso view, and a 40° field of view; Bevy's default is
    // 45°, which drew the model about 12 % smaller than RoboCAD's window at
    // the same distance). The first meshes frame it from this heading.
    let (yaw, pitch) = crate::camera::ViewPreset::Iso.yaw_pitch();
    let fov = ROBOCAD_FOV_DEG.to_radians();
    let orbit = Orbit { focus: Vec3::ZERO, radius: 0.5, yaw, pitch, fov, extent: 0.15, ..Default::default() };
    commands
        .spawn((
            Camera3d::default(),
            // As every other mode: this crate is built without `tonemapping_luts`, so the
            // default TonyMcMapface would sample Bevy's placeholder LUT.
            bevy::core_pipeline::tonemapping::Tonemapping::None,
            // Parts are millimetres shown in metres: the default 0.1 m near plane would cut them.
            Projection::Perspective(PerspectiveProjection { fov, near: 0.001, near_clip_plane: Vec4::new(0.0, 0.0, -1.0, -0.001), ..default() }),
            MeshPickingCamera,
            orbit.transform(),
            orbit,
            rules(),
            // The 3D view draws between the panel's docks (top bar, model tree,
            // inspector, status bar, switcher strip), so framing centres the bodies where they can be seen.
            ViewArea::Docks { left: LEFT_WIDTH, right: RIGHT_WIDTH, top: TOPBAR, bottom: STATUSBAR + SWITCHER_STRIP },
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

/// Bounds (Bevy's frame, metres) as the orbit's centre and half diagonal,
/// as `CadMeshes::frame` makes them.
fn centre_extent((lo, hi): (Vec3, Vec3)) -> (Vec3, f32) {
    ((lo + hi) / 2.0, ((hi - lo).length() / 2.0).max(0.005))
}

/// The input gate CAD writes into its rules (`alt_left`): Alt+left-drag
/// is the camera's only while the Select tool has the left button to
/// itself (no catalogue interaction such as a placement, a plane or sketch
/// tool or a pick-then-form tool, and no command surface).
pub(super) fn gate(doc: Option<&CadDocument>) -> bool {
    doc.is_some_and(|d| d.tool == CadTool::Select && d.ops.active.is_none() && d.ops.surface.is_none())
}

/// SimSync, before `CameraSet::Place`: the camera's data from CAD's state.
/// - The orbit's bounds (`centre`, `extent`) are every drawn body's
///   (`CadMeshes::bounds(None)`), written when the drawn bodies change
///   (`CadMeshes::epoch`), so `camera_fit`, `camera_home` and the zoom
///   limits always use the current model.
/// - A framing request (`CadMeshes::fit`: `cad_fit` of everything or one
///   node, and the first meshes) frames its bounds from the current
///   heading, keeping the trackball, without replacing the orbit's
///   bounds (`Orbit::frame_bounds`; CAD's framing ignores the aspect).
/// - The rules' input gate ([`gate`]), written only on a change.
#[allow(clippy::type_complexity)]
pub(super) fn fit(
    meshes: Option<ResMut<CadMeshes>>,
    doc: Option<Res<CadDocument>>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    camera: Option<Single<(&mut Orbit, &mut OrbitRules, &Camera, &ViewArea)>>,
    mut seen: Local<Option<u64>>,
) {
    let Some(camera) = camera else { return };
    let (mut orbit, mut rules, camera, area) = camera.into_inner();
    let alt_left = gate(doc.as_deref());
    if rules.alt_left != alt_left {
        rules.alt_left = alt_left;
    }
    let Some(mut meshes) = meshes else {
        *seen = None;
        return;
    };
    // A fresh cache (entering CAD mode) starts its epoch again.
    if meshes.is_added() || *seen != Some(meshes.epoch) {
        *seen = Some(meshes.epoch);
        if let Some((centre, extent)) = meshes.bounds(None).map(centre_extent)
            && (orbit.centre != centre || orbit.extent != extent)
        {
            orbit.centre = centre;
            orbit.extent = extent;
        }
    }
    // Read before taking, so the cache is not marked changed every frame.
    if let Some((centre, extent)) = meshes.fit {
        meshes.fit = None;
        let aspect = crate::camera::view_aspect(camera, window.as_deref().copied(), Some(area));
        orbit.frame_bounds(centre, extent, &rules, aspect);
    }
}

/// Whether the mouse is over a UI node (a panel): the pointer is the panel's
/// then. CAD's tools test it before taking a press in the 3D view.
pub(super) fn over_ui(hover: Option<&HoverMap>, nodes: &Query<(), With<Node>>) -> bool {
    hover.and_then(|h| h.get(&PointerId::Mouse)).is_some_and(|hits| hits.keys().any(|e| nodes.contains(*e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_runtime::cad_client::MeshData;

    /// A box from `lo` to `hi` (RoboCAD's mm, Z up) as drawn mesh data.
    fn cube(lo: [f64; 3], hi: [f64; 3]) -> MeshData {
        MeshData { vertices: vec![lo, hi], ..Default::default() }
    }

    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).init_resource::<CadMeshes>().add_systems(Update, fit);
        let orbit = Orbit { focus: Vec3::ZERO, radius: 0.5, yaw: 0.7, pitch: 0.45, extent: 0.15, ..Default::default() };
        let camera = app.world_mut().spawn((Camera::default(), orbit, rules(), ViewArea::Window)).id();
        (app, camera)
    }

    fn orbit(app: &App, camera: Entity) -> Orbit {
        app.world().get::<Orbit>(camera).unwrap().clone()
    }

    #[test]
    fn the_orbit_bounds_follow_every_drawn_body_and_a_node_fit_keeps_them() {
        let (mut app, camera) = app();
        app.world_mut().resource_mut::<CadMeshes>().insert_drawn("a", 1, cube([0.0, 0.0, 0.0], [100.0, 100.0, 100.0]));
        app.update();
        // Model (x, y, z) mm is display (x, z, −y) m.
        let all = (Vec3::new(0.05, 0.05, -0.05), (Vec3::splat(0.1).length() / 2.0));
        let o = orbit(&app, camera);
        assert!((o.centre - all.0).length() < 1e-6 && (o.extent - all.1).abs() < 1e-6, "{o:?}");
        assert_eq!(o.focus, Vec3::ZERO, "the bounds alone move nothing");
        // Another body: the bounds grow (camera_fit / camera_home frame the current model).
        app.world_mut().resource_mut::<CadMeshes>().insert_drawn("b", 1, cube([200.0, 0.0, 0.0], [300.0, 100.0, 100.0]));
        app.update();
        let o = orbit(&app, camera);
        assert!((o.centre - Vec3::new(0.15, 0.05, -0.05)).length() < 1e-6);
        let (centre, extent) = (o.centre, o.extent);
        // A node fit (cad_fit {id}): framed from the heading, the trackball kept, the bounds left.
        app.world_mut().get_mut::<Orbit>(camera).unwrap().set_trackball(true, &rules());
        let rotation = orbit(&app, camera).rotation();
        let node = app.world().resource::<CadMeshes>().bounds(Some(&["a".to_string()].into())).unwrap();
        app.world_mut().resource_mut::<CadMeshes>().frame(node);
        app.update();
        let o = orbit(&app, camera);
        assert!((o.focus - all.0).length() < 1e-6 && (o.radius - all.1 * 3.2).abs() < 1e-5, "{o:?}");
        // The same rotation (f32 `angle_between` is acos noise near 0, about 1e-3 rad; the dot is exact to rounding).
        assert!(o.centre == centre && o.extent == extent && o.trackball.is_some() && o.rotation().dot(rotation).abs() > 1.0 - 1e-6, "{o:?}");
        assert!(app.world().resource::<CadMeshes>().fit.is_none());
    }

    #[test]
    fn the_gate_keeps_alt_left_drag_off_without_a_document() {
        let (mut app, camera) = app();
        app.world_mut().get_mut::<OrbitRules>(camera).unwrap().alt_left = true;
        app.update();
        let rules = app.world().get::<OrbitRules>(camera).unwrap().clone();
        // No document: no tool owns the left button for the camera to share.
        assert!(!rules.alt_left && rules.robocad_gestures && !rules.keys);
        assert!(!gate(None));
    }
}

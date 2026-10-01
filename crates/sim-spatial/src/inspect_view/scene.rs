//! The spatial view's scene: cameras, lights and parts spawned on entering,
//! part picks as the mode's action, part transforms and colours, and the guides.
use super::{ModelColor, Part, SceneContent, SpatialScene};
use crate::camera::Orbit;
use crate::app::{self, ViewerMode};
use crate::{animation, builder, inspect, lesson, linked, models, physics_view, ui_kit, view};
use bevy::{core_pipeline::tonemapping::Tonemapping, picking::mesh_picking::MeshPickingCamera, prelude::*};
use sim_inspect::selection::SelectionTarget;
use sim_inspect::spatial::{SpatialCommand, SpatialShape};

pub(super) fn setup_scene(
    mut commands: Commands,
    scene: Option<Res<SpatialScene>>,
    fonts: Res<ui_kit::UiFonts>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut models: Option<ResMut<models::ModelLibrary>>,
) {
    let k = ui_kit::Kit::new(&fonts);
    // A mode switch installs the scene before this runs (`app::switch::arrive`).
    let Some(scene) = scene else {
        error!("the spatial view was entered without a scene");
        return;
    };
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
        // The shared camera (`crate::camera`), placed by `CameraSet::Place`:
        // the home request frames the overview from here (`super::camera`).
        Orbit { focus, radius: radius * 3.5, yaw: 0.35, pitch: 0.6, home: true, centre: focus, extent: radius, ..Default::default() },
        super::camera::spatial_rules(),
        super::camera::view_area(&scene),
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
    // The split view's captions: floating over the 3D view (placed by `view::split`).
    for right in [false, true] {
        commands.spawn((
            view::SplitLabel(right),
            Node { border_radius: BorderRadius::all(Val::Px(4.)), position_type: PositionType::Absolute, padding: UiRect::axes(Val::Px(8.), Val::Px(3.)), ..default() },
            BackgroundColor(view::BACKDROP),
            Visibility::Hidden,
            GlobalZIndex(23),
            Pickable::IGNORE,
            children![(k.text("", 12., if right { view::COMPANION } else { ui_kit::ACCENT }, 2), Pickable::IGNORE)],
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
    // The close-up's frame over the 3D view (placed by `view::inset`) and its caption.
    commands.spawn((
        view::InsetFrame,
        Node { border_radius: BorderRadius::all(Val::Px(4.)), position_type: PositionType::Absolute, border: UiRect::all(Val::Px(2.)), justify_content: JustifyContent::FlexStart, align_items: AlignItems::FlexStart, ..default() },
        BorderColor::all(ui_kit::ACCENT),
        Visibility::Hidden,
        GlobalZIndex(22),
        Pickable::IGNORE,
        children![(k.text("", 11., ui_kit::ACCENT, 2), BackgroundColor(view::BACKDROP), Node { padding: UiRect::axes(Val::Px(6.), Val::Px(2.)), ..default() }, Pickable::IGNORE)],
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

/// A primary click on a part, as its mode's action (applied in
/// `ViewerSet::Actions` the same frame): the builder's `PickPart` in Build,
/// the spatial view's selection in Inspect (its handler writes the shared
/// selection), the lesson page's `Pick` in Lessons.
#[allow(clippy::too_many_arguments)]
pub(super) fn pick_part(
    click: On<Pointer<Click>>,
    parts: Query<&Part>,
    scene: Res<SpatialScene>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    builder: Option<Res<builder::Builder>>,
    learn: Option<Res<lesson::Learn>>,
    mode: Option<Res<State<ViewerMode>>>,
    lesson_out: Option<MessageWriter<app::actions::Act<lesson::actions::LessonCommand>>>,
    inspect_out: Option<MessageWriter<app::actions::Act<inspect::InspectAction>>>,
    build_out: Option<MessageWriter<app::actions::Act<builder::system_actions::SystemAction>>>,
) {
    use app::actions::Act;
    if click.button != bevy::picking::pointer::PointerButton::Primary {
        return;
    }
    let Ok(part) = parts.get(click.entity) else { return };
    let id = scene.spatial.parts[part.index].component.clone();
    let mode = mode.map(|m| *m.get());
    if learn.is_some() && mode == Some(ViewerMode::Lessons) {
        if let Some(mut out) = lesson_out {
            out.write(Act::ui(lesson::actions::LessonCommand::Ui(lesson::LessonAction::Pick(id))));
        }
        return;
    }
    let shift = keys.as_ref().is_some_and(|k| k.pressed(KeyCode::ShiftLeft) || k.pressed(KeyCode::ShiftRight));
    // The builder stays in the window in other modes: it takes picks in Build only
    // (without states, as in tests, a builder means build mode, as before).
    if builder.is_some() && mode.is_none_or(|m| m == ViewerMode::Build) {
        if let Some(mut out) = build_out {
            let action = builder::BuildAction::PickPart { index: part.index, component: id, add: shift, world: click.hit.position.map(|p| p.to_array()) };
            out.write(Act::ui(builder::system_actions::SystemAction::Ui(action)));
        }
        return;
    }
    let Some(mut out) = inspect_out else { return };
    if shift {
        // Toggles the part among the components shown selected (a refusal is dropped, as before).
        let mut ids = scene.details.components.clone();
        if !ids.remove(&id) {
            ids.insert(id);
        }
        let target = if ids.is_empty() { SelectionTarget::None } else { SelectionTarget::Components { ids } };
        out.write(Act::quiet(inspect::InspectAction::Select { target }));
    } else {
        // The parts list's select (a refusal is logged, as before).
        out.write(Act::ui(inspect::InspectAction::Display { action: SpatialCommand::Select { component: id } }));
    }
}

pub(crate) fn update_parts(
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

pub(super) fn draw_guides(scene: Res<SpatialScene>, mut gizmos: Gizmos) {
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

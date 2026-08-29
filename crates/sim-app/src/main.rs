use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use sim_core::ObjectId;
use sim_geometry::{RestTransform, VisualBinding, VisualMotion, project};
use sim_runtime::{ActuatorConfig, ActuatorSimulation};

const DISPLAY_SCALE: f32 = 8.0;

#[derive(Resource)]
struct Simulator {
    simulation: ActuatorSimulation,
    accumulator: f64,
    paused: bool,
    last_error: Option<String>,
}

#[derive(Component)]
struct StateVisual {
    binding: VisualBinding,
    rest: RestTransform,
}

#[derive(Component)]
struct StatusText;

#[derive(Component)]
struct OrbitCamera {
    focus: Vec3,
    radius: f32,
    yaw: f32,
    pitch: f32,
}

fn main() {
    let simulation = ActuatorSimulation::new(ActuatorConfig::default())
        .expect("the built-in actuator model must compile");
    App::new()
        .insert_resource(Simulator {
            simulation,
            accumulator: 0.0,
            paused: false,
            last_error: None,
        })
        .insert_resource(AmbientLight {
            color: Color::srgb(0.82, 0.86, 0.95),
            brightness: 500.0,
            affects_lightmapped_meshes: true,
        })
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Multiphysics actuator slice".to_owned(),
                resolution: (1280.0_f32, 760.0_f32).into(),
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                keyboard_controls,
                orbit_camera,
                advance_simulation,
                project_visuals,
                update_status,
            )
                .chain(),
        )
        .run();
}

fn setup(
    mut commands: Commands,
    simulator: Res<Simulator>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let camera_focus = Vec3::new(0.0, 0.0, 0.55);
    let camera_position = Vec3::new(4.2, 3.3, 5.8);
    let camera_offset = camera_position - camera_focus;
    let camera_radius = camera_offset.length();
    commands.spawn((
        Camera3d::default(),
        Transform::from_translation(camera_position).looking_at(camera_focus, Vec3::Y),
        OrbitCamera {
            focus: camera_focus,
            radius: camera_radius,
            yaw: camera_offset.x.atan2(camera_offset.z),
            pitch: (camera_offset.y / camera_radius).asin(),
        },
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 8_000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.8, -0.6, 0.0)),
    ));

    let chassis_material = materials.add(Color::srgb(0.18, 0.21, 0.25));
    let rail_material = materials.add(Color::srgb(0.55, 0.59, 0.63));
    let motor_material = materials.add(Color::srgb(0.17, 0.34, 0.44));
    let copper_material = materials.add(Color::srgb(0.78, 0.32, 0.16));
    let gear_material = materials.add(Color::srgb(0.86, 0.62, 0.18));
    let screw_material = materials.add(Color::srgb(0.69, 0.72, 0.76));
    let carriage_material = materials.add(Color::srgb(0.78, 0.24, 0.18));
    let stop_material = materials.add(Color::srgb(0.44, 0.16, 0.14));

    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(4.2, 0.18, 3.0))),
        MeshMaterial3d(chassis_material),
        Transform::from_xyz(0.0, -0.53, 0.0),
    ));
    for z in [0.63, 1.29] {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(2.15, 0.09, 0.09))),
            MeshMaterial3d(rail_material.clone()),
            Transform::from_xyz(0.62, -0.20, z),
        ));
    }
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(0.34, 0.70))),
        MeshMaterial3d(motor_material),
        Transform::from_xyz(-1.42, -0.02, 0.0)
            .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
    ));

    let model = &simulator.simulation.model;
    let motor_object = object_named(model, "DC motor");
    let gear_object = object_named(model, "10:1 gearbox");
    let screw_object = object_named(model, "lead screw");
    let carriage_object = object_named(model, "linear carriage");
    let ids = &simulator.simulation.ids;

    spawn_rotating_part(
        &mut commands,
        &mut meshes,
        copper_material,
        motor_object,
        ids.motor_angle,
        Vec3::new(-0.96, -0.02, 0.0),
        0.10,
        0.34,
        1.0,
    );
    spawn_gear(
        &mut commands,
        &mut meshes,
        gear_material.clone(),
        gear_object,
        ids.motor_angle,
        Vec3::new(-0.69, -0.02, 0.0),
        0.12,
        0.10,
        12,
        1.0,
    );
    spawn_gear(
        &mut commands,
        &mut meshes,
        gear_material.clone(),
        gear_object,
        ids.motor_angle,
        Vec3::new(-0.69, -0.02, 0.36),
        0.24,
        0.12,
        24,
        -0.5,
    );
    spawn_gear(
        &mut commands,
        &mut meshes,
        gear_material.clone(),
        gear_object,
        ids.motor_angle,
        Vec3::new(-0.48, -0.02, 0.36),
        0.10,
        0.10,
        12,
        -0.5,
    );
    spawn_gear(
        &mut commands,
        &mut meshes,
        gear_material,
        gear_object,
        ids.gear_angle,
        Vec3::new(-0.48, -0.02, 0.96),
        0.50,
        0.12,
        60,
        1.0,
    );
    spawn_rotating_part(
        &mut commands,
        &mut meshes,
        screw_material,
        screw_object,
        ids.gear_angle,
        Vec3::new(0.65, -0.02, 0.96),
        0.055,
        1.85,
        1.0,
    );

    let carriage_rest = RestTransform {
        translation: Vec3::new(-0.22, 0.08, 0.96),
        rotation: Quat::IDENTITY,
    };
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.38, 0.48, 0.72))),
        MeshMaterial3d(carriage_material),
        Transform::from_translation(carriage_rest.translation),
        StateVisual {
            binding: VisualBinding {
                object: carriage_object,
                source: ids.carriage_position,
                motion: VisualMotion::Translate { axis: Vec3::X },
                scale: DISPLAY_SCALE,
            },
            rest: carriage_rest,
        },
    ));
    for (x, label) in [(-0.22, "HOME"), (0.98, "150 mm")] {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(0.06, 0.68, 0.90))),
            MeshMaterial3d(stop_material.clone()),
            Transform::from_xyz(x, -0.03, 0.96),
            Name::new(label),
        ));
    }

    commands.spawn((
        Text::new("Starting simulation…"),
        TextFont {
            font_size: 18.0,
            ..default()
        },
        TextColor(Color::srgb(0.93, 0.95, 0.98)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(16.0),
            left: Val::Px(18.0),
            ..default()
        },
        StatusText,
    ));
}

#[allow(clippy::too_many_arguments)]
fn spawn_rotating_part(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: Handle<StandardMaterial>,
    object: ObjectId,
    source: sim_core::StateId,
    translation: Vec3,
    radius: f32,
    length: f32,
    angle_scale: f32,
) {
    let rest = RestTransform {
        translation,
        rotation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
    };
    commands
        .spawn((
            Mesh3d(meshes.add(Cylinder::new(radius, length))),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(translation).with_rotation(rest.rotation),
            StateVisual {
                binding: VisualBinding {
                    object,
                    source,
                    // Bevy's Cylinder is authored along local Y. `rest` turns
                    // that local shaft axis onto the machine's world X axis.
                    motion: VisualMotion::Rotate { axis: Vec3::Y },
                    scale: angle_scale,
                },
                rest,
            },
        ))
        .with_children(|parent| {
            parent.spawn((
                Mesh3d(meshes.add(Cuboid::new(radius * 0.10, length * 0.9, radius * 0.34))),
                MeshMaterial3d(material),
                // A narrow axial witness stripe makes spin visible. Its long
                // dimension must follow the cylinder's local Y shaft axis;
                // placing it on local X creates a spurious sweeping arm.
                Transform::from_xyz(0.0, 0.0, radius * 0.72),
            ));
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_gear(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: Handle<StandardMaterial>,
    object: ObjectId,
    source: sim_core::StateId,
    translation: Vec3,
    pitch_radius: f32,
    thickness: f32,
    tooth_count: usize,
    angle_scale: f32,
) {
    let rest = RestTransform {
        translation,
        rotation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
    };
    let root_radius = pitch_radius * 0.86;
    let tooth_depth = pitch_radius * 0.22;
    let tooth_width = std::f32::consts::TAU * pitch_radius / tooth_count as f32 * 0.58;
    commands
        .spawn((
            Mesh3d(meshes.add(Cylinder::new(root_radius, thickness))),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(translation).with_rotation(rest.rotation),
            StateVisual {
                binding: VisualBinding {
                    object,
                    source,
                    motion: VisualMotion::Rotate { axis: Vec3::Y },
                    scale: angle_scale,
                },
                rest,
            },
        ))
        .with_children(|parent| {
            for tooth in 0..tooth_count {
                let angle = std::f32::consts::TAU * tooth as f32 / tooth_count as f32;
                let radius = root_radius + tooth_depth * 0.42;
                parent.spawn((
                    Mesh3d(meshes.add(Cuboid::new(tooth_depth, thickness * 1.08, tooth_width))),
                    MeshMaterial3d(material.clone()),
                    Transform::from_xyz(radius * angle.cos(), 0.0, radius * angle.sin())
                        .with_rotation(Quat::from_rotation_y(-angle)),
                ));
            }
            parent.spawn((
                Mesh3d(meshes.add(Cylinder::new(pitch_radius * 0.24, thickness * 1.22))),
                MeshMaterial3d(material),
            ));
        });
}

fn object_named(model: &sim_core::ModelWorld, name: &str) -> ObjectId {
    model
        .objects
        .iter()
        .find_map(|(id, object)| (object.name == name).then_some(id))
        .unwrap_or_else(|| panic!("built-in model is missing object `{name}`"))
}

fn keyboard_controls(keys: Res<ButtonInput<KeyCode>>, mut simulator: ResMut<Simulator>) {
    if keys.just_pressed(KeyCode::ArrowUp) {
        simulator.simulation.inputs.target_position =
            (simulator.simulation.inputs.target_position + 0.010).min(0.150);
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        simulator.simulation.inputs.target_position =
            (simulator.simulation.inputs.target_position - 0.010).max(0.0);
    }
    if keys.just_pressed(KeyCode::KeyO) {
        let obstruction = &mut simulator.simulation.inputs.obstruction_position;
        *obstruction = if obstruction.is_some() {
            None
        } else {
            Some(0.080)
        };
    }
    if keys.just_pressed(KeyCode::KeyB) {
        let voltage = &mut simulator.simulation.inputs.supply_voltage;
        *voltage = if *voltage > 18.0 { 12.0 } else { 24.0 };
    }
    if keys.just_pressed(KeyCode::Space) {
        simulator.paused = !simulator.paused;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        let config = simulator.simulation.config.clone();
        simulator.simulation =
            ActuatorSimulation::new(config).expect("reset model must compile identically");
        simulator.accumulator = 0.0;
        simulator.last_error = None;
    }
}

fn orbit_camera(
    buttons: Res<ButtonInput<MouseButton>>,
    mut mouse_motion: EventReader<MouseMotion>,
    mut mouse_wheel: EventReader<MouseWheel>,
    camera: Single<(&mut Transform, &mut OrbitCamera)>,
) {
    let drag = mouse_motion
        .read()
        .fold(Vec2::ZERO, |sum, event| sum + event.delta);
    let zoom = mouse_wheel.read().fold(0.0, |sum, event| {
        sum + match event.unit {
            MouseScrollUnit::Line => event.y,
            MouseScrollUnit::Pixel => event.y * 0.02,
        }
    });
    if (!buttons.pressed(MouseButton::Left) || drag == Vec2::ZERO) && zoom == 0.0 {
        return;
    }

    let (mut transform, mut orbit) = camera.into_inner();
    if buttons.pressed(MouseButton::Left) {
        orbit.yaw -= drag.x * 0.008;
        orbit.pitch = (orbit.pitch + drag.y * 0.008).clamp(-1.35, 1.35);
    }
    orbit.radius = (orbit.radius * (-zoom * 0.12).exp()).clamp(2.0, 14.0);

    let horizontal = orbit.pitch.cos() * orbit.radius;
    transform.translation = orbit.focus
        + Vec3::new(
            orbit.yaw.sin() * horizontal,
            orbit.pitch.sin() * orbit.radius,
            orbit.yaw.cos() * horizontal,
        );
    transform.look_at(orbit.focus, Vec3::Y);
}

fn advance_simulation(time: Res<Time>, mut simulator: ResMut<Simulator>) {
    if simulator.paused || simulator.last_error.is_some() {
        return;
    }
    simulator.accumulator = (simulator.accumulator + time.delta_secs_f64()).min(0.050);
    let step = simulator.simulation.config.plant_step;
    while simulator.accumulator >= step {
        if let Err(error) = simulator.simulation.step() {
            simulator.last_error = Some(error.to_string());
            break;
        }
        simulator.accumulator -= step;
    }
}

fn project_visuals(simulator: Res<Simulator>, mut visuals: Query<(&StateVisual, &mut Transform)>) {
    for (visual, mut transform) in &mut visuals {
        if let Ok(projected) = project(
            visual.binding,
            visual.rest,
            &simulator.simulation.model.state,
        ) {
            transform.translation = projected.translation;
            transform.rotation = projected.rotation;
        }
    }
}

fn update_status(simulator: Res<Simulator>, mut text: Single<&mut Text, With<StatusText>>) {
    let sample = match simulator.simulation.sample() {
        Ok(sample) => sample,
        Err(error) => {
            **text = Text::new(format!("State error: {error}"));
            return;
        }
    };
    let obstruction = simulator
        .simulation
        .inputs
        .obstruction_position
        .map(|position| format!("{:.0} mm", position * 1000.0))
        .unwrap_or_else(|| "off".to_owned());
    let solver = simulator
        .last_error
        .as_deref()
        .unwrap_or(if simulator.paused {
            "paused"
        } else {
            "running"
        });
    **text = Text::new(format!(
        "CONTROLLER → DRIVER → MOTOR → GEAR → SCREW → CARRIAGE\n\
         target  {:>6.1} mm    position {:>6.1} mm    velocity {:>7.2} mm/s\n\
         duty    {:>6.1} %     bus      {:>6.1} V     current  {:>7.2} A{}\n\
         motor   {:>6.1} rad/s force    {:>6.0} N     reaction {:>7.0} N\n\
         Newton  {:>6} iter   residual {:>8.1e}    obstruction {}\n\
         state: {}\n\n\
         Left-drag orbit · wheel zoom\n\
         ↑/↓ target · O obstruction · B brownout · Space pause · R reset",
        sample.target_position * 1000.0,
        sample.position * 1000.0,
        sample.velocity * 1000.0,
        sample.duty * 100.0,
        sample.bus_voltage,
        sample.current,
        if sample.current_limited { " LIMIT" } else { "" },
        sample.motor_speed,
        sample.drive_force,
        sample.chassis_reaction_force,
        sample.newton_iterations,
        sample.residual_norm,
        obstruction,
        solver,
    ));
}

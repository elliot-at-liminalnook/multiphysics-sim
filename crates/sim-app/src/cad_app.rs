//! `sim-app --scene cad --model robot.simrobot.json`: the robot the CAD
//! tool exported, simulated live. A v3 (physical) file is drawn as its
//! links' collision meshes in their simulated poses, with joint axes,
//! contact points and flexible-link deflections as gizmos, and the stress
//! field from the last results file painted on the links (key S); a v2
//! file is drawn as section outlines as before. The file is watched; a
//! save in the CAD tool rebuilds the model in place.
//!
//! Keys: Space pause, R rebuild, Up/Down move the selected joint's target,
//! Left/Right select a joint, S stress colouring, C contacts, + / - speed.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use sim_phenomena::scenarios::cad_physical::results_path;
use sim_phenomena::scenarios::cad_robot::{AnyRobot, BuildOptions};
use std::time::SystemTime;

#[derive(Resource)]
struct CadSim {
    path: String,
    robot: Option<AnyRobot>,
    error: Option<String>,
    modified: Option<SystemTime>,
    paused: bool,
    accumulator: f64,
    speed: f64,
    selected: usize,
    checked_at: f64,
    stress: bool,
    show_contacts: bool,
    /// Link entities (physical models), rebuilt with the model.
    links: Vec<Entity>,
    needs_meshes: bool,
    results: Option<serde_json::Value>,
    sim_seconds: f64,
    wall_seconds: f64,
}

#[derive(Component)]
struct OrbitCamera {
    focus: Vec3,
    radius: f32,
    yaw: f32,
    pitch: f32,
}

#[derive(Component)]
struct StatusText;

#[derive(Component)]
struct LinkMesh(usize);

#[derive(Component)]
struct RobotRoot;

/// CAD/world coordinates (z up) to Bevy (y up).
fn to_bevy(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[2] as f32, -(p[1] as f32))
}

pub fn run(model: String) {
    App::new()
        .insert_resource(CadSim {
            path: model,
            robot: None,
            error: None,
            modified: None,
            paused: false,
            accumulator: 0.0,
            speed: 1.0,
            selected: 0,
            checked_at: 0.0,
            stress: false,
            show_contacts: true,
            links: Vec::new(),
            needs_meshes: false,
            results: None,
            sim_seconds: 0.0,
            wall_seconds: 0.0,
        })
        .insert_resource(AmbientLight { color: Color::srgb(0.85, 0.88, 0.95), brightness: 400.0, affects_lightmapped_meshes: true })
        .add_plugins(DefaultPlugins.set(WindowPlugin { primary_window: Some(Window { title: "robocad → simulation".to_owned(), resolution: (1280.0_f32, 800.0_f32).into(), ..default() }), ..default() }))
        .add_systems(Startup, setup)
        .add_systems(Update, (watch_file, keyboard, orbit_camera, advance, spawn_meshes, pose_links, draw, status).chain())
        .run();
}

fn setup(mut commands: Commands) {
    let focus = Vec3::new(0.0, 0.12, 0.0);
    let position = Vec3::new(0.35, 0.3, 0.6);
    let offset = position - focus;
    commands.spawn((Camera3d::default(), Transform::from_translation(position).looking_at(focus, Vec3::Y), OrbitCamera { focus, radius: offset.length(), yaw: offset.x.atan2(offset.z), pitch: (offset.y / offset.length()).asin() }));
    commands.spawn((DirectionalLight { illuminance: 9_000.0, shadows_enabled: false, ..default() }, Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, -0.5, 0.0))));
    commands.spawn((DirectionalLight { illuminance: 2_500.0, shadows_enabled: false, ..default() }, Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.3, 2.4, 0.0))));
    commands.spawn((Text::new(""), TextFont { font_size: 14.0, ..default() }, Node { position_type: PositionType::Absolute, left: Val::Px(12.0), top: Val::Px(10.0), ..default() }, StatusText));
    commands.spawn((RobotRoot, Transform::IDENTITY, Visibility::default()));
}

fn rebuild(sim: &mut CadSim) {
    match AnyRobot::load(&sim.path, &BuildOptions::default()) {
        Ok(robot) => {
            sim.error = None;
            sim.selected = sim.selected.min(robot.joint_names().len().saturating_sub(1));
            sim.robot = Some(robot);
            sim.needs_meshes = true;
            sim.sim_seconds = 0.0;
            sim.wall_seconds = 0.0;
        }
        Err(e) => {
            sim.error = Some(e);
        }
    }
    sim.modified = std::fs::metadata(&sim.path).and_then(|m| m.modified()).ok();
    sim.results = std::fs::read_to_string(results_path(&sim.path)).ok().and_then(|t| serde_json::from_str(&t).ok());
}

fn watch_file(mut sim: ResMut<CadSim>, time: Res<Time>) {
    let now = time.elapsed_secs_f64();
    if sim.robot.is_none() && sim.error.is_none() {
        rebuild(&mut sim);
        return;
    }
    if now - sim.checked_at < 1.0 {
        return;
    }
    sim.checked_at = now;
    let modified = std::fs::metadata(&sim.path).and_then(|m| m.modified()).ok();
    if modified.is_some() && modified != sim.modified {
        rebuild(&mut sim);
    }
}

fn keyboard(keys: Res<ButtonInput<KeyCode>>, mut sim: ResMut<CadSim>) {
    if keys.just_pressed(KeyCode::Space) {
        sim.paused = !sim.paused;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        rebuild(&mut sim);
    }
    if keys.just_pressed(KeyCode::KeyS) {
        sim.stress = !sim.stress;
        sim.results = std::fs::read_to_string(results_path(&sim.path)).ok().and_then(|t| serde_json::from_str(&t).ok());
        sim.needs_meshes = true;
    }
    if keys.just_pressed(KeyCode::KeyC) {
        sim.show_contacts = !sim.show_contacts;
    }
    if keys.just_pressed(KeyCode::Equal) {
        sim.speed = (sim.speed * 2.0).min(8.0);
    }
    if keys.just_pressed(KeyCode::Minus) {
        sim.speed = (sim.speed / 2.0).max(0.125);
    }
    let count = sim.robot.as_ref().map(|r| r.joint_names().len()).unwrap_or(0);
    if count > 0 {
        if keys.just_pressed(KeyCode::ArrowRight) {
            sim.selected = (sim.selected + 1) % count;
        }
        if keys.just_pressed(KeyCode::ArrowLeft) {
            sim.selected = (sim.selected + count - 1) % count;
        }
        let step = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) { 0.05 } else { 0.01 };
        let delta = if keys.pressed(KeyCode::ArrowUp) { step } else if keys.pressed(KeyCode::ArrowDown) { -step } else { 0.0 };
        if delta != 0.0 {
            let selected = sim.selected;
            if let Some(robot) = sim.robot.as_ref() {
                let current = robot.targets().get(selected).copied().unwrap_or(0.0);
                robot.set_target(selected, current + delta);
            }
        }
    }
}

fn advance(mut sim: ResMut<CadSim>, time: Res<Time>) {
    if sim.paused {
        return;
    }
    let dt = time.delta_secs_f64().min(0.05) * sim.speed;
    sim.accumulator += dt;
    let grid = 0.02;
    let whole = (sim.accumulator / grid).floor();
    if whole < 1.0 {
        return;
    }
    sim.accumulator -= whole * grid;
    // A slow model must not stall the window: at most one grid per frame.
    let advance_by = grid;
    sim.accumulator = sim.accumulator.min(grid);
    let start = std::time::Instant::now();
    if let Some(robot) = sim.robot.as_mut() {
        if let Err(e) = robot.advance(advance_by) {
            sim.error = Some(format!("simulation stopped: {e} (press R to rebuild)"));
            sim.paused = true;
        }
    }
    sim.sim_seconds += advance_by;
    sim.wall_seconds += start.elapsed().as_secs_f64();
}

/// Turbo colour map for stress (0 → blue, 1 → red).
fn colormap(t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    let r = (1.7 * t - 0.3).clamp(0.0, 1.0) * 0.95 + 0.05;
    let g = (1.0 - (2.0 * t - 1.0).abs()).clamp(0.0, 1.0) * 0.85 + 0.1;
    let b = (1.0 - 1.7 * t).clamp(0.0, 1.0) * 0.95 + 0.05;
    [r, g, b, 1.0]
}

fn spawn_meshes(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, mut sim: ResMut<CadSim>, root: Query<Entity, With<RobotRoot>>) {
    if !sim.needs_meshes {
        return;
    }
    sim.needs_meshes = false;
    for e in sim.links.drain(..) {
        commands.entity(e).despawn();
    }
    let Some(AnyRobot::Physical(robot)) = sim.robot.as_ref() else { return };
    let Ok(root) = root.single() else { return };
    let palette = [[0.85, 0.55, 0.25], [0.35, 0.65, 0.9], [0.45, 0.8, 0.5], [0.85, 0.4, 0.45], [0.8, 0.75, 0.35], [0.65, 0.5, 0.85], [0.6, 0.6, 0.65]];
    let stress = sim.stress;
    let results = sim.results.clone();
    let mut spawned = Vec::new();
    for (li, link) in robot.model.links.iter().enumerate() {
        let c = &link.collision;
        let base = if link.ground { [0.55, 0.55, 0.6] } else { palette[li % palette.len()] };
        // Stress hotspots: nearest cell's peak stress, normalised by yield.
        let hotspot: Option<(Vec<[f64; 3]>, Vec<f64>, f64)> = if stress {
            results.as_ref().and_then(|r| {
                let l = &r["links"][&link.name];
                let cells: Vec<[f64; 3]> = l["hotspot"]["cells"].as_array()?.iter().filter_map(|c| Some([c[0].as_f64()?, c[1].as_f64()?, c[2].as_f64()?])).collect();
                let vals: Vec<f64> = l["hotspot"]["stress_pa"].as_array()?.iter().filter_map(|v| v.as_f64()).collect();
                let yield_strength = robot.model.material_of(link).yield_strength;
                if cells.is_empty() { None } else { Some((cells, vals, yield_strength)) }
            })
        } else {
            None
        };
        let color_at = |p: [f64; 3]| -> [f32; 4] {
            match &hotspot {
                Some((cells, vals, yield_strength)) => {
                    let mut best = (f64::INFINITY, 0.0);
                    for (c, v) in cells.iter().zip(vals) {
                        let d = (c[0] - p[0]).powi(2) + (c[1] - p[1]).powi(2) + (c[2] - p[2]).powi(2);
                        if d < best.0 {
                            best = (d, *v);
                        }
                    }
                    // Scale so that yield is red; the scale is logarithmic over 3 decades.
                    let ratio = (best.1 / yield_strength.max(1.0)).max(1e-6);
                    colormap(((ratio.log10() + 3.0) / 3.0) as f32)
                }
                None => [base[0], base[1], base[2], 1.0],
            }
        };
        let mut positions: Vec<[f32; 3]> = Vec::new();
        let mut normals: Vec<[f32; 3]> = Vec::new();
        let mut colors: Vec<[f32; 4]> = Vec::new();
        let tris: Vec<[usize; 3]> = if c.triangles.is_empty() { hull_triangles(&c.hull) } else { c.triangles.clone() };
        for t in &tris {
            let Some(a) = c.vertices.get(t[0]).or_else(|| c.hull.get(t[0])) else { continue };
            let Some(b) = c.vertices.get(t[1]).or_else(|| c.hull.get(t[1])) else { continue };
            let Some(d) = c.vertices.get(t[2]).or_else(|| c.hull.get(t[2])) else { continue };
            let (a, b, d) = (*a, *b, *d);
            let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let e2 = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
            let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
            let n = [(n[0] / len) as f32, (n[1] / len) as f32, (n[2] / len) as f32];
            for p in [a, b, d] {
                positions.push([p[0] as f32, p[1] as f32, p[2] as f32]);
                normals.push(n);
                colors.push(color_at(p));
            }
        }
        if positions.is_empty() {
            continue;
        }
        let count = positions.len() as u32;
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        mesh.insert_indices(Indices::U32((0..count).collect()));
        let material = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.7, metallic: 0.05, cull_mode: None, ..default() });
        let entity = commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(material), Transform::IDENTITY, Visibility::default(), LinkMesh(li))).id();
        commands.entity(root).add_child(entity);
        spawned.push(entity);
    }
    sim.links = spawned;
}

/// A crude fan triangulation of a convex hull's points (for links exported
/// without triangles): faces are made by gift-wrapping around the centroid.
fn hull_triangles(hull: &[[f64; 3]]) -> Vec<[usize; 3]> {
    if hull.len() < 4 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let n = hull.len();
    let c = hull.iter().fold([0.0; 3], |acc, p| [acc[0] + p[0] / n as f64, acc[1] + p[1] / n as f64, acc[2] + p[2] / n as f64]);
    for i in 0..n {
        for j in i + 1..n {
            for k in j + 1..n {
                let (a, b, d) = (hull[i], hull[j], hull[k]);
                let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let e2 = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
                let nrm = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
                let side = |p: [f64; 3]| nrm[0] * (p[0] - a[0]) + nrm[1] * (p[1] - a[1]) + nrm[2] * (p[2] - a[2]);
                let all_below = hull.iter().all(|p| side(*p) <= 1e-9);
                let all_above = hull.iter().all(|p| side(*p) >= -1e-9);
                if all_below || all_above {
                    if side(c) > 0.0 { out.push([i, k, j]) } else { out.push([i, j, k]) }
                }
            }
        }
    }
    out
}

fn pose_links(sim: Res<CadSim>, mut query: Query<(&LinkMesh, &mut Transform)>, mut root: Query<&mut Transform, (With<RobotRoot>, Without<LinkMesh>)>) {
    let Some(AnyRobot::Physical(robot)) = sim.robot.as_ref() else { return };
    if let Ok(mut t) = root.single_mut() {
        *t = Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2));
    }
    let poses = robot.poses();
    for (link, mut transform) in query.iter_mut() {
        let Some((r, p)) = poses.get(link.0) else { continue };
        let q = nalgebra::UnitQuaternion::from_matrix(r);
        transform.translation = Vec3::new(p.x as f32, p.y as f32, p.z as f32);
        transform.rotation = Quat::from_xyzw(q.i as f32, q.j as f32, q.k as f32, q.w as f32);
    }
}

fn draw(sim: Res<CadSim>, mut gizmos: Gizmos) {
    let ground = Color::srgb(0.35, 0.38, 0.45);
    let Some(robot) = sim.robot.as_ref() else {
        for i in -20..=20 {
            let x = i as f32 * 0.05;
            gizmos.line(Vec3::new(x, 0.0, -1.0), Vec3::new(x, 0.0, 1.0), ground);
            gizmos.line(Vec3::new(-1.0, 0.0, x), Vec3::new(1.0, 0.0, x), ground);
        }
        return;
    };
    match robot {
        AnyRobot::Planar(robot) => {
            for i in -20..=20 {
                let x = i as f32 * 0.05;
                gizmos.line(Vec3::new(x, 0.0, -0.5), Vec3::new(x, 0.0, 0.5), ground);
                gizmos.line(Vec3::new(-1.0, 0.0, x), Vec3::new(1.0, 0.0, x), ground);
            }
            let palette = [Color::srgb(0.9, 0.55, 0.2), Color::srgb(0.3, 0.7, 0.95), Color::srgb(0.4, 0.85, 0.5), Color::srgb(0.9, 0.35, 0.4), Color::srgb(0.8, 0.8, 0.3), Color::srgb(0.7, 0.5, 0.9)];
            for (bi, pts) in robot.outlines() {
                let color = palette[bi % palette.len()];
                let points: Vec<Vec3> = pts.iter().map(|p| Vec3::new(p[0] as f32, p[1] as f32, 0.0)).collect();
                gizmos.linestrip(points.clone(), color);
                if let (Some(a), Some(b)) = (points.first(), points.last()) {
                    gizmos.line(*b, *a, color);
                }
            }
            for (_, com, _) in robot.poses() {
                gizmos.circle(Isometry3d::from_translation(Vec3::new(com[0] as f32, com[1] as f32, 0.001)), 0.004, Color::WHITE);
            }
            for chain in &robot.chains {
                let tip = [robot.runtime.get(chain.tip[0]), robot.runtime.get(chain.tip[1])];
                gizmos.circle(Isometry3d::from_translation(Vec3::new(tip[0] as f32, tip[1] as f32, 0.001)), 0.006, Color::srgb(1.0, 0.3, 0.3));
            }
        }
        AnyRobot::Physical(robot) => {
            let floor = robot.model.world.floor_z as f32;
            for i in -20..=20 {
                let x = i as f32 * 0.05;
                gizmos.line(Vec3::new(x, floor, -1.0), Vec3::new(x, floor, 1.0), ground);
                gizmos.line(Vec3::new(-1.0, floor, x), Vec3::new(1.0, floor, x), ground);
            }
            for (_, point, axes) in robot.joint_frames() {
                let p = to_bevy([point.x, point.y, point.z]);
                for (k, a) in axes.iter().enumerate() {
                    let d = to_bevy([a.x, a.y, a.z]) * 0.02;
                    let color = [Color::srgb(1.0, 0.95, 0.3), Color::srgb(0.3, 1.0, 0.95), Color::srgb(1.0, 0.5, 1.0)][k % 3];
                    gizmos.line(p - d, p + d, color);
                }
                gizmos.sphere(Isometry3d::from_translation(p), 0.003, Color::WHITE);
            }
            if sim.show_contacts {
                for c in robot.contacts() {
                    let p = to_bevy([c.point.x, c.point.y, c.point.z]);
                    let f = to_bevy([c.force.x, c.force.y, c.force.z]) * 0.005;
                    let color = if c.other.is_some() { Color::srgb(1.0, 0.4, 0.2) } else { Color::srgb(1.0, 0.2, 0.2) };
                    gizmos.sphere(Isometry3d::from_translation(p), 0.002, color);
                    gizmos.line(p, p + f, color);
                }
            }
            for (_, point, u) in robot.deflections() {
                let p = to_bevy([point.x, point.y, point.z]);
                let d = to_bevy([u.x, u.y, u.z]) * 50.0;
                gizmos.line(p, p + d, Color::srgb(0.6, 1.0, 0.6));
            }
        }
    }
}

fn status(sim: Res<CadSim>, mut query: Query<&mut Text, With<StatusText>>) {
    let Ok(mut text) = query.single_mut() else { return };
    let mut lines = vec![format!("{}  {}", sim.path, if sim.paused { "(paused)" } else { "" })];
    if let Some(e) = &sim.error {
        lines.push(format!("error: {e}"));
    }
    if let Some(robot) = sim.robot.as_ref() {
        let ratio = if sim.sim_seconds > 0.0 { sim.wall_seconds / sim.sim_seconds } else { 0.0 };
        lines.push(format!("t = {:.2} s  ×{}  ({:.1}× real time)", robot.time(), sim.speed, ratio));
        let targets = robot.targets();
        for (j, (name, angle)) in robot.joint_names().iter().zip(robot.joint_angles()).enumerate() {
            lines.push(format!("{} {:<18} {:7.2}°  → {:7.2}°", if j == sim.selected { "▸" } else { " " }, name, angle.to_degrees(), targets.get(j).copied().unwrap_or(0.0).to_degrees()));
        }
        if let AnyRobot::Physical(robot) = robot {
            for w in robot.warnings.iter().take(3) {
                lines.push(format!("warning: {w}"));
            }
            if sim.stress {
                lines.push(match &sim.results {
                    Some(r) => format!("stress from {} (peak {} MPa on the reddest link; blue = 0.1 % of yield, red = yield)", results_path(&sim.path), r["links"].as_object().map(|l| l.values().map(|v| v["peak_stress_pa"].as_f64().unwrap_or(0.0)).fold(0.0, f64::max) / 1e6).map(|x| format!("{x:.2}")).unwrap_or("?".into())),
                    None => "no results file yet: run `sim-cad run <model>` to write one".to_owned(),
                });
            }
        }
        lines.push("Space pause · R rebuild · ←/→ select joint · ↑/↓ move target (Shift: faster) · S stress · C contacts · +/- speed · save in robocad to reload".to_owned());
    }
    text.0 = lines.join("\n");
}

fn orbit_camera(mut query: Query<(&mut Transform, &mut OrbitCamera)>, mouse: Res<ButtonInput<MouseButton>>, mut motion: EventReader<bevy::input::mouse::MouseMotion>, mut wheel: EventReader<bevy::input::mouse::MouseWheel>) {
    let Ok((mut transform, mut cam)) = query.single_mut() else { return };
    let mut delta = Vec2::ZERO;
    for m in motion.read() {
        delta += m.delta;
    }
    for w in wheel.read() {
        cam.radius = (cam.radius * (1.0 - w.y * 0.05)).clamp(0.05, 20.0);
    }
    if mouse.pressed(MouseButton::Left) {
        cam.yaw -= delta.x * 0.005;
        cam.pitch = (cam.pitch + delta.y * 0.005).clamp(-1.4, 1.4);
    }
    if mouse.pressed(MouseButton::Right) {
        let right = transform.right();
        let up = transform.up();
        let radius = cam.radius;
        cam.focus -= right * delta.x * 0.001 * radius;
        cam.focus += up * delta.y * 0.001 * radius;
    }
    let offset = Vec3::new(cam.yaw.sin() * cam.pitch.cos(), cam.pitch.sin(), cam.yaw.cos() * cam.pitch.cos()) * cam.radius;
    *transform = Transform::from_translation(cam.focus + offset).looking_at(cam.focus, Vec3::Y);
}

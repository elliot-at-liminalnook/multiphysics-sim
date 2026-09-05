//! Interactive viewer for every phenomenon in the surprise suite.
//!
//! Renders any [`Exhibit`] through one entity pool: spheres, rods and blocks
//! as meshes; lines, arrows and polylines as gizmos; the exhibit's signal on
//! a chart board behind the scene; readouts and controls as text.

use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use sim_phenomena::exhibit::{Exhibit, Shape};

#[derive(Resource)]
struct Gallery {
    exhibits: Vec<Box<dyn Exhibit>>,
    current: usize,
    paused: bool,
    speed: f64,
    accumulator: f64,
    chart: Vec<f64>,
    chart_range: (f64, f64),
    chart_clock: f64,
    last_error: Option<String>,
    /// Wall seconds spent advancing, simulated seconds advanced, and the
    /// wall clock of the last report: `SIM_VIEWER_STATS=1` prints them.
    stats: (f64, f64, f64),
}

#[derive(Component)]
struct Pooled {
    kind: usize,
}

#[derive(Resource)]
struct Pool {
    meshes: [Handle<Mesh>; 3],
    entities: Vec<(Entity, Handle<StandardMaterial>)>,
}

#[derive(Component)]
struct Status;

#[derive(Component)]
struct OrbitCamera {
    focus: Vec3,
    radius: f32,
    yaw: f32,
    pitch: f32,
}

/// The chart board shows the last minute of real time, sampled at 30 Hz.
const CHART_POINTS: usize = 1800;
const CHART_INTERVAL: f64 = 1.0 / 30.0;

pub fn run() {
    let exhibits = sim_phenomena::exhibits::all();
    // `PHENOMENA_EXHIBIT=<number or title fragment>` opens on that exhibit.
    let current = std::env::var("PHENOMENA_EXHIBIT")
        .ok()
        .and_then(|wanted| {
            let lower = wanted.to_lowercase();
            wanted.parse::<usize>().ok().and_then(|n| n.checked_sub(1)).filter(|n| *n < exhibits.len())
                .or_else(|| exhibits.iter().position(|e| e.title().to_lowercase().contains(&lower)))
        })
        .unwrap_or(0);
    App::new()
        .insert_resource(Gallery {
            exhibits,
            current,
            paused: false,
            speed: 1.0,
            accumulator: 0.0,
            stats: (0.0, 0.0, 0.0),
            chart: Vec::new(),
            chart_range: (-1.0, 1.0),
            chart_clock: 0.0,
            last_error: None,
        })
        .insert_resource(AmbientLight {
            color: Color::srgb(0.85, 0.88, 0.95),
            brightness: 650.0,
            affects_lightmapped_meshes: true,
        })
        .insert_resource(ClearColor(Color::srgb(0.94, 0.95, 0.965)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "The surprise suite".to_owned(),
                resolution: (1380.0_f32, 840.0_f32).into(),
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (keyboard_controls, orbit_camera, advance, render, update_status).chain(),
        )
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let focus = Vec3::new(0.0, 0.3, 0.0);
    let position = Vec3::new(3.0, 2.6, 9.0);
    let offset = position - focus;
    let radius = offset.length();
    commands.spawn((
        Camera3d::default(),
        Transform::from_translation(position).looking_at(focus, Vec3::Y),
        OrbitCamera { focus, radius, yaw: offset.x.atan2(offset.z), pitch: (offset.y / radius).asin() },
    ));
    commands.spawn((
        DirectionalLight { illuminance: 9_000.0, shadows_enabled: true, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, -0.5, 0.0)),
    ));
    commands.insert_resource(Pool {
        meshes: [
            meshes.add(Sphere::new(1.0)),
            meshes.add(Cylinder::new(1.0, 1.0)),
            meshes.add(Cuboid::new(2.0, 2.0, 2.0)),
        ],
        entities: Vec::new(),
    });
    commands.spawn((
        Text::new("Starting the surprise suite..."),
        TextFont { font_size: 15.0, ..default() },
        TextColor(Color::srgb(0.12, 0.15, 0.19)),
        Node { position_type: PositionType::Absolute, top: Val::Px(14.0), left: Val::Px(18.0), ..default() },
        Status,
    ));
}

fn keyboard_controls(keys: Res<ButtonInput<KeyCode>>, mut gallery: ResMut<Gallery>) {
    let count = gallery.exhibits.len();
    let mut switched = false;
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if keys.just_pressed(KeyCode::BracketRight) || keys.just_pressed(KeyCode::KeyN) || (keys.just_pressed(KeyCode::Tab) && !shift) {
        gallery.current = (gallery.current + 1) % count;
        switched = true;
    }
    if keys.just_pressed(KeyCode::BracketLeft) || keys.just_pressed(KeyCode::KeyP) || (keys.just_pressed(KeyCode::Tab) && shift) {
        gallery.current = (gallery.current + count - 1) % count;
        switched = true;
    }
    let digits = [
        KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5,
        KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9, KeyCode::Digit0,
    ];
    for (i, key) in digits.iter().enumerate() {
        if keys.just_pressed(*key) && i < count {
            gallery.current = i;
            switched = true;
        }
    }
    let current = gallery.current;
    let mut nudge = 0.0;
    if keys.just_pressed(KeyCode::ArrowRight) { nudge += 1.0; }
    if keys.just_pressed(KeyCode::ArrowLeft) { nudge -= 1.0; }
    if nudge != 0.0 {
        let knob = gallery.exhibits[current].knob();
        let step = if shift { knob.step * 5.0 } else { knob.step };
        let value = (knob.value + nudge * step).clamp(knob.min, knob.max);
        let value = (value / knob.step).round() * knob.step;
        gallery.exhibits[current].set_knob(value);
        switched = true;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        gallery.exhibits[current].reset();
        switched = true;
    }
    if keys.just_pressed(KeyCode::Space) { gallery.paused = !gallery.paused; }
    if keys.just_pressed(KeyCode::ArrowUp) { gallery.speed = (gallery.speed * 2.0).min(64.0); }
    if keys.just_pressed(KeyCode::ArrowDown) { gallery.speed = (gallery.speed / 2.0).max(1.0 / 64.0); }
    if switched {
        gallery.chart.clear();
        gallery.chart_clock = 0.0;
        gallery.accumulator = 0.0;
        gallery.last_error = None;
    }
}

fn advance(time: Res<Time>, mut gallery: ResMut<Gallery>) {
    if gallery.paused || gallery.last_error.is_some() { return; }
    let current = gallery.current;
    let scale = gallery.exhibits[current].time_scale() * gallery.speed;
    let real = time.delta_secs_f64().min(0.05);
    let mut dt = real * scale;
    if dt <= 0.0 { return; }
    // A gridded exhibit takes whole steps: the remainder carries over.
    let grid = gallery.exhibits[current].grid();
    if grid > 0.0 {
        gallery.accumulator += dt;
        let whole = (gallery.accumulator / grid).floor();
        if whole < 1.0 { return; }
        dt = whole * grid;
        gallery.accumulator -= dt;
    }
    let started = std::time::Instant::now();
    if let Err(error) = gallery.exhibits[current].advance(dt) {
        gallery.last_error = Some(error);
        return;
    }
    gallery.stats.0 += started.elapsed().as_secs_f64();
    gallery.stats.1 += dt;
    gallery.stats.2 += real;
    if gallery.stats.2 >= 1.0 {
        if std::env::var_os("SIM_VIEWER_STATS").is_some() {
            let (wall, sim, real) = gallery.stats;
            eprintln!("viewer: {:.3} s simulated in {:.3} s of {:.3} s wall ({:.2}× real time), sim t = {:.2}", sim, wall, real, sim / real, gallery.exhibits[current].time());
        }
        gallery.stats = (0.0, 0.0, 0.0);
    }
    gallery.chart_clock += real;
    if gallery.chart_clock >= CHART_INTERVAL {
        gallery.chart_clock -= CHART_INTERVAL;
        let (_, value) = gallery.exhibits[current].signal();
        gallery.chart.push(value);
    }
    if gallery.chart.len() > CHART_POINTS {
        let n = gallery.chart.len() - CHART_POINTS;
        gallery.chart.drain(..n);
    }
    let (lo, hi) = gallery.chart.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
    if lo.is_finite() {
        let pad = ((hi - lo) * 0.1).max(1.0e-6);
        gallery.chart_range = (lo - pad, hi + pad);
    }
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

fn color(c: [f32; 3]) -> Color {
    Color::srgb(c[0], c[1], c[2])
}

#[allow(clippy::too_many_arguments)]
fn render(
    mut commands: Commands,
    gallery: Res<Gallery>,
    mut pool: ResMut<Pool>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut pooled: Query<(&mut Transform, &mut Visibility, &mut Mesh3d, &mut Pooled)>,
    mut gizmos: Gizmos,
) {
    let exhibit = &gallery.exhibits[gallery.current];
    let mut shapes = Vec::with_capacity(256);
    exhibit.shapes(&mut shapes);

    // Chart board behind the scene.
    let (x0, x1, y0, y1, z) = (-3.4_f32, 3.4_f32, -2.6_f32, -1.4_f32, -2.5_f32);
    let board = Color::srgb(0.72, 0.75, 0.79);
    gizmos.line(Vec3::new(x0, y0, z), Vec3::new(x1, y0, z), board);
    gizmos.line(Vec3::new(x0, y1, z), Vec3::new(x1, y1, z), board);
    gizmos.line(Vec3::new(x0, y0, z), Vec3::new(x0, y1, z), board);
    let (lo, hi) = gallery.chart_range;
    let n = gallery.chart.len();
    if n > 1 {
        let points = gallery.chart.iter().enumerate().map(|(i, v)| {
            let x = x0 + (x1 - x0) * i as f32 / (CHART_POINTS - 1) as f32;
            let y = y0 + (y1 - y0) * ((v - lo) / (hi - lo)) as f32;
            Vec3::new(x, y, z)
        });
        gizmos.linestrip(points, Color::srgb(0.86, 0.32, 0.20));
    }

    let mut mesh_shapes = Vec::new();
    for shape in shapes {
        match shape {
            Shape::Line { from, to, color: c } => gizmos.line(v3(from), v3(to), color(c)),
            Shape::Arrow { from, to, color: c } => { gizmos.arrow(v3(from), v3(to), color(c)); }
            Shape::Polyline { points, color: c } => gizmos.linestrip(points.into_iter().map(v3), color(c)),
            other => mesh_shapes.push(other),
        }
    }
    while pool.entities.len() < mesh_shapes.len() {
        let material = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.6, ..default() });
        let entity = commands
            .spawn((
                Mesh3d(pool.meshes[0].clone()),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                Visibility::Hidden,
                Pooled { kind: 0 },
            ))
            .id();
        pool.entities.push((entity, material));
    }
    for (index, (entity, material)) in pool.entities.iter().enumerate() {
        let Ok((mut transform, mut visibility, mut mesh, mut pooled)) = pooled.get_mut(*entity) else { continue };
        let Some(shape) = mesh_shapes.get(index) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        *visibility = Visibility::Visible;
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
                Transform::from_translation(v3(*center))
                    .with_rotation(Quat::from_xyzw(rotation[1] as f32, rotation[2] as f32, rotation[3] as f32, rotation[0] as f32))
                    .with_scale(v3(*half)),
                *color,
            ),
            _ => unreachable!(),
        };
        if pooled.kind != kind {
            pooled.kind = kind;
            mesh.0 = pool.meshes[kind].clone();
        }
        *transform = t;
        if let Some(m) = materials.get_mut(material) {
            m.base_color = color(c);
        }
    }
}

/// Bevy's built-in font carries no Greek, arrows, subscripts or typographic
/// punctuation; spell them out so the overlay never shows missing-glyph boxes.
fn ascii(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        let replacement: &str = match c {
            'α' => "alpha", 'β' => "beta", 'γ' => "gamma", 'δ' => "delta", 'ε' => "eps", 'ζ' => "zeta",
            'η' => "eta", 'θ' => "theta", 'κ' => "kappa", 'λ' => "lambda", 'μ' | 'µ' => "u", 'ξ' => "xi",
            'π' => "pi", 'ρ' => "rho", 'σ' => "sigma", 'τ' => "tau", 'φ' => "phi", 'χ' => "chi",
            'ψ' => "psi", 'ω' => "omega", 'Δ' => "delta", 'Ω' => "Omega", 'Θ' => "Theta",
            'ê' => "e", 'é' => "e",
            '₀' => "0", '₁' => "1", '₂' => "2", '₃' => "3", '₄' => "4",
            '⁰' => "^0", '¹' => "^1", '²' => "^2", '³' => "^3", '⁻' => "^-",
            '→' => "->", '←' => "<-", '↑' => "up", '↓' => "down", '⇒' => "=>",
            '·' => "|", '×' => "x", '−' => "-", '–' => "-", '—' => "-", '≈' => "~", '≤' => "<=", '≥' => ">=",
            '°' => " deg", '√' => "sqrt", '∝' => "~", '′' => "'", '…' => "...", '∞' => "inf",
            _ => { out.push(c); continue; }
        };
        out.push_str(replacement);
    }
    out
}

fn update_status(gallery: Res<Gallery>, mut text: Single<&mut Text, With<Status>>) {
    let exhibit = &gallery.exhibits[gallery.current];
    let knob = exhibit.knob();
    let readouts = exhibit
        .readouts()
        .iter()
        .map(|r| format!("  {:<30} {:>12.4} {}", r.label, r.value, r.unit))
        .collect::<Vec<_>>()
        .join("\n");
    let (signal, _) = exhibit.signal();
    let error = gallery.last_error.as_deref().map(|e| format!("\nSimulation error: {e}\n")).unwrap_or_default();
    **text = Text::new(ascii(&format!(
        "{n:02} / {count}  {title}\n{summary}\n\n{verdict}\n\nknob  {label}: {value:.4} {unit}   [{min} .. {max}]   <- -> adjust (shift x5)\n\n{readouts}\n\n  t = {time:.3} {tunit}   speed x{speed}   chart: {signal}{error}\n\n] next  [ previous  (or Tab / shift-Tab, N / P; digits 1-9,0 jump to the first ten) | R reset | space pause | up/down speed | left-drag orbit | wheel zoom",
        n = gallery.current + 1,
        count = gallery.exhibits.len(),
        title = exhibit.title(),
        summary = exhibit.summary(),
        verdict = exhibit.verdict(),
        label = knob.label,
        value = knob.value,
        unit = knob.unit,
        min = knob.min,
        max = knob.max,
        time = exhibit.time(),
        tunit = exhibit.time_unit(),
        speed = gallery.speed,
    )));
}

fn orbit_camera(
    buttons: Res<ButtonInput<MouseButton>>,
    mut mouse_motion: EventReader<MouseMotion>,
    mut mouse_wheel: EventReader<MouseWheel>,
    camera: Single<(&mut Transform, &mut OrbitCamera)>,
) {
    let drag = mouse_motion.read().fold(Vec2::ZERO, |sum, event| sum + event.delta);
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
    orbit.radius = (orbit.radius * (-zoom * 0.12).exp()).clamp(3.0, 30.0);
    let horizontal = orbit.pitch.cos() * orbit.radius;
    transform.translation = orbit.focus
        + Vec3::new(orbit.yaw.sin() * horizontal, orbit.pitch.sin() * orbit.radius, orbit.yaw.cos() * horizontal);
    transform.look_at(orbit.focus, Vec3::Y);
}

//! The shared camera without a window: RoboCAD's presets in the display
//! frame, fit framing, the projection toggle, zoom toward the cursor,
//! trackball, glides, the viewport and gesture areas, the gestures and the
//! numpad keys on a MinimalPlugins app, the action handler, and the
//! `camera_*` / `camera:*` routing.
use super::apply::{FOV_DEG, apply, control_action};
use super::input::{ALT_DRAG_SLOP, DragKind, arrow_action, drag_kind, keys, navigate};
use super::orbit::area_aspect;
use super::viewport::{area, viewport, wanted};
use super::*;
use crate::app::actions::{Act, Action, Origin, Replies, control_matches};
use crate::app::route::{annotate_for, route};
use bevy::camera::ScalingMode;
use bevy::prelude::*;
use bevy::ecs::message::Messages;
use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::input::touch::TouchPhase;
use bevy::window::{PrimaryWindow, WindowResolution};
use sim_api::Outcome;

const EPS: f32 = 1e-5;

fn close(a: Vec3, b: Vec3, eps: f32) -> bool {
    (a - b).length() < eps
}

/// Rules whose pitch limit lets top and bottom reach RoboCAD's 89.5°.
fn cad_rules() -> OrbitRules {
    OrbitRules { pitch_limit: 89.5f32.to_radians(), radius: RadiusLimits::Extent { min: 0.05, max: 40.0 }, zoom_to_cursor: true, yield_to_ui: true, keys: false, ..default() }
}

/// The direction from the focus toward the eye.
fn eye_direction(orbit: &Orbit) -> Vec3 {
    orbit.rotation() * Vec3::Z
}

/// RoboCAD's eye direction for (yaw, pitch) degrees, model frame (Z up), as display (x, z, −y).
fn robocad_direction(yaw: f32, pitch: f32) -> Vec3 {
    let (y, p) = (yaw.to_radians(), pitch.to_radians());
    let model = Vec3::new(p.cos() * y.cos(), p.cos() * y.sin(), p.sin());
    Vec3::new(model.x, model.z, -model.y)
}

#[test]
fn presets_look_as_robocads_views() {
    let rules = cad_rules();
    for view in ViewPreset::ALL {
        let mut orbit = Orbit { radius: 1.0, ..default() };
        orbit.preset(view, &rules, 0.0);
        let (yaw, pitch) = view.robocad_degrees();
        assert!(close(eye_direction(&orbit), robocad_direction(yaw, pitch), EPS), "{view:?}: {:?} vs {:?}", eye_direction(&orbit), robocad_direction(yaw, pitch));
        assert!(orbit.glide.is_none() && orbit.trackball.is_none(), "{view:?} cuts to the turntable");
    }
    let at = |view| {
        let mut orbit = Orbit { radius: 1.0, ..default() };
        orbit.preset(view, &rules, 0.0);
        eye_direction(&orbit)
    };
    // Front looks along display −Z (model +Y), from model −Y (display +Z).
    assert!(close(at(ViewPreset::Front), Vec3::Z, EPS));
    assert!(close(at(ViewPreset::Back), -Vec3::Z, EPS));
    // Right from model +X; top looks down (model +Z is display +Y).
    assert!(close(at(ViewPreset::Right), Vec3::X, EPS));
    assert!(close(at(ViewPreset::Left), -Vec3::X, EPS));
    assert!(at(ViewPreset::Top).y > 0.9999 && at(ViewPreset::Bottom).y < -0.9999);
    // The front view's camera is upright (display +Y up).
    let mut front = Orbit { radius: 1.0, ..default() };
    front.preset(ViewPreset::Front, &rules, 0.0);
    assert!(close(front.rotation() * Vec3::Y, Vec3::Y, EPS));
    // A mode's pitch limit holds (Robot's 1.4 rad).
    let mut robot = Orbit { radius: 1.0, ..default() };
    robot.preset(ViewPreset::Top, &OrbitRules::default(), 0.0);
    assert_eq!(robot.pitch, 1.4);
}

#[test]
fn robocad_angles_round_trip() {
    for (yaw, pitch) in [(-90.0, 0.0), (90.0, 0.0), (0.0, 0.0), (180.0, 0.0), (-35.0, 28.0), (-90.0, 89.5), (135.0, -40.0), (-179.0, 10.0)] {
        let (y, p) = robocad_to_display(yaw, pitch);
        let (ry, rp) = display_to_robocad(y, p);
        let same = |a: f32, b: f32| ((a - b + 540.0).rem_euclid(360.0) - 180.0).abs() < 1e-3;
        assert!(same(ry, yaw) && (rp - pitch).abs() < 1e-3, "({yaw}, {pitch}) came back as ({ry}, {rp})");
        assert!(ry > -180.0 && ry <= 180.0);
    }
}

#[test]
fn opposite_turns_round_and_negates_pitch() {
    let rules = cad_rules();
    let mut orbit = Orbit { radius: 1.0, yaw: 0.3, pitch: 0.4, ..default() };
    let before = eye_direction(&orbit);
    orbit.opposite(&rules);
    let after = eye_direction(&orbit);
    assert!(close(after, Vec3::new(-before.x, -before.y, -before.z), EPS), "{before:?} → {after:?}");
}

#[test]
fn fit_frames_the_bounds() {
    let mut orbit = Orbit { radius: 1.0, yaw: 0.7, pitch: 0.45, centre: Vec3::new(1.0, 2.0, 3.0), extent: 0.5, ..default() };
    let rules = OrbitRules { framing: Framing::Bounds { scale: 3.2, aspect: false, view: Some((0.1, 0.2)) }, ..default() };
    orbit.frame(&rules, 2.0, false);
    assert!((orbit.radius - 1.6).abs() < EPS && orbit.focus == Vec3::new(1.0, 2.0, 3.0));
    // A fit keeps the heading; home takes the framing's view.
    assert!((orbit.yaw - 0.7).abs() < EPS && (orbit.pitch - 0.45).abs() < EPS);
    orbit.home = true;
    orbit.frame(&rules, 2.0, true);
    assert!((orbit.yaw - 0.1).abs() < EPS && (orbit.pitch - 0.2).abs() < EPS && !orbit.home);
    // The aspect-aware framing divides by the aspect clamped to 0.1–1.
    let rules = OrbitRules { framing: Framing::Bounds { scale: 3.2, aspect: true, view: None }, ..default() };
    for (aspect, radius) in [(0.5, 3.2), (2.0, 1.6), (0.05, 16.0)] {
        assert!((orbit.framing(&rules, aspect, true).radius - radius).abs() < 1e-4, "aspect {aspect}");
    }
    // A fixed framing (Phenomena) is its pose for home.
    let pose = Pose { focus: Vec3::Y, radius: 9.0, yaw: 0.2, pitch: 0.3 };
    let rules = OrbitRules { framing: Framing::Fixed(pose), ..default() };
    assert_eq!(orbit.framing(&rules, 1.0, false), pose);
    // A fit in the trackball keeps its rotation; only focus and distance move.
    let rules = OrbitRules::default();
    let mut orbit = Orbit { radius: 1.0, centre: Vec3::X, extent: 0.5, ..default() };
    orbit.set_trackball(true, &rules);
    orbit.rotate(Vec2::new(40.0, 25.0), &rules);
    let rotation = orbit.rotation();
    orbit.frame(&rules, 1.0, false);
    assert!(orbit.trackball.is_some() && orbit.rotation().angle_between(rotation) < 1e-5 && orbit.focus == Vec3::X);
}

#[test]
fn the_projection_toggles_between_perspective_and_orthographic() {
    // CAD's perspective: a millimetre near plane.
    let cad = Projection::Perspective(PerspectiveProjection { near: 0.001, near_clip_plane: Vec4::new(0.0, 0.0, -1.0, -0.001), ..default() });
    let mut orbit = Orbit { radius: 0.5, extent: 0.15, ..default() };
    assert!(orbit.projection(&cad).is_none(), "an unchanged perspective is not rewritten");
    orbit.fov = 40f32.to_radians();
    match orbit.projection(&cad) {
        Some(Projection::Perspective(p)) => assert!(p.near == 0.001 && p.near_clip_plane == Vec4::new(0.0, 0.0, -1.0, -0.001) && (p.fov - orbit.fov).abs() < 1e-6),
        _ => panic!("a fov change keeps the perspective"),
    }
    orbit.orthographic = true;
    let ortho = orbit.projection(&cad).expect("orthographic");
    match &ortho {
        Projection::Orthographic(o) => match o.scaling_mode {
            ScalingMode::FixedVertical { viewport_height } => assert!((viewport_height - 2.0 * 0.5 * (orbit.fov / 2.0).tan()).abs() < 1e-6, "{viewport_height}"),
            _ => panic!("RoboCAD's height: 2 × distance × tan(fov/2)"),
        },
        _ => panic!("orthographic"),
    }
    assert!(orbit.projection(&ortho).is_none(), "an unchanged orthographic is not rewritten");
    orbit.orthographic = false;
    // Without a recorded perspective, Bevy's default near plane.
    match orbit.projection(&ortho) {
        Some(Projection::Perspective(p)) => assert!(p.near == PerspectiveProjection::default().near && (p.fov - orbit.fov).abs() < 1e-6),
        _ => panic!("back to perspective"),
    }
    // The mode's own near plane, as `place` records it while perspective.
    orbit.perspective = Some((0.001, Vec4::new(0.0, 0.0, -1.0, -0.001)));
    match orbit.projection(&ortho) {
        Some(Projection::Perspective(p)) => assert!(p.near == 0.001 && p.near_clip_plane.w == -0.001 && (p.fov - orbit.fov).abs() < 1e-6),
        _ => panic!("back to perspective"),
    }
}

/// A point's normalised device position seen by a perspective camera at
/// the orbit's placement (camera looks along its −Z).
fn ndc(orbit: &Orbit, point: Vec3, aspect: f32) -> Vec2 {
    let v = orbit.rotation().inverse() * (point - orbit.eye());
    let t = (orbit.fov * 0.5).tan();
    Vec2::new(v.x / (-v.z * t * aspect), v.y / (-v.z * t))
}

#[test]
fn zoom_toward_the_cursor_keeps_the_point_under_it() {
    let rules = cad_rules();
    let aspect = 1.6;
    let mut orbit = Orbit { focus: Vec3::new(0.1, 0.0, 0.2), radius: 2.0, yaw: 0.5, pitch: 0.3, extent: 1.0, ..default() };
    // The cursor at NDC (0.4, −0.2): its ray from the eye meets the focus plane at the anchor.
    let cursor = Vec2::new(0.4, -0.2);
    let t = (orbit.fov * 0.5).tan();
    let direction = (orbit.rotation() * Vec3::new(cursor.x * t * aspect, cursor.y * t, -1.0)).normalize();
    let anchor = orbit.focus_plane_hit(orbit.eye(), direction).expect("the ray meets the focus plane");
    assert!((ndc(&orbit, anchor, aspect) - cursor).length() < 1e-4);
    let applied = orbit.zoom(0.5, Some(anchor), &rules);
    assert!((applied - 0.5).abs() < EPS && (orbit.radius - 1.0).abs() < EPS);
    assert!((ndc(&orbit, anchor, aspect) - cursor).length() < 1e-4, "{:?}", ndc(&orbit, anchor, aspect));
    // A point off the focus plane stays put too (the eye scales about the anchor).
    let off = anchor + Vec3::new(0.05, 0.1, -0.2);
    let before = ndc(&orbit, off, aspect);
    orbit.zoom(1.3, Some(off), &rules);
    assert!((ndc(&orbit, off, aspect) - before).length() < 1e-4);
    // Clamped by the limits (extent × 0.05): the anchor still stays put.
    let before = ndc(&orbit, anchor, aspect);
    let applied = orbit.zoom(1e-4, Some(anchor), &rules);
    assert!((orbit.radius - 0.05).abs() < EPS && applied > 1e-4, "{}", orbit.radius);
    assert!((ndc(&orbit, anchor, aspect) - before).length() < 1e-4);
    orbit.zoom(1e6, None, &rules);
    assert!((orbit.radius - 40.0).abs() < 1e-3);
    // Nonsense factors do nothing.
    for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let was = orbit.clone();
        assert_eq!(orbit.zoom(bad, None, &rules), 1.0);
        assert_eq!(orbit, was);
    }
    // A ray running away from the focus plane has no anchor.
    assert!(orbit.focus_plane_hit(orbit.eye(), orbit.rotation() * Vec3::Z).is_none());
}

#[test]
fn trackball_enter_and_leave_keep_the_view_direction() {
    let rules = OrbitRules::default();
    let mut orbit = Orbit { radius: 1.0, yaw: 0.7, pitch: 0.4, ..default() };
    let before = eye_direction(&orbit);
    orbit.set_trackball(true, &rules);
    assert!(orbit.trackball.is_some() && close(eye_direction(&orbit), before, EPS));
    orbit.rotate(Vec2::new(30.0, -20.0), &rules);
    let turned = eye_direction(&orbit);
    assert!(!close(turned, before, 1e-3), "the trackball turns");
    orbit.set_trackball(false, &rules);
    assert!(orbit.trackball.is_none() && close(eye_direction(&orbit), turned, 1e-4), "{:?} vs {turned:?}", eye_direction(&orbit));
    // A preset returns to the turntable.
    orbit.set_trackball(true, &rules);
    orbit.preset(ViewPreset::Iso, &rules, 0.0);
    assert!(orbit.trackball.is_none());
}

#[test]
fn a_glide_reaches_its_target_and_heading_names_it() {
    let mut orbit = Orbit { focus: Vec3::X, radius: 1.0, ..default() };
    let to = Pose { focus: Vec3::Y, radius: 2.0, yaw: 1.0, pitch: 0.3 };
    orbit.glide_to(to, GLIDE_S);
    let heads_to = |o: &Orbit| (o.heading().0 - 1.0).abs() < EPS && (o.heading().1 - 0.3).abs() < EPS;
    assert!(heads_to(&orbit));
    orbit.step(GLIDE_S * 0.4);
    assert!(orbit.glide.is_some() && heads_to(&orbit));
    assert!(orbit.yaw > 0.0 && orbit.yaw < 1.0);
    orbit.step(GLIDE_S * 0.4);
    orbit.step(GLIDE_S * 0.4);
    assert!(orbit.glide.is_none());
    assert!(close(orbit.focus, Vec3::Y, EPS) && (orbit.radius - 2.0).abs() < 1e-4 && (orbit.yaw - 1.0).abs() < EPS && (orbit.pitch - 0.3).abs() < EPS);
    // An unplaced view (radius 0) cuts; a user gesture stops scripted motion.
    let mut fresh = Orbit::default();
    fresh.glide_to(to, GLIDE_S);
    assert!(fresh.glide.is_none() && fresh.radius == 2.0);
    orbit.glide_to(Pose { yaw: -1.0, ..to }, GLIDE_S);
    orbit.spin = 0.2;
    orbit.interrupt();
    assert!(orbit.glide.is_none() && orbit.spin == 0.0);
}

fn window(physical: (u32, u32), scale: f32) -> Window {
    Window { resolution: WindowResolution::new(physical.0, physical.1).with_scale_factor_override(scale), ..default() }
}

#[test]
fn view_areas_set_the_viewport_and_where_gestures_start() {
    // 800 × 500 logical at scale 2.
    let w = window((1600, 1000), 2.0);
    let (viewport, sub) = wanted(&w, &ViewArea::Window);
    assert!(viewport.is_none() && sub.is_none());
    assert_eq!(area(&w, &ViewArea::Window), Rect::new(0.0, 0.0, 800.0, 500.0));
    let docks = ViewArea::Docks { left: 100.0, right: 100.0, top: 50.0, bottom: 50.0 };
    let (viewport, sub) = wanted(&w, &docks);
    let viewport = viewport.expect("the docks leave room");
    assert_eq!((viewport.physical_position, viewport.physical_size), (UVec2::new(200, 100), UVec2::new(1200, 800)));
    assert!(sub.is_none());
    assert_eq!(area(&w, &docks), Rect::new(100.0, 50.0, 700.0, 450.0));
    // Docks wider than the window: the whole window.
    let crowded = ViewArea::Docks { left: 500.0, right: 400.0, top: 50.0, bottom: 50.0 };
    assert!(wanted(&w, &crowded).0.is_none());
    assert_eq!(area(&w, &crowded), Rect::new(0.0, 0.0, 800.0, 500.0));
    // A lesson card (physical pixels), scrolled so its top 100 px are hidden.
    let card = ViewArea::Card { full: Rect::new(100.0, 100.0, 500.0, 400.0), visible: Rect::new(100.0, 200.0, 500.0, 400.0) };
    let (viewport, sub) = wanted(&w, &card);
    let viewport = viewport.expect("a card viewport");
    assert_eq!((viewport.physical_position, viewport.physical_size), (UVec2::new(100, 200), UVec2::new(400, 200)));
    assert_eq!(sub, Some(bevy::camera::SubCameraView { full_size: UVec2::new(400, 300), offset: Vec2::new(0.0, 100.0), size: UVec2::new(400, 200) }));
    assert_eq!(area(&w, &card), Rect::new(50.0, 100.0, 250.0, 200.0));
    // Scrolled out of sight: a 1 × 1 viewport, and no gesture starts anywhere.
    let hidden = ViewArea::Card { full: Rect::new(100.0, 100.0, 500.0, 400.0), visible: Rect::new(100.0, 400.0, 500.0, 400.0) };
    let (viewport, sub) = wanted(&w, &hidden);
    assert_eq!(viewport.map(|v| v.physical_size), Some(UVec2::ONE));
    assert!(sub.is_none());
    assert!(!area(&w, &hidden).contains(Vec2::ZERO) && !area(&w, &hidden).contains(Vec2::new(100.0, 150.0)));
}

// ---- systems on a MinimalPlugins app ----

fn spawn_orbit(app: &mut App, orbit: Orbit, rules: OrbitRules, area: ViewArea) -> Entity {
    app.world_mut().spawn((Camera::default(), GlobalTransform::default(), orbit, rules, area)).id()
}

fn handler_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    crate::app::actions::register::<CameraAction>(&mut app);
    app.init_resource::<Replies>().add_systems(Update, apply);
    app
}

/// One REST camera command through the handler, answered.
fn rest(app: &mut App, action: CameraAction) -> Result<Value, String> {
    let reply = app.world_mut().resource_mut::<Replies>().open();
    app.world_mut().write_message(Act { action, origin: Origin::Rest(reply) });
    app.update();
    match app.world_mut().resource_mut::<Replies>().take(reply) {
        Some(Outcome::Done(result)) => result,
        Some(_) => panic!("the camera answers at once, with JSON"),
        None => panic!("no answer"),
    }
}

fn orbit_of(app: &App, entity: Entity) -> Orbit {
    app.world().get::<Orbit>(entity).expect("an orbit camera").clone()
}

#[test]
fn the_handler_applies_camera_actions_and_refuses_bad_values() {
    let mut app = handler_app();
    let cam = spawn_orbit(&mut app, Orbit { radius: 1.0, extent: 0.1, ..default() }, OrbitRules::default(), ViewArea::Window);
    // A key's view: Robot's pitch limit (1.4 rad) holds RoboCAD's 89.5° top.
    app.world_mut().write_message(Act::ui(CameraAction::View { view: ViewPreset::Top }));
    app.update();
    let orbit = orbit_of(&app, cam);
    assert_eq!(orbit.pitch, 89.5f32.to_radians().clamp(-1.4, 1.4));
    assert!(orbit.yaw.abs() < EPS);
    let state = rest(&mut app, CameraAction::State).unwrap();
    assert!((state["camera"]["pitch"].as_f64().unwrap() - 1.4).abs() < 1e-6, "{state}");
    // Field of view: RoboCAD's 5–120 degrees.
    let e = rest(&mut app, CameraAction::Fov { degrees: 2.0 }).unwrap_err();
    assert!(e.contains("5–120"), "{e}");
    assert!(rest(&mut app, CameraAction::Fov { degrees: f32::NAN }).is_err());
    rest(&mut app, CameraAction::Fov { degrees: 40.0 }).unwrap();
    assert!((orbit_of(&app, cam).fov - 40f32.to_radians()).abs() < 1e-6);
    // Zoom: a positive factor within the limits (extent 0.1 × 0.3–20).
    assert!(rest(&mut app, CameraAction::Zoom { factor: 0.0, at: None }).is_err());
    assert!(rest(&mut app, CameraAction::Zoom { factor: f32::INFINITY, at: None }).is_err());
    let answer = rest(&mut app, CameraAction::Zoom { factor: 0.5, at: None }).unwrap();
    assert!((orbit_of(&app, cam).radius - 0.5).abs() < EPS && (answer["applied"].as_f64().unwrap() - 0.5).abs() < 1e-6);
    // No viewport size yet (no window): `at` falls back to the focus, and says so.
    let answer = rest(&mut app, CameraAction::Zoom { factor: 0.8, at: Some([10.0, 10.0]) }).unwrap();
    assert!(answer["anchor"].is_null() && answer["note"].is_string(), "{answer}");
    // Projection and orbit mode toggle without an argument.
    rest(&mut app, CameraAction::Projection { orthographic: None }).unwrap();
    assert!(orbit_of(&app, cam).orthographic);
    rest(&mut app, CameraAction::Projection { orthographic: None }).unwrap();
    assert!(!orbit_of(&app, cam).orthographic);
    rest(&mut app, CameraAction::OrbitMode { mode: None }).unwrap();
    assert!(orbit_of(&app, cam).trackball.is_some());
    assert!(rest(&mut app, CameraAction::Spin { rate: 0.2 }).unwrap_err().contains("turntable"));
    rest(&mut app, CameraAction::OrbitMode { mode: Some(OrbitMode::Turntable) }).unwrap();
    assert!(orbit_of(&app, cam).trackball.is_none());
    rest(&mut app, CameraAction::Spin { rate: 0.2 }).unwrap();
    assert_eq!(orbit_of(&app, cam).spin, 0.2);
    // Any other action stops the spin.
    rest(&mut app, CameraAction::Orbit { dx: 10.0, dy: 0.0, degrees: None }).unwrap();
    assert_eq!(orbit_of(&app, cam).spin, 0.0);
    assert!(rest(&mut app, CameraAction::Pan { dx: f32::NAN, dy: 0.0 }).is_err());
    rest(&mut app, CameraAction::Home).unwrap();
    assert!(orbit_of(&app, cam).home, "place frames a home request");
    // camera_set: checked whole, then cut (or glided).
    let good = CameraState { focus: [1.0, 2.0, 3.0], radius: 0.5, yaw: 0.7, pitch: 0.45, orthographic: true, fov_deg: Some(30.0), trackball: None, seconds: None };
    for bad in [CameraState { radius: -1.0, ..good }, CameraState { fov_deg: Some(200.0), ..good }, CameraState { trackball: Some([0.0; 4]), ..good }, CameraState { focus: [f32::NAN, 0.0, 0.0], ..good }, CameraState { seconds: Some(-1.0), ..good }] {
        assert!(rest(&mut app, CameraAction::Set { state: bad }).is_err(), "{bad:?}");
    }
    rest(&mut app, CameraAction::Set { state: good }).unwrap();
    let orbit = orbit_of(&app, cam);
    assert!(orbit.focus == Vec3::new(1.0, 2.0, 3.0) && orbit.radius == 0.5 && orbit.yaw == 0.7 && orbit.pitch == 0.45 && orbit.orthographic && orbit.glide.is_none() && !orbit.home);
    assert!((orbit.fov - 30f32.to_radians()).abs() < 1e-6);
    rest(&mut app, CameraAction::Set { state: CameraState { yaw: -0.2, seconds: Some(1.0), trackball: Some([0.0, 0.0, 0.0, 2.0]), ..good } }).unwrap();
    let orbit = orbit_of(&app, cam);
    let (yaw, pitch) = orbit.heading();
    assert!(orbit.glide.is_some() && (yaw + 0.2).abs() < EPS && (pitch - 0.45).abs() < EPS && orbit.trackball == Some(Quat::IDENTITY));
}

#[test]
fn a_mode_without_an_orbit_camera_refuses_camera_actions() {
    let mut app = handler_app();
    let e = rest(&mut app, CameraAction::View { view: ViewPreset::Front }).unwrap_err();
    assert!(e.contains("no orbit camera"), "{e}");
    assert!(rest(&mut app, CameraAction::State).is_err());
}

fn motion(app: &mut App, delta: Vec2) {
    app.world_mut().write_message(MouseMotion { delta });
    app.update();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
}

#[test]
fn drags_latch_where_they_start_and_the_wheel_zooms_inside_the_view() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<MouseMotion>()
        .add_message::<MouseWheel>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_systems(Update, navigate);
    let win = app.world_mut().spawn((window((800, 500), 1.0), PrimaryWindow)).id();
    let rules = OrbitRules { zoom_modifier: true, ..default() };
    let cam = spawn_orbit(&mut app, Orbit { radius: 1.0, extent: 1.0, ..default() }, rules, ViewArea::Docks { left: 100.0, right: 100.0, top: 50.0, bottom: 50.0 });
    let cursor = |app: &mut App, p: Vec2| app.world_mut().get_mut::<Window>(win).unwrap().set_cursor_position(Some(p));
    let mouse = |app: &mut App, button: MouseButton, down: bool| {
        let mut buttons = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        if down { buttons.press(button) } else { buttons.release(button) }
    };
    // A right-drag that starts in the view orbits, and keeps orbiting over a dock.
    cursor(&mut app, Vec2::new(400.0, 250.0));
    mouse(&mut app, MouseButton::Right, true);
    motion(&mut app, Vec2::new(10.0, 0.0));
    assert!((orbit_of(&app, cam).yaw + 0.07).abs() < EPS);
    cursor(&mut app, Vec2::new(50.0, 250.0));
    motion(&mut app, Vec2::new(10.0, 0.0));
    assert!((orbit_of(&app, cam).yaw + 0.14).abs() < EPS);
    // Released: nothing; one that starts over a dock is not the camera's.
    mouse(&mut app, MouseButton::Right, false);
    motion(&mut app, Vec2::new(10.0, 0.0));
    mouse(&mut app, MouseButton::Right, true);
    motion(&mut app, Vec2::new(10.0, 0.0));
    assert!((orbit_of(&app, cam).yaw + 0.14).abs() < EPS);
    mouse(&mut app, MouseButton::Right, false);
    motion(&mut app, Vec2::ZERO);
    // A middle-drag in the view pans.
    cursor(&mut app, Vec2::new(400.0, 250.0));
    mouse(&mut app, MouseButton::Middle, true);
    motion(&mut app, Vec2::new(0.0, 20.0));
    let orbit = orbit_of(&app, cam);
    assert!(orbit.focus != Vec3::ZERO && (orbit.yaw + 0.14).abs() < EPS);
    mouse(&mut app, MouseButton::Middle, false);
    motion(&mut app, Vec2::ZERO);
    // The wheel needs Ctrl or Cmd here (zoom_modifier), and the cursor in the view.
    let wheel = |app: &mut App| {
        app.world_mut().write_message(MouseWheel { unit: MouseScrollUnit::Line, x: 0.0, y: 1.0, window: win, phase: TouchPhase::Moved });
        app.update();
    };
    wheel(&mut app);
    assert_eq!(orbit_of(&app, cam).radius, 1.0);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ControlLeft);
    wheel(&mut app);
    assert!((orbit_of(&app, cam).radius - (-0.12f32).exp()).abs() < EPS);
    cursor(&mut app, Vec2::new(50.0, 250.0));
    let radius = orbit_of(&app, cam).radius;
    wheel(&mut app);
    assert_eq!(orbit_of(&app, cam).radius, radius, "the wheel over a dock is the dock's");
    // A disabled camera (its mode's tool owns the pointer) takes no gesture.
    app.world_mut().get_mut::<OrbitRules>(cam).unwrap().enabled = false;
    cursor(&mut app, Vec2::new(400.0, 250.0));
    mouse(&mut app, MouseButton::Right, true);
    motion(&mut app, Vec2::new(10.0, 0.0));
    wheel(&mut app);
    let orbit = orbit_of(&app, cam);
    assert!((orbit.yaw + 0.14).abs() < EPS && orbit.radius == radius);
}

#[test]
fn numpad_keys_write_camera_actions_where_the_rules_ask_for_them() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).init_resource::<ButtonInput<KeyCode>>().add_systems(Update, keys);
    crate::app::actions::register::<CameraAction>(&mut app);
    let cam = spawn_orbit(&mut app, Orbit::default(), OrbitRules::default(), ViewArea::Window);
    let press = |app: &mut App, codes: &[KeyCode]| -> Vec<CameraAction> {
        {
            let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            input.release_all();
            input.clear();
            for code in codes {
                input.press(*code);
            }
        }
        app.update();
        app.world_mut().resource_mut::<Messages<Act<CameraAction>>>().drain().map(|a| a.action).collect()
    };
    assert_eq!(press(&mut app, &[KeyCode::Numpad1]), vec![CameraAction::View { view: ViewPreset::Front }]);
    assert_eq!(press(&mut app, &[KeyCode::ControlLeft, KeyCode::Numpad7]), vec![CameraAction::View { view: ViewPreset::Bottom }]);
    assert_eq!(press(&mut app, &[KeyCode::Numpad5]), vec![CameraAction::Projection { orthographic: None }]);
    assert_eq!(press(&mut app, &[KeyCode::NumpadDecimal]), vec![CameraAction::Fit]);
    assert_eq!(press(&mut app, &[KeyCode::Home]), vec![CameraAction::Home]);
    // CAD reads its own keymap.
    app.world_mut().get_mut::<OrbitRules>(cam).unwrap().keys = false;
    assert!(press(&mut app, &[KeyCode::Numpad1]).is_empty());
}

#[test]
fn the_viewport_system_writes_the_card_viewport() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_systems(Update, viewport);
    app.world_mut().spawn((window((1600, 1000), 2.0), PrimaryWindow));
    let card = ViewArea::Card { full: Rect::new(100.0, 100.0, 500.0, 400.0), visible: Rect::new(100.0, 200.0, 500.0, 400.0) };
    let cam = spawn_orbit(&mut app, Orbit::default(), OrbitRules::default(), card);
    app.update();
    let camera = app.world().get::<Camera>(cam).unwrap();
    assert_eq!(camera.viewport.as_ref().map(|v| v.physical_position), Some(UVec2::new(100, 200)));
    assert!(camera.sub_camera_view.is_some());
    *app.world_mut().get_mut::<ViewArea>(cam).unwrap() = ViewArea::Window;
    app.update();
    let camera = app.world().get::<Camera>(cam).unwrap();
    assert!(camera.viewport.is_none() && camera.sub_camera_view.is_none());
}

// ---- actions and routing ----

fn command(name: &str, args: Value) -> sim_api::Command {
    sim_api::Command { command: name.into(), args }
}

#[test]
fn every_camera_command_and_control_parses() {
    for spec in CameraAction::commands() {
        if let Err(e) = CameraAction::parse(&command(spec.name, spec.example.clone())) {
            panic!("{}: {e}", spec.name);
        }
    }
    let patterns = <CameraAction as Action>::controls();
    let listed = controls();
    assert_eq!(listed.len(), ViewPreset::ALL.len() + 5);
    for control in &listed {
        let id = control["id"].as_str().unwrap();
        let action = control_action(id).unwrap_or_else(|| panic!("{id} has no action"));
        assert!(patterns.iter().any(|p| control_matches(p, id)), "{id} fits no camera control pattern");
        assert!(control["enabled"] == true && control["disabled_reason"].is_null() && control["label"].is_string());
        // The system_ui activation and the listed REST form reach the same action.
        let activate = command("system_ui", json!({"action": {"operation": "activate", "id": id, "ui_revision": 0}}));
        assert!(is_camera_control(&activate));
        assert_eq!(CameraAction::parse(&activate).as_ref(), Ok(&action), "{id}");
        let mut rest = control["action"].as_object().unwrap().clone();
        let name = rest.remove("command").unwrap();
        assert_eq!(CameraAction::parse(&command(name.as_str().unwrap(), Value::Object(rest))).as_ref(), Ok(&action), "{id}");
    }
    for unknown in ["camera:view:sideways", "camera:zap", "mode:cad", "camera"] {
        assert!(control_action(unknown).is_none(), "{unknown}");
    }
    let e = CameraAction::parse(&command("system_ui", json!({"action": {"operation": "activate", "id": "camera:zap"}}))).unwrap_err();
    assert!(e.contains("camera:zap"), "{e}");
    assert_eq!(FOV_DEG, 5.0..=120.0);
}

#[test]
fn camera_commands_route_in_every_orbit_mode() {
    let view = command("camera_view", json!({"view": "top"}));
    let fit = command("system_ui", json!({"action": {"operation": "activate", "id": "camera:fit"}}));
    for mode in [ViewerMode::Cad, ViewerMode::Robot, ViewerMode::Build, ViewerMode::Phenomena, ViewerMode::Inspect, ViewerMode::Lessons] {
        assert!(ORBIT_MODES.contains(&mode));
        assert_eq!(route(mode, true, &view).map(|f| f.name), Ok("camera"), "camera_view in {mode:?}");
        assert_eq!(route(mode, true, &fit).map(|f| f.name), Ok("camera"), "camera:fit in {mode:?}");
    }
    let e = match route(ViewerMode::Place, true, &view) {
        Err(e) => e,
        Ok(f) => panic!("camera_view in place mode went to {}", f.name),
    };
    for word in ["`camera_view`", "inspect", "build", "lessons", "robot", "cad", "phenomena", "active mode is place"] {
        assert!(e.contains(word), "{word}: {e}");
    }
    // The headless server has no camera to move: every camera command and control is refused.
    for command in [&fit, &view, &command("camera_state", json!({})), &command("camera_orbit", json!({"degrees": [10, 0]}))] {
        let e = route(ViewerMode::Inspect, false, command).err().unwrap_or_else(|| panic!("{} routed headless", command.command));
        assert!(e.contains("headless") && e.contains("camera/fit"), "{e}");
    }
    // Inspect's own commands still route there.
    assert_eq!(route(ViewerMode::Inspect, false, &command("fit", json!({}))).map(|f| f.name), Ok("inspect"));
}

#[test]
fn headless_controls_list_no_camera_controls() {
    let controls = command("system_ui", json!({"action": {"operation": "controls"}}));
    let answer = || Outcome::Done(Ok(json!({"controls": []})));
    let ids = |outcome: Outcome| -> Vec<String> {
        match outcome {
            Outcome::Done(Ok(v)) => v["controls"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_string()).collect(),
            _ => panic!("an answer"),
        }
    };
    // Robot mode: annotate adds both the camera's and the switcher's controls
    // (inspect's own switcher list already ends with the latter).
    let windowed = ids(annotate_for(ViewerMode::Robot, true, &controls, answer()));
    assert!(windowed.iter().any(|id| id.starts_with("camera:")) && windowed.iter().any(|id| id.starts_with("mode:")));
    let headless = ids(annotate_for(ViewerMode::Robot, false, &controls, answer()));
    assert!(!headless.iter().any(|id| id.starts_with("camera:")), "{headless:?}");
    assert!(headless.iter().any(|id| id.starts_with("mode:")), "the switcher's controls stay");
    assert!(ids(annotate_for(ViewerMode::Inspect, false, &controls, answer())).is_empty());
}

// ---- fit, home and node framing (cad-views-export F1) ----

/// An orbit in the trackball, turned off every axis view.
fn tilted(rules: &OrbitRules) -> Orbit {
    let mut orbit = Orbit { focus: Vec3::Y, radius: 1.0, centre: Vec3::X, extent: 0.5, ..default() };
    orbit.set_trackball(true, rules);
    orbit.rotate(Vec2::new(40.0, 25.0), rules);
    orbit
}

#[test]
fn fits_and_heading_keeping_homes_keep_the_trackball() {
    // A home whose framing keeps the heading (view None: CAD, Robot) keeps the trackball's rotation.
    let rules = OrbitRules::default();
    let mut orbit = tilted(&rules);
    let rotation = orbit.rotation();
    orbit.home = true;
    orbit.frame(&rules, 1.0, true);
    assert!(orbit.trackball.is_some() && orbit.rotation().angle_between(rotation) < 1e-5, "home kept the view's direction");
    assert!(orbit.focus == Vec3::X && (orbit.radius - 1.6).abs() < EPS && !orbit.home);
    // A glided fit moves only focus and distance: the rotation holds all the way.
    let glide = OrbitRules { glide_home: true, ..default() };
    let mut orbit = tilted(&glide);
    let rotation = orbit.rotation();
    orbit.frame(&glide, 1.0, false);
    assert!(orbit.glide.is_some(), "a placed view glides where the rules ask");
    orbit.step(GLIDE_S * 0.5);
    assert!(orbit.trackball.is_some() && orbit.rotation().angle_between(rotation) < 1e-5);
    assert!(orbit.focus != Vec3::Y && orbit.focus != Vec3::X, "halfway");
    orbit.step(GLIDE_S);
    assert!(orbit.glide.is_none() && close(orbit.focus, Vec3::X, EPS) && orbit.rotation().angle_between(rotation) < 1e-5);
    // A framing that imposes a view returns to the turntable at that view.
    for framing in [Framing::Bounds { scale: 3.2, aspect: false, view: Some((0.1, 0.2)) }, Framing::Fixed(Pose { focus: Vec3::Z, radius: 2.0, yaw: 0.1, pitch: 0.2 })] {
        let rules = OrbitRules { framing, ..default() };
        assert!(!Orbit::keeps_heading(&rules, true) && Orbit::keeps_heading(&rules, false));
        let mut orbit = tilted(&rules);
        orbit.frame(&rules, 1.0, true);
        assert!(orbit.trackball.is_none() && (orbit.yaw - 0.1).abs() < EPS && (orbit.pitch - 0.2).abs() < EPS, "{framing:?}");
    }
    assert!(Orbit::keeps_heading(&OrbitRules::default(), true));
}

#[test]
fn a_node_fit_frames_its_bounds_and_leaves_the_modes_bounds() {
    let rules = cad_rules();
    let mut orbit = Orbit { focus: Vec3::Y, radius: 1.0, yaw: 0.7, pitch: 0.45, centre: Vec3::ZERO, extent: 2.0, ..default() };
    let node = Vec3::new(0.3, 0.1, -0.2);
    orbit.frame_bounds(node, 0.05, &rules, 1.5);
    assert!(close(orbit.focus, node, EPS) && (orbit.radius - 0.16).abs() < EPS, "{orbit:?}");
    assert!(orbit.centre == Vec3::ZERO && orbit.extent == 2.0, "the mode's bounds stay (zoom limits, a later fit)");
    assert!((orbit.yaw - 0.7).abs() < EPS && (orbit.pitch - 0.45).abs() < EPS);
    // The trackball stays.
    orbit.set_trackball(true, &rules);
    orbit.rotate(Vec2::new(-30.0, 12.0), &rules);
    let rotation = orbit.rotation();
    orbit.frame_bounds(Vec3::X, 0.1, &rules, 1.0);
    assert!(orbit.trackball.is_some() && orbit.rotation().angle_between(rotation) < 1e-5 && orbit.focus == Vec3::X);
    // A later fit frames the whole content again.
    orbit.frame(&rules, 1.0, false);
    assert!(orbit.focus == Vec3::ZERO && (orbit.radius - 6.4).abs() < 1e-4);
    // Even where home imposes a view, a node fit keeps the heading; an
    // aspect-aware framing divides by the aspect clamped to 0.1–1; a fixed
    // framing (no scale of its own) uses 3.2.
    let by_aspect = OrbitRules { framing: Framing::Bounds { scale: 2.0, aspect: true, view: Some((0.0, 0.0)) }, ..default() };
    orbit.frame_bounds(Vec3::ZERO, 1.0, &by_aspect, 0.5);
    assert!((orbit.radius - 4.0).abs() < 1e-4 && orbit.trackball.is_some());
    let fixed = OrbitRules { framing: Framing::Fixed(Pose { focus: Vec3::Z, radius: 9.0, yaw: 0.0, pitch: 0.0 }), ..default() };
    orbit.frame_bounds(Vec3::ZERO, 1.0, &fixed, 1.0);
    assert!((orbit.radius - 3.2).abs() < 1e-4);
}

#[test]
fn turntable_reads_the_trackball_heading_without_changing_it() {
    let rules = OrbitRules::default();
    let mut orbit = Orbit { radius: 1.0, yaw: 0.7, pitch: 0.4, ..default() };
    assert_eq!(orbit.turntable(), (0.7, 0.4));
    orbit.set_trackball(true, &rules);
    orbit.rotate(Vec2::new(30.0, -20.0), &rules);
    let before = orbit.clone();
    let (yaw, pitch) = orbit.turntable();
    assert_eq!(orbit, before, "read only");
    let turntable = Orbit { radius: 1.0, yaw, pitch, ..default() };
    assert!(close(eye_direction(&turntable), eye_direction(&orbit), 1e-4));
    // camera_state's RoboCAD angles are the view's, not the stale stored ones.
    let (ry, rp) = display_to_robocad(yaw, pitch);
    let state = state_json(&orbit, None);
    assert!((state["robocad"]["yaw"].as_f64().unwrap() as f32 - ry).abs() < 1e-3 && (state["robocad"]["pitch"].as_f64().unwrap() as f32 - rp).abs() < 1e-3, "{state}");
}

#[test]
fn the_first_home_framing_uses_the_view_areas_aspect() {
    // 800 × 500 logical at scale 2.
    let w = window((1600, 1000), 2.0);
    let docks = ViewArea::Docks { left: 100.0, right: 100.0, top: 50.0, bottom: 50.0 };
    let card = ViewArea::Card { full: Rect::new(100.0, 100.0, 500.0, 400.0), visible: Rect::new(100.0, 200.0, 500.0, 400.0) };
    assert!((area_aspect(&w, &ViewArea::Window) - 1.6).abs() < EPS);
    assert!((area_aspect(&w, &docks) - 1.5).abs() < EPS);
    // A card's projection is the whole card's, scrolled or not.
    assert!((area_aspect(&w, &card) - 4.0 / 3.0).abs() < EPS);
    // Before Bevy's first camera update the camera has no logical size.
    let camera = Camera::default();
    assert!(camera.logical_viewport_size().is_none());
    assert!((view_aspect(&camera, Some(&w), Some(&docks)) - 1.5).abs() < EPS);
    assert_eq!(view_aspect(&camera, None, Some(&docks)), 1.0);
    // `place` on the first frame: an aspect-aware home between docks that leave a 200 × 400 view.
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_systems(Update, super::orbit::place);
    app.world_mut().spawn((window((800, 500), 1.0), PrimaryWindow));
    let rules = OrbitRules { framing: Framing::Bounds { scale: 3.2, aspect: true, view: None }, ..default() };
    let area = ViewArea::Docks { left: 300.0, right: 300.0, top: 50.0, bottom: 50.0 };
    let cam = app.world_mut().spawn((Camera::default(), Transform::default(), Projection::default(), Orbit { extent: 1.0, home: true, ..default() }, rules, area)).id();
    app.update();
    let orbit = orbit_of(&app, cam);
    assert!(!orbit.home && (orbit.radius - 6.4).abs() < 1e-4, "framed for a 0.5 aspect, not 1: {}", orbit.radius);
}

// ---- RoboCAD's gestures and keys ----

#[test]
fn drag_kinds_follow_each_modes_feel() {
    use DragKind::*;
    // Every mode: right orbits, Shift+right and middle (also with Shift) pan, Alt does nothing more.
    assert_eq!(drag_kind(false, true, false, false, false), Some(Orbit));
    assert_eq!(drag_kind(false, true, false, true, false), Some(Pan));
    assert_eq!(drag_kind(false, false, true, false, false), Some(Pan));
    assert_eq!(drag_kind(false, false, true, true, false), Some(Pan));
    assert_eq!(drag_kind(false, true, false, false, true), Some(Orbit));
    // RoboCAD's: Shift+middle orbits, Alt+right snaps, Alt+Shift+right pans.
    assert_eq!(drag_kind(true, false, true, true, false), Some(Orbit));
    assert_eq!(drag_kind(true, false, true, false, false), Some(Pan));
    assert_eq!(drag_kind(true, true, false, false, false), Some(Orbit));
    assert_eq!(drag_kind(true, true, false, true, false), Some(Pan));
    assert_eq!(drag_kind(true, true, false, false, true), Some(OrbitSnap));
    assert_eq!(drag_kind(true, true, false, true, true), Some(Pan));
    assert_eq!(drag_kind(true, false, true, true, true), Some(Orbit));
    assert_eq!(drag_kind(true, false, false, false, true), None);
}

#[test]
fn snap_to_axis_is_robocads_snap_orthographic() {
    let rules = cad_rules();
    let pole = 89.5f32.to_radians();
    // Above 45°: top, at the nearest quarter turn.
    let mut orbit = Orbit { radius: 1.0, yaw: 0.3, pitch: 0.9, ..default() };
    orbit.snap_to_axis(&rules);
    assert!(orbit.yaw.abs() < EPS && (orbit.pitch - pole).abs() < EPS, "{orbit:?}");
    // Level and near 90°: RoboCAD's right view.
    let mut orbit = Orbit { radius: 1.0, yaw: 1.2, pitch: -0.2, ..default() };
    orbit.snap_to_axis(&rules);
    let mut right = Orbit { radius: 1.0, ..default() };
    right.preset(ViewPreset::Right, &rules, 0.0);
    assert!(orbit.pitch == 0.0 && close(eye_direction(&orbit), eye_direction(&right), EPS));
    // From the trackball: back to the turntable, below −45° to the bottom.
    let mut orbit = Orbit { radius: 1.0, yaw: 3.0, pitch: -1.0, ..default() };
    orbit.set_trackball(true, &rules);
    orbit.snap_to_axis(&rules);
    assert!(orbit.trackball.is_none() && (orbit.yaw.abs() - std::f32::consts::PI).abs() < 1e-4 && (orbit.pitch + pole).abs() < EPS, "{orbit:?}");
    // A mode's pitch limit holds.
    let mut orbit = Orbit { radius: 1.0, pitch: 1.0, ..default() };
    orbit.snap_to_axis(&OrbitRules::default());
    assert_eq!(orbit.pitch, 1.4);
}

#[test]
fn arrow_keys_are_robocads_orbit_and_pan_steps() {
    let orbit = |yaw: f32, pitch: f32| Some(CameraAction::Orbit { dx: 0.0, dy: 0.0, degrees: Some([yaw, pitch]) });
    assert_eq!(arrow_action(KeyCode::ArrowRight, false, false), orbit(10.0, 0.0));
    assert_eq!(arrow_action(KeyCode::ArrowLeft, false, false), orbit(-10.0, 0.0));
    assert_eq!(arrow_action(KeyCode::ArrowUp, true, false), orbit(0.0, 90.0));
    assert_eq!(arrow_action(KeyCode::ArrowDown, false, false), orbit(0.0, -10.0));
    // Shift pans 4 px per degree, the other way horizontally (RoboCAD's pan(-dx × 4, dy × 4)).
    assert_eq!(arrow_action(KeyCode::ArrowLeft, false, true), Some(CameraAction::Pan { dx: 40.0, dy: 0.0 }));
    assert_eq!(arrow_action(KeyCode::ArrowUp, true, true), Some(CameraAction::Pan { dx: 0.0, dy: 360.0 }));
    assert_eq!(arrow_action(KeyCode::KeyA, false, false), None);
}

#[test]
fn camera_orbit_takes_degrees_as_robocads_arrow_keys() {
    let mut app = handler_app();
    let cam = spawn_orbit(&mut app, Orbit { radius: 1.0, extent: 1.0, ..default() }, cad_rules(), ViewArea::Window);
    rest(&mut app, CameraAction::Orbit { dx: 0.0, dy: 0.0, degrees: Some([10.0, 5.0]) }).unwrap();
    let orbit = orbit_of(&app, cam);
    assert!((orbit.yaw - 10f32.to_radians()).abs() < EPS && (orbit.pitch - 5f32.to_radians()).abs() < EPS);
    // RoboCAD's yaw rises by the same 10° (its right arrow: `Camera.orbit(-25, 0)` at 0.4°/px).
    let (ry, rp) = display_to_robocad(orbit.yaw, orbit.pitch);
    assert!((ry + 80.0).abs() < 1e-3 && (rp - 5.0).abs() < 1e-3, "({ry}, {rp})");
    assert!(rest(&mut app, CameraAction::Orbit { dx: 5.0, dy: 0.0, degrees: Some([1.0, 0.0]) }).unwrap_err().contains("not both"));
    assert!(rest(&mut app, CameraAction::Orbit { dx: 0.0, dy: 0.0, degrees: Some([f32::NAN, 0.0]) }).is_err());
    assert_eq!(orbit_of(&app, cam).yaw, orbit.yaw, "a refusal changes nothing");
    // REST: degrees alone, or dx/dy alone as before.
    assert_eq!(CameraAction::parse(&command("camera_orbit", json!({"degrees": [10, 0]}))), Ok(CameraAction::Orbit { dx: 0.0, dy: 0.0, degrees: Some([10.0, 0.0]) }));
    assert_eq!(CameraAction::parse(&command("camera_orbit", json!({"dx": 30, "dy": 0}))), Ok(CameraAction::Orbit { dx: 30.0, dy: 0.0, degrees: None }));
}

/// The keys system's actions for keys held this frame.
fn keys_app(rules: OrbitRules) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).init_resource::<ButtonInput<KeyCode>>().add_systems(Update, keys);
    crate::app::actions::register::<CameraAction>(&mut app);
    let cam = spawn_orbit(&mut app, Orbit::default(), rules, ViewArea::Window);
    (app, cam)
}

fn press_keys(app: &mut App, codes: &[KeyCode]) -> Vec<CameraAction> {
    {
        let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        input.release_all();
        input.clear();
        for code in codes {
            input.press(*code);
        }
    }
    app.update();
    app.world_mut().resource_mut::<Messages<Act<CameraAction>>>().drain().map(|a| a.action).collect()
}

#[test]
fn arrow_keys_only_where_the_rules_ask_and_no_text_field_types() {
    let (mut app, cam) = keys_app(OrbitRules { keys: false, robocad_gestures: true, ..default() });
    assert_eq!(press_keys(&mut app, &[KeyCode::ArrowRight]), vec![arrow_action(KeyCode::ArrowRight, false, false).unwrap()]);
    assert_eq!(press_keys(&mut app, &[KeyCode::ShiftLeft, KeyCode::ArrowLeft]), vec![CameraAction::Pan { dx: 40.0, dy: 0.0 }]);
    // Cmd is RoboCAD's Ctrl on a Mac (Qt maps it so).
    assert_eq!(press_keys(&mut app, &[KeyCode::SuperLeft, KeyCode::ArrowUp]), vec![arrow_action(KeyCode::ArrowUp, true, false).unwrap()]);
    assert!(press_keys(&mut app, &[KeyCode::Numpad1]).is_empty(), "CAD reads its own keymap");
    // A text field has the keyboard: no camera key, arrow or numpad.
    app.world_mut().get_mut::<OrbitRules>(cam).unwrap().typing = true;
    assert!(press_keys(&mut app, &[KeyCode::ArrowRight]).is_empty());
    app.world_mut().get_mut::<OrbitRules>(cam).unwrap().keys = true;
    assert!(press_keys(&mut app, &[KeyCode::Numpad1]).is_empty());
    // Other modes keep their keys: no arrows (Robot moves joints with them).
    let (mut app, _) = keys_app(OrbitRules::default());
    assert!(press_keys(&mut app, &[KeyCode::ArrowRight]).is_empty());
    assert_eq!(press_keys(&mut app, &[KeyCode::Numpad1]), vec![CameraAction::View { view: ViewPreset::Front }]);
}

#[test]
fn robocad_drags_shift_middle_orbits_alt_right_snaps_and_alt_left_drags_past_the_slop() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<MouseMotion>()
        .add_message::<MouseWheel>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_systems(Update, navigate);
    let win = app.world_mut().spawn((window((800, 500), 1.0), PrimaryWindow)).id();
    let rules = OrbitRules { robocad_gestures: true, ..cad_rules() };
    let cam = spawn_orbit(&mut app, Orbit { radius: 1.0, extent: 1.0, ..default() }, rules, ViewArea::Window);
    let cursor = |app: &mut App, p: Vec2| app.world_mut().get_mut::<Window>(win).unwrap().set_cursor_position(Some(p));
    let mouse = |app: &mut App, button: MouseButton, down: bool| {
        let mut buttons = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        if down { buttons.press(button) } else { buttons.release(button) }
    };
    let key = |app: &mut App, code: KeyCode, down: bool| {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        if down { keys.press(code) } else { keys.release(code) }
    };
    cursor(&mut app, Vec2::new(400.0, 250.0));
    // Shift+middle orbits (it pans elsewhere).
    key(&mut app, KeyCode::ShiftLeft, true);
    mouse(&mut app, MouseButton::Middle, true);
    motion(&mut app, Vec2::new(10.0, 0.0));
    let orbit = orbit_of(&app, cam);
    assert!((orbit.yaw + 0.07).abs() < EPS && orbit.focus == Vec3::ZERO, "{orbit:?}");
    mouse(&mut app, MouseButton::Middle, false);
    key(&mut app, KeyCode::ShiftLeft, false);
    motion(&mut app, Vec2::ZERO);
    // Alt while right-drag orbiting snaps to the nearest axis view.
    key(&mut app, KeyCode::AltLeft, true);
    mouse(&mut app, MouseButton::Right, true);
    motion(&mut app, Vec2::new(10.0, 10.0));
    let orbit = orbit_of(&app, cam);
    assert!(orbit.yaw.abs() < EPS && orbit.pitch == 0.0, "{orbit:?}");
    mouse(&mut app, MouseButton::Right, false);
    motion(&mut app, Vec2::ZERO);
    // Alt+left: within the slop it is a click (the mode's); past it, an orbit.
    mouse(&mut app, MouseButton::Left, true);
    motion(&mut app, Vec2::ZERO);
    cursor(&mut app, Vec2::new(400.0 + ALT_DRAG_SLOP, 250.0));
    motion(&mut app, Vec2::new(ALT_DRAG_SLOP, 0.0));
    assert!(orbit_of(&app, cam).yaw.abs() < EPS, "a click's wobble does not orbit");
    cursor(&mut app, Vec2::new(410.0, 250.0));
    motion(&mut app, Vec2::new(4.0, 0.0));
    let yaw = orbit_of(&app, cam).yaw;
    assert!((yaw + 0.028).abs() < EPS, "{yaw}");
    // It keeps orbiting until the left button is up, Alt or not.
    key(&mut app, KeyCode::AltLeft, false);
    motion(&mut app, Vec2::new(10.0, 0.0));
    assert!((orbit_of(&app, cam).yaw - (yaw - 0.07)).abs() < EPS);
    mouse(&mut app, MouseButton::Left, false);
    motion(&mut app, Vec2::new(10.0, 0.0));
    let yaw = orbit_of(&app, cam).yaw;
    // Not while the mode's tool owns the left button (`alt_left` false).
    app.world_mut().get_mut::<OrbitRules>(cam).unwrap().alt_left = false;
    key(&mut app, KeyCode::AltLeft, true);
    cursor(&mut app, Vec2::new(400.0, 250.0));
    mouse(&mut app, MouseButton::Left, true);
    motion(&mut app, Vec2::ZERO);
    cursor(&mut app, Vec2::new(450.0, 250.0));
    motion(&mut app, Vec2::new(50.0, 0.0));
    assert_eq!(orbit_of(&app, cam).yaw, yaw);
    mouse(&mut app, MouseButton::Left, false);
    motion(&mut app, Vec2::ZERO);
    // Nor in a mode without RoboCAD's gestures.
    *app.world_mut().get_mut::<OrbitRules>(cam).unwrap() = OrbitRules::default();
    mouse(&mut app, MouseButton::Left, true);
    motion(&mut app, Vec2::ZERO);
    cursor(&mut app, Vec2::new(400.0, 250.0));
    motion(&mut app, Vec2::new(-50.0, 0.0));
    assert_eq!(orbit_of(&app, cam).yaw, yaw);
}

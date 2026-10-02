//! RoboCAD camera gesture and keyboard cases.
use super::*;

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

/// A kit text field for the keys tests to focus.
const FIELD: crate::ui_kit::text::FieldId = crate::ui_kit::text::FieldId("camera.test");

/// The keys system's actions for keys held this frame, gated on the shared
/// typing condition as `CameraPlugin` registers it (with the kit's text
/// entry and one field, so a test can give it the keyboard).
fn keys_app(rules: OrbitRules) -> (App, Entity) {
    use crate::ui_kit::text::{TextEntryPlugin, TextField, TextFieldApp, typing};
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TextEntryPlugin))
        .init_resource::<ButtonInput<KeyCode>>()
        .add_systems(Update, keys.run_if(not(typing)))
        .add_text_field(FIELD, TextField::new("Test"));
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
    // A kit text field has the keyboard: no camera key, arrow or numpad.
    use bevy::ecs::system::RunSystemOnce;
    use crate::ui_kit::text::TextFocus;
    app.world_mut().run_system_once(|mut f: TextFocus| assert!(f.focus(FIELD, ""))).unwrap();
    assert!(press_keys(&mut app, &[KeyCode::ArrowRight]).is_empty());
    app.world_mut().get_mut::<OrbitRules>(cam).unwrap().keys = true;
    assert!(press_keys(&mut app, &[KeyCode::Numpad1]).is_empty());
    // It gives the keyboard up: the keys are the camera's again.
    app.world_mut().run_system_once(|mut f: TextFocus| f.blur(FIELD)).unwrap();
    assert_eq!(press_keys(&mut app, &[KeyCode::Numpad1]), vec![CameraAction::View { view: ViewPreset::Front }]);
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

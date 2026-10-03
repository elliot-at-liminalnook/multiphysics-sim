//! Written fixtures for the device bindings' Bevy mapping and the target
//! rule (not executed by their author). The bindings file's own validation
//! and refusals are tested with the shared parser (`sim_runtime::drive_bindings`).
use super::bindings::{BindingsFile, ButtonBinding, DEFAULT_DEADZONE, KeyAxis, SCHEMA, gamepad_button, key_code, shape, stick_axis, supported_only, unmapped_names};
use super::input::live_target;
use super::{DriveBindings, DriveTarget, LiveTarget};
use crate::app::ViewerMode;
use bevy::prelude::*;
use serde_json::json;
use sim_domain_control::drive::kinematics::Axes;
use sim_runtime::drive_bindings::{BUTTONS, KEYS, STICKS};

fn held(keys: &[KeyCode]) -> impl Fn(KeyCode) -> bool + '_ {
    move |k| keys.contains(&k)
}

/// Every key, stick and button name the shared tables list maps to a Bevy
/// value (a missing arm would make a valid bindings file unusable here),
/// and distinct names map to distinct values.
#[test]
fn every_shared_name_maps_to_a_bevy_value() {
    assert_eq!(unmapped_names(), Vec::<&str>::new());
    let keys: Vec<KeyCode> = KEYS.iter().map(|k| key_code(k).unwrap()).collect();
    assert!(keys.iter().enumerate().all(|(i, k)| !keys[..i].contains(k)), "two key names share a KeyCode");
    let sticks: Vec<GamepadAxis> = STICKS.iter().map(|s| stick_axis(s.0).unwrap()).collect();
    assert!(sticks.iter().enumerate().all(|(i, s)| !sticks[..i].contains(s)), "two stick names share a GamepadAxis");
    let buttons: Vec<GamepadButton> = BUTTONS.iter().map(|b| gamepad_button(b.0).unwrap()).collect();
    assert!(buttons.iter().enumerate().all(|(i, b)| !buttons[..i].contains(b)), "two button names share a GamepadButton");
    assert_eq!(key_code("KeyW"), Some(KeyCode::KeyW));
    assert_eq!(key_code("ArrowUp"), Some(KeyCode::ArrowUp));
    assert_eq!(key_code("Space"), None);
    assert_eq!(stick_axis("left_stick_y"), Some(GamepadAxis::LeftStickY));
    assert_eq!(gamepad_button("dpad_up"), Some(GamepadButton::DPadUp));
}

/// The committed defaults validate, round-trip through their stored form and
/// describe every binding plus the two stops that always hold.
#[test]
fn default_bindings_validate_and_round_trip() {
    let file = BindingsFile::default();
    assert_eq!(file.schema, SCHEMA);
    file.validate().unwrap();
    let stored = serde_json::to_value(&file).unwrap();
    assert_eq!(BindingsFile::from_value(&stored).unwrap(), file);
    let b = DriveBindings::default();
    assert!(!b.stored);
    let described = b.describe();
    assert_eq!(described.len(), file.keyboard.axes.len() + file.keyboard.actions.len() + file.gamepad.axes.len() + file.gamepad.buttons.len() + 2);
    assert!(described.iter().any(|(input, does)| input == "W" && does.starts_with("forward +1")));
    assert!(described.iter().any(|(input, does)| input == "Escape" && does.starts_with("stop")));
    assert_eq!(file.gamepad.deadzone, DEFAULT_DEADZONE);
    // Every bound key and button reaches the Bevy lists.
    assert_eq!(b.keys().count(), file.keyboard.axes.len() + file.keyboard.actions.len());
    assert_eq!(b.buttons().count(), file.gamepad.buttons.len());
}

/// Keyboard: held keys sum per axis and clamp. A is a left turn (+yaw), D a
/// right turn, Q a left strafe; W and S together cancel.
#[test]
fn keyboard_combinations_make_the_expected_axes() {
    let b = DriveBindings::default();
    let axes = |keys: &[KeyCode]| b.keyboard_axes(held(keys));
    assert_eq!(axes(&[KeyCode::KeyW, KeyCode::KeyA]), Axes { forward: 1.0, lateral: 0.0, yaw: 1.0 });
    assert_eq!(axes(&[KeyCode::KeyW, KeyCode::KeyD]), Axes { forward: 1.0, lateral: 0.0, yaw: -1.0 });
    assert_eq!(axes(&[KeyCode::KeyS, KeyCode::KeyQ]), Axes { forward: -1.0, lateral: 1.0, yaw: 0.0 });
    assert_eq!(axes(&[KeyCode::KeyW, KeyCode::KeyS]), Axes::ZERO);
    assert_eq!(axes(&[KeyCode::KeyX, KeyCode::KeyB, KeyCode::Space, KeyCode::KeyG]), Axes::ZERO);
    // Two keys on one axis in the same direction clamp at 1.
    let mut file = BindingsFile::default();
    file.keyboard.axes.push(KeyAxis { key: "ArrowUp".into(), axis: "forward".into(), direction: 1 });
    let b2 = DriveBindings::new(file, true).unwrap();
    assert_eq!(b2.keyboard_axes(held(&[KeyCode::KeyW, KeyCode::ArrowUp])).forward, 1.0);
    // The action keys: X stop, B ("brake") halt; never Space, which a focused kit button takes.
    assert_eq!(b.key_actions(), &[(KeyCode::KeyX, "stop".to_string()), (KeyCode::KeyB, "halt".to_string())]);
    assert!(b.describe().iter().any(|(input, does)| input == "B" && does == "profile action `halt`"));
}

/// Gamepad: the deadzone zeroes small deflections and rescales the rest
/// (full deflection stays 1); inverted sticks flip; stick up is +forward,
/// stick left (negative X) is +yaw and +lateral with the defaults. Bevy's
/// f32 stick values go through the shared f64 shape.
#[test]
fn gamepad_deadzone_and_invert() {
    assert_eq!(shape(0.1, 0.15), 0.0);
    assert_eq!(shape(-0.14, 0.15), 0.0);
    assert_eq!(shape(1.0, 0.15), 1.0);
    assert_eq!(shape(-1.0, 0.15), -1.0);
    assert!((shape(0.575, 0.15) - 0.5).abs() < 1e-6);
    assert_eq!(shape(f64::NAN, 0.15), 0.0);
    let b = DriveBindings::default();
    let stick = |ly: f32, lx: f32, rx: f32| {
        b.gamepad_axes(
            |axis| match axis {
                GamepadAxis::LeftStickY => Some(ly),
                GamepadAxis::LeftStickX => Some(lx),
                GamepadAxis::RightStickX => Some(rx),
                _ => None,
            },
            |_| false,
        )
    };
    assert_eq!(stick(1.0, 0.0, 0.0), Axes { forward: 1.0, lateral: 0.0, yaw: 0.0 });
    assert_eq!(stick(0.1, -0.1, 0.1), Axes::ZERO);
    assert_eq!(stick(0.0, -1.0, -1.0), Axes { forward: 0.0, lateral: 1.0, yaw: 1.0 });
    // An f32 stick value inside the deadzone stays zero after widening.
    assert_eq!(stick(0.14, 0.0, 0.0), Axes::ZERO);
    // A button bound to an axis adds while held.
    let mut file = BindingsFile::default();
    file.gamepad.buttons.push(ButtonBinding { button: "dpad_up".into(), action: None, axis: Some("forward".into()), direction: Some(1) });
    let b2 = DriveBindings::new(file, true).unwrap();
    assert_eq!(b2.gamepad_axes(|_| None, |button| button == GamepadButton::DPadUp).forward, 1.0);
    assert!(b2.buttons().any(|b| b == GamepadButton::DPadUp));
    assert_eq!(b.button_actions(), &[(GamepadButton::South, "stop".to_string()), (GamepadButton::East, "halt".to_string())]);
}

/// A bad binding is still refused naming its field through `DriveBindings::new`
/// (the shared parser's refusal, unchanged).
#[test]
fn bad_bindings_are_refused_naming_the_field() {
    let mut v = serde_json::to_value(BindingsFile::default()).unwrap();
    v["keyboard"]["axes"][2]["key"] = json!("KeyG");
    let e = BindingsFile::from_value(&v).unwrap_err();
    assert!(e.starts_with("drive_bindings.keyboard.axes[2].key:") && e.contains("graphs"), "{e}");
    let mut file = BindingsFile::default();
    file.keyboard.actions[1].key = "Space".into();
    let e = DriveBindings::new(file, true).unwrap_err();
    assert!(e.starts_with("drive_bindings.keyboard.actions[1].key:") && e.contains("activation key"), "{e}");
}

/// Device axes the profile does not support are zeroed and named, and the
/// supported ones are kept: W+Q (or a slightly diagonal left stick) on a
/// differential robot (forward and yaw only) still drives forward.
#[test]
fn unsupported_axes_are_zeroed_and_named() {
    let differential = [true, false, true];
    let b = DriveBindings::default();
    let (axes, ignored) = supported_only(b.keyboard_axes(held(&[KeyCode::KeyW, KeyCode::KeyQ])), differential);
    assert_eq!((axes, ignored), (Axes { forward: 1.0, lateral: 0.0, yaw: 0.0 }, vec!["lateral"]));
    // Q alone sends nothing (zero axes) and says why.
    let (axes, ignored) = supported_only(b.keyboard_axes(held(&[KeyCode::KeyQ])), differential);
    assert_eq!((axes, ignored), (Axes::ZERO, vec!["lateral"]));
}

/// The run panel's key line comes from the bindings: the defaults read
/// "W/S forward · A/D turn · Q/E lateral · X stop · B halt · Esc stop", an
/// axis the profile lacks and a mode's own key are marked.
#[test]
fn key_summary_is_generated_from_the_bindings() {
    let b = DriveBindings::default();
    assert_eq!(b.key_summary([true; 3], &[]), "W/S forward · Q/E lateral · A/D turn · X stop · B halt · Esc stop");
    let differential = b.key_summary([true, false, true], &[]);
    assert!(differential.contains("Q/E lateral (not in profile)"), "{differential}");
    let mut file = BindingsFile::default();
    file.keyboard.axes.push(KeyAxis { key: "ArrowUp".into(), axis: "forward".into(), direction: 1 });
    let owned = DriveBindings::new(file, true).unwrap().key_summary([true; 3], &[KeyCode::ArrowUp]);
    assert!(owned.contains("↑ (mode key, ignored)"), "{owned}");
}

/// The poller acts only on a live target written for the current mode: a
/// target left by another mode (or Build's under the lesson screen) is
/// ignored, and no target or no mode reads nothing.
#[test]
fn a_target_for_another_mode_is_ignored() {
    let live = LiveTarget { mode: ViewerMode::Build, supported: [true, false, true], run: "rover.system.json".into() };
    let target = DriveTarget { live: Some(live.clone()), owned_keys: Vec::new() };
    assert_eq!(live_target(&target, Some(ViewerMode::Build)), Some(&live));
    assert_eq!(live_target(&target, Some(ViewerMode::Robot)), None);
    assert_eq!(live_target(&target, Some(ViewerMode::Lessons)), None);
    assert_eq!(live_target(&target, None), None);
    assert_eq!(live_target(&DriveTarget::default(), Some(ViewerMode::Build)), None);
}

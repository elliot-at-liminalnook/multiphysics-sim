//! Written fixtures for the device bindings (not executed by their author).
use super::bindings::{BindingsFile, DEFAULT_DEADZONE, SCHEMA, shape};
use super::DriveBindings;
use super::input::supported_only;
use bevy::prelude::*;
use serde_json::json;
use sim_domain_control::drive::kinematics::Axes;

fn held(keys: &[KeyCode]) -> impl Fn(KeyCode) -> bool + '_ {
    move |k| keys.contains(&k)
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
    file.keyboard.axes.push(super::bindings::KeyAxis { key: "ArrowUp".into(), axis: "forward".into(), direction: 1 });
    let b2 = DriveBindings::new(file, true).unwrap();
    assert_eq!(b2.keyboard_axes(held(&[KeyCode::KeyW, KeyCode::ArrowUp])).forward, 1.0);
    // The action keys: X stop, B ("brake") halt; never Space, which a focused kit button takes.
    assert_eq!(b.key_actions(), &[(KeyCode::KeyX, "stop".to_string()), (KeyCode::KeyB, "halt".to_string())]);
    assert!(b.describe().iter().any(|(input, does)| input == "B" && does == "profile action `halt`"));
}

/// Gamepad: the deadzone zeroes small deflections and rescales the rest
/// (full deflection stays 1); inverted sticks flip; stick up is +forward,
/// stick left (negative X) is +yaw and +lateral with the defaults.
#[test]
fn gamepad_deadzone_and_invert() {
    assert_eq!(shape(0.1, 0.15), 0.0);
    assert_eq!(shape(-0.14, 0.15), 0.0);
    assert_eq!(shape(1.0, 0.15), 1.0);
    assert_eq!(shape(-1.0, 0.15), -1.0);
    assert!((shape(0.575, 0.15) - 0.5).abs() < 1e-6);
    assert_eq!(shape(f32::NAN, 0.15), 0.0);
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
    // A button bound to an axis adds while held.
    let mut file = BindingsFile::default();
    file.gamepad.buttons.push(super::bindings::ButtonBinding { button: "dpad_up".into(), action: None, axis: Some("forward".into()), direction: Some(1) });
    let b2 = DriveBindings::new(file, true).unwrap();
    assert_eq!(b2.gamepad_axes(|_| None, |button| button == GamepadButton::DPadUp).forward, 1.0);
    assert_eq!(b.button_actions(), &[(GamepadButton::South, "stop".to_string()), (GamepadButton::East, "halt".to_string())]);
}

/// A bad binding is refused naming its field; unknown fields and another
/// schema are refused before validation.
#[test]
fn bad_bindings_are_refused_naming_the_field() {
    fn refuse(edit: impl Fn(&mut serde_json::Value)) -> String {
        let mut v = serde_json::to_value(BindingsFile::default()).unwrap();
        edit(&mut v);
        BindingsFile::from_value(&v).unwrap_err()
    }
    let e = refuse(|v| v["keyboard"]["axes"][2]["key"] = json!("KeyNope"));
    assert!(e.starts_with("drive_bindings.keyboard.axes[2].key:") && e.contains("KeyNope"), "{e}");
    let e = refuse(|v| v["keyboard"]["axes"][2]["key"] = json!("KeyG"));
    assert!(e.starts_with("drive_bindings.keyboard.axes[2].key:") && e.contains("graphs"), "{e}");
    let e = refuse(|v| v["keyboard"]["axes"][1]["key"] = json!("KeyW"));
    assert!(e.starts_with("drive_bindings.keyboard.axes[1].key:") && e.contains("drive_bindings.keyboard.axes[0].key"), "{e}");
    let e = refuse(|v| v["keyboard"]["axes"][0]["axis"] = json!("up"));
    assert!(e.starts_with("drive_bindings.keyboard.axes[0].axis:"), "{e}");
    let e = refuse(|v| v["keyboard"]["axes"][0]["direction"] = json!(2));
    assert!(e.starts_with("drive_bindings.keyboard.axes[0].direction:"), "{e}");
    let e = refuse(|v| v["keyboard"]["actions"][0]["action"] = json!(""));
    assert!(e.starts_with("drive_bindings.keyboard.actions[0].action:"), "{e}");
    let e = refuse(|v| v["gamepad"]["deadzone"] = json!(1.0));
    assert!(e.starts_with("drive_bindings.gamepad.deadzone:"), "{e}");
    let e = refuse(|v| v["gamepad"]["axes"][1]["stick"] = json!("left_stick_y"));
    assert!(e.starts_with("drive_bindings.gamepad.axes[1].stick:") && e.contains("already bound"), "{e}");
    let e = refuse(|v| v["gamepad"]["buttons"][0]["axis"] = json!("forward"));
    assert!(e.starts_with("drive_bindings.gamepad.buttons[0]:"), "{e}");
    let e = refuse(|v| v["gamepad"]["buttons"][1]["button"] = json!("turbo"));
    assert!(e.starts_with("drive_bindings.gamepad.buttons[1].button:"), "{e}");
    // The UI kit's activation and focus keys and Escape are refused by name, for axes and actions alike.
    let e = refuse(|v| v["keyboard"]["actions"][1]["key"] = json!("Space"));
    assert!(e.starts_with("drive_bindings.keyboard.actions[1].key:") && e.contains("`Space`") && e.contains("activation key"), "{e}");
    let e = refuse(|v| v["keyboard"]["axes"][0]["key"] = json!("Enter"));
    assert!(e.starts_with("drive_bindings.keyboard.axes[0].key:") && e.contains("activation key"), "{e}");
    let e = refuse(|v| v["keyboard"]["axes"][0]["key"] = json!("Tab"));
    assert!(e.starts_with("drive_bindings.keyboard.axes[0].key:") && e.contains("focus key"), "{e}");
    let e = refuse(|v| v["keyboard"]["actions"][0]["key"] = json!("Escape"));
    assert!(e.starts_with("drive_bindings.keyboard.actions[0].key:") && e.contains("Stop"), "{e}");
    let e = refuse(|v| v["keyboard"]["extra"] = json!(1));
    assert!(e.contains("unknown field `extra`"), "{e}");
    let e = refuse(|v| v["schema"] = json!("sim.drive-bindings/2"));
    assert!(e.starts_with("drive_bindings.schema:") && e.contains(SCHEMA), "{e}");
}

/// Device axes the profile does not support are zeroed and named, and the
/// supported ones are kept: W+Q (or a slightly diagonal left stick) on a
/// differential robot (forward and yaw only) still drives forward.
#[test]
fn unsupported_axes_are_zeroed_and_named() {
    let differential = [true, false, true];
    let (axes, ignored) = supported_only(Axes { forward: 1.0, lateral: 1.0, yaw: 0.0 }, differential);
    assert_eq!(axes, Axes { forward: 1.0, lateral: 0.0, yaw: 0.0 });
    assert_eq!(ignored, vec!["lateral"]);
    // The default bindings' W+Q, through the keyboard: forward survives.
    let b = DriveBindings::default();
    let (axes, ignored) = supported_only(b.keyboard_axes(held(&[KeyCode::KeyW, KeyCode::KeyQ])), differential);
    assert_eq!((axes, ignored), (Axes { forward: 1.0, lateral: 0.0, yaw: 0.0 }, vec!["lateral"]));
    // Q alone sends nothing (zero axes) and says why.
    let (axes, ignored) = supported_only(b.keyboard_axes(held(&[KeyCode::KeyQ])), differential);
    assert_eq!((axes, ignored), (Axes::ZERO, vec!["lateral"]));
    // A supported or zero axis is never named; every axis supported leaves the input unchanged.
    let a = Axes { forward: -0.5, lateral: 0.25, yaw: 1.0 };
    assert_eq!(supported_only(a, [true; 3]), (a, Vec::<&str>::new()));
    assert_eq!(supported_only(Axes { forward: 0.0, lateral: 0.0, yaw: -1.0 }, differential), (Axes { forward: 0.0, lateral: 0.0, yaw: -1.0 }, Vec::<&str>::new()));
    assert_eq!(supported_only(Axes { forward: 1.0, lateral: -1.0, yaw: 1.0 }, [false; 3]).1, vec!["forward", "lateral", "yaw"]);
}

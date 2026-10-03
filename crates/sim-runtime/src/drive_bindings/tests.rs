//! The device bindings (ported from sim-spatial's drive_input tests, with
//! key, stick and button names as strings) and the browser's W3C devices
//! through them. Written by reading; not yet executed.
use super::*;
use serde_json::json;

fn held<'a>(keys: &'a [&'a str]) -> impl Fn(&str) -> bool + 'a {
    move |k| keys.iter().any(|h| *h == k)
}
fn resolved() -> Resolved {
    BindingsFile::default().resolve().unwrap()
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
    let described = file.describe();
    assert_eq!(described.len(), file.keyboard.axes.len() + file.keyboard.actions.len() + file.gamepad.axes.len() + file.gamepad.buttons.len() + 2);
    assert!(described.iter().any(|(input, does)| input == "W" && does.starts_with("forward +1")));
    assert!(described.iter().any(|(input, does)| input == "Escape" && does.starts_with("stop")));
    assert_eq!(file.gamepad.deadzone, DEFAULT_DEADZONE);
    // The host answer: not stored, the defaults, the description and the W3C table.
    let answer = json(None);
    assert_eq!(answer["stored"], json!(false));
    assert_eq!(answer["bindings"], stored);
    assert_eq!(answer["describe"].as_array().unwrap().len(), described.len());
    assert_eq!(answer["w3c_standard_gamepad"]["sticks"][1], json!({"stick": "left_stick_y", "label": "Left stick Y", "source": {"kind": "axis", "index": 1}, "sign": -1.0}));
    assert_eq!(json(Some(&file))["stored"], json!(true));
}

/// Keyboard: held keys sum per axis and clamp. A is a left turn (+yaw), D a
/// right turn, Q a left strafe; W and S together cancel.
#[test]
fn keyboard_combinations_make_the_expected_axes() {
    let b = resolved();
    let axes = |keys: &[&str]| b.keyboard_axes(held(keys));
    assert_eq!(axes(&["KeyW", "KeyA"]), Axes { forward: 1.0, lateral: 0.0, yaw: 1.0 });
    assert_eq!(axes(&["KeyW", "KeyD"]), Axes { forward: 1.0, lateral: 0.0, yaw: -1.0 });
    assert_eq!(axes(&["KeyS", "KeyQ"]), Axes { forward: -1.0, lateral: 1.0, yaw: 0.0 });
    assert_eq!(axes(&["KeyW", "KeyS"]), Axes::ZERO);
    assert_eq!(axes(&["KeyX", "KeyB", "Space", "KeyG"]), Axes::ZERO);
    // Two keys on one axis in the same direction clamp at 1.
    let mut file = BindingsFile::default();
    file.keyboard.axes.push(KeyAxis { key: "ArrowUp".into(), axis: "forward".into(), direction: 1 });
    let b2 = file.resolve().unwrap();
    assert_eq!(b2.keyboard_axes(held(&["KeyW", "ArrowUp"])).forward, 1.0);
    // The action keys: X stop, B ("brake") halt; never Space, which a focused kit button takes.
    assert_eq!(b.key_actions, vec![("KeyX".to_string(), "stop".to_string()), ("KeyB".to_string(), "halt".to_string())]);
    assert!(BindingsFile::default().describe().iter().any(|(input, does)| input == "B" && does == "profile action `halt`"));
    assert_eq!(b.all_keys().collect::<Vec<_>>(), vec!["KeyW", "KeyS", "KeyA", "KeyD", "KeyQ", "KeyE", "KeyX", "KeyB"]);
    assert_eq!(key_label("KeyW"), "W");
    assert_eq!(key_label("Digit1"), "1");
    assert_eq!(key_label("ArrowUp"), "↑");
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
    assert!((shape(0.575, 0.15) - 0.5).abs() < 1e-12);
    assert_eq!(shape(f64::NAN, 0.15), 0.0);
    let b = resolved();
    let stick = |ly: f64, lx: f64, rx: f64| {
        b.gamepad_axes(
            |stick| match stick {
                "left_stick_y" => Some(ly),
                "left_stick_x" => Some(lx),
                "right_stick_x" => Some(rx),
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
    file.gamepad.buttons.push(ButtonBinding { button: "dpad_up".into(), action: None, axis: Some("forward".into()), direction: Some(1) });
    let b2 = file.resolve().unwrap();
    assert_eq!(b2.gamepad_axes(|_| None, |button| button == "dpad_up").forward, 1.0);
    assert_eq!(b.button_actions, vec![("south".to_string(), "stop".to_string()), ("east".to_string(), "halt".to_string())]);
    assert_eq!(b2.all_buttons().collect::<Vec<_>>(), vec!["dpad_up", "south", "east"]);
}

/// A bad binding is refused naming its field; unknown fields and another
/// schema are refused before validation.
#[test]
fn bad_bindings_are_refused_naming_the_field() {
    fn refuse(edit: impl Fn(&mut Value)) -> String {
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
    let b = resolved();
    let (axes, ignored) = supported_only(b.keyboard_axes(held(&["KeyW", "KeyQ"])), differential);
    assert_eq!((axes, ignored), (Axes { forward: 1.0, lateral: 0.0, yaw: 0.0 }, vec!["lateral"]));
    let (axes, ignored) = supported_only(b.keyboard_axes(held(&["KeyQ"])), differential);
    assert_eq!((axes, ignored), (Axes::ZERO, vec!["lateral"]));
    let a = Axes { forward: -0.5, lateral: 0.25, yaw: 1.0 };
    assert_eq!(supported_only(a, [true; 3]), (a, Vec::<&str>::new()));
    assert_eq!(supported_only(Axes { forward: 0.0, lateral: 0.0, yaw: -1.0 }, differential), (Axes { forward: 0.0, lateral: 0.0, yaw: -1.0 }, Vec::<&str>::new()));
    assert_eq!(supported_only(Axes { forward: 1.0, lateral: -1.0, yaw: 1.0 }, [false; 3]).1, vec!["forward", "lateral", "yaw"]);
}

fn pad(axes: [f64; 4]) -> W3cGamepad {
    W3cGamepad { mapping: "standard".into(), id: "test pad".into(), axes: axes.map(Some).to_vec(), buttons: vec![Some(0.0); 17], pressed: vec![false; 17] }
}
fn devices(keys: &[&str], gamepads: Vec<W3cGamepad>) -> BrowserDevices {
    BrowserDevices { keys: keys.iter().map(|k| k.to_string()).collect(), gamepads }
}

/// The browser's W3C Standard Gamepad: stick Y is +1 down, so the left
/// stick pushed up (axes[1] = -1) is +forward; the right stick pushed left
/// (axes[2] = -1) is +yaw with the default inversion.
#[test]
fn w3c_sticks_mean_the_same_motion_as_native() {
    let b = resolved();
    let all = [true; 3];
    let d = browser_axes(&b, &devices(&[], vec![pad([0.0, -1.0, 0.0, 0.0])]), all);
    assert_eq!(d.axes, Axes3 { forward: 1.0, lateral: 0.0, yaw: 0.0 });
    assert_eq!(d.source, Some("gamepad"));
    let d = browser_axes(&b, &devices(&[], vec![pad([0.0, 0.0, -1.0, 0.0])]), all);
    assert_eq!(d.axes, Axes3 { forward: 0.0, lateral: 0.0, yaw: 1.0 });
    // Inside the deadzone: nothing, and no source.
    let d = browser_axes(&b, &devices(&[], vec![pad([0.1, -0.1, 0.1, 0.0])]), all);
    assert_eq!((d.axes, d.source), (Axes3 { forward: 0.0, lateral: 0.0, yaw: 0.0 }, None));
    // Keyboard and pad sum and clamp; opposite inputs cancel to no source.
    let d = browser_axes(&b, &devices(&["KeyW"], vec![pad([0.0, -1.0, 0.0, 0.0])]), all);
    assert_eq!((d.axes.forward, d.source), (1.0, Some("keyboard+gamepad")));
    let d = browser_axes(&b, &devices(&["KeyS"], vec![pad([0.0, -1.0, 0.0, 0.0])]), all);
    assert_eq!((d.axes.forward, d.source), (0.0, None));
    assert_eq!((d.keyboard.forward, d.gamepad.forward), (-1.0, 1.0));
}

#[test]
fn non_standard_pads_are_ignored_and_named() {
    let b = resolved();
    let mut other = pad([0.0, -1.0, 0.0, 0.0]);
    other.mapping = String::new();
    other.id = "Odd Stick".into();
    let d = browser_axes(&b, &devices(&[], vec![other]), [true; 3]);
    assert_eq!(d.axes, Axes3 { forward: 0.0, lateral: 0.0, yaw: 0.0 });
    assert_eq!(d.ignored_pads.len(), 1);
    assert!(d.ignored_pads[0].starts_with("gamepad 0 `Odd Stick`: mapping `` is not the W3C standard layout"), "{}", d.ignored_pads[0]);
}

#[test]
fn held_action_keys_and_buttons_are_listed() {
    let b = resolved();
    let mut p = pad([0.0; 4]);
    p.pressed[0] = true; // south: stop
    let d = browser_axes(&b, &devices(&["KeyB"], vec![p]), [true; 3]);
    assert_eq!(
        d.held_actions,
        vec![HeldAction { input: "key KeyB".into(), action: "halt".into() }, HeldAction { input: "button south".into(), action: "stop".into() }]
    );
    assert_eq!(d.axes, Axes3 { forward: 0.0, lateral: 0.0, yaw: 0.0 });
}

/// Q on a differential robot (no lateral axis) is zeroed and named.
#[test]
fn browser_lateral_on_a_differential_robot_is_zeroed_and_named() {
    let b = resolved();
    let d = browser_axes(&b, &devices(&["KeyQ"], vec![]), [true, false, true]);
    assert_eq!(d.axes, Axes3 { forward: 0.0, lateral: 0.0, yaw: 0.0 });
    assert_eq!(d.ignored, vec!["lateral"]);
    assert_eq!(d.source, None);
    // The keyboard's own axes still show what was pressed.
    assert_eq!(d.keyboard.lateral, 1.0);
    // The DeviceAxes JSON the browser reads.
    let json = serde_json::to_value(&d).unwrap();
    assert_eq!(json["ignored"], json!(["lateral"]));
    assert_eq!(json["axes"], json!({"forward": 0.0, "lateral": 0.0, "yaw": 0.0}));
}

/// A null axis or button value (a NaN after JSON.stringify) reads as 0; the
/// other axes of the pad still drive.
#[test]
fn null_axis_values_read_as_zero() {
    let b = resolved();
    let devices: BrowserDevices = serde_json::from_value(json!({
        "keys": [],
        "gamepads": [{"mapping": "standard", "id": "pad", "axes": [null, -1.0, null, 0.0], "buttons": [null, 0.0], "pressed": [false, false]}]
    }))
    .unwrap();
    assert_eq!(devices.gamepads[0].axes[0], None);
    let d = browser_axes(&b, &devices, [true; 3]);
    assert_eq!(d.axes, Axes3 { forward: 1.0, lateral: 0.0, yaw: 0.0 });
    assert_eq!(d.source, Some("gamepad"));
}

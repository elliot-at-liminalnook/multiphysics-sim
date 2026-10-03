//! Device bindings (`sim.drive-bindings/1`): which keys, sticks and buttons
//! make the normalized drive axes (forward, lateral, yaw in -1..1) and which
//! send a drive profile's named actions. Per device, shared by every robot
//! (a robot's own limits and actions are its `sim.drive/1` profile's).
//!
//! One parser and one set of defaults for every host: the native viewer
//! (`sim-spatial`'s `drive_input`, which maps the names below to Bevy's
//! `KeyCode`, `GamepadAxis` and `GamepadButton` and persists the file as the
//! `drive_bindings` preferences group) and the browser (`sim-web`'s
//! `default_drive_bindings`, `validate_drive_bindings` and
//! `drive_device_axes`, which read the W3C `KeyboardEvent.code` and
//! Standard Gamepad layout through [`BrowserDevices`]). No Bevy here.
//!
//! The committed defaults are [`BindingsFile::default`] (data in code, not a
//! file): a host stores them only once a user sets bindings, so a change to
//! the defaults reaches every user who never customized them. Names are
//! strings validated here, so every refusal names its field
//! (`drive_bindings.keyboard.axes[2].key`).
//!
//! Signs: forward is positive ahead, lateral positive to the left and yaw
//! positive counter-clockwise seen from above (a left turn), as
//! `sim_domain_control::drive::kinematics`. Stick values in this file's
//! convention are gilrs's: up and right are positive on every platform
//! (gilrs-0.11.2/src/gamepad.rs:1171 flips the platforms whose Y axis points
//! down), so the default forward stick is not inverted and the default yaw
//! and lateral sticks are (stick left must be +yaw and +lateral). The W3C
//! Standard Gamepad reports a stick's Y as +1 *down*; [`STICKS`] carries the
//! sign that converts it ([`W3cSource`]), applied in [`browser_axes`], so a
//! bindings file means the same motion in the browser and the native viewer.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_domain_control::drive::kinematics::{AXIS_NAMES, Axes};

/// The bindings schema this build reads and writes.
pub const SCHEMA: &str = "sim.drive-bindings/1";
/// The default gamepad stick deadzone (fraction of full deflection).
pub const DEFAULT_DEADZONE: f64 = 0.15;
/// What each axis's positive direction means (index order of `AXIS_NAMES`).
pub const POSITIVE: [&str; 3] = ["ahead", "left", "turn left (CCW)"];
/// Keys a host already owns: binding them is refused by name. G, C, J, F and
/// H are Robot mode's view keys. Space, Enter and Tab are the native UI
/// kit's activation and focus keys: a focused button consumes their press
/// (`ui_kit::activation::consume_keys`) and activates itself, so a drive
/// action bound to them would press that button instead of reaching the
/// robot. Escape is always Stop.
pub const RESERVED: [(&str, &str); 9] = [
    ("KeyG", "robot mode's graphs key"),
    ("KeyC", "robot mode's contacts overlay key"),
    ("KeyJ", "robot mode's joint frames overlay key"),
    ("KeyF", "robot mode's deflections overlay key"),
    ("KeyH", "robot mode's stress overlay key"),
    ("Space", "the UI kit's activation key (a focused button takes the press and activates itself)"),
    ("Enter", "the UI kit's activation key (a focused button takes the press and activates itself)"),
    ("Tab", "the UI kit's focus key"),
    ("Escape", "the drive's Stop key, always (not bindable)"),
];

/// Physical keys a binding may name: W3C `KeyboardEvent.code` strings, which
/// are also Bevy's `KeyCode` variant names (letters, digits and arrows).
pub const KEYS: [&str; 40] = [
    "KeyA", "KeyB", "KeyC", "KeyD", "KeyE", "KeyF", "KeyG", "KeyH", "KeyI", "KeyJ", "KeyK", "KeyL", "KeyM", "KeyN", "KeyO", "KeyP", "KeyQ", "KeyR",
    "KeyS", "KeyT", "KeyU", "KeyV", "KeyW", "KeyX", "KeyY", "KeyZ", "Digit0", "Digit1", "Digit2", "Digit3", "Digit4", "Digit5", "Digit6", "Digit7",
    "Digit8", "Digit9", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight",
];

/// Where the browser reads a binding name on a W3C Standard Gamepad
/// (`navigator.getGamepads()[i]` with `mapping == "standard"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "index")]
pub enum W3cSource {
    /// `gamepad.axes[i]`.
    Axis(usize),
    /// `gamepad.buttons[i].value` (0..1; an analog trigger).
    ButtonValue(usize),
    /// Not in the Standard Gamepad layout: never read in the browser.
    Unmapped,
}

/// A stick axis a binding may name: (name, label, W3C source, sign). The
/// sign converts the W3C value to this file's (gilrs) convention: the
/// Standard Gamepad's stick Y is +1 down, gilrs's +1 up, so Y is -1.
/// `left_z` / `right_z` are gilrs's analog trigger axes; the browser reads
/// the Standard Gamepad's trigger buttons 6 and 7 (0..1) for them, so there a
/// released trigger is 0 (gilrs may report -1..1 on some platforms).
pub const STICKS: [(&str, &str, W3cSource, f64); 6] = [
    ("left_stick_x", "Left stick X", W3cSource::Axis(0), 1.0),
    ("left_stick_y", "Left stick Y", W3cSource::Axis(1), -1.0),
    ("right_stick_x", "Right stick X", W3cSource::Axis(2), 1.0),
    ("right_stick_y", "Right stick Y", W3cSource::Axis(3), -1.0),
    ("left_z", "Left Z axis", W3cSource::ButtonValue(6), 1.0),
    ("right_z", "Right Z axis", W3cSource::ButtonValue(7), 1.0),
];
/// Buttons a binding may name (Bevy's standard `GamepadButton`s), with their
/// labels and W3C Standard Gamepad button index (`None`: not in that layout,
/// so never pressed in the browser).
pub const BUTTONS: [(&str, &str, Option<usize>); 19] = [
    ("south", "South button (A / Cross)", Some(0)),
    ("east", "East button (B / Circle)", Some(1)),
    ("north", "North button (Y / Triangle)", Some(3)),
    ("west", "West button (X / Square)", Some(2)),
    ("c", "C button", None),
    ("z", "Z button", None),
    ("left_trigger", "Left bumper", Some(4)),
    ("left_trigger2", "Left trigger", Some(6)),
    ("right_trigger", "Right bumper", Some(5)),
    ("right_trigger2", "Right trigger", Some(7)),
    ("select", "Select", Some(8)),
    ("start", "Start", Some(9)),
    ("mode", "Mode", Some(16)),
    ("left_thumb", "Left stick press", Some(10)),
    ("right_thumb", "Right stick press", Some(11)),
    ("dpad_up", "D-pad up", Some(12)),
    ("dpad_down", "D-pad down", Some(13)),
    ("dpad_left", "D-pad left", Some(14)),
    ("dpad_right", "D-pad right", Some(15)),
];

/// The stored bindings file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingsFile {
    pub schema: String,
    pub keyboard: KeyboardBindings,
    pub gamepad: GamepadBindings,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyboardBindings {
    #[serde(default)]
    pub axes: Vec<KeyAxis>,
    #[serde(default)]
    pub actions: Vec<KeyAction>,
}
/// A held key adds `direction` (+1 or -1) to `axis`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyAxis {
    /// A physical key by its W3C code name ([`KEYS`]: `KeyW`, `Digit1`, `ArrowUp`; not Space, Enter, Tab or Escape).
    pub key: String,
    /// `forward`, `lateral` or `yaw`.
    pub axis: String,
    pub direction: i8,
}
/// A key press sends the robot's drive profile action `action`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyAction {
    pub key: String,
    pub action: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GamepadBindings {
    /// Stick deflection below this fraction (0..1) reads as zero; above it the
    /// rest of the travel is rescaled to 0..1, so full deflection is still 1.
    pub deadzone: f64,
    #[serde(default)]
    pub axes: Vec<StickAxis>,
    #[serde(default)]
    pub buttons: Vec<ButtonBinding>,
}
/// A stick axis drives `axis` (negated when `invert`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StickAxis {
    /// `left_stick_x`, `left_stick_y`, `right_stick_x`, `right_stick_y`, `left_z` or `right_z` ([`STICKS`]).
    pub stick: String,
    pub axis: String,
    #[serde(default)]
    pub invert: bool,
}
/// A button: either a named action (on press) or an axis contribution
/// (`axis` with `direction`, while held). Exactly one of the two.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ButtonBinding {
    /// `south`, `east`, `north`, `west`, `dpad_up`, … (see [`BUTTONS`]).
    pub button: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<i8>,
}

fn key_axis(key: &str, axis: &str, direction: i8) -> KeyAxis {
    KeyAxis { key: key.into(), axis: axis.into(), direction }
}
impl Default for BindingsFile {
    /// W/S forward, A/D yaw (A turns left), Q/E lateral (Q left), X the
    /// profile's `stop`, B its `halt` ("brake"; not Space, which a focused
    /// kit button takes); the left stick's Y forward and X lateral, the right
    /// stick's X yaw, South `stop`, East `halt`, deadzone 0.15. A robot whose
    /// profile lacks an axis (the differential rover has no lateral) ignores
    /// that axis's inputs ([`supported_only`]).
    fn default() -> Self {
        Self {
            schema: SCHEMA.into(),
            keyboard: KeyboardBindings {
                axes: vec![
                    key_axis("KeyW", "forward", 1),
                    key_axis("KeyS", "forward", -1),
                    key_axis("KeyA", "yaw", 1),
                    key_axis("KeyD", "yaw", -1),
                    key_axis("KeyQ", "lateral", 1),
                    key_axis("KeyE", "lateral", -1),
                ],
                actions: vec![KeyAction { key: "KeyX".into(), action: "stop".into() }, KeyAction { key: "KeyB".into(), action: "halt".into() }],
            },
            gamepad: GamepadBindings {
                deadzone: DEFAULT_DEADZONE,
                axes: vec![
                    StickAxis { stick: "left_stick_y".into(), axis: "forward".into(), invert: false },
                    StickAxis { stick: "right_stick_x".into(), axis: "yaw".into(), invert: true },
                    StickAxis { stick: "left_stick_x".into(), axis: "lateral".into(), invert: true },
                ],
                buttons: vec![
                    ButtonBinding { button: "south".into(), action: Some("stop".into()), axis: None, direction: None },
                    ButtonBinding { button: "east".into(), action: Some("halt".into()), axis: None, direction: None },
                ],
            },
        }
    }
}

/// A key name as shown: `KeyW` → `W`, `Digit1` → `1`, `ArrowUp` → `↑`.
pub fn key_label(name: &str) -> String {
    match name {
        "ArrowUp" => "↑".into(),
        "ArrowDown" => "↓".into(),
        "ArrowLeft" => "←".into(),
        "ArrowRight" => "→".into(),
        _ => name.strip_prefix("Key").or_else(|| name.strip_prefix("Digit")).unwrap_or(name).into(),
    }
}
/// A stick name's label.
pub fn stick_label(name: &str) -> Option<&'static str> {
    STICKS.iter().find(|s| s.0 == name).map(|s| s.1)
}
/// A button name's label.
pub fn button_label(name: &str) -> Option<&'static str> {
    BUTTONS.iter().find(|b| b.0 == name).map(|b| b.1)
}
/// An axis name's index in `AXIS_NAMES`, refused naming `field`.
fn axis_index(name: &str, field: &str) -> Result<usize, String> {
    AXIS_NAMES.iter().position(|a| *a == name).ok_or_else(|| format!("{field}: `{name}` is not an axis; axes are forward, lateral, yaw"))
}
fn direction(value: i8, field: &str) -> Result<f64, String> {
    match value {
        1 | -1 => Ok(value as f64),
        v => Err(format!("{field}: {v} must be 1 or -1")),
    }
}
fn action_name(name: &str, field: &str) -> Result<(), String> {
    if name.trim().is_empty() || name.trim() != name {
        return Err(format!("{field}: `{name}` must be a non-empty profile action name without surrounding spaces"));
    }
    Ok(())
}

/// A validated file resolved for input: names with axis indices. Hosts map
/// the names to their own device types (`sim-spatial`: Bevy's; the browser:
/// [`browser_axes`]). Built only from a valid file ([`BindingsFile::resolve`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    /// (key name, axis index, direction ±1).
    pub keys: Vec<(String, usize, f64)>,
    /// (key name, profile action).
    pub key_actions: Vec<(String, String)>,
    pub deadzone: f64,
    /// (stick name, axis index, invert).
    pub sticks: Vec<(String, usize, bool)>,
    /// (button name, axis index, direction ±1).
    pub button_axes: Vec<(String, usize, f64)>,
    /// (button name, profile action).
    pub button_actions: Vec<(String, String)>,
}

impl BindingsFile {
    /// A stored `drive_bindings` value: the schema first (another version is
    /// refused naming both), then its shape (an unknown field is named by
    /// serde), then [`BindingsFile::validate`].
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let schema = value.get("schema").and_then(Value::as_str).ok_or_else(|| format!("drive_bindings.schema: missing; this build reads {SCHEMA}"))?;
        if schema != SCHEMA {
            return Err(format!("drive_bindings.schema: `{schema}` is not read by this build, which reads {SCHEMA}"));
        }
        let file: BindingsFile = serde_json::from_value(value.clone()).map_err(|e| format!("drive_bindings: {e}"))?;
        file.validate()?;
        Ok(file)
    }
    /// Every refusal names its field (`drive_bindings.keyboard.axes[2].key`).
    pub fn validate(&self) -> Result<(), String> {
        self.resolve().map(drop)
    }
    /// Check the file and resolve its names to axis indices.
    pub fn resolve(&self) -> Result<Resolved, String> {
        if self.schema != SCHEMA {
            return Err(format!("drive_bindings.schema: `{}` is not read by this build, which reads {SCHEMA}", self.schema));
        }
        // Each key and button is bound once, so one press has one meaning.
        let mut seen: Vec<(String, String)> = Vec::new();
        let mut claim = |name: &str, field: String| -> Result<(), String> {
            if let Some((_, first)) = seen.iter().find(|(n, _)| n == name) {
                return Err(format!("{field}: `{name}` is already bound at {first}"));
            }
            seen.push((name.to_string(), field));
            Ok(())
        };
        let key = |name: &str, field: &str| -> Result<(), String> {
            if let Some((_, why)) = RESERVED.iter().find(|(k, _)| *k == name) {
                return Err(format!("{field}: `{name}` is {why}"));
            }
            if KEYS.contains(&name) {
                Ok(())
            } else {
                Err(format!("{field}: `{name}` is not a bindable key (KeyA..KeyZ, Digit0..Digit9, ArrowUp/Down/Left/Right)"))
            }
        };
        let mut out = Resolved { keys: Vec::new(), key_actions: Vec::new(), deadzone: self.gamepad.deadzone, sticks: Vec::new(), button_axes: Vec::new(), button_actions: Vec::new() };
        for (i, b) in self.keyboard.axes.iter().enumerate() {
            let field = format!("drive_bindings.keyboard.axes[{i}]");
            key(&b.key, &format!("{field}.key"))?;
            claim(&b.key, format!("{field}.key"))?;
            out.keys.push((b.key.clone(), axis_index(&b.axis, &format!("{field}.axis"))?, direction(b.direction, &format!("{field}.direction"))?));
        }
        for (i, b) in self.keyboard.actions.iter().enumerate() {
            let field = format!("drive_bindings.keyboard.actions[{i}]");
            key(&b.key, &format!("{field}.key"))?;
            claim(&b.key, format!("{field}.key"))?;
            action_name(&b.action, &format!("{field}.action"))?;
            out.key_actions.push((b.key.clone(), b.action.clone()));
        }
        if !(self.gamepad.deadzone.is_finite() && (0.0..1.0).contains(&self.gamepad.deadzone)) {
            return Err(format!("drive_bindings.gamepad.deadzone: {} must be at least 0 and below 1", self.gamepad.deadzone));
        }
        for (i, b) in self.gamepad.axes.iter().enumerate() {
            let field = format!("drive_bindings.gamepad.axes[{i}]");
            if stick_label(&b.stick).is_none() {
                return Err(format!("{field}.stick: `{}` is not a stick axis ({})", b.stick, STICKS.map(|s| s.0).join(", ")));
            }
            claim(&format!("stick {}", b.stick), format!("{field}.stick"))?;
            out.sticks.push((b.stick.clone(), axis_index(&b.axis, &format!("{field}.axis"))?, b.invert));
        }
        for (i, b) in self.gamepad.buttons.iter().enumerate() {
            let field = format!("drive_bindings.gamepad.buttons[{i}]");
            if button_label(&b.button).is_none() {
                return Err(format!("{field}.button: `{}` is not a gamepad button ({})", b.button, BUTTONS.map(|s| s.0).join(", ")));
            }
            claim(&format!("button {}", b.button), format!("{field}.button"))?;
            match (&b.action, &b.axis, b.direction) {
                (Some(action), None, None) => {
                    action_name(action, &format!("{field}.action"))?;
                    out.button_actions.push((b.button.clone(), action.clone()));
                }
                (None, Some(axis), Some(d)) => out.button_axes.push((b.button.clone(), axis_index(axis, &format!("{field}.axis"))?, direction(d, &format!("{field}.direction"))?)),
                _ => return Err(format!("{field}: give either action, or axis with direction (1 | -1)")),
            }
        }
        Ok(out)
    }
    /// (input label, what it does): each binding, then the stops that hold
    /// whatever the bindings are. Labels follow the native viewer's names.
    pub fn describe(&self) -> Vec<(String, String)> {
        let axis = |name: &str, sign: f64| {
            let i = AXIS_NAMES.iter().position(|a| *a == name).unwrap_or(0);
            format!("{name} {} ({})", if sign > 0.0 { "+1" } else { "−1" }, if sign > 0.0 { POSITIVE[i].to_string() } else { format!("opposite of {}", POSITIVE[i]) })
        };
        let mut out: Vec<(String, String)> = self.keyboard.axes.iter().map(|b| (key_label(&b.key), axis(&b.axis, b.direction as f64))).collect();
        out.extend(self.keyboard.actions.iter().map(|b| (key_label(&b.key), format!("profile action `{}`", b.action))));
        for b in &self.gamepad.axes {
            let label = stick_label(&b.stick).map_or(b.stick.clone(), str::to_string);
            let i = AXIS_NAMES.iter().position(|a| *a == b.axis).unwrap_or(0);
            let toward = if b.invert { "inverted: stick down/left" } else { "stick up/right" };
            out.push((label, format!("{} ({toward} is +, {}; deadzone {})", b.axis, POSITIVE[i], self.gamepad.deadzone)));
        }
        for b in &self.gamepad.buttons {
            let label = button_label(&b.button).map_or(b.button.clone(), str::to_string);
            let does = match (&b.action, &b.axis, b.direction) {
                (Some(action), ..) => format!("profile action `{action}`"),
                (None, Some(a), Some(d)) => axis(a, d as f64),
                _ => "nothing".into(),
            };
            out.push((label, does));
        }
        out.push(("Escape".into(), "stop (always; held inputs must be released before they drive again)".into()));
        out.push(("Window focus loss".into(), "stop, when the keys or gamepad were driving (a REST client's requests are left to the deadman)".into()));
        out
    }
}

impl Resolved {
    /// The keyboard's axes: each held key's contribution, summed per axis and
    /// clamped to -1..1 (W+S is 0; W with A is forward and a left turn).
    pub fn keyboard_axes(&self, held: impl Fn(&str) -> bool) -> Axes {
        let mut a = [0.0; 3];
        for (key, axis, sign) in &self.keys {
            if held(key) {
                a[*axis] += sign;
            }
        }
        Axes::from_array(a.map(|v: f64| v.clamp(-1.0, 1.0)))
    }
    /// The gamepad's axes: each bound stick (its value in this file's
    /// convention, see the module docs) through the deadzone and its
    /// inversion, plus held axis buttons, summed per axis and clamped.
    pub fn gamepad_axes(&self, value: impl Fn(&str) -> Option<f64>, held: impl Fn(&str) -> bool) -> Axes {
        let mut a = [0.0; 3];
        for (stick, axis, invert) in &self.sticks {
            let v = shape(value(stick).unwrap_or(0.0), self.deadzone);
            a[*axis] += if *invert { -v } else { v };
        }
        for (button, axis, sign) in &self.button_axes {
            if held(button) {
                a[*axis] += sign;
            }
        }
        Axes::from_array(a.map(|v: f64| v.clamp(-1.0, 1.0)))
    }
    /// Every key the keyboard bindings read (axes and actions).
    pub fn all_keys(&self) -> impl Iterator<Item = &str> + '_ {
        self.keys.iter().map(|(k, ..)| k.as_str()).chain(self.key_actions.iter().map(|(k, _)| k.as_str()))
    }
    /// Every button the gamepad bindings read (axes and actions).
    pub fn all_buttons(&self) -> impl Iterator<Item = &str> + '_ {
        self.button_axes.iter().map(|(b, ..)| b.as_str()).chain(self.button_actions.iter().map(|(b, _)| b.as_str()))
    }
}

/// A stick value through the deadzone: zero inside it, the rest of the travel
/// rescaled to 0..1 with the sign kept. NaN reads as zero.
pub fn shape(raw: f64, deadzone: f64) -> f64 {
    if !(raw.abs() > deadzone) {
        return 0.0;
    }
    (((raw.abs() - deadzone) / (1.0 - deadzone)).min(1.0)) * raw.signum()
}

/// `axes` with every axis the profile does not support (`supported`, the
/// resolved limits' flags in `AXIS_NAMES` order) set to zero, and the names
/// of the unsupported axes that were nonzero. Device input only: REST stays
/// strict (`kinematics::scale` refuses a nonzero unsupported axis by name).
pub fn supported_only(axes: Axes, supported: [bool; 3]) -> (Axes, Vec<&'static str>) {
    let mut a = axes.to_array();
    let mut ignored = Vec::new();
    for i in 0..3 {
        if !supported[i] {
            if a[i] != 0.0 {
                ignored.push(AXIS_NAMES[i]);
            }
            a[i] = 0.0;
        }
    }
    (Axes::from_array(a), ignored)
}

/// A host's bindings answer (REST `drive_bindings`, the browser's
/// `default_drive_bindings` / `validate_drive_bindings`): the bindings in
/// effect, whether stored, the described table and the W3C Standard Gamepad
/// table the browser reads them through.
pub fn json(stored: Option<&BindingsFile>) -> Value {
    let file = stored.cloned().unwrap_or_default();
    let describe = file.describe();
    json!({"schema": SCHEMA, "stored": stored.is_some(), "bindings": file,
        "describe": describe.into_iter().map(|(input, does)| json!({"input": input, "does": does})).collect::<Vec<_>>(),
        "w3c_standard_gamepad": {
            "sticks": STICKS.iter().map(|(name, label, source, sign)| json!({"stick": name, "label": label, "source": source, "sign": sign})).collect::<Vec<_>>(),
            "buttons": BUTTONS.iter().map(|(name, label, index)| json!({"button": name, "label": label, "index": index})).collect::<Vec<_>>(),
        }})
}

/// One W3C gamepad as the browser polls it each animation frame.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct W3cGamepad {
    /// `Gamepad.mapping`: only `"standard"` pads are read (other layouts'
    /// indices mean nothing fixed); others are named in [`DeviceAxes::ignored_pads`].
    #[serde(default)]
    pub mapping: String,
    #[serde(default)]
    pub id: String,
    /// `gamepad.axes`. A null (what `JSON.stringify` makes of a NaN) reads as 0,
    /// so one bad axis does not refuse the whole poll.
    pub axes: Vec<Option<f64>>,
    /// `buttons[i].value` (0..1; 1 when pressed for a digital button); null reads as 0.
    pub buttons: Vec<Option<f64>>,
    /// `buttons[i].pressed`.
    pub pressed: Vec<bool>,
}
/// What the browser's devices hold now.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserDevices {
    /// `KeyboardEvent.code` of every key held (keydown seen, keyup not yet),
    /// already filtered by the page: not while a text field has focus, not
    /// while a Cmd/Ctrl/Alt chord is held, not keys disarmed by a stop.
    #[serde(default)]
    pub keys: Vec<String>,
    #[serde(default)]
    pub gamepads: Vec<W3cGamepad>,
}
/// The browser devices through the bindings.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DeviceAxes {
    /// What to request: keyboard plus gamepad, each with the axes the
    /// profile does not support zeroed first ([`supported_only`]), summed and
    /// clamped to -1..1. Zero when idle.
    pub axes: Axes3,
    /// Unsupported axes some device asked for (zeroed in `axes`).
    pub ignored: Vec<&'static str>,
    /// `keyboard`, `gamepad`, `keyboard+gamepad`, or None when `axes` is zero.
    pub source: Option<&'static str>,
    /// Keyboard axes ([`Resolved::keyboard_axes`]).
    pub keyboard: Axes3,
    /// Every standard pad summed and clamped ([`Resolved::gamepad_axes`]).
    pub gamepad: Axes3,
    /// Bound action keys and buttons held now (`"key KeyX"`, `"button south"`
    /// with the action): the page sends an action on its rising edge.
    pub held_actions: Vec<HeldAction>,
    /// Pads not read, with the reason.
    pub ignored_pads: Vec<String>,
}
/// Axes in JSON form.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Axes3 {
    pub forward: f64,
    pub lateral: f64,
    pub yaw: f64,
}
impl From<Axes> for Axes3 {
    fn from(a: Axes) -> Self {
        Self { forward: a.forward, lateral: a.lateral, yaw: a.yaw }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HeldAction {
    /// `key KeyX` or `button south`: the input, for edge detection.
    pub input: String,
    pub action: String,
}

/// The browser's held keys and standard pads through `resolved`: the W3C
/// values converted to this file's sign convention ([`STICKS`]), then the
/// same [`Resolved::keyboard_axes`] / [`Resolved::gamepad_axes`] the native
/// viewer uses, then [`supported_only`] per device with `supported` (the
/// robot's resolved limits' flags). No scaling to a twist here: that is
/// `DriveRequest::interpret` against the robot's profile.
pub fn browser_axes(resolved: &Resolved, devices: &BrowserDevices, supported: [bool; 3]) -> DeviceAxes {
    let keyboard = resolved.keyboard_axes(|k| devices.keys.iter().any(|h| h == k));
    let mut pad = [0.0; 3];
    let mut held_actions: Vec<HeldAction> = resolved
        .key_actions
        .iter()
        .filter(|(k, _)| devices.keys.iter().any(|h| h == k))
        .map(|(k, a)| HeldAction { input: format!("key {k}"), action: a.clone() })
        .collect();
    let mut ignored_pads = Vec::new();
    for (i, g) in devices.gamepads.iter().enumerate() {
        if g.mapping != "standard" {
            ignored_pads.push(format!("gamepad {i} `{}`: mapping `{}` is not the W3C standard layout; its axes and buttons are not read", g.id, g.mapping));
            continue;
        }
        let value = |stick: &str| -> Option<f64> {
            let (_, _, source, sign) = STICKS.iter().find(|s| s.0 == stick)?;
            let raw = match source {
                W3cSource::Axis(i) => g.axes.get(*i).copied(),
                W3cSource::ButtonValue(i) => g.buttons.get(*i).copied(),
                W3cSource::Unmapped => None,
            }?
            .unwrap_or(0.0);
            Some(sign * raw)
        };
        let pressed = |button: &str| -> bool {
            BUTTONS.iter().find(|b| b.0 == button).and_then(|b| b.2).and_then(|i| g.pressed.get(i).copied()).unwrap_or(false)
        };
        let a = resolved.gamepad_axes(value, pressed).to_array();
        for k in 0..3 {
            pad[k] += a[k];
        }
        for (button, action) in &resolved.button_actions {
            if pressed(button) {
                let input = format!("button {button}");
                if !held_actions.iter().any(|h| h.input == input) {
                    held_actions.push(HeldAction { input, action: action.clone() });
                }
            }
        }
    }
    let gamepad = Axes::from_array(pad.map(|v: f64| v.clamp(-1.0, 1.0)));
    let (kb, kb_ignored) = supported_only(keyboard, supported);
    let (gp, gp_ignored) = supported_only(gamepad, supported);
    let ignored: Vec<&'static str> = AXIS_NAMES.iter().copied().filter(|n| kb_ignored.contains(n) || gp_ignored.contains(n)).collect();
    let (k, g) = (kb.to_array(), gp.to_array());
    let axes = Axes::from_array([0, 1, 2].map(|i| (k[i] + g[i]).clamp(-1.0, 1.0)));
    // None whenever nothing is requested, including a keyboard and a pad cancelling out.
    let source = match (kb != Axes::ZERO, gp != Axes::ZERO) {
        _ if axes == Axes::ZERO => None,
        (true, false) => Some("keyboard"),
        (false, true) => Some("gamepad"),
        (true, true) => Some("keyboard+gamepad"),
        (false, false) => None,
    };
    DeviceAxes { axes: axes.into(), ignored, source, keyboard: keyboard.into(), gamepad: gamepad.into(), held_actions, ignored_pads }
}

#[cfg(test)]
mod tests;

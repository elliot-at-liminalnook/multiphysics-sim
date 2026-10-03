//! Device bindings (`sim.drive-bindings/1`): which keys, sticks and buttons
//! make the normalized drive axes (forward, lateral, yaw in -1..1) and which
//! send a drive profile's named actions. Per device, shared by every robot
//! (a robot's own limits and actions are its `sim.drive/1` profile's), and
//! persisted as the `drive_bindings` group of the viewer preferences
//! (`app::settings::SettingsOwner`, the one owner of preference files).
//!
//! The committed defaults are [`BindingsFile::default`] (data in code, not a
//! file): they are stored in the preferences only once a user sets bindings
//! (REST `drive_bindings`), so a change to the defaults reaches every user
//! who never customized them. Names are strings validated here, so every
//! refusal names its field (`drive_bindings.keyboard.axes[2].key`).
//!
//! Signs: forward is positive ahead, lateral positive to the left and yaw
//! positive counter-clockwise seen from above (a left turn), as
//! `sim_domain_control::drive::kinematics`. Gamepad sticks read through gilrs
//! report up and right as positive on every platform
//! (gilrs-0.11.2/src/gamepad.rs:1171 flips the platforms whose Y axis points
//! down), so the default forward stick is not inverted and the default yaw
//! and lateral sticks are (stick left must be +yaw and +lateral).
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sim_domain_control::drive::kinematics::{AXIS_NAMES, Axes};

/// The bindings schema this build reads and writes.
pub const SCHEMA: &str = "sim.drive-bindings/1";
/// The default gamepad stick deadzone (fraction of full deflection).
pub const DEFAULT_DEADZONE: f64 = 0.15;
/// What each axis's positive direction means (index order of `AXIS_NAMES`).
const POSITIVE: [&str; 3] = ["ahead", "left", "turn left (CCW)"];
/// Keys robot mode or the UI kit already owns: binding them is refused by
/// name. Space, Enter and Tab are the kit's activation and focus keys: a
/// focused button consumes their press (`ui_kit::activation::consume_keys`)
/// and activates itself, so a drive action bound to them would press that
/// button instead of reaching the robot. Escape is always Stop.
const RESERVED: [(&str, &str); 9] = [
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

/// The stored bindings file (the preferences' `drive_bindings` group).
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
    /// A physical key by its W3C code name, as Bevy's `KeyCode` (`KeyW`, `Digit1`, `ArrowUp`; not Space, Enter, Tab or Escape).
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
    /// `left_stick_x`, `left_stick_y`, `right_stick_x`, `right_stick_y`, `left_z` or `right_z`.
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
    /// that axis's inputs (`input::supported_only`).
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

/// Physical keys a binding may name (Bevy `KeyCode` names): letters, digits
/// and arrows. Modifiers, Escape (always Stop) and Space/Enter/Tab (the UI
/// kit's activation and focus keys) are not bindable ([`RESERVED`] names why).
fn key_code(name: &str) -> Option<KeyCode> {
    use KeyCode::*;
    Some(match name {
        "KeyA" => KeyA, "KeyB" => KeyB, "KeyC" => KeyC, "KeyD" => KeyD, "KeyE" => KeyE, "KeyF" => KeyF, "KeyG" => KeyG,
        "KeyH" => KeyH, "KeyI" => KeyI, "KeyJ" => KeyJ, "KeyK" => KeyK, "KeyL" => KeyL, "KeyM" => KeyM, "KeyN" => KeyN,
        "KeyO" => KeyO, "KeyP" => KeyP, "KeyQ" => KeyQ, "KeyR" => KeyR, "KeyS" => KeyS, "KeyT" => KeyT, "KeyU" => KeyU,
        "KeyV" => KeyV, "KeyW" => KeyW, "KeyX" => KeyX, "KeyY" => KeyY, "KeyZ" => KeyZ,
        "Digit0" => Digit0, "Digit1" => Digit1, "Digit2" => Digit2, "Digit3" => Digit3, "Digit4" => Digit4,
        "Digit5" => Digit5, "Digit6" => Digit6, "Digit7" => Digit7, "Digit8" => Digit8, "Digit9" => Digit9,
        "ArrowUp" => ArrowUp, "ArrowDown" => ArrowDown, "ArrowLeft" => ArrowLeft, "ArrowRight" => ArrowRight,
        _ => return None,
    })
}
/// A key name as shown: `KeyW` → `W`, `Digit1` → `1`, `ArrowUp` → `↑`.
fn key_label(name: &str) -> String {
    match name {
        "ArrowUp" => "↑".into(),
        "ArrowDown" => "↓".into(),
        "ArrowLeft" => "←".into(),
        "ArrowRight" => "→".into(),
        _ => name.strip_prefix("Key").or_else(|| name.strip_prefix("Digit")).unwrap_or(name).into(),
    }
}
/// Stick axes a binding may name, with their labels.
pub const STICKS: [(&str, GamepadAxis, &str); 6] = [
    ("left_stick_x", GamepadAxis::LeftStickX, "Left stick X"),
    ("left_stick_y", GamepadAxis::LeftStickY, "Left stick Y"),
    ("right_stick_x", GamepadAxis::RightStickX, "Right stick X"),
    ("right_stick_y", GamepadAxis::RightStickY, "Right stick Y"),
    ("left_z", GamepadAxis::LeftZ, "Left Z axis"),
    ("right_z", GamepadAxis::RightZ, "Right Z axis"),
];
/// Buttons a binding may name (Bevy's standard `GamepadButton`s), with their labels.
pub const BUTTONS: [(&str, GamepadButton, &str); 19] = [
    ("south", GamepadButton::South, "South button (A / Cross)"),
    ("east", GamepadButton::East, "East button (B / Circle)"),
    ("north", GamepadButton::North, "North button (Y / Triangle)"),
    ("west", GamepadButton::West, "West button (X / Square)"),
    ("c", GamepadButton::C, "C button"),
    ("z", GamepadButton::Z, "Z button"),
    ("left_trigger", GamepadButton::LeftTrigger, "Left bumper"),
    ("left_trigger2", GamepadButton::LeftTrigger2, "Left trigger"),
    ("right_trigger", GamepadButton::RightTrigger, "Right bumper"),
    ("right_trigger2", GamepadButton::RightTrigger2, "Right trigger"),
    ("select", GamepadButton::Select, "Select"),
    ("start", GamepadButton::Start, "Start"),
    ("mode", GamepadButton::Mode, "Mode"),
    ("left_thumb", GamepadButton::LeftThumb, "Left stick press"),
    ("right_thumb", GamepadButton::RightThumb, "Right stick press"),
    ("dpad_up", GamepadButton::DPadUp, "D-pad up"),
    ("dpad_down", GamepadButton::DPadDown, "D-pad down"),
    ("dpad_left", GamepadButton::DPadLeft, "D-pad left"),
    ("dpad_right", GamepadButton::DPadRight, "D-pad right"),
];
fn stick(name: &str) -> Option<(GamepadAxis, &'static str)> {
    STICKS.iter().find(|(n, ..)| *n == name).map(|(_, a, l)| (*a, *l))
}
fn button(name: &str) -> Option<(GamepadButton, &'static str)> {
    BUTTONS.iter().find(|(n, ..)| *n == name).map(|(_, b, l)| (*b, *l))
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

/// The bindings resolved for input: Bevy keys, sticks and buttons with axis
/// indices. Built only from a valid file ([`DriveBindings::new`]).
#[derive(Clone, Debug, PartialEq)]
struct Compiled {
    keys: Vec<(KeyCode, usize, f64)>,
    key_actions: Vec<(KeyCode, String)>,
    deadzone: f64,
    sticks: Vec<(GamepadAxis, usize, bool)>,
    button_axes: Vec<(GamepadButton, usize, f64)>,
    button_actions: Vec<(GamepadButton, String)>,
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
        self.compile().map(drop)
    }
    fn compile(&self) -> Result<Compiled, String> {
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
        let key = |name: &str, field: &str| -> Result<KeyCode, String> {
            if let Some((_, why)) = RESERVED.iter().find(|(k, _)| *k == name) {
                return Err(format!("{field}: `{name}` is {why}"));
            }
            key_code(name).ok_or_else(|| format!("{field}: `{name}` is not a bindable key (KeyA..KeyZ, Digit0..Digit9, ArrowUp/Down/Left/Right)"))
        };
        let mut out = Compiled { keys: Vec::new(), key_actions: Vec::new(), deadzone: self.gamepad.deadzone, sticks: Vec::new(), button_axes: Vec::new(), button_actions: Vec::new() };
        for (i, b) in self.keyboard.axes.iter().enumerate() {
            let field = format!("drive_bindings.keyboard.axes[{i}]");
            let code = key(&b.key, &format!("{field}.key"))?;
            claim(&b.key, format!("{field}.key"))?;
            out.keys.push((code, axis_index(&b.axis, &format!("{field}.axis"))?, direction(b.direction, &format!("{field}.direction"))?));
        }
        for (i, b) in self.keyboard.actions.iter().enumerate() {
            let field = format!("drive_bindings.keyboard.actions[{i}]");
            let code = key(&b.key, &format!("{field}.key"))?;
            claim(&b.key, format!("{field}.key"))?;
            action_name(&b.action, &format!("{field}.action"))?;
            out.key_actions.push((code, b.action.clone()));
        }
        if !(self.gamepad.deadzone.is_finite() && (0.0..1.0).contains(&self.gamepad.deadzone)) {
            return Err(format!("drive_bindings.gamepad.deadzone: {} must be at least 0 and below 1", self.gamepad.deadzone));
        }
        for (i, b) in self.gamepad.axes.iter().enumerate() {
            let field = format!("drive_bindings.gamepad.axes[{i}]");
            let (axis, _) = stick(&b.stick).ok_or_else(|| format!("{field}.stick: `{}` is not a stick axis ({})", b.stick, STICKS.map(|s| s.0).join(", ")))?;
            claim(&format!("stick {}", b.stick), format!("{field}.stick"))?;
            out.sticks.push((axis, axis_index(&b.axis, &format!("{field}.axis"))?, b.invert));
        }
        for (i, b) in self.gamepad.buttons.iter().enumerate() {
            let field = format!("drive_bindings.gamepad.buttons[{i}]");
            let (code, _) = button(&b.button).ok_or_else(|| format!("{field}.button: `{}` is not a gamepad button ({})", b.button, BUTTONS.map(|s| s.0).join(", ")))?;
            claim(&format!("button {}", b.button), format!("{field}.button"))?;
            match (&b.action, &b.axis, b.direction) {
                (Some(action), None, None) => {
                    action_name(action, &format!("{field}.action"))?;
                    out.button_actions.push((code, action.clone()));
                }
                (None, Some(axis), Some(d)) => out.button_axes.push((code, axis_index(axis, &format!("{field}.axis"))?, direction(d, &format!("{field}.direction"))?)),
                _ => return Err(format!("{field}: give either action, or axis with direction (1 | -1)")),
            }
        }
        Ok(out)
    }
}

/// The active, validated bindings: what the drive input systems read and the
/// inspector describes. Written only by `drive_input::sync_bindings` from the
/// settings owner's `drive_bindings` (the defaults until the user sets them).
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct DriveBindings {
    /// The bindings in effect.
    pub file: BindingsFile,
    /// The preferences store them (false: the committed defaults).
    pub stored: bool,
    compiled: Compiled,
}
impl Default for DriveBindings {
    fn default() -> Self {
        Self::new(BindingsFile::default(), false).expect("the committed default drive bindings validate (drive_input tests)")
    }
}
impl DriveBindings {
    pub fn new(file: BindingsFile, stored: bool) -> Result<Self, String> {
        let compiled = file.compile()?;
        Ok(Self { file, stored, compiled })
    }
    /// (input label, what it does), for the inspector: each binding, then
    /// the stops that hold whatever the bindings are.
    pub fn describe(&self) -> Vec<(String, String)> {
        let axis = |name: &str, sign: f64| {
            let i = AXIS_NAMES.iter().position(|a| *a == name).unwrap_or(0);
            format!("{name} {} ({})", if sign > 0.0 { "+1" } else { "−1" }, if sign > 0.0 { POSITIVE[i].to_string() } else { format!("opposite of {}", POSITIVE[i]) })
        };
        let mut out: Vec<(String, String)> = self.file.keyboard.axes.iter().map(|b| (key_label(&b.key), axis(&b.axis, b.direction as f64))).collect();
        out.extend(self.file.keyboard.actions.iter().map(|b| (key_label(&b.key), format!("profile action `{}`", b.action))));
        for b in &self.file.gamepad.axes {
            let label = stick(&b.stick).map_or(b.stick.clone(), |(_, l)| l.to_string());
            let i = AXIS_NAMES.iter().position(|a| *a == b.axis).unwrap_or(0);
            let toward = if b.invert { "inverted: stick down/left" } else { "stick up/right" };
            out.push((label, format!("{} ({toward} is +, {}; deadzone {})", b.axis, POSITIVE[i], self.file.gamepad.deadzone)));
        }
        for b in &self.file.gamepad.buttons {
            let label = button(&b.button).map_or(b.button.clone(), |(_, l)| l.to_string());
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
    /// The keyboard's axes: each held key's contribution, summed per axis and
    /// clamped to -1..1 (W+S is 0; W with A is forward and a left turn).
    pub fn keyboard_axes(&self, held: impl Fn(KeyCode) -> bool) -> Axes {
        let mut a = [0.0; 3];
        for (code, axis, sign) in &self.compiled.keys {
            if held(*code) {
                a[*axis] += sign;
            }
        }
        Axes::from_array(a.map(|v: f64| v.clamp(-1.0, 1.0)))
    }
    /// The gamepad's axes: each bound stick through the deadzone (and its
    /// inversion), plus held axis buttons, summed per axis and clamped.
    pub fn gamepad_axes(&self, value: impl Fn(GamepadAxis) -> Option<f32>, held: impl Fn(GamepadButton) -> bool) -> Axes {
        let mut a = [0.0; 3];
        for (stick, axis, invert) in &self.compiled.sticks {
            let v = shape(value(*stick).unwrap_or(0.0), self.compiled.deadzone);
            a[*axis] += if *invert { -v } else { v };
        }
        for (code, axis, sign) in &self.compiled.button_axes {
            if held(*code) {
                a[*axis] += sign;
            }
        }
        Axes::from_array(a.map(|v: f64| v.clamp(-1.0, 1.0)))
    }
    /// Every key the keyboard bindings read (axes and actions).
    pub fn keys(&self) -> impl Iterator<Item = KeyCode> + '_ {
        self.compiled.keys.iter().map(|(k, ..)| *k).chain(self.compiled.key_actions.iter().map(|(k, _)| *k))
    }
    /// Keys that send a named action on press.
    pub fn key_actions(&self) -> &[(KeyCode, String)] {
        &self.compiled.key_actions
    }
    /// Buttons that send a named action on press.
    pub fn button_actions(&self) -> &[(GamepadButton, String)] {
        &self.compiled.button_actions
    }
    /// Every button the gamepad bindings read (axes and actions).
    pub fn buttons(&self) -> impl Iterator<Item = GamepadButton> + '_ {
        self.compiled.button_axes.iter().map(|(b, ..)| *b).chain(self.compiled.button_actions.iter().map(|(b, _)| *b))
    }
}

/// A stick value through the deadzone: zero inside it, the rest of the travel
/// rescaled to 0..1 with the sign kept. NaN reads as zero.
pub fn shape(raw: f32, deadzone: f64) -> f64 {
    let v = raw as f64;
    if !(v.abs() > deadzone) {
        return 0.0;
    }
    (((v.abs() - deadzone) / (1.0 - deadzone)).min(1.0)) * v.signum()
}

/// REST `drive_bindings`' answer: the bindings in effect, whether stored, and
/// the described table.
pub fn json(stored: Option<&BindingsFile>) -> Value {
    let file = stored.cloned().unwrap_or_default();
    let describe = DriveBindings::new(file.clone(), stored.is_some()).map(|b| b.describe()).unwrap_or_default();
    json!({"schema": SCHEMA, "stored": stored.is_some(), "bindings": file,
        "describe": describe.into_iter().map(|(input, does)| json!({"input": input, "does": does})).collect::<Vec<_>>()})
}

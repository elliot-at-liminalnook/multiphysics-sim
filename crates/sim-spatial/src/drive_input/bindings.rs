//! Device bindings (`sim.drive-bindings/1`) in the native viewer: the Bevy
//! glue over the one shared parser (`sim_runtime::drive_bindings`, no Bevy).
//! The file, its validation (every refusal naming its field), its defaults,
//! the deadzone shape, `supported_only` and the REST answer are the shared
//! crate's, so the browser and this viewer read a bindings file the same
//! way; this module only maps its names to Bevy's `KeyCode`, `GamepadAxis`
//! and `GamepadButton` and keeps [`DriveBindings`], the active set, as a
//! resource. The file is persisted as the `drive_bindings` group of the
//! viewer preferences (`app::settings::SettingsOwner`, the one owner of
//! preference files).
use bevy::prelude::*;
use sim_domain_control::drive::kinematics::Axes;
pub use sim_runtime::drive_bindings::{BindingsFile, ButtonBinding, DEFAULT_DEADZONE, KeyAxis, Resolved, SCHEMA, json, shape, supported_only};
use sim_runtime::drive_bindings::{BUTTONS, KEYS, STICKS};

/// The Bevy key for a binding's key name: every name in
/// `drive_bindings::KEYS` (W3C `KeyboardEvent.code` strings, which are also
/// Bevy's `KeyCode` variant names) maps; anything else is None. A KEYS name
/// without an arm here is a bug (`drive_input` tests check every name).
pub fn key_code(name: &str) -> Option<KeyCode> {
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

/// The gilrs axis Bevy reports for a stick name (`drive_bindings::STICKS`).
pub fn stick_axis(name: &str) -> Option<GamepadAxis> {
    Some(match name {
        "left_stick_x" => GamepadAxis::LeftStickX,
        "left_stick_y" => GamepadAxis::LeftStickY,
        "right_stick_x" => GamepadAxis::RightStickX,
        "right_stick_y" => GamepadAxis::RightStickY,
        "left_z" => GamepadAxis::LeftZ,
        "right_z" => GamepadAxis::RightZ,
        _ => return None,
    })
}

/// The Bevy button for a button name (`drive_bindings::BUTTONS`).
pub fn gamepad_button(name: &str) -> Option<GamepadButton> {
    Some(match name {
        "south" => GamepadButton::South,
        "east" => GamepadButton::East,
        "north" => GamepadButton::North,
        "west" => GamepadButton::West,
        "c" => GamepadButton::C,
        "z" => GamepadButton::Z,
        "left_trigger" => GamepadButton::LeftTrigger,
        "left_trigger2" => GamepadButton::LeftTrigger2,
        "right_trigger" => GamepadButton::RightTrigger,
        "right_trigger2" => GamepadButton::RightTrigger2,
        "select" => GamepadButton::Select,
        "start" => GamepadButton::Start,
        "mode" => GamepadButton::Mode,
        "left_thumb" => GamepadButton::LeftThumb,
        "right_thumb" => GamepadButton::RightThumb,
        "dpad_up" => GamepadButton::DPadUp,
        "dpad_down" => GamepadButton::DPadDown,
        "dpad_left" => GamepadButton::DPadLeft,
        "dpad_right" => GamepadButton::DPadRight,
        _ => return None,
    })
}

/// Every name the shared tables list that has no Bevy value here (empty in a
/// correct build): checked by the tests and by [`DriveBindings::new`].
pub fn unmapped_names() -> Vec<&'static str> {
    let keys = KEYS.iter().copied().filter(|k| key_code(k).is_none());
    let sticks = STICKS.iter().map(|s| s.0).filter(|s| stick_axis(s).is_none());
    let buttons = BUTTONS.iter().map(|b| b.0).filter(|b| gamepad_button(b).is_none());
    keys.chain(sticks).chain(buttons).collect()
}

/// The active, validated bindings: what the device poller reads and the
/// inspector and Build's run panel describe. Written only by
/// `drive_input::plugin::sync_bindings` from the settings owner's
/// `drive_bindings` (the defaults until the user sets them).
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct DriveBindings {
    /// The bindings in effect.
    pub file: BindingsFile,
    /// The preferences store them (false: the committed defaults).
    pub stored: bool,
    /// The file resolved by the shared parser (names with axis indices).
    pub resolved: Resolved,
    /// Every key the keyboard bindings read (axes and actions), as Bevy keys.
    keys: Vec<KeyCode>,
    /// Keys that send a named action on press.
    key_actions: Vec<(KeyCode, String)>,
    /// Every button the gamepad bindings read (axes and actions).
    buttons: Vec<GamepadButton>,
    /// Buttons that send a named action on press.
    button_actions: Vec<(GamepadButton, String)>,
}
impl Default for DriveBindings {
    fn default() -> Self {
        Self::new(BindingsFile::default(), false).expect("the committed default drive bindings validate and map to Bevy (drive_input tests)")
    }
}
impl DriveBindings {
    /// `file` validated and resolved by the shared parser, then its names
    /// mapped to Bevy values. A validated name without a Bevy value can only
    /// be a build whose tables and mapping disagree; it is refused by name.
    pub fn new(file: BindingsFile, stored: bool) -> Result<Self, String> {
        let resolved = file.resolve()?;
        let key = |name: &str| key_code(name).ok_or_else(|| format!("drive_bindings: key `{name}` has no Bevy KeyCode in this build"));
        let button = |name: &str| gamepad_button(name).ok_or_else(|| format!("drive_bindings: button `{name}` has no Bevy GamepadButton in this build"));
        for (stick, ..) in &resolved.sticks {
            stick_axis(stick).ok_or_else(|| format!("drive_bindings: stick `{stick}` has no Bevy GamepadAxis in this build"))?;
        }
        let keys = resolved.all_keys().map(key).collect::<Result<Vec<_>, _>>()?;
        let key_actions = resolved.key_actions.iter().map(|(k, a)| -> Result<(KeyCode, String), String> { Ok((key(k)?, a.clone())) }).collect::<Result<Vec<_>, String>>()?;
        let buttons = resolved.all_buttons().map(button).collect::<Result<Vec<_>, _>>()?;
        let button_actions = resolved.button_actions.iter().map(|(b, a)| -> Result<(GamepadButton, String), String> { Ok((button(b)?, a.clone())) }).collect::<Result<Vec<_>, String>>()?;
        Ok(Self { file, stored, resolved, keys, key_actions, buttons, button_actions })
    }
    /// (input label, what it does), for the inspector and REST: the shared
    /// `BindingsFile::describe` (each binding, then the stops that always hold).
    pub fn describe(&self) -> Vec<(String, String)> {
        self.file.describe()
    }
    /// The keyboard's axes through the shared `Resolved::keyboard_axes`
    /// (summed per axis and clamped to -1..1), `held` asked per Bevy key.
    pub fn keyboard_axes(&self, held: impl Fn(KeyCode) -> bool) -> Axes {
        self.resolved.keyboard_axes(|name| key_code(name).is_some_and(|k| held(k)))
    }
    /// The gamepad's axes through the shared `Resolved::gamepad_axes` (each
    /// bound stick through the deadzone and its inversion, plus held axis
    /// buttons). `value` is Bevy's `Gamepad::get` (f32, gilrs's convention,
    /// which the bindings file uses), widened to f64.
    pub fn gamepad_axes(&self, value: impl Fn(GamepadAxis) -> Option<f32>, held: impl Fn(GamepadButton) -> bool) -> Axes {
        self.resolved.gamepad_axes(|name| stick_axis(name).and_then(|a| value(a)).map(f64::from), |name| gamepad_button(name).is_some_and(|b| held(b)))
    }
    /// Every key the keyboard bindings read (axes and actions).
    pub fn keys(&self) -> impl Iterator<Item = KeyCode> + '_ {
        self.keys.iter().copied()
    }
    /// Keys that send a named action on press.
    pub fn key_actions(&self) -> &[(KeyCode, String)] {
        &self.key_actions
    }
    /// Buttons that send a named action on press.
    pub fn button_actions(&self) -> &[(GamepadButton, String)] {
        &self.button_actions
    }
    /// Every button the gamepad bindings read (axes and actions).
    pub fn buttons(&self) -> impl Iterator<Item = GamepadButton> + '_ {
        self.buttons.iter().copied()
    }
    /// A one-line key summary for a run panel, generated from the bindings:
    /// per axis its keys (`W/S forward`), an axis `supported` lacks marked
    /// "(not in profile)", keys in `owned` (the mode's own keys) marked
    /// "(mode key, ignored)", then the action keys and Escape's stop.
    pub fn key_summary(&self, supported: [bool; 3], owned: &[KeyCode]) -> String {
        use sim_domain_control::drive::kinematics::AXIS_NAMES;
        use sim_runtime::drive_bindings::key_label;
        let label = |name: &str| {
            let l = key_label(name);
            if key_code(name).is_some_and(|k| owned.contains(&k)) { format!("{l} (mode key, ignored)") } else { l }
        };
        let mut parts = Vec::new();
        for (i, axis) in AXIS_NAMES.iter().enumerate() {
            // Positive direction first: W/S, A/D, Q/E with the defaults.
            let mut keys: Vec<(&str, f64)> = self.resolved.keys.iter().filter(|(_, a, _)| *a == i).map(|(k, _, s)| (k.as_str(), *s)).collect();
            if keys.is_empty() {
                continue;
            }
            keys.sort_by(|a, b| b.1.total_cmp(&a.1));
            let names: Vec<String> = keys.iter().map(|(k, _)| label(k)).collect();
            let what: &str = if *axis == "yaw" { "turn" } else { *axis };
            let unsupported = if supported[i] { "" } else { " (not in profile)" };
            parts.push(format!("{} {what}{unsupported}", names.join("/")));
        }
        for (key, action) in &self.resolved.key_actions {
            parts.push(format!("{} {action}", label(key)));
        }
        parts.push("Esc stop".into());
        parts.join(" · ")
    }
}

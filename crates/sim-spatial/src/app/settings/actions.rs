use super::*;
use crate::app::actions::{self, Act, Action, InFlight, Replies, Spec};
use bevy::ecs::message::Messages;
use sim_api::Outcome;
#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum SettingsAction {
    SettingsStatus,
    SettingsRetry,
    /// Read the drive bindings, set them (`bindings`, a whole
    /// `sim.drive-bindings/1` value) or reset them to the defaults (`reset`).
    DriveBindings {
        #[serde(default)]
        bindings: Option<Value>,
        #[serde(default)]
        reset: bool,
    },
}
impl Action for SettingsAction {
    fn commands() -> Vec<Spec> {
        vec![
            actions::spec(
                "settings_status",
                actions::ALL,
                serde_json::json!({}),
                "Preference readiness, dirty revisions and publication diagnostics",
            ),
            actions::spec(
                "settings_retry",
                actions::ALL,
                serde_json::json!({}),
                "Retry failed preference load/save without modifying data",
            ),
            actions::spec(
                "drive_bindings",
                actions::ALL,
                serde_json::json!({}),
                format!(
                    "Device bindings for driving a robot that has a drive profile (keyboard and gamepad: Robot mode's controlled run, robot_drive; one device poller for both): which keys, gamepad sticks and buttons make the normalized axes forward, lateral and yaw (-1..1: + ahead, + left, + turn left/CCW) and which send the robot's sim.drive/1 profile actions by name. Shared by every robot and kept in the viewer preferences (group drive_bindings), saved through the one preferences owner (settings_status). No arguments: read them. {{\"bindings\": {{…}}}}: set them, a whole {schema} value {{schema, keyboard {{axes: [{{key (KeyCode name: KeyA..KeyZ, Digit0..Digit9, Arrow*), axis, direction 1 | -1}}], actions: [{{key, action}}]}}, gamepad {{deadzone 0..1, axes: [{{stick (left_stick_x | left_stick_y | right_stick_x | right_stick_y | left_z | right_z), axis, invert}}], buttons: [{{button (south | east | north | west | dpad_up | …), action, or axis with direction}}]}}}}; unknown fields, another schema, an unknown key, stick, button or axis, a key or button bound twice, robot mode's own keys (G, C, J, F, H; Build mode's own keys, the arrows, N, G, U and R, are accepted but not read for driving in Build mode), the kit's activation and focus keys (Space, Enter, Tab, Escape) and a deadzone outside 0..1 are refused naming the field (drive_bindings.keyboard.axes[2].key). {{\"reset\": true}}: back to the defaults (W/S forward, A/D yaw, Q/E lateral, X stop, B halt; left stick Y forward, right stick X yaw, left stick X lateral, South stop, East halt; deadzone {deadzone}), which are then not stored. Answers schema, stored, bindings (in effect), describe [{{input, does}}] and w3c_standard_gamepad (the stick and button table the browser reads them through). Escape stops the drive; window focus loss stops what the keys or gamepad were driving.",
                    schema = crate::drive_input::bindings::SCHEMA,
                    deadzone = crate::drive_input::bindings::DEFAULT_DEADZONE,
                ),
            ),
        ]
    }
}
pub(super) fn apply(
    mut messages: ResMut<Messages<Act<SettingsAction>>>,
    mut flight: ResMut<InFlight<SettingsAction>>,
    mut replies: ResMut<Replies>,
    mut owner: ResMut<SettingsOwner>,
) {
    actions::apply(&mut messages, &mut flight, &mut replies, |action, _| match action {
        SettingsAction::SettingsStatus => Outcome::Done(Ok(owner.status())),
        SettingsAction::SettingsRetry => {
            owner.retry();
            Outcome::Done(Ok(owner.status()))
        }
        SettingsAction::DriveBindings { bindings, reset } => Outcome::Done(drive_bindings(&mut *owner, bindings.as_ref(), *reset)),
    });
}

/// REST `drive_bindings`: set (validated, every refusal naming its field),
/// reset or read; the answer is the bindings in effect.
fn drive_bindings(owner: &mut SettingsOwner, bindings: Option<&Value>, reset: bool) -> Result<Value, String> {
    use crate::drive_input::bindings as drive;
    match (bindings, reset) {
        (Some(_), true) => return Err("drive_bindings: give bindings (a whole sim.drive-bindings/1 value) or reset, not both".into()),
        (Some(value), false) => owner.set_drive_bindings(Some(drive::BindingsFile::from_value(value)?))?,
        (None, true) => owner.set_drive_bindings(None)?,
        (None, false) => {}
    }
    let mut answer = drive::json(owner.drive_bindings.as_ref());
    answer["settings"] = owner.status();
    Ok(answer)
}

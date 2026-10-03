//! The drive input system: keyboard and gamepad through the device
//! bindings into `RobotAction::Drive`, and [`DriveInput`], what it last did.
use super::bindings::DriveBindings;
use crate::app::actions::Act;
use crate::robot::{DriveRequest, RobotAction, RobotView};
use crate::ui_kit::text::Typing;
use bevy::prelude::*;
use bevy::window::{RequestRedraw, WindowFocused};
use serde_json::{Value, json};
use sim_domain_control::drive::kinematics::{AXIS_NAMES, Axes};

/// What the drive input is doing, for the inspector (and `robot_state`
/// through it). Written by [`devices`] (axes, source, ignored axes, last
/// action; `last_error` cleared when the run's generation or file changes:
/// Reset, a replay, a reload, another file) and by robot mode's apply system
/// (`last_error`: the last drive request's refusal from any origin, cleared
/// by the next accepted one).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct DriveInput {
    /// Device input is live: the loaded run is controlled (it has a drive profile).
    pub active: bool,
    /// The combined normalized axes sent this frame (each -1..1; an axis the
    /// profile does not support is always 0 here).
    pub axes: Axes,
    /// Which device made nonzero axes: "keyboard", "gamepad" or "keyboard+gamepad"; None when idle.
    pub source: Option<&'static str>,
    /// Axes a device input asks for this frame that the robot's profile does
    /// not support (`kinematics::AXIS_NAMES`): zeroed before sending
    /// ([`supported_only`]), so Q/E or a slightly diagonal stick on a
    /// differential robot never loses the supported axes.
    pub ignored: Vec<&'static str>,
    /// The last named action or stop sent from a device (with its reason for a stop).
    pub last_action: Option<String>,
    /// The last refused drive request (any origin), verbatim.
    pub last_error: Option<String>,
}
impl DriveInput {
    pub fn json(&self) -> Value {
        json!({"active": self.active, "axes": {"forward": self.axes.forward, "lateral": self.axes.lateral, "yaw": self.axes.yaw},
            "source": self.source, "ignored_axes": self.ignored, "last_action": self.last_action, "last_error": self.last_error})
    }
}

/// `axes` with every axis the profile does not support (`supported`, the
/// resolved limits' flags in `AXIS_NAMES` order) set to zero, and the names
/// of the unsupported axes that were nonzero. Device input only: REST
/// `robot_drive` stays strict (`kinematics::scale` refuses a nonzero
/// unsupported axis by name).
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

/// Keys the Leg calibration panel owns while it is shown (Q/A hold-to-move,
/// Z STOP; `hardware::actions::input::keys`): never read for driving then.
const PANEL_KEYS: [KeyCode; 3] = [KeyCode::KeyQ, KeyCode::KeyA, KeyCode::KeyZ];
const MODIFIERS: [KeyCode; 6] = [KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::AltLeft, KeyCode::AltRight];

/// The input system's memory across frames.
#[derive(Default)]
pub(crate) struct Latch {
    /// Nonzero axes were sent last frame (a zero request is owed on release).
    sending: bool,
    /// ... and the keyboard contributed to them.
    keyboard_sending: bool,
    was_typing: bool,
    /// Keys held at a stop (or a panel key held while the panel was shown):
    /// ignored until released, so a held key never drives again by itself.
    blocked_keys: Vec<KeyCode>,
    /// The gamepad was held at a stop: ignored until, with a window of this
    /// app focused, every bound stick is inside its deadzone and every bound
    /// button is released.
    pad_blocked: bool,
    /// The opened file and run generation `DriveInput::last_error` belongs
    /// to: a Reset, replay or reload (a new generation) or another file clears it.
    run: Option<(std::path::PathBuf, Option<u64>)>,
}

fn add(a: Axes, b: Axes) -> Axes {
    let (a, b) = (a.to_array(), b.to_array());
    Axes::from_array([0, 1, 2].map(|i| (a[i] + b[i]).clamp(-1.0, 1.0)))
}
fn axes_request(a: Axes) -> RobotAction {
    RobotAction::Drive { request: DriveRequest::Axes { forward: a.forward, lateral: a.lateral, yaw: a.yaw } }
}

/// Input (`InputSet::Window`, robot mode's chain): the bound keys
/// (`ButtonInput<KeyCode>`, physical keys, never `KeyboardInput` text) and
/// every connected gamepad (the `Gamepad` components gilrs keeps, read for
/// driving only while a window of this app has focus) through
/// [`DriveBindings`] into `RobotAction::Drive`, the same action as
/// `system_ui` drive:* and REST `robot_drive`.
///
/// Coexistence: this acts only while the loaded run is controlled
/// (`RunController::controlled()`, a robot with a controller binding and a
/// `sim.drive/1` profile); the preset motion keys (`actions::motion_keys`)
/// act only while `motion_keys_active()` (a built preset with a motion
/// config). A run is one or the other, so the two never both read W/A/S/D.
///
/// - Axes the profile does not support are zeroed before sending
///   ([`supported_only`]) and listed in `DriveInput::ignored`: W+Q on a
///   differential robot drives forward instead of refusing the whole request.
/// - While any axis is nonzero, `Drive { Axes }` is sent every frame (the
///   run thread's deadman needs fresh requests) and a redraw is requested so
///   frames keep coming; when every input is back to neutral, one zero
///   request is sent. These repeated requests are quiet (`Act::quiet`): a
///   refusal reaches `DriveInput::last_error` (the apply system records it)
///   and the Drive block, not the header's run message, which they would
///   otherwise overwrite every frame.
/// - Stop (`DriveRequest::Stop`): Escape, always (not while a text field has
///   the keyboard, where Escape is the field's Cancel; the Leg calibration
///   panel's Escape STOP runs as well), shown in the header only when the
///   devices were driving (otherwise Escape is likely meant for something
///   else, and it is sent quietly); the window losing focus while the
///   devices drive (Bevy also releases every key then, and the gamepad is
///   not read without focus); a kit text field taking the keyboard while
///   keys drive, once. After any stop or named action (every profile action
///   today is stop or halt) the held inputs are disarmed until released.
/// - A bound action key or button press sends `Drive { Action { name } }`,
///   preceded by a zero request while driving (so an action name the
///   profile refuses still stops the robot).
/// - Cmd/Ctrl/Alt chords are not driving keys; while the Leg calibration
///   panel is shown its Q/A/Z are its own (a key held when it opened stays
///   ignored until released, as `motion_keys` does for A).
#[allow(clippy::too_many_arguments)]
pub(in crate::robot) fn devices(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    windows: Query<&Window>,
    mut focus: MessageReader<WindowFocused>,
    view: Option<Res<RobotView>>,
    hardware: Option<Res<crate::robot::hardware::Hardware>>,
    bindings: Res<DriveBindings>,
    text: Typing,
    mut input: ResMut<DriveInput>,
    mut latch: Local<Latch>,
    mut out: MessageWriter<Act<RobotAction>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let lost = focus.read().filter(|e| !e.focused).count() > 0;
    let typing = text.get();
    let typing_started = !std::mem::replace(&mut latch.was_typing, typing) && typing;
    let run = view.as_ref().and_then(|v| v.run.as_ref());
    // A refusal belongs to its run: a new generation (Reset, replay, reload) or file clears it.
    let run_key = view.as_ref().map(|v| (v.path.clone(), run.map(|r| r.generation())));
    let mut last_error = input.last_error.clone();
    if latch.run != run_key {
        latch.run = run_key;
        last_error = None;
    }
    let Some(supported) = run.and_then(|r| r.controlled()).map(|c| c.controlled.resolved.limits.supported) else {
        // Not controlled: nothing is read; a REST drive refusal stays visible until the run changes.
        let run = latch.run.take();
        *latch = Latch { was_typing: typing, run, ..default() };
        input.set_if_neq(DriveInput { last_error, ..default() });
        return;
    };
    let chord = keys.any_pressed(MODIFIERS);
    let panel = hardware.is_some_and(|h| h.open);
    latch.blocked_keys.retain(|k| keys.pressed(*k));
    if panel {
        for k in PANEL_KEYS {
            if keys.pressed(k) && !latch.blocked_keys.contains(&k) {
                latch.blocked_keys.push(k);
            }
        }
    }
    // (reason, send, shown): focus loss stops only what the devices were
    // driving (a REST client's requests are its own, under the deadman);
    // Escape always sends, in the header only when the devices were driving.
    let stop = if lost {
        Some(("the window lost focus", latch.sending, true))
    } else if !typing && !chord && keys.just_pressed(KeyCode::Escape) {
        Some(("Escape", true, latch.sending))
    } else if typing_started && latch.keyboard_sending {
        Some(("a text field took the keyboard", true, true))
    } else {
        None
    };
    let disarm = |latch: &mut Latch| {
        let held: Vec<KeyCode> = bindings.keys().filter(|k| keys.pressed(*k) && !latch.blocked_keys.contains(k)).collect();
        latch.blocked_keys.extend(held);
        latch.pad_blocked = true;
        latch.sending = false;
        latch.keyboard_sending = false;
    };
    if let Some((reason, send, shown)) = stop {
        let last_action = if send {
            let action = RobotAction::Drive { request: DriveRequest::Stop };
            out.write(if shown { Act::ui(action) } else { Act::quiet(action) });
            Some(format!("stop ({reason})"))
        } else {
            input.last_action.clone()
        };
        disarm(&mut *latch);
        input.set_if_neq(DriveInput { active: true, axes: Axes::ZERO, source: None, ignored: Vec::new(), last_action, last_error });
        return;
    }
    let readable = |k: &KeyCode, latch: &Latch| !typing && !chord && !latch.blocked_keys.contains(k);
    let keyboard = bindings.keyboard_axes(|k| keys.pressed(k) && readable(&k, &*latch));
    let mut actions: Vec<String> = bindings.key_actions().iter().filter(|(k, _)| keys.just_pressed(*k) && readable(k, &*latch)).map(|(_, name)| name.clone()).collect();
    let focused = windows.iter().any(|w| w.focused);
    // The sticks and buttons as gilrs holds them, focused or not: the
    // unblock test below needs them even while they are not read for driving.
    let mut raw_pad = Axes::ZERO;
    for gamepad in &pads {
        raw_pad = add(raw_pad, bindings.gamepad_axes(|axis| gamepad.get(axis), |b| gamepad.pressed(b)));
    }
    let mut pad = if focused { raw_pad } else { Axes::ZERO };
    if focused {
        for gamepad in &pads {
            actions.extend(bindings.button_actions().iter().filter(|(b, _)| gamepad.just_pressed(*b)).map(|(_, name)| name.clone()));
        }
    }
    if latch.pad_blocked {
        // Released only while focused (a pad is not read without focus, so
        // its neutral state then proves nothing) and fully at rest.
        let held = pads.iter().any(|g| bindings.buttons().any(|b| g.pressed(b)));
        if focused && raw_pad == Axes::ZERO && !held {
            latch.pad_blocked = false;
        }
        pad = Axes::ZERO;
    }
    if !actions.is_empty() {
        if latch.sending {
            out.write(Act::quiet(axes_request(Axes::ZERO)));
        }
        for name in &actions {
            out.write(Act::ui(RobotAction::Drive { request: DriveRequest::Action { name: name.clone() } }));
        }
        disarm(&mut *latch);
        input.set_if_neq(DriveInput { active: true, axes: Axes::ZERO, source: None, ignored: Vec::new(), last_action: Some(actions.join(", ")), last_error });
        return;
    }
    // Axes the profile lacks are dropped per device (so they never cancel a supported one) and named.
    let (keyboard, keyboard_ignored) = supported_only(keyboard, supported);
    let (pad, pad_ignored) = supported_only(pad, supported);
    let ignored: Vec<&'static str> = AXIS_NAMES.iter().copied().filter(|n| keyboard_ignored.contains(n) || pad_ignored.contains(n)).collect();
    let axes = add(keyboard, pad);
    let source = match (keyboard != Axes::ZERO, pad != Axes::ZERO) {
        (true, false) => Some("keyboard"),
        (false, true) => Some("gamepad"),
        (true, true) => Some("keyboard+gamepad"),
        (false, false) => None,
    };
    if axes != Axes::ZERO {
        out.write(Act::quiet(axes_request(axes)));
        redraw.write(RequestRedraw);
        latch.sending = true;
        latch.keyboard_sending = keyboard != Axes::ZERO;
    } else if latch.sending {
        out.write(Act::quiet(axes_request(Axes::ZERO)));
        latch.sending = false;
        latch.keyboard_sending = false;
    }
    let last_action = input.last_action.clone();
    input.set_if_neq(DriveInput { active: true, axes, source, ignored, last_action, last_error });
}

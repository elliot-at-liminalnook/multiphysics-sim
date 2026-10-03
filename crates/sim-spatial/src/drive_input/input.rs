//! The one drive device poller: keyboard and gamepad through the device
//! bindings into [`DriveDevice`] for the mode whose [`DriveTarget`] is live,
//! and [`DriveInput`], what it last did.
use super::bindings::DriveBindings;
use super::{Disarm, DriveDevice, DriveTarget};
use crate::app::ViewerMode;
use crate::app::actions::Act;
use crate::ui_kit::text::Typing;
use bevy::prelude::*;
use bevy::window::{RequestRedraw, WindowFocused};
use serde_json::{Value, json};
use sim_domain_control::drive::kinematics::{AXIS_NAMES, Axes};
use sim_runtime::drive_bindings::supported_only;
use sim_runtime::drive_host::DriveRequest;

/// What the drive input is doing, for Robot mode's inspector (and
/// `robot_state` through it) and Build mode's run panel.
///
/// Writers, one per field and mode:
/// - [`devices`] (every mode): `active`, `axes`, `source`, `ignored`,
///   `last_action`; it carries `last_error` unchanged.
/// - `last_error`: in Robot mode robot mode's apply (`robot::actions::apply`:
///   the last drive request's refusal from any origin, cleared by the next
///   accepted one) and robot mode's target writer (`robot::controls::drive_target`:
///   cleared when the run's file or generation changes: Reset, a replay, a
///   reload, another file); in Build mode the builder's device drain
///   (`builder::actions::drive_devices`: it mirrors
///   `system_state.live_run.drive.last_refusal`, `Builder::drive_request`'s record
///   of the last refusal from any origin).
/// - Every field: reset by [`super::leave_mode`] when a mode that drives exits.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct DriveInput {
    /// Device input is live: the active mode's target is live (a controlled run).
    pub active: bool,
    /// The combined normalized axes sent this frame (each -1..1; an axis the
    /// profile does not support is always 0 here).
    pub axes: Axes,
    /// Which device made nonzero axes: "keyboard", "gamepad" or "keyboard+gamepad"; None when idle.
    pub source: Option<&'static str>,
    /// Axes a device input asks for this frame that the robot's profile does
    /// not support (`kinematics::AXIS_NAMES`): zeroed before sending
    /// (`drive_bindings::supported_only`), so Q/E or a slightly diagonal
    /// stick on a differential robot never loses the supported axes.
    pub ignored: Vec<&'static str>,
    /// The last named action or stop sent from a device (with its reason for a stop).
    pub last_action: Option<String>,
    /// The last refused drive request (any origin), verbatim.
    pub last_error: Option<String>,
}
/// Why `drive_input.ignored_axes` lists an axis.
pub const IGNORED_RULE: &str = "device axes the robot's drive profile does not support are zeroed before sending and listed here; REST robot_drive and system_drive refuse them by name";
impl DriveInput {
    /// `drive_input` as both modes' state answers carry it ([`super::insert_state`], the one serializer).
    pub fn json(&self) -> Value {
        json!({"active": self.active, "axes": {"forward": self.axes.forward, "lateral": self.axes.lateral, "yaw": self.axes.yaw}, "axes_unit": "normalized, -1..1",
            "source": self.source, "ignored_axes": self.ignored, "ignored_rule": IGNORED_RULE,
            "last_action": self.last_action, "last_error": self.last_error, "disarm_rule": super::DISARM_RULE})
    }
}

const MODIFIERS: [KeyCode; 6] = [KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::AltLeft, KeyCode::AltRight];

/// The poller's memory across frames.
#[derive(Default)]
pub(crate) struct Latch {
    /// Nonzero axes were sent last frame (a zero request is owed on release).
    sending: bool,
    /// ... and the keyboard contributed to them.
    keyboard_sending: bool,
    was_typing: bool,
    /// Keys held at a stop, when the target became live, or a mode's own
    /// key ([`DriveTarget::owned_keys`]): ignored until released, so a held
    /// key never drives again by itself.
    blocked_keys: Vec<KeyCode>,
    /// The gamepad was held at a stop: ignored until, with a window of this
    /// app focused, every bound stick is inside its deadzone and every bound
    /// button is released.
    pad_blocked: bool,
    /// The live target last frame (mode, run): a new one disarms held inputs.
    target: Option<(ViewerMode, String)>,
}

fn add(a: Axes, b: Axes) -> Axes {
    let (a, b) = (a.to_array(), b.to_array());
    Axes::from_array([0, 1, 2].map(|i| (a[i] + b[i]).clamp(-1.0, 1.0)))
}
fn axes_request(a: Axes) -> DriveRequest {
    DriveRequest::Axes { forward: a.forward, lateral: a.lateral, yaw: a.yaw }
}

/// The target this poller acts for in `mode`: the live target only while
/// its mode is the current one (a target a mode left behind, or one written
/// for Build while the window shows Lessons, is ignored). None: nothing is
/// read.
pub fn live_target(target: &DriveTarget, mode: Option<ViewerMode>) -> Option<&super::LiveTarget> {
    target.live.as_ref().filter(|t| Some(t.mode) == mode)
}

/// Input (`InputSet::Window`, every mode; the one system that reads drive
/// device input): the bound keys (`ButtonInput<KeyCode>`, physical keys,
/// never `KeyboardInput` text) and every connected gamepad (the `Gamepad`
/// components gilrs keeps, read for driving only while a window of this app
/// has focus) through [`DriveBindings`] into `Act<DriveDevice>` for the
/// mode whose [`DriveTarget`] is live, the mode stamped on each message.
/// Robot mode forwards them as `RobotAction::Drive` (the same action as
/// `system_ui` drive:* and REST `robot_drive`), Build mode applies them with
/// `Builder::drive_request` (the same apply as the run panel's drive buttons and
/// REST `system_drive`).
///
/// Coexistence: a target is live only for a controlled run (Robot: a robot
/// with a controller binding and a `sim.drive/1` profile; Build: a robot
/// system's run whose profile has loaded). Robot mode's preset motion keys
/// (`actions::motion_keys`) act only for a built preset with a motion
/// config. A run is one or the other, so the two never both read W/A/S/D.
///
/// - Nothing is read while no target is live for the current mode; a target
///   that becomes live, or changes (mode or run), disarms every held input
///   first, so a key held across a load or a mode switch never starts
///   driving by itself. A target that goes away or changes while the
///   devices drive gets one quiet Stop, stamped with its own mode, when that
///   mode is still current (Build's Reset or Stop, a draft or drag opened,
///   another Robot file); a left mode stops its own run on exit.
/// - Axes the profile does not support are zeroed before sending
///   (`supported_only`, per device) and listed in `DriveInput::ignored`:
///   W+Q on a differential robot drives forward instead of refusing the
///   whole request.
/// - While any axis is nonzero, `Axes` is sent every frame (the run
///   thread's deadman needs fresh requests) and a redraw is requested so
///   frames keep coming; when every input is back to neutral, one zero
///   request is sent. These repeated requests are quiet (`Act::quiet`): a
///   refusal reaches `DriveInput::last_error` and the mode's drive block,
///   not the mode's status line, which they would otherwise overwrite every frame.
/// - Stop (`DriveRequest::Stop`): Escape, always (not while a text field has
///   the keyboard, where Escape is the field's Cancel, nor in a
///   Cmd/Ctrl/Alt chord; the mode's own Escape still runs: Robot's Leg
///   calibration STOP, Build's back-to-Select), shown in the status line
///   only when the devices were driving (otherwise Escape is likely meant
///   for something else, and it is sent quietly); the window losing focus
///   while the devices drive (Bevy also releases every key then, and the
///   gamepad is not read without focus); a kit text field taking the
///   keyboard while keys drive, once. After any stop or named action (every
///   profile action today is stop or halt) the held inputs are disarmed
///   until released.
/// - [`Disarm`] ([`super::DISARM_RULE`]): a stop, halt, named action, Pause
///   or Reset the current mode's one apply accepted from any origin (the
///   Drive block's or run panel's Stop, its Pause, `system_ui`, REST) in the
///   previous frame's Actions. The messages are drained every frame, before
///   any early return, so none waits for a later target. One for the live
///   target's mode disarms exactly as a device stop does: if the devices
///   were driving and the message is a drive stop or action
///   (`Disarm::stop`), one quiet Stop first (the held axes this poller sent
///   in the click's frame were applied after the click, so without it the
///   last request would be those axes; a Pause or Reset sends none, so the
///   paused request's on-loss rule is not replaced by a fresh zero), then every held key, the gamepad and the
///   sending latch are disarmed until released. `last_action` names it
///   ("stop (<reason>)" when a Stop was sent, "disarmed (<reason>)" when an
///   input was held but not sending, unchanged when nothing was held, so a
///   device's own stop, echoed back by Robot's apply, keeps its reason). A
///   message for another mode is ignored. Escape, focus loss and a text
///   field taking the keyboard in the same frame take precedence (they
///   disarm too).
/// - A bound action key or button press sends `Action { name }`, preceded by
///   a zero request while driving (so an action name the profile refuses
///   still stops the robot).
/// - Cmd/Ctrl/Alt chords are not driving keys; the mode's own keys
///   ([`DriveTarget::owned_keys`]: Q/A/Z while Robot's Leg calibration
///   panel is shown, Build's editing keys) are never read for driving (a
///   key held when it became owned stays ignored until released).
///
/// Ordering: this runs in `InputSet::Window` with no edge to another
/// feature's systems. The kit focuses a field on a pointer press before
/// Input (`ui_kit::activation`); a feature system in `InputSet::Window`
/// that focuses its own field on a press this frame may run after this
/// one, so such a focus counts as typing from the next frame: at most one
/// more quiet request from keys held then, before the stop-on-typing fires.
#[allow(clippy::too_many_arguments)]
pub(crate) fn devices(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    windows: Query<&Window>,
    mut focus: MessageReader<WindowFocused>,
    mut disarms: MessageReader<Disarm>,
    mode: Option<Res<State<ViewerMode>>>,
    target: Res<DriveTarget>,
    bindings: Res<DriveBindings>,
    text: Typing,
    mut input: ResMut<DriveInput>,
    mut latch: Local<Latch>,
    mut out: MessageWriter<Act<DriveDevice>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let lost = focus.read().filter(|e| !e.focused).count() > 0;
    // Drained every frame (before any early return): a disarm is for this frame's target only.
    let disarmed: Vec<Disarm> = disarms.read().cloned().collect();
    let typing = text.get();
    let typing_started = !std::mem::replace(&mut latch.was_typing, typing) && typing;
    let last_error = input.last_error.clone();
    let current = mode.map(|m| *m.get());
    // The zero a vanished or replaced target is owed: one Stop, stamped with
    // the previous target's mode, only while that mode is still current (a
    // Build Reset, the run stopped, a draft or drag opened, another file
    // loaded in Robot mode). After a mode switch the left mode's exit has
    // stopped its own run (`builder::actions::leave_drive`; Robot's run is
    // dropped), so nothing is written for a mode that is gone.
    let owed_stop = |latch: &Latch, out: &mut MessageWriter<Act<DriveDevice>>| -> bool {
        match &latch.target {
            Some((old, _)) if latch.sending && Some(*old) == current => {
                out.write(Act::quiet(DriveDevice { mode: *old, request: DriveRequest::Stop }));
                true
            }
            _ => false,
        }
    };
    let Some(live) = live_target(&target, current) else {
        // No target in this mode: nothing is read; a refusal stays visible
        // until its mode's writer clears it.
        let last_action = if owed_stop(&latch, &mut out) { Some("stop (the drive target went away)".to_string()) } else { None };
        *latch = Latch { was_typing: typing, ..default() };
        input.set_if_neq(DriveInput { last_error, last_action, ..default() });
        return;
    };
    let (mode, supported) = (live.mode, live.supported);
    let send = |out: &mut MessageWriter<Act<DriveDevice>>, request: DriveRequest, shown: bool| {
        let device = DriveDevice { mode, request };
        out.write(if shown { Act::ui(device) } else { Act::quiet(device) });
    };
    let chord = keys.any_pressed(MODIFIERS);
    latch.blocked_keys.retain(|k| keys.pressed(*k));
    let disarm = |latch: &mut Latch| {
        let held: Vec<KeyCode> = bindings.keys().filter(|k| keys.pressed(*k) && !latch.blocked_keys.contains(k)).collect();
        latch.blocked_keys.extend(held);
        latch.pad_blocked = true;
        latch.sending = false;
        latch.keyboard_sending = false;
    };
    // A target that became live or changed: what is held now was meant for
    // something else (the previous run, another mode).
    let identity = Some((live.mode, live.run.clone()));
    if latch.target != identity {
        if owed_stop(&latch, &mut out) {
            input.last_action = Some("stop (the drive target changed)".to_string());
        }
        latch.target = identity;
        disarm(&mut *latch);
    }
    for k in &target.owned_keys {
        if keys.pressed(*k) && !latch.blocked_keys.contains(k) {
            latch.blocked_keys.push(*k);
        }
    }
    // (reason, send, shown): focus loss stops only what the devices were
    // driving (a REST client's requests are its own, under the deadman);
    // Escape always sends, shown only when the devices were driving.
    let stop = if lost {
        Some(("the window lost focus", latch.sending, true))
    } else if !typing && !chord && keys.just_pressed(KeyCode::Escape) {
        Some(("Escape", true, latch.sending))
    } else if typing_started && latch.keyboard_sending {
        Some(("a text field took the keyboard", true, true))
    } else {
        None
    };
    if let Some((reason, sent, shown)) = stop {
        let last_action = if sent {
            send(&mut out, DriveRequest::Stop, shown);
            Some(format!("stop ({reason})"))
        } else {
            input.last_action.clone()
        };
        disarm(&mut *latch);
        input.set_if_neq(DriveInput { active: true, axes: Axes::ZERO, source: None, ignored: Vec::new(), last_action, last_error });
        return;
    }
    // A stop, halt, action, Pause or Reset the mode applied last frame (DISARM_RULE); the newest
    // reason is shown, and a Stop is owed if any of them was a drive stop or action (`Disarm::stop`).
    if let Some(applied) = disarmed.iter().rev().find(|d| d.mode == mode) {
        let owed_stop = disarmed.iter().any(|d| d.mode == mode && d.stop);
        let held_keys = bindings.keys().any(|k| keys.pressed(k) && !latch.blocked_keys.contains(&k));
        let held_pad = !latch.pad_blocked && pads.iter().any(|g| bindings.buttons().any(|b| g.pressed(b)) || bindings.gamepad_axes(|axis| g.get(axis), |b| g.pressed(b)) != Axes::ZERO);
        let last_action = if latch.sending && owed_stop {
            send(&mut out, DriveRequest::Stop, false);
            Some(format!("stop ({})", applied.reason))
        } else if held_keys || held_pad {
            Some(format!("disarmed ({})", applied.reason))
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
            send(&mut out, axes_request(Axes::ZERO), false);
        }
        for name in &actions {
            send(&mut out, DriveRequest::Action { name: name.clone() }, true);
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
        send(&mut out, axes_request(axes), false);
        redraw.write(RequestRedraw);
        latch.sending = true;
        latch.keyboard_sending = keyboard != Axes::ZERO;
    } else if latch.sending {
        send(&mut out, axes_request(Axes::ZERO), false);
        latch.sending = false;
        latch.keyboard_sending = false;
    }
    let last_action = input.last_action.clone();
    input.set_if_neq(DriveInput { active: true, axes, source, ignored, last_action, last_error });
}

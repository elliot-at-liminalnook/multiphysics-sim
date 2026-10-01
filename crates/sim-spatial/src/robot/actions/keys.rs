//! Robot mode's input systems: the buttons, keys and 3D-view clicks that
//! write [`RobotAction`] (applied by [`super::apply`]).
use super::*;

/// Input: a pressed button's action (tabs, links, run, speed, reload,
/// overlays, graphs, jog, motion, recording, replay, recorded and gait).
pub(in crate::robot) fn buttons(clicks: Query<(&Interaction, &RobotAction), Changed<Interaction>>, mut out: MessageWriter<Act<RobotAction>>) {
    for (interaction, action) in &clicks {
        if *interaction == Interaction::Pressed {
            out.write(Act::ui(action.clone()));
        }
    }
}

/// Input: physical W/A/S/D (press/release) and X (Stop) while a built preset
/// has a motion config: the same `RobotAction::Motion` as the buttons,
/// `system_ui` motion:* and REST `robot_input`. Robot mode's camera is
/// mouse-only, so these keys take no camera action. Bevy releases every key
/// when the window loses keyboard focus, which requests zero, as the browser's blur.
///
/// While the Leg calibration panel is shown, A is its hold-to-move key
/// (lower), as the page's capture-phase key handler takes it from WASD; W, S
/// and D still steer. The panel opening sends the held keys again without A
/// (when the physical keys drive), so an A held for WASD when it opened does
/// not keep the robot strafing: its release is the panel's and is never seen
/// here. Closing the panel sends nothing: a still-held A drives again only
/// on a fresh press (a jog's A never becomes a strafe).
pub(in crate::robot) fn motion_keys(keys: Res<ButtonInput<KeyCode>>, view: Res<RobotView>, hardware: Option<Res<crate::robot::hardware::Hardware>>, mut was_open: Local<bool>, mut out: MessageWriter<Act<RobotAction>>) {
    const ALL: [(KeyCode, char); 4] = [(KeyCode::KeyW, 'w'), (KeyCode::KeyA, 'a'), (KeyCode::KeyS, 's'), (KeyCode::KeyD, 'd')];
    let panel = hardware.is_some_and(|h| h.open);
    // Not `panel && !replace(..)`: the replace must run when the panel closes too.
    let opened = !std::mem::replace(&mut *was_open, panel) && panel;
    let map: Vec<(KeyCode, char)> = ALL.into_iter().filter(|(_, k)| !(panel && *k == 'a')).collect();
    let Some(run) = view.run.as_ref().filter(|r| r.motion_keys_active()) else { return };
    let request = if keys.just_pressed(KeyCode::KeyX) {
        Some(MotionRequest::Stop)
    } else if (opened && run.keys_physical()) || map.iter().any(|(c, _)| keys.just_pressed(*c) || keys.just_released(*c)) {
        let held: Vec<char> = map.iter().filter(|(c, _)| keys.pressed(*c)).map(|(_, k)| *k).collect();
        // A release with only a latched (system_ui/REST) key active leaves that key's request alone.
        let pressed = map.iter().any(|(c, _)| keys.just_pressed(*c));
        (pressed || run.keys_physical()).then_some(MotionRequest::HeldKeys(held))
    } else {
        None
    };
    if let Some(request) = request {
        out.write(Act::ui(RobotAction::Motion { request }));
    }
}

/// Cmd/Ctrl/Alt chords (Cmd+R, Cmd+C, Cmd+=, …) are not robot mode's keys.
fn chord(keys: &ButtonInput<KeyCode>) -> bool {
    keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::AltLeft, KeyCode::AltRight])
}

/// Input, a planar (v2) file only: the planar viewer's keys as robot actions —
/// Space Run/Pause, R Reset (rebuild from the loaded model), ←/→ select a joint
/// (`SelectJoint`), ↑/↓ move its target while held (the same `RobotAction::Jog`
/// as the jog buttons; ±0.01 rad, Shift ±0.05 rad) and S stress (refused by
/// name, as key H). C (contacts) and =/− (speed) are robot mode's own keys.
pub(in crate::robot) fn planar_keys(keys: Res<ButtonInput<KeyCode>>, view: Res<RobotView>, mut out: MessageWriter<Act<RobotAction>>, mut redraw: MessageWriter<bevy::window::RequestRedraw>) {
    let Some(p) = view.planar.as_ref() else { return };
    if chord(&keys) {
        return;
    }
    if keys.just_pressed(KeyCode::Space) {
        let action = if p.run.check(RunAction::Pause).is_ok() { RunAction::Pause } else { RunAction::Start };
        out.write(Act::ui(RobotAction::Run { action }));
    }
    if keys.just_pressed(KeyCode::KeyR) {
        out.write(Act::ui(RobotAction::Run { action: RunAction::Reset }));
    }
    if keys.just_pressed(KeyCode::KeyS) {
        out.write(Act::ui(overlay_toggle(&view, "stress")));
    }
    let names = p.joint_names();
    if names.is_empty() {
        return;
    }
    let count = names.len();
    let current = p.selected_joint.min(count - 1);
    if keys.just_pressed(KeyCode::ArrowRight) {
        out.write(Act::ui(RobotAction::SelectJoint { index: (current + 1) % count }));
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        out.write(Act::ui(RobotAction::SelectJoint { index: (current + count - 1) % count }));
    }
    let step = if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) { planar::KEY_STEP_SHIFT_RAD } else { planar::KEY_STEP_RAD };
    let delta = if keys.pressed(KeyCode::ArrowUp) { step } else if keys.pressed(KeyCode::ArrowDown) { -step } else { 0.0 };
    if delta != 0.0 {
        out.write(Act::ui(RobotAction::Jog { joint: names[current].clone(), delta }));
        // Held: keep frames coming so the target keeps moving (as the planar viewer did per frame).
        redraw.write(bevy::window::RequestRedraw);
    }
}

/// Input: key G, the same `RobotAction::ToggleGraphs` as the Graphs button and `system_ui` graphs:toggle.
pub(in crate::robot) fn graph_key(keys: Res<ButtonInput<KeyCode>>, mut out: MessageWriter<Act<RobotAction>>) {
    if !chord(&keys) && keys.just_pressed(KeyCode::KeyG) {
        out.write(Act::ui(RobotAction::ToggleGraphs));
    }
}

/// Input: keys C / J / F / H, the same `RobotAction::Overlay` as the inspector buttons and `system_ui` overlay:*.
pub(in crate::robot) fn overlay_keys(keys: Res<ButtonInput<KeyCode>>, view: Res<RobotView>, mut out: MessageWriter<Act<RobotAction>>) {
    if chord(&keys) {
        return;
    }
    for (kind, _, key) in OVERLAYS {
        if keys.just_pressed(key) {
            out.write(Act::ui(overlay_toggle(&view, kind)));
        }
    }
}

/// Input: keys =/+ and − (main row and numpad), the same `RobotAction::Speed` as the header
/// −/+ buttons, `system_ui` run:speed_* and REST robot_speed. A refusal at ×8 / ×0.125 shows in the header.
pub(in crate::robot) fn speed_keys(keys: Res<ButtonInput<KeyCode>>, mut out: MessageWriter<Act<RobotAction>>) {
    if chord(&keys) {
        return;
    }
    let speed = if keys.any_just_pressed([KeyCode::Equal, KeyCode::NumpadAdd]) {
        SpeedRequest::Up
    } else if keys.any_just_pressed([KeyCode::Minus, KeyCode::NumpadSubtract]) {
        SpeedRequest::Down
    } else {
        return;
    };
    out.write(Act::ui(RobotAction::Speed { speed }));
}

/// A click on a link in the 3D view selects it (the same `SelectLink` as the list).
pub(in crate::robot) fn pick_link(click: On<Pointer<Click>>, links: Query<&LinkMesh>, view: Res<RobotView>, mut out: MessageWriter<Act<RobotAction>>) {
    if click.button != bevy::picking::pointer::PointerButton::Primary {
        return;
    }
    if let Ok(link) = links.get(click.entity) {
        let name = view.link_name(link.0).unwrap_or_default().to_string();
        out.write(Act::quiet(RobotAction::SelectLink { index: link.0, name }));
    }
}

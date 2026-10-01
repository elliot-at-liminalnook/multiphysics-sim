//! Place mode's first-person fly camera (moved from `place_view.rs`): the
//! [`Fly`] state (yaw, pitch, speed), its rotation ([`orientation`]) and the
//! continuous input ([`fly`], `CameraSet::Place`): W/A/S/D move on the
//! level, Q/E down/up, Shift ×3 (none of them while a kit text field has
//! the keyboard: `ui_kit::text::Typing`), the right (or left) button held turns the
//! view with the mouse, and the wheel scales the speed. Fly is first-person,
//! not orbit state: it has no focus to orbit, and the orbit gestures
//! (`input::navigate`) do not touch it. Place mode's actions (`camera`,
//! `state`) read and write the same fields, so REST and the keys agree.
//!
//! Decision (recorded): Bevy 0.19.1's `FreeCamera` (crate
//! `bevy_camera_controller`, feature `free_camera`) is not adopted. The
//! crate is not in this workspace's lockfile or registry (it would add a
//! dependency); it grabs the cursor on a right-click; and its state
//! (velocity, speed multipliers) is not REST-addressable the way `Fly`'s
//! yaw, pitch and speed are for `camera` and `state`. Its `PanCamera` is a
//! 2D controller.
use bevy::ecs::message::{MessageCursor, Messages};
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::ui_kit::SWITCHER_STRIP;
use crate::ui_kit::text::Typing;

/// The fly camera's heading and speed (Place mode).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fly {
    /// About +Y, radians.
    pub(crate) yaw: f32,
    /// Up positive, radians; within ±1.5.
    pub(crate) pitch: f32,
    /// Metres per second, 0.05–8 (Shift triples it).
    pub(crate) speed: f32,
}

/// The fly camera's rotation from its yaw and pitch.
pub(crate) fn orientation(fly: &Fly) -> Quat {
    Quat::from_euler(EulerRot::YXZ, fly.yaw, fly.pitch, 0.0) * Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2)
}

/// CameraSet::Place: continuous flying (W/A/S/D, Q/E, Shift), mouse look
/// and the wheel's speed. Nothing without a [`Fly`] camera (every mode but
/// Place) or without the input resources and messages (a window-free app:
/// they are read as options, since a `MessageReader` of an unregistered
/// message fails validation and panics). The mouse messages are skipped
/// either way, so a fly camera spawned later does not take a stale burst.
/// Pointer input belongs to the view only: the wheel is ignored over the
/// switcher strip (`ui_kit::SWITCHER_STRIP`), and a drag turns the view
/// only if its first button went down in the view ([`in_view`]); `drag`
/// holds that answer until every button is up (None: none held). While a
/// kit text field has the keyboard the keys do not move it (a typed "w" is
/// held in `ButtonInput` too: the kit consumes only its press); the mouse
/// look and the wheel still work.
#[allow(clippy::too_many_arguments)]
pub(super) fn fly(
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    mut drag: Local<Option<bool>>,
    time: Option<Res<Time>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    motion: Option<Res<Messages<MouseMotion>>>,
    wheel: Option<Res<Messages<MouseWheel>>>,
    mut motion_seen: Local<MessageCursor<MouseMotion>>,
    mut wheel_seen: Local<MessageCursor<MouseWheel>>,
    mut q: Query<(&mut Transform, &mut Fly)>,
    typing: Typing,
) {
    let (Some(time), Some(keys), Some(buttons), Some(motion), Some(wheel)) = (time, keys, buttons, motion, wheel) else { return };
    let pointer_in_view = in_view(window.as_deref().copied());
    let held = buttons.pressed(MouseButton::Right) || buttons.pressed(MouseButton::Left);
    if !held {
        *drag = None;
    } else if drag.is_none() {
        // The first button down (or one already held when this system
        // first sees it, e.g. the click on Place's switcher button).
        *drag = Some(pointer_in_view);
    }
    let Ok((mut t, mut fly)) = q.single_mut() else {
        motion_seen.clear(&motion);
        wheel_seen.clear(&wheel);
        return;
    };
    if *drag == Some(true) {
        for m in motion_seen.read(&motion) {
            fly.yaw -= m.delta.x * 0.004;
            fly.pitch = (fly.pitch - m.delta.y * 0.004).clamp(-1.5, 1.5);
        }
    } else {
        motion_seen.clear(&motion);
    }
    for w in wheel_seen.read(&wheel) {
        if !pointer_in_view {
            continue;
        }
        fly.speed = (fly.speed * if w.y > 0.0 { 1.15 } else { 1.0 / 1.15 }).clamp(0.05, 8.0);
    }
    t.rotation = orientation(&fly);
    if typing.get() {
        return;
    }
    let (forward, right) = (*t.forward(), *t.right());
    let flat = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
    let mut d = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        d += flat;
    }
    if keys.pressed(KeyCode::KeyS) {
        d -= flat;
    }
    if keys.pressed(KeyCode::KeyD) {
        d += right;
    }
    if keys.pressed(KeyCode::KeyA) {
        d -= right;
    }
    if keys.pressed(KeyCode::KeyE) {
        d += Vec3::Y;
    }
    if keys.pressed(KeyCode::KeyQ) {
        d -= Vec3::Y;
    }
    let boost = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) { 3.0 } else { 1.0 };
    t.translation += d.normalize_or_zero() * fly.speed * boost * time.delta_secs();
}

/// Whether the pointer is over the 3D view: in the window and above the
/// switcher strip (logical px). No cursor (outside the window) is not in
/// the view; no window at all (a window-free app or test) counts as in it.
fn in_view(window: Option<&Window>) -> bool {
    let Some(window) = window else { return true };
    window.cursor_position().is_some_and(|c| c.y < window.height() - SWITCHER_STRIP)
}

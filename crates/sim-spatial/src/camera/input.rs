//! Pointer navigation and the shared numpad camera keys.
//!
//! [`navigate`] (SimSync) is the one right-drag orbit, middle- or
//! Shift+right-drag pan and wheel zoom of every orbit camera, moved from
//! CAD's, Robot's, Phenomena's and the spatial view's own `orbit` systems: a
//! drag that starts inside the view (and, where the mode's rules yield to the
//! UI, not over a UI node) keeps going over a panel until both buttons are
//! up; one that starts elsewhere is not the camera's. Any drag or zoom stops
//! a glide or spin.
//!
//! [`keys`] (Input) maps the numpad views to [`CameraAction`]s for the modes
//! whose rules ask for it (CAD binds its own keymap and sets `keys` false).
use super::viewport::area;
use super::{CameraAction, Orbit, OrbitRules, ViewArea, ViewPreset};
use crate::app::actions::Act;
use bevy::ecs::message::{MessageCursor, Messages};
use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// Whether the mouse is over a UI node (a panel): the pointer is the panel's then.
fn over_ui(hover: Option<&HoverMap>, nodes: &Query<(), With<Node>>) -> bool {
    hover.and_then(|h| h.get(&PointerId::Mouse)).is_some_and(|hits| hits.keys().any(|e| nodes.contains(*e)))
}

fn held(keys: Option<&ButtonInput<KeyCode>>, codes: [KeyCode; 2]) -> bool {
    keys.is_some_and(|k| k.any_pressed(codes))
}

/// Ctrl or Cmd: a lesson page's wheel zooms its view only with one held.
fn command_held(keys: Option<&ButtonInput<KeyCode>>) -> bool {
    held(keys, [KeyCode::ControlLeft, KeyCode::ControlRight]) || held(keys, [KeyCode::SuperLeft, KeyCode::SuperRight])
}

/// The wheel's travel in lines this frame (pixels at 0.02 per line, as every
/// mode's wheel did).
fn wheel_lines(wheel: Option<&Messages<MouseWheel>>, cursor: &mut MessageCursor<MouseWheel>) -> f32 {
    let Some(wheel) = wheel else { return 0.0 };
    cursor.read(wheel).fold(0.0, |sum, e| {
        sum + match e.unit {
            MouseScrollUnit::Line => e.y,
            MouseScrollUnit::Pixel => e.y * 0.02,
        }
    })
}

/// Where a pointer gesture may act on a camera.
fn accepts(rules: &OrbitRules, view: &ViewArea, window: &Window, cursor: Vec2, over_ui: bool) -> bool {
    rules.enabled && (!rules.yield_to_ui || !over_ui) && area(window, view).contains(cursor)
}

/// SimSync (`CameraSet::Navigate`): right-drag orbits, middle- or
/// Shift+right-drag pans, the wheel zooms (toward the point under the
/// cursor where the rules ask for it). The pointer messages are drained every
/// frame, so motion from before a drag never arrives late. `Orbit` is
/// written only when a gesture moves it.
#[allow(clippy::too_many_arguments)]
pub(super) fn navigate(
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    motion: Option<Res<Messages<MouseMotion>>>,
    wheel: Option<Res<Messages<MouseWheel>>>,
    mut cursors: Local<(MessageCursor<MouseMotion>, MessageCursor<MouseWheel>)>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    hover: Option<Res<HoverMap>>,
    nodes: Query<(), With<Node>>,
    mut cameras: Query<(Entity, &mut Orbit, &OrbitRules, &ViewArea, &Camera)>,
    mut latched: Local<Option<Entity>>,
) {
    let (motion_cursor, wheel_cursor) = &mut *cursors;
    let drag = motion.as_deref().map_or(Vec2::ZERO, |m| motion_cursor.read(m).fold(Vec2::ZERO, |sum, e| sum + e.delta));
    let lines = wheel_lines(wheel.as_deref(), wheel_cursor);
    let keys = keys.as_deref();
    let (Some(window), Some(buttons)) = (window, buttons) else {
        *latched = None;
        return;
    };
    let cursor = window.cursor_position();
    let over_ui = over_ui(hover.as_deref(), &nodes);

    // The drag latch: decided when a drag button goes down, held until both are up.
    if buttons.any_just_pressed([MouseButton::Right, MouseButton::Middle]) {
        *latched = cursor.and_then(|c| cameras.iter().find(|(_, _, rules, view, _)| accepts(rules, view, &window, c, over_ui)).map(|(e, ..)| e));
    }
    if !buttons.any_pressed([MouseButton::Right, MouseButton::Middle]) {
        *latched = None;
    }
    if let Some(entity) = *latched
        && drag != Vec2::ZERO
        && let Ok((_, mut orbit, rules, _, _)) = cameras.get_mut(entity)
        && rules.enabled
    {
        let shift = held(keys, [KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        orbit.interrupt();
        if buttons.pressed(MouseButton::Middle) || (buttons.pressed(MouseButton::Right) && shift) {
            orbit.pan(drag, rules);
        } else {
            orbit.rotate(drag, rules);
        }
    }

    // The wheel: the camera under the cursor, Ctrl/Cmd where the rules ask for it.
    let (Some(cursor), true) = (cursor, lines != 0.0) else { return };
    let command = command_held(keys);
    for (_, mut orbit, rules, view, camera) in &mut cameras {
        if !accepts(rules, view, &window, cursor, over_ui) || (rules.zoom_modifier && !command) {
            continue;
        }
        let anchor = if rules.zoom_to_cursor { cursor_anchor(&orbit, camera, cursor) } else { None };
        orbit.interrupt();
        orbit.zoom((-0.12 * lines).exp(), anchor, rules);
        break;
    }
}

/// The display-frame point under a window position (logical pixels, as
/// `Window::cursor_position`) on the plane through the focus facing the view.
/// The ray is cast from the orbit's own placement (the camera is a root
/// entity whose transform `place` writes from the orbit), so a change made
/// earlier this frame is already counted; the projection is the camera's
/// last computed one.
pub(super) fn cursor_anchor(orbit: &Orbit, camera: &Camera, at: Vec2) -> Option<Vec3> {
    let ray = camera.viewport_to_world(&GlobalTransform::from(orbit.transform()), at).ok()?;
    orbit.focus_plane_hit(ray.origin, *ray.direction)
}

/// Input: the shared numpad camera keys (Blender's layout, RoboCAD's views):
/// 1 front, 3 right, 7 top (with Ctrl the opposite side), 9 the opposite
/// view, 5 orthographic/perspective, 0 iso, `.` fit, Home the home view.
/// Read only while an enabled orbit camera's rules ask for them.
pub(super) fn keys(keys: Option<Res<ButtonInput<KeyCode>>>, cameras: Query<&OrbitRules, With<Orbit>>, mut out: MessageWriter<Act<CameraAction>>) {
    let Some(keys) = keys else { return };
    if !cameras.iter().any(|rules| rules.keys && rules.enabled) {
        return;
    }
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let view = |plain: ViewPreset, with_ctrl: ViewPreset| CameraAction::View { view: if ctrl { with_ctrl } else { plain } };
    for key in keys.get_just_pressed() {
        let action = match key {
            KeyCode::Numpad1 => view(ViewPreset::Front, ViewPreset::Back),
            KeyCode::Numpad3 => view(ViewPreset::Right, ViewPreset::Left),
            KeyCode::Numpad7 => view(ViewPreset::Top, ViewPreset::Bottom),
            KeyCode::Numpad9 => CameraAction::Opposite,
            KeyCode::Numpad5 => CameraAction::Projection { orthographic: None },
            KeyCode::Numpad0 => CameraAction::View { view: ViewPreset::Iso },
            KeyCode::NumpadDecimal => CameraAction::Fit,
            KeyCode::Home => CameraAction::Home,
            _ => continue,
        };
        out.write(Act::ui(action));
    }
}

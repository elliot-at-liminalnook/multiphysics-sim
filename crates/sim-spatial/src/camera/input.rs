//! Pointer navigation and the shared numpad camera keys.
//!
//! [`navigate`] (SimSync) is the one right-drag orbit, middle- or
//! Shift+right-drag pan and wheel zoom of every orbit camera, moved from
//! CAD's, Robot's, Phenomena's and the spatial view's own `orbit` systems: a
//! drag that starts inside the view (and, where the mode's rules yield to the
//! UI, not over a UI node) keeps going over a panel until both buttons are
//! up; one that starts elsewhere is not the camera's. Any drag or zoom stops
//! a glide or spin. With `robocad_gestures` (CAD) it also takes RoboCAD's
//! Shift+middle orbit, Alt+right axis snap and Alt+left-drag orbit
//! ([`drag_kind`], [`ALT_DRAG_SLOP`]).
//!
//! [`keys`] (Input) maps the numpad views to [`CameraAction`]s for the modes
//! whose rules ask for it (CAD binds its own keymap and sets `keys` false),
//! and RoboCAD's arrow keys ([`arrow_action`]) where `robocad_gestures` is
//! on; neither while the rules say a text field is `typing`.
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

/// How far (window logical pixels, Manhattan: |dx| + |dy|) an Alt+left
/// press must move before the camera takes it as an orbit; a shorter one
/// is a click and stays the mode's (CAD's Alt+click candidates menu). The
/// same 6 px as CAD's click/drag slop (`cad::pick`), so the two agree on
/// which presses are drags.
pub const ALT_DRAG_SLOP: f32 = 6.0;

/// What a right or middle drag does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DragKind {
    Orbit,
    Pan,
    /// Orbit, then snap to the nearest axis view (RoboCAD: Alt held while
    /// right-drag orbiting).
    OrbitSnap,
}

/// A right/middle drag's kind from the buttons held and the modifiers.
/// Every mode: middle or Shift+right pans, right orbits. With RoboCAD's
/// gestures (`ui/viewport.py` `mouseMoveEvent`): orbit is right without
/// Shift, or middle with Shift; anything else held pans; Alt while
/// right-orbiting snaps.
pub(super) fn drag_kind(robocad: bool, right: bool, middle: bool, shift: bool, alt: bool) -> Option<DragKind> {
    if !(right || middle) {
        return None;
    }
    if !robocad {
        return Some(if middle || shift { DragKind::Pan } else { DragKind::Orbit });
    }
    let orbit = (right && !shift) || (middle && shift);
    Some(match (orbit, right && alt) {
        (false, _) => DragKind::Pan,
        (true, true) if !shift => DragKind::OrbitSnap,
        (true, _) => DragKind::Orbit,
    })
}

/// An Alt+left press a camera may take: where it went down and whether it
/// has moved past [`ALT_DRAG_SLOP`] (from then on it orbits until the left
/// button is up, Alt or not).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct AltLeft {
    camera: Entity,
    start: Vec2,
    orbiting: bool,
}

/// The pointer gestures in progress.
#[derive(Default)]
pub(super) struct Drags {
    /// The camera a right or middle drag is for: decided when a drag
    /// button goes down, held until both are up.
    latched: Option<Entity>,
    alt_left: Option<AltLeft>,
}

/// SimSync (`CameraSet::Navigate`): right-drag orbits, middle- or
/// Shift+right-drag pans, the wheel zooms (toward the point under the
/// cursor where the rules ask for it), and RoboCAD's gestures where the
/// rules ask for them ([`drag_kind`], Alt+left-drag). The pointer messages
/// are drained every frame, so motion from before a drag never arrives
/// late. `Orbit` is written only when a gesture moves it.
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
    mut drags: Local<Drags>,
) {
    let (motion_cursor, wheel_cursor) = &mut *cursors;
    let drag = motion.as_deref().map_or(Vec2::ZERO, |m| motion_cursor.read(m).fold(Vec2::ZERO, |sum, e| sum + e.delta));
    let lines = wheel_lines(wheel.as_deref(), wheel_cursor);
    let keys = keys.as_deref();
    let (Some(window), Some(buttons)) = (window, buttons) else {
        *drags = Drags::default();
        return;
    };
    let cursor = window.cursor_position();
    let over_ui = over_ui(hover.as_deref(), &nodes);
    let shift = held(keys, [KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let alt = held(keys, [KeyCode::AltLeft, KeyCode::AltRight]);

    // The drag latch: decided when a drag button goes down, held until both are up.
    if buttons.any_just_pressed([MouseButton::Right, MouseButton::Middle]) {
        drags.latched = cursor.and_then(|c| cameras.iter().find(|(_, _, rules, view, _)| accepts(rules, view, &window, c, over_ui)).map(|(e, ..)| e));
    }
    if !buttons.any_pressed([MouseButton::Right, MouseButton::Middle]) {
        drags.latched = None;
    }
    // Alt+left (RoboCAD's gestures): noted at the press, an orbit once past the slop.
    if buttons.just_pressed(MouseButton::Left) {
        drags.alt_left = cursor.filter(|_| alt).and_then(|c| {
            cameras.iter().find(|(_, _, rules, view, _)| rules.robocad_gestures && rules.alt_left && accepts(rules, view, &window, c, over_ui)).map(|(camera, ..)| AltLeft { camera, start: c, orbiting: false })
        });
    }
    if !buttons.pressed(MouseButton::Left) {
        drags.alt_left = None;
    }
    if let (Some(press), Some(c)) = (drags.alt_left.as_mut(), cursor)
        && !press.orbiting
        && (c - press.start).abs().element_sum() > ALT_DRAG_SLOP
    {
        press.orbiting = true;
    }

    if let Some(entity) = drags.latched
        && drag != Vec2::ZERO
        && let Ok((_, mut orbit, rules, _, _)) = cameras.get_mut(entity)
        && rules.enabled
        && let Some(kind) = drag_kind(rules.robocad_gestures, buttons.pressed(MouseButton::Right), buttons.pressed(MouseButton::Middle), shift, alt)
    {
        orbit.interrupt();
        match kind {
            DragKind::Pan => orbit.pan(drag, rules),
            DragKind::Orbit => orbit.rotate(drag, rules),
            DragKind::OrbitSnap => {
                // RoboCAD snaps after every orbit step while Alt is held.
                orbit.rotate(drag, rules);
                orbit.snap_to_axis(rules);
            }
        }
    } else if let Some(press) = drags.alt_left
        && press.orbiting
        && drag != Vec2::ZERO
        && let Ok((_, mut orbit, rules, _, _)) = cameras.get_mut(press.camera)
        && rules.enabled
    {
        orbit.interrupt();
        orbit.rotate(drag, rules);
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

/// RoboCAD's arrow keys (`ui/viewport.py` `keyPressEvent`): orbit by 10°
/// (Ctrl, which is Cmd on a Mac as Qt maps it, 90°) — right turns yaw
/// up, up pitches up — or with Shift pan by 4 px per degree the other
/// way (RoboCAD's `pan(-dx × 4, dy × 4)`). None for any other key.
pub(super) fn arrow_action(key: KeyCode, command: bool, shift: bool) -> Option<CameraAction> {
    let step = if command { 90.0 } else { 10.0 };
    let (dx, dy) = match key {
        KeyCode::ArrowRight => (step, 0.0),
        KeyCode::ArrowLeft => (-step, 0.0),
        KeyCode::ArrowUp => (0.0, step),
        KeyCode::ArrowDown => (0.0, -step),
        _ => return None,
    };
    Some(if shift { CameraAction::Pan { dx: -dx * 4.0, dy: dy * 4.0 } } else { CameraAction::Orbit { dx: 0.0, dy: 0.0, degrees: Some([dx, dy]) } })
}

/// Input: the shared numpad camera keys (Blender's layout, RoboCAD's views):
/// 1 front, 3 right, 7 top (with Ctrl the opposite side), 9 the opposite
/// view, 5 orthographic/perspective, 0 iso, `.` fit, Home the home view;
/// and RoboCAD's arrow keys ([`arrow_action`]). Each set is read only
/// while an enabled orbit camera's rules ask for it (`keys`,
/// `robocad_gestures`) and no text field of its mode has the keyboard
/// (`typing`).
pub(super) fn keys(keys: Option<Res<ButtonInput<KeyCode>>>, cameras: Query<&OrbitRules, With<Orbit>>, mut out: MessageWriter<Act<CameraAction>>) {
    let Some(keys) = keys else { return };
    let ready = |rules: &&OrbitRules| rules.enabled && !rules.typing;
    let numpad = cameras.iter().filter(ready).any(|rules| rules.keys);
    let arrows = cameras.iter().filter(ready).any(|rules| rules.robocad_gestures);
    if !(numpad || arrows) {
        return;
    }
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let command = command_held(Some(&*keys));
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let view = |plain: ViewPreset, with_ctrl: ViewPreset| CameraAction::View { view: if ctrl { with_ctrl } else { plain } };
    for key in keys.get_just_pressed() {
        let action = match key {
            KeyCode::Numpad1 if numpad => view(ViewPreset::Front, ViewPreset::Back),
            KeyCode::Numpad3 if numpad => view(ViewPreset::Right, ViewPreset::Left),
            KeyCode::Numpad7 if numpad => view(ViewPreset::Top, ViewPreset::Bottom),
            KeyCode::Numpad9 if numpad => CameraAction::Opposite,
            KeyCode::Numpad5 if numpad => CameraAction::Projection { orthographic: None },
            KeyCode::Numpad0 if numpad => CameraAction::View { view: ViewPreset::Iso },
            KeyCode::NumpadDecimal if numpad => CameraAction::Fit,
            KeyCode::Home if numpad => CameraAction::Home,
            other if arrows => match arrow_action(*other, command, shift) {
                Some(action) => action,
                None => continue,
            },
            _ => continue,
        };
        out.write(Act::ui(action));
    }
}

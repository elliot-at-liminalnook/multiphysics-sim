//! The spatial view's camera: its viewport beside the panels (or inside a
//! lesson card) and the orbit, pan and zoom controls.
use super::{Orbit, SpatialScene};
use crate::view;
use bevy::{
    camera::Viewport,
    input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    prelude::*,
};

pub(crate) fn camera_viewport(
    scene: Res<SpatialScene>,
    window: Single<&Window>,
    mut camera: Single<&mut Camera, With<Orbit>>,
) {
    if let Some(view) = scene.learn_view {
        // Embedded in a lesson card: render only the visible part of the
        // card, with the projection of the whole card (no squash when scrolled).
        let (viewport, sub) = if view.visible.width() < 1.0 || view.visible.height() < 1.0 {
            (Viewport { physical_position: UVec2::ZERO, physical_size: UVec2::ONE, ..default() }, None)
        } else {
            let sub = bevy::camera::SubCameraView {
                full_size: view.full.size().max(Vec2::ONE).as_uvec2(),
                offset: view.visible.min - view.full.min,
                size: view.visible.size().max(Vec2::ONE).as_uvec2(),
            };
            (Viewport { physical_position: view.visible.min.max(Vec2::ZERO).as_uvec2(), physical_size: view.visible.size().max(Vec2::ONE).as_uvec2(), ..default() }, Some(sub))
        };
        if camera.viewport.as_ref().is_none_or(|old| old.physical_size != viewport.physical_size || old.physical_position != viewport.physical_position) {
            camera.viewport = Some(viewport);
        }
        if camera.sub_camera_view != sub {
            camera.sub_camera_view = sub;
        }
        return;
    }
    if camera.sub_camera_view.is_some() {
        camera.sub_camera_view = None;
    }
    let scale = window.scale_factor();
    let width = (window.width() - scene.left() - scene.right()).max(1.0);
    let height = (window.height() - scene.top() - scene.bottom()).max(1.0);
    let viewport = Viewport {
        physical_position: UVec2::new((scene.left() * scale) as u32, (scene.top() * scale) as u32),
        physical_size: UVec2::new((width * scale) as u32, (height * scale) as u32),
        ..default()
    };
    if camera.viewport.as_ref().is_none_or(|old| {
        old.physical_size != viewport.physical_size
            || old.physical_position != viewport.physical_position
    }) {
        camera.viewport = Some(viewport);
    }
}
pub(super) fn orbit(
    time: Res<Time>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    scene: Res<SpatialScene>,
    window: Single<&Window>,
    camera: Single<(&mut Transform, &mut Orbit)>,
) {
    let drag = motion.read().fold(Vec2::ZERO, |sum, e| sum + e.delta);
    let zoom = wheel.read().fold(0.0, |sum, e| {
        sum + match e.unit {
            MouseScrollUnit::Line => e.y,
            MouseScrollUnit::Pixel => e.y * 0.02,
        }
    });
    let (mut transform, mut orbit) = camera.into_inner();
    let learn = scene.learn_view;
    let in_scene = window.cursor_position().is_some_and(|p| match learn {
        Some(v) => v.visible.contains(p * window.scale_factor()),
        None => {
            p.x > scene.left()
                && p.x < window.width() - scene.right()
                && p.y > scene.top()
                && p.y < window.height() - scene.bottom()
        }
    });
    // In a lesson the wheel scrolls the page; zoom needs Ctrl or Cmd.
    let zoom = if learn.is_some() && !(keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) || keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight)) { 0.0 } else { zoom };
    let (_, extent) = scene.bounds();
    if orbit.home {
        // The overview: a glide once the view has been placed, a cut the first time.
        let pose = view::frame_pose(&scene, None, 1.0, 0.35, 0.60, view::aspect(&scene, &window));
        let seconds = if orbit.radius > 0. && orbit.focus != Vec3::ZERO { view::GLIDE_S } else { 0. };
        orbit.glide_to(pose, seconds);
    }
    if scene.reduced_motion {
        // Cuts instead of glides; no circling.
        orbit.finish_glide();
        orbit.spin = 0.;
    }
    orbit.step(time.delta_secs().min(0.1));
    if in_scene {
        let dragging = (buttons.pressed(MouseButton::Right) || buttons.pressed(MouseButton::Middle)) && drag != Vec2::ZERO;
        if dragging || zoom != 0.0 {
            orbit.interrupt();
        }
        let pan_modifier = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        if buttons.pressed(MouseButton::Right) && !pan_modifier {
            orbit.yaw -= drag.x * 0.007;
            orbit.pitch = (orbit.pitch + drag.y * 0.007).clamp(-1.4, 1.4);
        }
        if buttons.pressed(MouseButton::Middle)
            || (buttons.pressed(MouseButton::Right) && pan_modifier)
        {
            let shift =
                (transform.right() * -drag.x + transform.up() * drag.y) * orbit.radius * 0.0015;
            orbit.focus += shift;
        }
        orbit.radius = (orbit.radius * (-zoom * 0.12).exp()).clamp(extent * 0.3, extent * 20.0);
    }
    let horizontal = orbit.pitch.cos() * orbit.radius;
    transform.translation = orbit.focus
        + Vec3::new(
            orbit.yaw.sin() * horizontal,
            orbit.pitch.sin() * orbit.radius,
            orbit.yaw.cos() * horizontal,
        );
    transform.look_at(orbit.focus, Vec3::Y);
}

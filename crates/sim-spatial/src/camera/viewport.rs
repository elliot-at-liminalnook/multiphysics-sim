//! Where each orbit camera draws ([`ViewArea`] → `Camera::viewport` and
//! `sub_camera_view`), and the rectangle its gestures may start in
//! ([`area`]). Moved from CAD's, Robot's and the spatial view's own
//! viewport systems: docks give the rectangle between them (the whole window
//! when they leave no room), a lesson card renders only its visible part with
//! the projection of the whole card.
use super::{Orbit, ViewArea};
use bevy::camera::{SubCameraView, Viewport};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// The viewport between docks (logical insets), in physical pixels; None
/// when it does not fit inside the window (draw over the whole window rather
/// than outside it, as CAD's viewport did).
fn docks(window: &Window, left: f32, right: f32, top: f32, bottom: f32) -> Option<Viewport> {
    let scale = window.scale_factor();
    let width = window.width() - left - right;
    let height = window.height() - top - bottom;
    if !(width >= 1.0 && height >= 1.0 && left >= 0.0 && top >= 0.0) {
        return None;
    }
    let viewport = Viewport { physical_position: UVec2::new((left * scale) as u32, (top * scale) as u32), physical_size: UVec2::new((width * scale) as u32, (height * scale) as u32), ..default() };
    let fits = viewport.physical_size.cmpge(UVec2::ONE).all() && (viewport.physical_position + viewport.physical_size).cmple(window.physical_size()).all();
    fits.then_some(viewport)
}

/// The viewport and sub-view a view area asks for (the spatial view's
/// `camera_viewport` for a card: the visible part of the card, with the
/// projection of the whole card; an empty visible part draws 1×1).
pub(super) fn wanted(window: &Window, area: &ViewArea) -> (Option<Viewport>, Option<SubCameraView>) {
    match *area {
        ViewArea::Window => (None, None),
        ViewArea::Docks { left, right, top, bottom } => (docks(window, left, right, top, bottom), None),
        ViewArea::Card { full, visible } => {
            if visible.width() < 1.0 || visible.height() < 1.0 {
                return (Some(Viewport { physical_position: UVec2::ZERO, physical_size: UVec2::ONE, ..default() }), None);
            }
            let sub = SubCameraView { full_size: full.size().max(Vec2::ONE).as_uvec2(), offset: visible.min - full.min, size: visible.size().max(Vec2::ONE).as_uvec2() };
            (Some(Viewport { physical_position: visible.min.max(Vec2::ZERO).as_uvec2(), physical_size: visible.size().max(Vec2::ONE).as_uvec2(), ..default() }), Some(sub))
        }
    }
}

/// The logical-pixel rectangle a gesture may start in: between the docks
/// (the whole window when they leave no room, as the viewport then is), the
/// card's visible part (empty when nothing of it is shown), or the window.
pub(super) fn area(window: &Window, area: &ViewArea) -> Rect {
    let whole = Rect::from_corners(Vec2::ZERO, window.size());
    match *area {
        ViewArea::Window => whole,
        ViewArea::Docks { left, right, top, bottom } => match docks(window, left, right, top, bottom) {
            Some(_) => Rect::from_corners(Vec2::new(left, top), Vec2::new(window.width() - right, window.height() - bottom)),
            None => whole,
        },
        ViewArea::Card { visible, .. } => {
            if visible.width() < 1.0 || visible.height() < 1.0 {
                return Rect::EMPTY;
            }
            let scale = window.scale_factor().max(1e-6);
            Rect { min: visible.min / scale, max: visible.max / scale }
        }
    }
}

/// `Viewport` has no `PartialEq` in 0.19.1: the fields this module writes.
fn same(a: &Option<Viewport>, b: &Option<Viewport>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.physical_position == b.physical_position && a.physical_size == b.physical_size && a.depth == b.depth,
        (None, None) => true,
        _ => false,
    }
}

/// SimSync (`CameraSet::Viewport`): each orbit camera's viewport from its
/// view area, written only on a change.
pub(super) fn viewport(window: Option<Single<&Window, With<PrimaryWindow>>>, mut cameras: Query<(&mut Camera, &ViewArea), With<Orbit>>) {
    let Some(window) = window else { return };
    for (mut camera, area) in &mut cameras {
        let (viewport, sub) = wanted(&window, area);
        if !same(&camera.viewport, &viewport) {
            camera.viewport = viewport;
        }
        if camera.sub_camera_view != sub {
            camera.sub_camera_view = sub;
        }
    }
}

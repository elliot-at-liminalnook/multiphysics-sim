//! The spatial view's camera as data for the shared camera module
//! (`crate::camera`): its feel ([`spatial_rules`]) and, each frame, the
//! bounds it frames, the reader's motion preference, the lesson card's
//! Ctrl/Cmd zoom and where the view draws ([`sync_camera`]). Orbiting,
//! panning, zooming, gliding, spinning and the viewport are the module's.
use super::SpatialScene;
use crate::camera::{Framing, Orbit, OrbitRules, RadiusLimits, ViewArea};
use bevy::prelude::*;

/// The spatial view's camera feel (Inspect, Build and Lessons; the headless
/// inspect server frames with it too): right-drag orbits at 0.007 rad/px
/// within ±1.4 rad of pitch, the zoom stays within 0.3–20 × the scene's
/// extent, and home is the overview from yaw 0.35, pitch 0.60 at 2.9 × the
/// extent (divided by the view's aspect clamped to 0.1–1), gliding once the
/// view was placed.
pub(crate) fn spatial_rules() -> OrbitRules {
    OrbitRules {
        rate: 0.007,
        pan_rate: 0.0015,
        pitch_limit: 1.4,
        radius: RadiusLimits::Extent { min: 0.3, max: 20.0 },
        framing: Framing::Bounds { scale: 2.9, aspect: true, view: Some((0.35, 0.60)) },
        glide_home: true,
        zoom_to_cursor: false,
        yield_to_ui: false,
        keys: true,
        ..Default::default()
    }
}

/// Where the 3D view draws: inside the lesson card when embedded, else
/// between the panels.
pub(super) fn view_area(scene: &SpatialScene) -> ViewArea {
    match scene.learn_view {
        Some(v) => ViewArea::Card { full: v.full, visible: v.visible },
        None => ViewArea::Docks { left: scene.left(), right: scene.right(), top: scene.top(), bottom: scene.bottom() },
    }
}

/// SimSync, before `CameraSet::Viewport`: the scene's data into the shared
/// camera, each written only on a change. The bounds a home request frames,
/// reduced motion (cuts, no spin), the wheel needing Ctrl/Cmd inside a
/// lesson card (the page scrolls with it), and the view area. The numpad
/// keys stay on (`spatial_rules`): while a kit text field has the keyboard
/// the camera's key system does not run (`ui_kit::text::typing`).
pub(crate) fn sync_camera(scene: Res<SpatialScene>, camera: Single<(&mut Orbit, &mut OrbitRules, &mut ViewArea)>) {
    let (mut orbit, mut rules, mut area) = camera.into_inner();
    let (centre, extent) = scene.bounds();
    if orbit.centre != centre || orbit.extent != extent {
        orbit.centre = centre;
        orbit.extent = extent;
    }
    let zoom_modifier = scene.learn_view.is_some();
    if rules.reduced_motion != scene.reduced_motion || rules.zoom_modifier != zoom_modifier {
        rules.reduced_motion = scene.reduced_motion;
        rules.zoom_modifier = zoom_modifier;
    }
    let want = view_area(&scene);
    if *area != want {
        *area = want;
    }
}

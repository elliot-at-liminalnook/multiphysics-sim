//! The native camera and display state ↔ RoboCAD's view state, as pure
//! functions (tested in `views::tests`).
//!
//! Frames: RoboCAD's model frame is Z up in mm; CAD mode draws it under a
//! root rotated −90° about X and scaled by 0.001 (`cad/mesh.rs`
//! `root_transform`), so a model point (x, y, z) mm is the display point
//! (x, z, −y) / 1000 m, and a model direction (x, y, z) the display
//! direction (x, z, −y).
//!
//! - `target` mm ↔ `Orbit::focus` m through that map; `distance` mm ↔
//!   `radius` m (÷ 1000).
//! - `yaw`/`pitch` degrees (Z up, yaw about +Z) ↔ the turntable's radians
//!   through `camera::{robocad_to_display, display_to_robocad}` (yaw + 90°).
//! - `rot` (rows: the view's right, up and back in the model frame, world
//!   → view) ↔ the display camera's rotation `Q` (its columns `Q·X`, `Q·Y`,
//!   `Q·Z` are right, up and back in the display frame, Bevy's camera
//!   looking along −Z): `Q = M · rotᵀ` with `M` the model → display map,
//!   and `rot = (Mᵀ · Q)ᵀ`, i.e. row i = `M⁻¹ (Q · eᵢ)`. For a turntable
//!   view `Ry(Y)·Rx(−p)` this is RoboCAD's own `Camera.basis()` (right =
//!   (−sin y, cos y, 0), back = (cos p cos y, cos p sin y, sin p)), so the
//!   saved `rot` is the view's real basis in either mode.
//! - `fov` degrees ↔ `Orbit::fov` radians; `orthographic` as is; `mode`
//!   "trackball" ↔ `Orbit::trackball` (restored from `rot`), "turntable"
//!   ↔ none (restored from yaw and pitch, as RoboCAD's turntable ignores
//!   `rot`).
//! - `grid`, `display_mode`, `comment_pins` and `section` ↔ `CadDisplay`.
use crate::cad::display::{CadDisplay, DisplayMode, SectionPlane};
use crate::camera::{CameraState, Orbit, display_to_robocad, robocad_to_display};
use bevy::math::{DMat3, DVec3};
use bevy::prelude::*;
use crate::cad::types::{VIEW_PITCH_LIMIT, ViewPlane, ViewSection, ViewState};

/// The field of view the native camera takes, degrees (RoboCAD's `view.fov`
/// dialog, `QInputDialog.getDouble(…, 5, 120, 1)`; `camera_set` refuses
/// outside it). RoboCAD's view state allows 1–170.
pub(crate) const FOV_RANGE: (f64, f64) = (5.0, 120.0);

/// What a saved view takes from the orbit camera (`Orbit`): copied each
/// frame in CAD mode (`views::snapshot`), so the handler, which has no
/// camera access, can save it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewCamera {
    /// Display frame, metres.
    pub focus: Vec3,
    pub radius: f32,
    /// The turntable's yaw and pitch (radians, display frame).
    pub yaw: f32,
    pub pitch: f32,
    /// The camera's rotation (the trackball's, else the turntable's).
    pub rotation: Quat,
    pub trackball: bool,
    pub orthographic: bool,
    /// Vertical field of view, radians.
    pub fov: f32,
}

impl ViewCamera {
    pub fn of(o: &Orbit) -> ViewCamera {
        ViewCamera { focus: o.focus, radius: o.radius, yaw: o.yaw, pitch: o.pitch, rotation: o.rotation(), trackball: o.trackball.is_some(), orthographic: o.orthographic, fov: o.fov }
    }
}

/// Model direction → display direction: (x, y, z) → (x, z, −y).
fn m_to_d(v: DVec3) -> DVec3 {
    DVec3::new(v.x, v.z, -v.y)
}
/// Display direction → model direction: (x, y, z) → (x, −z, y).
fn d_to_m(v: DVec3) -> DVec3 {
    DVec3::new(v.x, -v.z, v.y)
}

/// A model point (mm) as the display point (m).
pub fn model_to_display(p: [f64; 3]) -> Vec3 {
    (m_to_d(DVec3::from_array(p)) / 1000.0).as_vec3()
}

/// A display point (m) as the model point (mm).
pub fn display_to_model(v: Vec3) -> [f64; 3] {
    (d_to_m(v.as_dvec3()) * 1000.0).to_array()
}

/// RoboCAD's `rot` (rows right, up, back in the model frame) of a display
/// camera rotation.
pub fn rot_of(rotation: Quat) -> [[f64; 3]; 3] {
    let q = rotation.as_dquat().normalize();
    [DVec3::X, DVec3::Y, DVec3::Z].map(|axis| d_to_m(q * axis).to_array())
}

/// The display camera rotation of RoboCAD's `rot` (orthonormal, det 1:
/// `ViewState::check`).
pub fn rotation_of(rot: &[[f64; 3]; 3]) -> Quat {
    let [right, up, back] = rot.map(|row| m_to_d(DVec3::from_array(row)));
    bevy::math::DQuat::from_mat3(&DMat3::from_cols(right, up, back)).normalize().as_quat()
}

/// Rounded to 1e-6 (f32 noise off the saved numbers).
fn round6(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

/// RoboCAD's name of a display mode (`DisplayMode`'s serde name).
pub fn mode_name(mode: DisplayMode) -> String {
    serde_json::to_value(mode).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| "shaded_edges".into())
}

/// A display mode by RoboCAD's name.
pub fn mode_of(name: &str) -> Option<DisplayMode> {
    serde_json::from_value(serde_json::Value::String(name.to_string())).ok()
}

/// The view state of the native camera and display (`capture_view`).
/// RoboCAD's yaw and pitch in trackball mode are the nearest turntable
/// heading of the trackball's view direction; the pitch is kept within
/// ±89.5° (`validate_state`).
pub fn capture(cam: &ViewCamera, display: &CadDisplay) -> ViewState {
    let (yaw, pitch) = if cam.trackball {
        let back = d_to_m(cam.rotation.as_dquat().normalize() * DVec3::Z);
        (back.y.atan2(back.x).to_degrees(), back.z.clamp(-1.0, 1.0).asin().to_degrees())
    } else {
        let (y, p) = display_to_robocad(cam.yaw, cam.pitch);
        (f64::from(y), f64::from(p))
    };
    ViewState {
        target: display_to_model(cam.focus).map(round6),
        distance: round6(f64::from(cam.radius) * 1000.0),
        yaw: round6(yaw),
        pitch: round6(pitch).clamp(-VIEW_PITCH_LIMIT, VIEW_PITCH_LIMIT),
        fov: round6(f64::from(cam.fov).to_degrees()),
        orthographic: cam.orthographic,
        mode: if cam.trackball { "trackball" } else { "turntable" }.to_string(),
        rot: rot_of(cam.rotation),
        grid: display.grid,
        display_mode: mode_name(display.mode),
        comment_pins: display.comment_pins,
        section: ViewSection { enabled: display.section.enabled, plane: display.section.plane.map(|p| ViewPlane { origin: p.origin, normal: p.normal, x_axis: p.x_axis }) },
    }
}

/// The native camera state of a view (checked as RoboCAD checks it first),
/// and a note when its field of view was brought into the native range.
/// RoboCAD's restore cuts (no glide).
pub fn camera_of(state: &ViewState) -> Result<(CameraState, Option<String>), String> {
    state.check()?;
    let (yaw, pitch) = robocad_to_display(state.yaw as f32, state.pitch as f32);
    // Checked in f64 by `check`; the camera's f32 must hold it too.
    let radius = (state.distance / 1000.0) as f32;
    if !(radius.is_finite() && radius > 0.0) {
        return Err(format!("the view's distance {} mm does not fit the native camera", state.distance));
    }
    let fov = state.fov.clamp(FOV_RANGE.0, FOV_RANGE.1);
    let note = (fov != state.fov).then(|| format!("the view's field of view {}° is outside the native camera's {}–{}° (RoboCAD's view.fov range); shown at {fov}°", state.fov, FOV_RANGE.0, FOV_RANGE.1));
    let camera = CameraState {
        focus: model_to_display(state.target).to_array(),
        radius,
        yaw,
        pitch,
        orthographic: state.orthographic,
        fov_deg: Some(fov as f32),
        trackball: (state.mode == "trackball").then(|| rotation_of(&state.rot).to_array()),
        seconds: None,
    };
    Ok((camera, note))
}

/// The display settings a view restores (`restore_view`): grid, display
/// mode, comment pins and the section. Checked first; nothing changes on
/// a refusal.
pub fn apply_display(state: &ViewState, display: &mut CadDisplay) -> Result<(), String> {
    state.check()?;
    let mode = mode_of(&state.display_mode).ok_or_else(|| format!("unknown display mode {}", state.display_mode))?;
    let plane = state.section.plane.map(|p| SectionPlane { origin: p.origin, normal: p.normal, x_axis: p.x_axis });
    if display.mode != mode {
        display.mode = mode;
    }
    if display.grid != state.grid {
        display.grid = state.grid;
    }
    if display.comment_pins != state.comment_pins {
        display.comment_pins = state.comment_pins;
    }
    if display.section.enabled != state.section.enabled || display.section.plane != plane {
        display.section.enabled = state.section.enabled;
        display.section.plane = plane;
    }
    Ok(())
}

/// A view's one-line description, as RoboCAD's list shows it
/// ("Orthographic · Cutaway").
pub fn details(state: &ViewState) -> String {
    let mut out = if state.orthographic { "Orthographic" } else { "Perspective" }.to_string();
    if state.section.enabled {
        out.push_str(" · Cutaway");
    }
    out
}

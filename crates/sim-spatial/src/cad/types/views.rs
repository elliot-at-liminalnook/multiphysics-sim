//! Saved views: RoboCAD's `/views` routes (`api.py` `saved_view_request`)
//! in its view-state schema (`saved_views.validate_state`).
//!
//! - `GET /views`: every saved view `{"id", "name", "state"}`.
//! - `POST /views {"name", "state"}` (201): one undo step "Save view". A
//!   headless service needs `state` (409 "Headless: provide view state"
//!   without it), so the client always sends it.
//! - `GET /views/{id}`, `PATCH /views/{id}` with `name` and/or `state` (one
//!   undo step "Update saved view"; 400 "Patch name or state" for anything
//!   else), `DELETE /views/{id}` (undo step "Delete saved view"). An unknown
//!   id is a 404 "Saved view not found"; a refused name or state is a 422
//!   with RoboCAD's `KernelError` text.
//! - `POST /views/{id}/restore` is GUI-only (409 "Restore requires a desktop
//!   window" headless) and moves RoboCAD's own camera; the native viewer
//!   never calls it: it reads the view and applies it to its own camera.
//!
//! [`ViewState`] reads tolerantly (every field defaults to RoboCAD's
//! default, unknown fields are ignored) and writes exactly the schema's
//! twelve keys, since `validate_state` refuses any
//! other field ("Unsupported saved view field").
use serde::{Deserialize, Serialize};

/// The saved view state's keys (`validate_state` refuses any other field).
#[cfg(test)]
pub const VIEW_STATE_KEYS: [&str; 12] = ["target", "distance", "yaw", "pitch", "fov", "orthographic", "mode", "rot", "grid", "display_mode", "comment_pins", "section"];
/// `validate_state`'s display modes (`Viewport.MODES`).
pub const VIEW_DISPLAY_MODES: [&str; 6] = ["shaded", "shaded_edges", "wireframe", "xray", "matcap", "render"];
/// `validate_state`'s pitch range, degrees.
pub const VIEW_PITCH_LIMIT: f64 = 89.5;
/// `SavedViewOps._view_name`: 1–120 characters after stripping.
pub const VIEW_NAME_MAX: usize = 120;

/// One saved view as RoboCAD stores it (`doc.saved_views[id]`).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct SavedView {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub state: ViewState,
    /// Why the view matters (the in-process editor's addition; RoboCAD carries it along).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The parts the view shows alone (empty: the whole model).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// `person` or `agent`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_kind: Option<String>,
}

/// A section plane in a view state (`Plane.to_json`: model mm; unit
/// normal and x axis once RoboCAD has validated it).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct ViewPlane {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
    pub x_axis: [f64; 3],
}

/// A view state's `section`: `{"enabled", "plane"}` (plane null when none).
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct ViewSection {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub plane: Option<ViewPlane>,
}

/// RoboCAD's view state (`validate_state`'s answer): the camera (model
/// frame, Z up, mm and degrees; `rot` rows are the view's right, up and
/// back in the model frame, world → view) and the display settings.
/// Missing fields read as RoboCAD's defaults; unknown fields are ignored.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct ViewState {
    /// The orbit target, mm.
    #[serde(default)]
    pub target: [f64; 3],
    /// Eye distance from the target, mm.
    #[serde(default = "default_distance")]
    pub distance: f64,
    /// Degrees about +Z (0 looks from +X).
    #[serde(default = "default_yaw")]
    pub yaw: f64,
    /// Degrees above the XY plane, within ±89.5.
    #[serde(default = "default_pitch")]
    pub pitch: f64,
    /// Vertical field of view, degrees.
    #[serde(default = "default_fov")]
    pub fov: f64,
    #[serde(default)]
    pub orthographic: bool,
    /// "turntable" or "trackball".
    #[serde(default = "default_mode")]
    pub mode: String,
    /// The trackball rotation: rows right, up, back (model frame).
    #[serde(default = "identity")]
    pub rot: [[f64; 3]; 3],
    #[serde(default = "yes")]
    pub grid: bool,
    /// One of [`VIEW_DISPLAY_MODES`].
    #[serde(default = "default_display_mode")]
    pub display_mode: String,
    #[serde(default = "yes")]
    pub comment_pins: bool,
    #[serde(default)]
    pub section: ViewSection,
}

fn default_distance() -> f64 {
    250.0
}
fn default_yaw() -> f64 {
    -35.0
}
fn default_pitch() -> f64 {
    28.0
}
fn default_fov() -> f64 {
    40.0
}
fn default_mode() -> String {
    "turntable".into()
}
fn identity() -> [[f64; 3]; 3] {
    [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
}
fn yes() -> bool {
    true
}
fn default_display_mode() -> String {
    "shaded_edges".into()
}

impl Default for ViewState {
    /// `validate_state({})`: RoboCAD's camera defaults, grid and pins on,
    /// shaded with edges, no section.
    fn default() -> Self {
        ViewState {
            target: [0.0; 3],
            distance: default_distance(),
            yaw: default_yaw(),
            pitch: default_pitch(),
            fov: default_fov(),
            orthographic: false,
            mode: default_mode(),
            rot: identity(),
            grid: true,
            display_mode: default_display_mode(),
            comment_pins: true,
            section: ViewSection::default(),
        }
    }
}

fn finite(v: &[f64]) -> bool {
    v.iter().all(|x| x.is_finite())
}

impl ViewState {
    /// RoboCAD's `validate_state` (with `annotations.camera_view`) rules,
    /// with its messages, so a state it would refuse is refused before
    /// anything is sent.
    pub fn check(&self) -> Result<(), String> {
        if !finite(&self.target) {
            return Err("camera target needs three finite coordinates".into());
        }
        if !finite(&[self.distance, self.yaw, self.pitch, self.fov]) {
            return Err("camera values must be finite numbers".into());
        }
        if self.distance <= 0.0 || !(1.0..=170.0).contains(&self.fov) {
            return Err("camera distance or field of view is out of range".into());
        }
        if self.mode != "turntable" && self.mode != "trackball" {
            return Err("invalid camera mode".into());
        }
        if !orthonormal(&self.rot) {
            return Err("camera rotation must be an orthonormal 3 by 3 matrix".into());
        }
        if !(-VIEW_PITCH_LIMIT..=VIEW_PITCH_LIMIT).contains(&self.pitch) {
            return Err("View pitch must be between -89.5 and 89.5 degrees".into());
        }
        if !VIEW_DISPLAY_MODES.contains(&self.display_mode.as_str()) {
            return Err("Unknown display mode".into());
        }
        if let Some(p) = &self.section.plane {
            if !finite(&p.origin) || !finite(&p.normal) || !finite(&p.x_axis) {
                return Err("Section plane needs finite 3D vectors".into());
            }
            let unit = |v: [f64; 3]| -> Result<[f64; 3], String> {
                let size = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                if size < 1e-10 { Err("Section axes must be nonzero".to_string()) } else { Ok(v.map(|x| x / size)) }
            };
            let (n, x) = (unit(p.normal)?, unit(p.x_axis)?);
            if (n[0] * x[0] + n[1] * x[1] + n[2] * x[2]).abs() > 1e-6 {
                return Err("Section axes must be perpendicular".into());
            }
        } else if self.section.enabled {
            return Err("An enabled section needs a plane".into());
        }
        Ok(())
    }
}

/// numpy's `allclose(R @ R.T, I, atol=1e-5)` and `isclose(det(R), 1)`
/// (rtol 1e-5, atol 1e-8), as `camera_view` checks `rot`.
fn orthonormal(r: &[[f64; 3]; 3]) -> bool {
    if !r.iter().all(|row| finite(row)) {
        return false;
    }
    for i in 0..3 {
        for j in 0..3 {
            let dot: f64 = (0..3).map(|k| r[i][k] * r[j][k]).sum();
            let want = if i == j { 1.0 } else { 0.0 };
            // allclose: |a - b| <= atol + rtol * |b| (rtol 1e-5 by default).
            if (dot - want).abs() > 1e-5 + 1e-5 * want {
                return false;
            }
        }
    }
    let det = r[0][0] * (r[1][1] * r[2][2] - r[1][2] * r[2][1]) - r[0][1] * (r[1][0] * r[2][2] - r[1][2] * r[2][0]) + r[0][2] * (r[1][0] * r[2][1] - r[1][1] * r[2][0]);
    (det - 1.0).abs() <= 1e-8 + 1e-5
}

/// `SavedViewOps._view_name`: the name stripped, 1–120 characters, else
/// RoboCAD's refusal.
pub fn check_view_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > VIEW_NAME_MAX {
        return Err("View name must contain 1–120 characters".into());
    }
    Ok(name.to_string())
}





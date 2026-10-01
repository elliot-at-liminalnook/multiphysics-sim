//! The one viewer camera (native-viewer.md §2, §3; "Shared camera and CAD
//! views"): every mode's 3D view orbits, pans, zooms, frames, glides and
//! spins through this module, and nothing else in the crate keeps orbit
//! state. A mode spawns its 3D camera with three components and writes
//! nothing but data into them:
//!
//! - [`Orbit`]: where the camera is (focus, radius, yaw, pitch or a
//!   trackball rotation, ortho and field of view), the bounds a fit frames
//!   (`centre`, `extent`), a pending home request, an eased glide and a spin.
//! - [`OrbitRules`]: the mode's feel (drag rate, pitch limit, zoom limits,
//!   framing, zoom to the cursor, RoboCAD's extra gestures) and its
//!   per-frame input gate (`enabled`, `zoom_modifier`, `reduced_motion`,
//!   `keys`, `alt_left`, `typing`), which the mode updates as data; the
//!   module holds no mode code.
//! - [`ViewArea`]: where the 3D view draws (the whole window, between the
//!   mode's docks, or inside a lesson card); the module sets the camera's
//!   viewport from it and starts gestures only inside it.
//!
//! Intents are [`CameraAction`]s (REST `camera_*`, `system_ui`
//! `camera:*`, keys, the CAD view menu, view cube and radial), applied by
//! one system ([`apply::apply`], `ViewerSet::Actions`). Pointer gestures
//! (right-drag orbit, middle or Shift+right-drag pan, wheel zoom) are
//! navigation and stay in SimSync ([`input::navigate`]), as §3 says; they
//! call the same [`Orbit`] methods the actions do.
//!
//! RoboCAD's own gestures (`ui/viewport.py` `mouseMoveEvent`,
//! `keyPressEvent`) are on where a mode's rules set `robocad_gestures`
//! (CAD only; the other modes keep their feel): Shift+middle-drag orbits
//! (instead of panning), Alt held while right-drag orbiting snaps to the
//! nearest axis view (`Camera.snap_orthographic`, applied after every
//! orbit step as RoboCAD does), Alt+left-drag orbits once the pointer has
//! moved past [`input::ALT_DRAG_SLOP`] (a shorter Alt+click stays the
//! mode's: CAD's candidates menu), and the arrow keys orbit 10° (Ctrl or
//! Cmd 90°) and with Shift pan. The keys are `CameraAction`s like the
//! numpad's, so REST can send them too (`camera_orbit {"degrees": …}`,
//! `camera_pan`).
//!
//! Two gate fields keep these from fighting the mode's own tools, each a
//! separate, single-purpose flag rather than `enabled` (which stops every
//! gesture): `typing` (a text field has the keyboard: no camera key is
//! read, numpad or arrow; the pointer gestures still work) and `alt_left`
//! (the mode's left-button owner leaves Alt+left-drag to the camera; false
//! while a tool owns the left button). CAD writes both every frame in
//! SimSync from `CadInputFocus` and its active tool, so the keys (Input)
//! read last frame's value: one frame behind a focus change.
//!
//! The display frame is Bevy's (Y up). RoboCAD's model frame is Z up: CAD
//! and Robot mode hang their models from a root rotated −90° about X
//! (model (x, y, z) → display (x, z, −y)), so the presets
//! ([`ViewPreset`]) use RoboCAD's view table (`ui/viewport.py`
//! `Camera.set_view`) through `yaw = robocad_yaw + 90°`.
//!
//! Place mode's first-person fly camera is [`fly::Fly`] (not orbit state).
mod apply;
pub(crate) mod fly;
mod input;
mod orbit;
mod viewport;
#[cfg(test)]
mod tests;

pub use apply::controls;
pub use orbit::{GLIDE_S, Glide, Pose};
pub(crate) use orbit::view_aspect;

use crate::app::actions::{self, Action, Spec, spec};
use crate::app::{ViewerMode, ViewerSet};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// The modes with an orbit camera (every mode but Place, whose camera flies).
pub const ORBIT_MODES: &[ViewerMode] = &[ViewerMode::Inspect, ViewerMode::Build, ViewerMode::Lessons, ViewerMode::Robot, ViewerMode::Cad, ViewerMode::Phenomena];

/// The orbit camera's state: the one orbit-state component of the crate.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Orbit {
    /// The point orbited around (display frame, metres).
    pub focus: Vec3,
    /// Eye distance from `focus`. 0 means "not placed yet" (a glide cuts).
    pub radius: f32,
    /// Turntable heading about +Y (radians; 0 looks along −Z from +Z).
    pub yaw: f32,
    /// Turntable elevation (radians; positive looks down).
    pub pitch: f32,
    /// Trackball mode: the camera's rotation (RoboCAD's `Camera.mode ==
    /// "trackball"`); None is the turntable (yaw, pitch). A preset returns
    /// to the turntable, as RoboCAD's `set_view` does.
    pub trackball: Option<Quat>,
    /// Orthographic projection (RoboCAD's `Camera.orthographic`); its
    /// visible height is `2 × radius × tan(fov / 2)`, as RoboCAD's.
    pub orthographic: bool,
    /// Vertical field of view, radians (Bevy's default π/4).
    pub fov: f32,
    /// The framed bounds: their centre and half diagonal. Modes write them
    /// when their content changes; zoom limits scale with `extent`.
    pub centre: Vec3,
    pub extent: f32,
    /// Frame `centre`/`extent` with the mode's [`Framing`] on the next step
    /// (a glide when the rules ask for one and the view was placed).
    pub home: bool,
    /// An eased move in progress.
    pub glide: Option<Glide>,
    /// Slow circling, rad/s; any user gesture stops it.
    pub spin: f32,
}

impl Default for Orbit {
    fn default() -> Self {
        Self {
            focus: Vec3::ZERO,
            radius: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            trackball: None,
            orthographic: false,
            fov: std::f32::consts::FRAC_PI_4,
            centre: Vec3::ZERO,
            extent: 0.1,
            home: false,
            glide: None,
            spin: 0.0,
        }
    }
}

/// How a mode limits the zoom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RadiusLimits {
    /// Multiples of `Orbit::extent` (CAD 0.05–40, Robot and the spatial view 0.3–20).
    Extent { min: f32, max: f32 },
    /// Fixed distances (Phenomena: sim-app's 3–30 m).
    Absolute { min: f32, max: f32 },
}

/// What a home request (and `camera_home`) frames.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Framing {
    /// `centre` at `extent × scale` (divided by the view's aspect clamped to
    /// 0.1–1 when `aspect`), from `view` (yaw, pitch) or the current heading.
    Bounds { scale: f32, aspect: bool, view: Option<(f32, f32)> },
    /// A fixed pose (Phenomena: sim-app's camera).
    Fixed(Pose),
}

/// A mode's camera feel and its per-frame input gate. The static fields are
/// set at spawn; the gate fields are data the mode updates each frame.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct OrbitRules {
    /// Orbit drag, radians per pixel (0.007; Phenomena 0.008).
    pub rate: f32,
    /// Pan drag, `radius ×` per pixel (0.0015).
    pub pan_rate: f32,
    /// Turntable pitch limit, radians (CAD 89.5° as RoboCAD; Robot and the
    /// spatial view 1.4; Phenomena 1.35).
    pub pitch_limit: f32,
    pub radius: RadiusLimits,
    pub framing: Framing,
    /// A home request glides (once the view was placed) instead of cutting.
    pub glide_home: bool,
    /// The wheel zooms toward the point under the cursor (RoboCAD's
    /// `Camera.zoom(factor, anchor)`), else toward the focus.
    pub zoom_to_cursor: bool,
    /// A UI node under the pointer owns a gesture that starts there (CAD:
    /// its floating tools sit over the view). Off where the view itself is
    /// inside a UI node (a lesson card).
    pub yield_to_ui: bool,
    // ---- the per-frame gate, written by the mode ----
    /// Gestures are accepted (false while the mode's own tool owns the
    /// pointer, or its keyboard focus is in a text field).
    pub enabled: bool,
    /// The wheel zooms only with Ctrl or Cmd (a lesson page scrolls with it).
    pub zoom_modifier: bool,
    /// Cuts instead of glides, and no spin (the reader's preference).
    pub reduced_motion: bool,
    /// The shared numpad camera keys are read (CAD reads its own keymap).
    pub keys: bool,
    /// RoboCAD's extra gestures and keys (see the module doc): Shift+middle
    /// orbits, Alt+right snaps, Alt+left-drag orbits, the arrow keys
    /// orbit and pan. CAD only; false keeps a mode's feel.
    pub robocad_gestures: bool,
    /// Alt+left-drag may orbit (with `robocad_gestures`): false while the
    /// mode's tool owns the left button (CAD: any tool but Select, or a
    /// catalogue interaction or command surface open).
    pub alt_left: bool,
    /// A text field of the mode has the keyboard: no camera key (numpad
    /// or arrow) is read. Pointer gestures are not affected.
    pub typing: bool,
}

impl Default for OrbitRules {
    fn default() -> Self {
        Self {
            rate: 0.007,
            pan_rate: 0.0015,
            pitch_limit: 1.4,
            radius: RadiusLimits::Extent { min: 0.3, max: 20.0 },
            framing: Framing::Bounds { scale: 3.2, aspect: false, view: None },
            glide_home: false,
            zoom_to_cursor: false,
            yield_to_ui: false,
            enabled: true,
            zoom_modifier: false,
            reduced_motion: false,
            keys: true,
            robocad_gestures: false,
            alt_left: true,
            typing: false,
        }
    }
}

/// Where a mode's 3D view draws (logical pixels unless noted). The module
/// writes the camera's `viewport` (and `sub_camera_view` for a card) from
/// it, and starts gestures only inside it.
#[derive(Component, Clone, Copy, Debug, PartialEq, Default)]
pub enum ViewArea {
    /// The whole window.
    #[default]
    Window,
    /// Between the mode's docks; the whole window when they leave no room.
    Docks { left: f32, right: f32, top: f32, bottom: f32 },
    /// Inside a lesson card: the card's whole rectangle and the part of it
    /// shown (scrolled), physical pixels; nothing is drawn when the visible
    /// part is empty.
    Card { full: Rect, visible: Rect },
}

/// RoboCAD's named views (`Camera.set_view`'s table).
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ViewPreset {
    Front,
    Back,
    Top,
    Bottom,
    Right,
    Left,
    Iso,
}

impl ViewPreset {
    pub const ALL: [ViewPreset; 7] = [ViewPreset::Front, ViewPreset::Back, ViewPreset::Top, ViewPreset::Bottom, ViewPreset::Right, ViewPreset::Left, ViewPreset::Iso];
    pub fn name(self) -> &'static str {
        match self {
            ViewPreset::Front => "front",
            ViewPreset::Back => "back",
            ViewPreset::Top => "top",
            ViewPreset::Bottom => "bottom",
            ViewPreset::Right => "right",
            ViewPreset::Left => "left",
            ViewPreset::Iso => "iso",
        }
    }
    /// RoboCAD's (yaw, pitch) in degrees, its model frame (Z up; yaw about +Z).
    pub fn robocad_degrees(self) -> (f32, f32) {
        match self {
            ViewPreset::Front => (-90.0, 0.0),
            ViewPreset::Back => (90.0, 0.0),
            ViewPreset::Right => (0.0, 0.0),
            ViewPreset::Left => (180.0, 0.0),
            ViewPreset::Top => (-90.0, 89.5),
            ViewPreset::Bottom => (-90.0, -89.5),
            ViewPreset::Iso => (-35.0, 28.0),
        }
    }
    /// The turntable (yaw, pitch) in the display frame, radians.
    pub fn yaw_pitch(self) -> (f32, f32) {
        let (yaw, pitch) = self.robocad_degrees();
        robocad_to_display(yaw, pitch)
    }
}

/// RoboCAD's turntable angles (degrees, Z up) as the display frame's
/// (radians, Y up): RoboCAD's eye direction `(cos p cos y, cos p sin y,
/// sin p)` maps to display `(cos p cos y, sin p, −cos p sin y)`, which is
/// the display turntable's `(sin Y cos p, sin p, cos Y cos p)` at
/// `Y = y + 90°`.
pub fn robocad_to_display(yaw_deg: f32, pitch_deg: f32) -> (f32, f32) {
    (orbit::wrap((yaw_deg + 90.0).to_radians()), pitch_deg.to_radians())
}

/// The inverse of [`robocad_to_display`] (degrees, yaw in (−180, 180]).
pub fn display_to_robocad(yaw: f32, pitch: f32) -> (f32, f32) {
    let mut y = yaw.to_degrees() - 90.0;
    while y <= -180.0 {
        y += 360.0;
    }
    while y > 180.0 {
        y -= 360.0;
    }
    (y, pitch.to_degrees())
}

/// RoboCAD's orbit modes.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OrbitMode {
    Turntable,
    Trackball,
}

/// A whole camera state (`camera_set`, a saved view's restore), display
/// frame: metres and radians.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CameraState {
    pub focus: [f32; 3],
    pub radius: f32,
    pub yaw: f32,
    pub pitch: f32,
    #[serde(default)]
    pub orthographic: bool,
    /// Vertical field of view, degrees (RoboCAD's unit); absent keeps it.
    #[serde(default)]
    pub fov_deg: Option<f32>,
    /// Trackball rotation as a quaternion `[x, y, z, w]`; absent is the turntable.
    #[serde(default)]
    pub trackball: Option<[f32; 4]>,
    /// Ease there over this many seconds (absent or 0: cut).
    #[serde(default)]
    pub seconds: Option<f32>,
}

/// Every camera intent (§3). REST commands keep the JSON form
/// `{"command": "camera_…", …}`; `system_ui` activations of `camera:*`
/// controls parse to the same values ([`Action::parse`]).
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "command", deny_unknown_fields)]
pub enum CameraAction {
    /// A RoboCAD named view (turntable; the distance and focus are kept).
    #[serde(rename = "camera_view")]
    View { view: ViewPreset },
    /// RoboCAD's `opposite`: yaw + 180°, pitch negated.
    #[serde(rename = "camera_opposite")]
    Opposite,
    /// Orthographic on or off; absent toggles (RoboCAD's `view.ortho`).
    #[serde(rename = "camera_projection")]
    Projection {
        #[serde(default)]
        orthographic: Option<bool>,
    },
    /// Vertical field of view in degrees (RoboCAD's `view.fov`, 5–120).
    #[serde(rename = "camera_fov")]
    Fov { degrees: f32 },
    /// Frame the mode's bounds from the current heading.
    #[serde(rename = "camera_fit")]
    Fit,
    /// The mode's home framing (its fixed view, glide and all).
    #[serde(rename = "camera_home")]
    Home,
    /// Pan by a drag of `dx`, `dy` window pixels (as a middle-drag).
    #[serde(rename = "camera_pan")]
    Pan { dx: f32, dy: f32 },
    /// Zoom by `factor` (< 1 closer; as the wheel's `exp(-0.12 × lines)`),
    /// toward window pixel `at` when given (the point under it stays put),
    /// else toward the focus.
    #[serde(rename = "camera_zoom")]
    Zoom {
        factor: f32,
        #[serde(default)]
        at: Option<[f32; 2]>,
    },
    /// Orbit by a drag of `dx`, `dy` window pixels (as a right-drag; the
    /// trackball when it is on), or by `degrees` `[yaw, pitch]` (RoboCAD's
    /// arrow keys: positive yaw turns the view as a drag to the left,
    /// positive pitch looks further down). Absent fields are 0.
    #[serde(rename = "camera_orbit")]
    Orbit {
        #[serde(default)]
        dx: f32,
        #[serde(default)]
        dy: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        degrees: Option<[f32; 2]>,
    },
    /// Turntable or trackball; absent toggles (RoboCAD's `view.orbit_mode`).
    #[serde(rename = "camera_orbit_mode")]
    OrbitMode {
        #[serde(default)]
        mode: Option<OrbitMode>,
    },
    /// Circle at `rate` rad/s (0 stops).
    #[serde(rename = "camera_spin")]
    Spin { rate: f32 },
    /// Set the whole camera state (a saved view's restore).
    #[serde(rename = "camera_set")]
    Set { state: CameraState },
    /// The camera as it is now.
    #[serde(rename = "camera_state")]
    State,
}

impl Action for CameraAction {
    fn commands() -> Vec<Spec> {
        let m = ORBIT_MODES;
        vec![
            spec("camera_view", m, json!({"view": "iso"}), "Every orbit mode (inspect, build, lessons, robot, cad, phenomena): one of RoboCAD's named views, front | back | top | bottom | right | left | iso (RoboCAD's yaw/pitch table, Z-up model frame; turntable; focus and distance kept). Display only."),
            spec("camera_opposite", m, json!({}), "Every orbit mode: look from the opposite side (RoboCAD's opposite: yaw + 180°, pitch negated). Display only."),
            spec("camera_projection", m, json!({"orthographic": true}), "Every orbit mode: orthographic on (true) or off (false); without orthographic it toggles. The orthographic height is 2 × distance × tan(fov/2), as RoboCAD's. Display only."),
            spec("camera_fov", m, json!({"degrees": 40}), "Every orbit mode: the vertical field of view in degrees, 5–120 as RoboCAD's view.fov (refused outside). Display only."),
            spec("camera_fit", m, json!({}), "Every orbit mode: frame the mode's content from the current heading, keeping the trackball when it is on (CAD: every drawn body, kept current as meshes arrive; cad_fit frames one node). Display only."),
            spec("camera_home", m, json!({}), "Every orbit mode: the mode's home framing (the spatial view's overview glides from its fixed direction; Phenomena returns to sim-app's camera; CAD and Robot frame their content from the current heading, keeping the trackball). Display only."),
            spec("camera_pan", m, json!({"dx": 40, "dy": 0}), "Every orbit mode: pan as a middle-drag of dx, dy window pixels. Display only."),
            spec("camera_zoom", m, json!({"factor": 0.8, "at": [640, 360]}), "Every orbit mode: zoom by factor (< 1 closer; within the mode's limits), toward window pixel at when given (the point under it stays under it), else toward the focus. Display only."),
            spec("camera_orbit", m, json!({"dx": 30, "dy": 0}), "Every orbit mode: orbit as a right-drag of dx, dy window pixels (turntable, or the trackball when it is on), or by degrees [yaw, pitch] instead (RoboCAD's arrow keys: [10, 0] is the right arrow; not both). Display only."),
            spec("camera_orbit_mode", m, json!({"mode": "trackball"}), "Every orbit mode: turntable or trackball (RoboCAD's orbit modes); without mode it toggles. A named view returns to the turntable. Display only."),
            spec("camera_spin", m, json!({"rate": 0.2}), "Every orbit mode: circle about the focus at rate rad/s (0 stops); any drag or zoom stops it. Display only."),
            spec("camera_set", m, json!({"state": {"focus": [0, 0, 0], "radius": 0.5, "yaw": 0.7, "pitch": 0.45, "orthographic": false, "fov_deg": 40}}), "Every orbit mode: set the whole camera (display frame, Y up, metres and radians; fov_deg degrees; trackball [x, y, z, w] or absent; seconds to glide, absent to cut). CAD's saved views restore through this. Display only."),
            spec("camera_state", m, json!({}), "Every orbit mode: the camera now: focus, radius, yaw, pitch, trackball, orthographic, fov_deg, centre, extent, gliding, spin, the RoboCAD yaw/pitch it corresponds to, and the view area."),
        ]
    }
    fn parse(command: &sim_api::Command) -> Result<Self, String> {
        if command.command == "system_ui" {
            let id = command.args["action"]["id"].as_str().ok_or("system_ui activate needs a camera:* control id")?;
            return apply::control_action(id).ok_or_else(|| format!("unknown camera control {id}; request controls"));
        }
        sim_api::decode::<Self>(command)
    }
    fn controls() -> &'static [&'static str] {
        &["camera:view:<view>", "camera:opposite", "camera:projection", "camera:fit", "camera:home", "camera:orbit_mode"]
    }
}

/// A `system_ui` activation of a `camera:*` control (routed to
/// [`CameraAction`] in every orbit mode by `app::route`).
pub fn is_camera_control(command: &sim_api::Command) -> bool {
    let action = &command.args["action"];
    command.command == "system_ui" && action["operation"] == "activate" && action["id"].as_str().is_some_and(|id| id.starts_with("camera:"))
}

/// The camera as JSON (`camera_state`, and the modes' `state`).
pub fn state_json(orbit: &Orbit, area: Option<&ViewArea>) -> Value {
    // The heading the view has now (the trackball's, when it is on).
    let (ty, tp) = orbit.turntable();
    let (ry, rp) = display_to_robocad(ty, tp);
    let mut out = Map::new();
    out.insert("focus".into(), json!(orbit.focus.to_array()));
    out.insert("radius".into(), json!(orbit.radius));
    out.insert("yaw".into(), json!(orbit.yaw));
    out.insert("pitch".into(), json!(orbit.pitch));
    out.insert("trackball".into(), json!(orbit.trackball.map(|q| q.to_array())));
    out.insert("orthographic".into(), json!(orbit.orthographic));
    out.insert("fov_deg".into(), json!(orbit.fov.to_degrees()));
    out.insert("centre".into(), json!(orbit.centre.to_array()));
    out.insert("extent".into(), json!(orbit.extent));
    out.insert("home_pending".into(), json!(orbit.home));
    out.insert("gliding".into(), json!(orbit.glide.is_some()));
    out.insert("spin".into(), json!(orbit.spin));
    out.insert("robocad".into(), json!({"yaw": ry, "pitch": rp}));
    out.insert("area".into(), json!(area.map(|a| format!("{a:?}"))));
    Value::Object(out)
}

/// Ordered within SimSync: the viewport first (gestures test the area it
/// sets), then the gestures, then the glide/spin step and the transform.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CameraSet {
    Viewport,
    Navigate,
    Place,
}

/// The shared camera: `CameraAction` registered once, its apply system in
/// Actions, the numpad keys in Input, and in SimSync the viewport, the
/// gestures and the step that places the camera. A mode orders its own
/// SimSync work against [`CameraSet`] (for example CAD's view snapshot runs
/// after `CameraSet::Place`).
pub struct CameraPlugin;
impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        actions::register::<CameraAction>(app);
        app.configure_sets(Update, (CameraSet::Viewport, CameraSet::Navigate, CameraSet::Place).chain().in_set(ViewerSet::SimSync))
            .add_systems(Update, input::keys.after(actions::serve).in_set(ViewerSet::Input))
            .add_systems(Update, apply::apply.in_set(ViewerSet::Actions))
            .add_systems(Update, viewport::viewport.in_set(CameraSet::Viewport))
            .add_systems(Update, input::navigate.in_set(CameraSet::Navigate))
            .add_systems(Update, (orbit::place, fly::fly).in_set(CameraSet::Place));
    }
}

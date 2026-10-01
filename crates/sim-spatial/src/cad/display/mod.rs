//! CAD mode's display state (cad-views-export): one [`CadDisplay`]
//! resource holding RoboCAD's display mode, grid, build plate, view cube,
//! high contrast and the section tool. Display only: nothing here writes
//! geometry or sends an edit; drawing is in Present.
//!
//! STUB (settled by the lead): part D fills in the types' fields, the
//! handler, the drawing and the section job.
use crate::app::actions::Call;
use crate::cad::actions::{CadAction, Cx};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use sim_api::Outcome;

/// RoboCAD's display modes (`ui/viewport.py` `display_mode`; the
/// view-state schema's `display_mode`), in its `view.mode_next` order.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    Shaded,
    #[default]
    ShadedEdges,
    Wireframe,
    Xray,
    Matcap,
    Render,
}

/// A section plane as RoboCAD writes it (`Plane.to_json`: model mm, unit
/// normal and x axis).
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SectionPlane {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
    pub x_axis: [f64; 3],
}

/// The section tool's state (the view-state schema's `section`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Section {
    pub enabled: bool,
    pub plane: Option<SectionPlane>,
}

/// The display state (display only): RoboCAD's defaults.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct CadDisplay {
    pub mode: DisplayMode,
    pub grid: bool,
    pub build_plate: bool,
    pub high_contrast: bool,
    pub view_cube: bool,
    /// The view-state schema's `comment_pins` (pins are drawn by cad-organize).
    pub comment_pins: bool,
    pub section: Section,
}
impl Default for CadDisplay {
    fn default() -> Self {
        Self { mode: DisplayMode::ShadedEdges, grid: true, build_plate: false, high_contrast: false, view_cube: true, comment_pins: true, section: Section::default() }
    }
}

/// `cad_display`'s arguments: each given setting is set; `next` cycles the
/// display mode (RoboCAD's `view.mode_next`, Z).
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct DisplayArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<DisplayMode>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub next: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_plate: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high_contrast: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_cube: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_pins: Option<bool>,
}

/// `cad_section`'s arguments (part D may add fields; these names stay).
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct SectionArgs {
    /// On, off; absent with nothing else toggles (RoboCAD's Ctrl+Shift+X).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane: Option<SectionPlane>,
}

/// `CadDisplay` and `CadSection`, from any entry point.
pub(in crate::cad) fn handle(_action: &CadAction, _call: &mut Call, _cx: &mut Cx) -> Outcome {
    Outcome::Done(Err("not implemented yet".into()))
}

/// CadPlugin: this part's systems and resources (inserted on entering CAD
/// mode; removed by `cad::clear`).
pub(in crate::cad) fn build(app: &mut App) {
    let _ = app;
}

/// This part's REST commands (appended to `CadAction::commands`).
pub(in crate::cad) fn specs() -> Vec<crate::app::actions::Spec> {
    Vec::new()
}

/// This part's `system_ui` controls: (id, label, action, ready).
pub(in crate::cad) fn controls(cx: &Cx) -> Vec<(String, String, CadAction, Result<(), String>)> {
    let _ = cx;
    Vec::new()
}

//! File workflows (cad-views-export): new, open, save as and import with
//! units, export in every RoboCAD format and the drawing, and render, each
//! on a job with progress and a named refusal; RoboCAD's unsaved-edit rule
//! before new or open.
//!
//! STUB (settled by the lead): part E fills it in.
use crate::app::actions::Call;
use crate::cad::actions::{CadAction, Cx};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use sim_api::Outcome;

/// The file forms, exports and renders in flight.
#[derive(Resource, Default)]
pub struct CadFiles {}

/// `cad_file`'s arguments.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct FileArgs {}

/// `cad_export`'s arguments.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct ExportArgs {}

/// `cad_render`'s arguments.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct RenderArgs {}

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

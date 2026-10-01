//! Saved views (cad-views-export): RoboCAD's `/views` in its view-state
//! schema (`saved_views.validate_state`), listed, saved, renamed,
//! replaced, deleted and restored onto the native camera.
//!
//! STUB (settled by the lead): part D fills it in.
use crate::app::actions::Call;
use crate::cad::actions::{CadAction, Cx};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use sim_api::Outcome;

/// Saved views as last listed, and the jobs in flight.
#[derive(Resource, Default)]
pub struct CadViews {}

/// `cad_views`' arguments.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct ViewsArgs {}

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

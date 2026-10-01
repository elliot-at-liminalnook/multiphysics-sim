//! P2: the active plane's systems (see `super` and the P2 brief).
use super::{CadActivePlane, PlaneMode, ViewAct};
use crate::cad::document::CadDocument;
use bevy::prelude::*;
use serde_json::Value;

/// The window's plane systems: the plane tools' picks, the header line.
pub(in crate::cad) fn build(app: &mut App) {
    let _ = app;
}

/// SimSync (core, after `cache::sync`): reset on a new generation, adopt a
/// plane tool's new node, fill and drop node frames, follow a selected plane node.
pub(in crate::cad) fn sync(doc: Option<ResMut<CadDocument>>, plane: Option<ResMut<CadActivePlane>>, sketches: Option<Res<super::CadSketches>>) {
    let _ = (doc, plane, sketches);
}

/// `Flow::View`: set the active plane or toggle 2D snapping; the answer.
pub(in crate::cad) fn view_act(doc: &mut CadDocument, plane: &mut CadActivePlane, act: ViewAct) -> Value {
    let _ = (doc, plane, act);
    Value::Null
}

/// `Flow::PlanePick` starts: RoboCAD's `PlaneTool.activate`.
pub(in crate::cad) fn begin(doc: &mut CadDocument, mode: PlaneMode) -> Result<(), String> {
    let _ = (doc, mode);
    Ok(())
}

/// `cad_state.plane`.
pub(in crate::cad) fn state_json(doc: &CadDocument, plane: &CadActivePlane) -> Value {
    let _ = (doc, plane);
    Value::Null
}

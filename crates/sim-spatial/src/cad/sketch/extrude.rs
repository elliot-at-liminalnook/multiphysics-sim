//! P4: RoboCAD's `ExtrudeTool` (extrude and revolve).
use crate::cad::document::CadDocument;
use crate::cad::ops::{Built, Env, OpEntry, Resolved};
use bevy::prelude::*;
use serde_json::{Map, Value};

/// The extrude or revolve tool's state (`CadDocument::ops.extrude`).
#[derive(Clone, Debug, PartialEq)]
pub struct ExtrudeState {
    pub revolve: bool,
}

pub(in crate::cad) fn build(app: &mut App) {
    let _ = app;
}

/// `Flow::Extrude` starts: RoboCAD's `ExtrudeTool.activate`.
pub(in crate::cad) fn begin(doc: &mut CadDocument, env: &Env, revolve: bool) -> Result<(), String> {
    let _ = env;
    doc.ops.extrude = Some(ExtrudeState { revolve });
    Ok(())
}

/// `Shape::Extrude`.
pub(crate) fn calls(entry: &OpEntry, revolve: bool, r: &Resolved, values: &Map<String, Value>, doc: &CadDocument, env: &Env) -> Result<Built, String> {
    let _ = (entry, revolve, r, values, doc, env);
    Err("P4".into())
}

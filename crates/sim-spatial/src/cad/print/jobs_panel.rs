//! The Print jobs section of the right dock (part D: stub).
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::ui_kit::Kit;
use bevy::prelude::*;
use serde_json::Value;

pub(super) fn show(_doc: &mut CadDocument, _open: Option<bool>) -> Result<Value, String> {
    Err("not yet".into())
}
pub(super) fn state_json(_doc: &CadDocument) -> Value {
    Value::Null
}
pub(super) fn controls(_doc: &CadDocument) -> Vec<(String, String, CadAction, Result<(), String>)> {
    Vec::new()
}
/// The right dock's Print jobs section (`panel::Part::Print`).
pub(in crate::cad) fn draw(_p: &mut ChildSpawnerCommands, _k: &Kit, _doc: &CadDocument) {}
/// What the section shows, as a comparable text.
pub(in crate::cad) fn key(_doc: &CadDocument) -> String {
    String::new()
}
pub(super) fn build(_app: &mut App) {}

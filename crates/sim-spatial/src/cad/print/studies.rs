//! The print studies (part D: stub).
use super::PrintCall;
use crate::app::actions::Call;
use crate::cad::document::CadDocument;
use crate::cad::ops::{Env, OpEntry, Resolved};
use bevy::prelude::*;
use serde_json::{Map, Value};
use sim_api::Outcome;
use sim_runtime::cad_client::SelectionItem;

/// The studies' state on the document.
#[derive(Default)]
pub(crate) struct StudiesState {}

/// A study run's start.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StudyPlan {}

pub(super) fn build(_call: PrintCall, _entry: &OpEntry, _r: &Resolved, _values: &Map<String, Value>, _doc: &CadDocument, _env: &Env) -> Result<StudyPlan, String> {
    Err("not yet".into())
}
pub(super) fn send(_doc: &mut CadDocument, _call: &mut Call, _plan: StudyPlan) -> Outcome {
    Outcome::Done(Err("not yet".into()))
}
pub(super) fn picks(_source: &str, _doc: &CadDocument) -> Vec<(String, String)> {
    Vec::new()
}
pub(super) fn seed(_entry: &OpEntry, _doc: &CadDocument, _env: &Env, _texts: &mut [String]) {}
pub(super) fn precheck(_entry: &OpEntry, _doc: &CadDocument, _selection: &[SelectionItem]) -> Option<String> {
    None
}
pub(super) fn state_json(_doc: &CadDocument) -> Value {
    Value::Null
}
pub(super) fn build_core(_app: &mut App) {}

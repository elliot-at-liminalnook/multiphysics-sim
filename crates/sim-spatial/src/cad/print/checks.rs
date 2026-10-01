//! "Wall thickness check…" and "Validate for printing" (part B: stub).
use super::PrintCall;
use crate::app::actions::Call;
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::cad::ops::{Env, OpEntry, Resolved};
use bevy::prelude::*;
use serde_json::{Map, Value};
use sim_api::Outcome;

/// The checks' state on the document.
#[derive(Default)]
pub(crate) struct ChecksState {}

/// A check run's inputs.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CheckPlan {}

pub(super) fn build(_call: PrintCall, _entry: &OpEntry, _r: &Resolved, _values: &Map<String, Value>, _doc: &CadDocument, _env: &Env) -> Result<CheckPlan, String> {
    Err("not yet".into())
}
pub(super) fn send(_doc: &mut CadDocument, _call: &mut Call, _plan: CheckPlan) -> Outcome {
    Outcome::Done(Err("not yet".into()))
}
pub(super) fn seed(_entry: &OpEntry, _doc: &CadDocument, _texts: &mut [String]) {}
pub(super) fn clear(_doc: &mut CadDocument) -> Value {
    Value::Null
}
pub(super) fn state_json(_doc: &CadDocument) -> Value {
    Value::Null
}
pub(super) fn controls(_doc: &CadDocument) -> Vec<(String, String, CadAction, Result<(), String>)> {
    Vec::new()
}
pub(super) fn build_core(_app: &mut App) {}

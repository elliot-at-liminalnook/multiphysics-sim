//! "Fastener hole…" and "Clearance offset…" (part C: stub).
use super::PrintCall;
use crate::app::actions::Call;
use crate::cad::document::CadDocument;
use crate::cad::ops::{Env, OpEntry, Resolved};
use serde_json::{Map, Value};
use sim_api::Outcome;

/// The edits' state on the document.
#[derive(Default)]
pub(crate) struct EditsState {}
impl EditsState {
    pub(crate) fn reset_picks(&mut self) {}
}

/// An edit run's call.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EditPlan {}

pub(super) fn build(_call: PrintCall, _entry: &OpEntry, _r: &Resolved, _values: &Map<String, Value>, _doc: &CadDocument, _env: &Env) -> Result<EditPlan, String> {
    Err("not yet".into())
}
pub(super) fn send(_doc: &mut CadDocument, _call: &mut Call, _plan: EditPlan) -> Outcome {
    Outcome::Done(Err("not yet".into()))
}
pub(super) fn seed(_entry: &OpEntry, _doc: &CadDocument, _env: &Env, _texts: &mut [String]) {}
pub(super) fn state_json(_doc: &CadDocument) -> Value {
    Value::Null
}

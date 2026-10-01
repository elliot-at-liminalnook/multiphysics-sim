//! The one poller of RoboCAD's print jobs (part D: stub).
use crate::app::actions::Call;
use crate::cad::document::CadDocument;
use bevy::prelude::*;
use serde_json::Value;
use sim_api::Outcome;

/// RoboCAD's print jobs as last polled, and the poll in flight.
#[derive(Default)]
pub struct PrintJobTracker {}
impl PrintJobTracker {
    /// Why leaving CAD mode would drop a job this window started.
    pub(crate) fn blockers(&self) -> Vec<String> {
        Vec::new()
    }
}

pub(super) fn edit_answered(_doc: &mut CadDocument, _seq: u64, _result: Option<&Value>) {}
pub(super) fn cancel(_doc: &mut CadDocument, _call: &mut Call, _job: Option<&str>, _confirm: Option<bool>) -> Outcome {
    Outcome::Done(Err("not yet".into()))
}
pub(super) fn state_json(_doc: &CadDocument) -> Value {
    Value::Null
}
pub(super) fn build_core(_app: &mut App) {}

//! P3: sketch edits and the `cad_sketch` action.
use super::SketchEdit;
use crate::app::actions::Call;
use crate::cad::actions::Cx;
use crate::cad::document::CadDocument;
use crate::cad::ops::{Built, Env, OpEntry, Resolved};
use serde_json::{Map, Value};
use sim_api::Outcome;

/// `Shape::SketchEdit`.
pub(crate) fn calls(entry: &OpEntry, edit: SketchEdit, r: &Resolved, values: &Map<String, Value>, doc: &CadDocument, env: &Env) -> Result<Built, String> {
    let _ = (entry, edit, r, values, doc, env);
    Err("P3".into())
}

/// `CadAction::CadSketch`.
pub(in crate::cad) fn sketch_action(node: Option<&str>, plane: Option<&str>, calls: &[Value], revision: Option<u64>, call: &mut Call, cx: &mut Cx) -> Outcome {
    let _ = (node, plane, calls, revision, call, cx);
    Outcome::Done(Err("P3".into()))
}

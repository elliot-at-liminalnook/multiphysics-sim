//! One guarded source-edit path, with explicit nongeometry auxiliary scope.
use super::{
    document::{CadDocument, EditDone},
};
use crate::app::actions::Call;
use serde_json::{Value, json};
use sim_api::Outcome;
use super::local::Workspace;
/// Start a mutating request; a REST caller waits for its answer. The one
/// path every document edit takes (the tools' commits too).
pub(super) fn edit(
    doc: &mut CadDocument,
    call: &mut Call,
    label: String,
    work: impl FnOnce(&mut Workspace) -> Result<EditDone, sim_runtime::cad_client::CadError>
    + Send
    + 'static,
) -> Outcome {
    edit_scoped(doc, call, label, false, work)
}

fn edit_scoped(
    doc: &mut CadDocument,
    call: &mut Call,
    label: String,
    auxiliary: bool,
    work: impl FnOnce(&mut Workspace) -> Result<EditDone, sim_runtime::cad_client::CadError>
    + Send
    + 'static,
) -> Outcome {
    match super::local::start(doc, label, call.rest(), auxiliary, move |ws| work(ws).map_err(|e| e.to_string())) {
        Err(e) => Outcome::Done(Err(e)),
        Ok(seq) if call.rest() => {
            *call.continuation = json!({"edit": seq, "generation": doc.generation});
            Outcome::Pending
        }
        Ok(_) => Outcome::Done(Ok(Value::Null)),
    }
}

/// One edit through [`edit`], refused by name with nothing sent when
/// `CadDocument::commit_refusal(began)` names a reason: an edit in flight,
/// not connected, the shown document behind RoboCAD's, or RoboCAD's
/// revision changed since `began` (the revision a form opened, a row was
/// read or a pick was made at; None checks only the first three). The one
/// helper cad-physical-inspect's panels, forms and inspector rows commit through.
pub(super) fn edit_at(
    doc: &mut CadDocument,
    call: &mut Call,
    began: Option<u64>,
    label: String,
    work: impl FnOnce(&mut Workspace) -> Result<EditDone, sim_runtime::cad_client::CadError>
    + Send
    + 'static,
) -> Outcome {
    if let Some(why) = doc.commit_refusal(began) {
        return Outcome::Done(Err(why));
    }
    edit(doc, call, label, work)
}

/// Only annotation metadata and source-owned named programs may commit while
/// a display-only preview is active. All document/revision/busy guards hold.
pub(super) fn edit_auxiliary_at(
    doc: &mut CadDocument,
    call: &mut Call,
    began: Option<u64>,
    label: String,
    work: impl FnOnce(&mut Workspace) -> Result<EditDone, sim_runtime::cad_client::CadError>
    + Send
    + 'static,
) -> Outcome {
    if let Some(why) = doc.commit_refusal_for(began, true) {
        return Outcome::Done(Err(why));
    }
    edit_scoped(doc, call, label, true, work)
}

/// An in-process edit of the open archive (`local::start`), refused by name
/// with nothing changed when `CadDocument::commit_refusal_for(began,
/// auxiliary)` names a reason. A REST caller waits for its answer
/// (`{message, result, revision}`); a click gets the status line.
pub(crate) fn local_edit_at(
    doc: &mut CadDocument,
    call: &mut Call,
    began: Option<u64>,
    label: String,
    auxiliary: bool,
    work: impl FnOnce(&mut super::local::Workspace) -> Result<EditDone, String> + Send + 'static,
) -> Outcome {
    if let Some(why) = doc.commit_refusal_for(began, auxiliary) {
        return Outcome::Done(Err(why));
    }
    match super::local::start(doc, label, call.rest(), auxiliary, work) {
        Err(e) => Outcome::Done(Err(e)),
        Ok(seq) if call.rest() => {
            *call.continuation = json!({"edit": seq, "generation": doc.generation});
            Outcome::Pending
        }
        Ok(_) => Outcome::Done(Ok(Value::Null)),
    }
}

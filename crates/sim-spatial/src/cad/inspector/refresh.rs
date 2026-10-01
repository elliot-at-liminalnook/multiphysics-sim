//! The physical model kept at the shown revision while something shows
//! it: RoboCAD's `GET /physical?flex=0` answers for one revision, so every
//! edit leaves it behind, and the joint physics rows ([`super::rows`]) and
//! the material properties dialog's defaults (`materials::form`) read it
//! only at the shown revision. While a joint is the inspected node (the
//! first selected node) or a material properties dialog is open at the
//! shown revision, [`sync`] fetches it again (`sync::fetch_physical`, one
//! Dedicated job) when it is missing or answers for another revision, no
//! fetch runs, no edit is in flight (the revision is about to move) and
//! the window is connected, once the revision has stayed put for half a second (RoboCAD derives the whole model for it): at most once per (document generation,
//! revision), so a failed fetch is shown, not retried in a loop (the rows'
//! "Fetch physical model" retries it). The rows show "Fetching RoboCAD's
//! physical model…" meanwhile. When it lands, an open properties dialog
//! takes the defaults it lacked (`materials::refilled_form`).
use crate::cad::document::CadDocument;
use crate::cad::selection::{CadItems, CadSelection};
use bevy::prelude::*;
use sim_runtime::cad_client::SelectionItem;

/// How long the shown revision must stay put before the model is fetched again.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(500);

/// Whether something shown reads the physical model now.
pub(crate) fn wanted(doc: &CadDocument, selection: &[SelectionItem]) -> bool {
    let joint = selection.first_node().and_then(|id| super::node(doc, id)).is_some_and(|n| n.kind == "joint");
    joint || crate::cad::materials::wants_physical(doc)
}

/// The (generation, revision) to fetch the physical model for now (None:
/// nothing to fetch); `requested` is the last one fetched from here.
pub(crate) fn due(doc: &CadDocument, selection: &[SelectionItem], requested: Option<(u64, u64)>) -> Option<(u64, u64)> {
    if doc.physical_job.is_some() || doc.edit.is_some() || !doc.connected() || !wanted(doc, selection) {
        return None;
    }
    let revision = super::current_revision(doc)?;
    if doc.physical.as_ref().is_some_and(|(r, _)| *r == revision) {
        return None;
    }
    let key = (doc.generation, revision);
    (requested != Some(key)).then_some(key)
}

/// JobResults (after `sync::receive`, so the shown revision and a landed
/// fetch are this frame's): see the module doc.
pub(crate) fn sync(doc: Option<ResMut<CadDocument>>, selection: CadSelection, mut requested: Local<Option<(u64, u64)>>, mut seen: Local<Option<((u64, u64), std::time::Instant)>>) {
    let Some(mut doc) = doc else { return };
    let due = due(&doc, &selection.items(), *requested);
    // Debounced: RoboCAD derives the whole model for this read (holding its
    // request thread), so it is asked only once the revision has stayed
    // put for SETTLE, not after every edit of a drag or a typing run.
    let settled = match (due, *seen) {
        (Some(key), Some((k, since))) if k == key => since.elapsed() >= SETTLE,
        (Some(key), _) => {
            *seen = Some((key, std::time::Instant::now()));
            false
        }
        (None, _) => {
            *seen = None;
            false
        }
    };
    if let Some(key) = due.filter(|_| settled) {
        // Remembered before the call: an error is shown once, not retried each frame.
        *requested = Some(key);
        if let Err(e) = crate::cad::sync::fetch_physical(&mut doc) {
            doc.physical = Some((key.1, Err(e)));
            doc.touch();
        }
    }
    // Read first: the document is only written (and marked changed) when the dialog changes.
    if let Some(form) = crate::cad::materials::refilled_form(&doc) {
        doc.materials.form = Some(form);
        doc.touch();
    }
}

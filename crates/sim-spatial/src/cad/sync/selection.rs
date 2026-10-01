//! The selection's half of the sync: RoboCAD's `/selection` adopted from
//! the poll (items of every kind, and a desktop window's mode), our pushes
//! (`PUT /selection` with the mode, one at a time, newest wins), and the
//! inspected node's `GET /nodes/{id}`.
use crate::cad::document::{CadDocument, Connection, SelectMode};
use crate::cad::selection::{CadItems, Shared, View};
use crate::jobs::{Job, Pool};
use sim_runtime::cad_client::{Selection, SelectionItem};
use std::time::Instant;

/// RoboCAD's selection as read by the poll (`sent`: when the read was
/// sent), adopted when it changed there: its items (every kind) when they
/// differ from the last read, and a desktop window's mode when it differs
/// from the last mode read (a headless service sends none: the viewer keeps
/// its own mode, a known limit). Nothing is adopted while our push is in
/// flight or for a read sent before the push answered (it predates it).
/// Items naming a node absent from the shown tree are left out while that
/// tree is current (`stale` is None); while it is behind RoboCAD's revision
/// they are kept, and the next tree prunes the ones still absent
/// (`take_snapshot`). A connection's first read is adopted even when it
/// equals the empty `remote_selection` (`selection_read`). `remote_selection` keeps RoboCAD's items as read.
/// Adopted items go into the shared selection (`Op::Set`, stamped with the
/// current revision) and the change is recorded as published, so it is
/// never pushed back. Returns whether the shown selection or mode changed.
pub(in crate::cad) fn adopt_selection(doc: &mut CadDocument, shared: &mut Shared, sent: Instant, selection: Selection) -> bool {
    let current = doc.selection_job.is_none() && doc.selection_pushed_at.is_none_or(|at| sent >= at);
    if !current {
        return false;
    }
    let mut changed = false;
    let mode = selection.mode.as_deref().and_then(SelectMode::parse);
    if mode.is_some() && mode != doc.remote_mode {
        doc.remote_mode = mode;
        if let Some(mode) = mode.filter(|m| *m != doc.select_mode) {
            doc.select_mode = mode;
            doc.hover = None;
            doc.candidates = None;
            changed = true;
        }
    }
    if !doc.selection_read || selection.items != doc.remote_selection {
        doc.selection_read = true;
        // A change not yet pushed (a new tree's prune in this snapshot)
        // stays pending, so RoboCAD's copy is corrected if it still differs.
        let pending = doc.published_selection != shared.view().changed();
        let items: Vec<SelectionItem> = match (&doc.doc, doc.stale.is_none()) {
            (Some(tree), true) => selection.items.iter().filter(|i| tree.nodes.iter().any(|n| n.id == i.0)).cloned().collect(),
            _ => selection.items.clone(),
        };
        doc.remote_selection = selection.items;
        if shared.items() != items {
            match shared.set(items) {
                Ok(c) => changed |= c,
                Err(e) => doc.show(Err(format!("RoboCAD's selection was not adopted: {e}"))),
            }
        }
        // RoboCAD's own change: not pushed back (`publish_changes`).
        if !pending {
            doc.published_selection = shared.view().changed();
        }
    }
    changed
}

/// The selection push has answered: RoboCAD now holds our selection (its
/// answer is kept as RoboCAD's copy; a desktop window's mode is then the
/// one pushed). A selection or mode change made while it was in flight is
/// pushed now (the newest only).
pub(in crate::cad) fn finish_selection(doc: &mut CadDocument, shared: View) {
    let Some(job) = &doc.selection_job else { return };
    let Some(result) = job.poll() else { return };
    let generation = job.generation();
    doc.selection_job = None;
    if generation != doc.generation {
        return;
    }
    doc.selection_pushed_at = Some(Instant::now());
    let again = std::mem::take(&mut doc.selection_again);
    match result {
        Ok(items) => {
            doc.remote_selection = items;
            // Without a newer change queued, the mode pushed is the current one.
            if !again && doc.health.as_ref().is_some_and(|h| h.gui) {
                doc.remote_mode = Some(doc.select_mode);
            }
        }
        Err(e) => doc.show(Err(format!("the selection was not pushed to RoboCAD: {e}"))),
    }
    if again && doc.connected() {
        let items = shared.items();
        if crate::cad::selection::differs_from_remote(doc, &items) {
            push_selection(doc, items);
        }
    }
}

/// What `PUT /selection` sends: `items` (the shared selection's CAD items)
/// as RoboCAD writes them and the selection mode (a headless RoboCAD
/// stores only the items).
pub(in crate::cad) fn selection_body(doc: &CadDocument, items: Vec<SelectionItem>) -> (Vec<SelectionItem>, Option<&'static str>) {
    (items, Some(doc.select_mode.name()))
}

/// Push `items` (the shared selection's CAD items) and the mode to
/// RoboCAD's `/selection`. One push at a time, so they land in order: while
/// one is in flight, the newest is pushed when it answers
/// (`finish_selection` reads the shared selection again then).
pub(crate) fn push_selection(doc: &mut CadDocument, items: Vec<SelectionItem>) {
    if doc.selection_job.is_some() {
        doc.selection_again = true;
        return;
    }
    let Some(client) = doc.client.clone() else { return };
    let (items, mode) = selection_body(doc, items);
    doc.selection_job = Some(Job::spawn(Pool::Dedicated, doc.generation, "cad-selection", move |_| {
        client.set_selection(&items, mode).map(|answered| answered.items).map_err(|e| e.to_string())
    }));
}

/// The inspected node's `GET /nodes/{id}`: refetched when the first
/// selected node or the shown revision changes; a result for another node
/// or revision is dropped.
pub(super) fn detail(doc: &mut CadDocument, shared: View) {
    let revision = doc.doc_key.as_ref().map_or(0, |k| k.1);
    let wanted = shared.items().first_node().map(|id| (id.to_string(), revision));
    if let Some(job) = &doc.detail_job {
        if let Some(result) = job.poll() {
            let generation = job.generation();
            doc.detail_job = None;
            if generation == doc.generation {
                if let Some(key) = doc.detail_key.clone().filter(|k| Some(k) == wanted.as_ref()) {
                    doc.detail = Some((key.0, key.1, result));
                    doc.touch();
                }
            }
        }
    }
    if doc.detail_key == wanted {
        return;
    }
    match wanted {
        None => {
            doc.detail_key = None;
            doc.detail_job = None;
            if doc.detail.take().is_some() {
                doc.touch();
            }
        }
        Some((id, revision)) => {
            let Some(client) = doc.client.clone().filter(|_| doc.connection == Connection::Connected) else { return };
            doc.detail_key = Some((id.clone(), revision));
            doc.detail_job = Some(Job::spawn(Pool::Dedicated, doc.generation, "cad-node-detail", move |_| client.node(&id).map_err(|e| e.to_string())));
        }
    }
}

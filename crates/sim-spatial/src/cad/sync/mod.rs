//! CAD mode's connection to RoboCAD, on `jobs` only (native-viewer.md §4):
//!
//! - **Connect** ([`start`], a `Pool::Dedicated` job stamped with the
//!   document's generation): a `.rcad` file starts RoboCAD's headless
//!   service (`cad/.venv/bin/python -m robocad.api FILE --port N`, a
//!   `jobs::ChildProcess`) and waits until it answers; a URL is attached to
//!   with one `GET /` (refused unless it answers `ok: true`). The child is
//!   put in the document's `ChildSlot` the moment it is spawned, so closing
//!   the window or leaving CAD mode while connecting stops it at once; a
//!   connect that fails or is cancelled stops it too.
//! - **Poll** (a `RunThread` "cad-poll", its own client with a
//!   [`POLL_TIMEOUT`]): `GET /` and `GET /selection` every [`POLL_PERIOD`]
//!   (or at once on `PollCommand::Refresh`); `GET /doc`, `/commands` and
//!   (GUI) `/autosave` when the (document id, revision) changed or on
//!   Refresh. Errors are published verbatim, never hidden. Between requests
//!   it checks its channel, so a dropped document's worker exits after at
//!   most the request in progress.
//! - **Results** ([`receive`], `ViewerSet::JobResults`): the connect, the
//!   poll's snapshot, the edit in flight, the selection push, the inspected
//!   node's detail and the physical description, each dropped when its
//!   generation (or node and revision) is no longer the shown one.
//!
//! Network work runs on `Pool::Dedicated` (the jobs module's pool rule: a
//! request may hold its thread for `cad_client::REQUEST_TIMEOUT`).
mod launch;
mod selection;

#[cfg(test)]
pub(super) use launch::{accept_served, serves};
pub(crate) use selection::push_selection;
pub(super) use selection::adopt_selection;
#[cfg(test)]
pub(super) use selection::{finish_selection, selection_body};

use super::document::{CadDocument, Connected, Connection, EditDone, PollCommand, PollSnapshot};
use super::selection::{Shared, View};
use super::CadTarget;
use crate::document::DocumentRegistry;
use crate::jobs::{Job, Pool, RunThread};
use crate::selection::Selection;
use bevy::prelude::*;
use launch::{log_tail, self_start};
use selection::detail;
#[cfg(not(test))]
use selection::finish_selection;
use serde_json::Value;
use sim_runtime::cad_client::{CadClient, EDIT_TIMEOUT, SelectionItem, service};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often the poll worker asks RoboCAD for its health and selection.
pub const POLL_PERIOD: Duration = Duration::from_millis(500);
/// The poll's timeout for `GET /` and `GET /selection`: shorter than
/// `cad_client::REQUEST_TIMEOUT` so a hung service shows as Lost within
/// seconds and a dropped document's worker exits soon. `/doc`, `/commands`
/// and `/autosave` keep `REQUEST_TIMEOUT` (a large document's `/doc` may take
/// longer; a slow tick is its own back-off).
pub const POLL_TIMEOUT: Duration = Duration::from_secs(5);
/// How long dropping the poll worker waits for it (it may sit in a request
/// for up to the client's timeout; the document is dropped off the UI
/// thread, and past this bound the worker exits at its next check).
const POLL_JOIN: Duration = Duration::from_millis(50);
/// Bytes of the self-started service's stderr log quoted in errors.
const LOG_TAIL: usize = 2000;

/// Start (or restart) the connection: a new generation, so every result of
/// the previous connection is dropped. The previous poll worker and jobs
/// are dropped off the UI thread. The previous child slot is closed and its
/// process stopped (non-blocking): callers restart only a connection that
/// never answered or whose self-started service has exited, so it holds no
/// edits (`actions::refresh`, `enter`, a new document).
pub(crate) fn start(doc: &mut CadDocument) {
    let old = (doc.connect.take(), doc.poll.take(), doc.detail_job.take(), doc.physical_job.take(), doc.selection_job.take(), doc.exit_log.take(), doc.edit.take());
    if old.0.is_some() || old.1.is_some() || old.6.is_some() {
        crate::jobs::drop_off_thread(old, "the previous RoboCAD connection");
    }
    if let Some(child) = std::mem::take(&mut doc.child).close() {
        child.stop();
    }
    doc.generation = super::document::next_generation();
    // cad-organize: thread commits sent on the old connection never land
    // here; the outliner's and references' open edits and picks were read
    // from the old service (a restarted one may report the same revision).
    crate::cad::threads::restarted(doc);
    crate::cad::tree::restarted(doc);
    crate::cad::references::restarted(doc);
    doc.client = None;
    doc.child_exit = None;
    doc.log = None;
    doc.seen_poll = 0;
    doc.detail_key = None;
    // A restarted service may report the same revision: the old physical
    // description could include edits that died with the old service.
    doc.physical = None;
    doc.dirty_known_at = None;
    // A new connection (perhaps a restarted service on the same file, whose
    // manifest restores the same document id and revision): its first tree
    // and selection are adopted, not compared with the old service's.
    doc.doc_key = None;
    doc.remote_selection.clear();
    doc.selection_read = false;
    // A desktop window's mode is adopted afresh from the new connection.
    doc.remote_mode = None;
    doc.selection_pushed_at = None;
    doc.edit_waited = false;
    doc.selection_again = false;
    doc.selection_error = None;
    let generation = doc.generation;
    let (what, job) = match doc.target.clone() {
        CadTarget::Service(url) => (
            format!("connecting to RoboCAD at {url}"),
            Job::spawn(Pool::Dedicated, generation, "cad-connect", move |_| {
                let client = CadClient::new(&url).map_err(|e| e.to_string())?;
                let health = client.health().map_err(|e| e.to_string())?;
                if !health.ok {
                    return Err(format!("{url} answered but is not a RoboCAD service (GET / has no ok:true)"));
                }
                Ok(Connected { client, health, self_started: false })
            }),
        ),
        CadTarget::File(path) => {
            let slot = doc.child.clone();
            (format!("starting RoboCAD's headless service on {}", path.display()), Job::spawn(Pool::Dedicated, generation, "cad-start", move |ctx| self_start(&path, &slot, ctx)))
        }
    };
    if let CadTarget::File(_) = doc.target {
        doc.url = None;
    }
    doc.connection = Connection::Connecting { what, since: Instant::now() };
    doc.shown_seconds = 0;
    doc.connect = Some(job);
    doc.touch();
}

/// The poll worker for a connected client (its own copy, with [`POLL_TIMEOUT`]).
fn spawn_poll(client: CadClient) -> RunThread<PollCommand, PollSnapshot> {
    let quick = client.clone().with_timeout(POLL_TIMEOUT);
    RunThread::spawn("cad-poll", PollSnapshot::default(), move |commands, shared| poll_loop(&quick, &client, &commands, &shared)).join_bound(POLL_JOIN)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Read the queued commands: a Refresh sets `refresh`. True when the
/// channel has closed (the document was dropped): the worker returns.
fn drain(commands: &Receiver<PollCommand>, refresh: &mut bool) -> bool {
    loop {
        match commands.try_recv() {
            Ok(PollCommand::Refresh) => *refresh = true,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => return true,
        }
    }
}

/// The poll worker's loop: returns when its channel closes (the document
/// was dropped), checked between every two requests.
fn poll_loop(client: &CadClient, slow: &CadClient, commands: &Receiver<PollCommand>, shared: &Arc<Mutex<PollSnapshot>>) {
    // The (document id, revision) of the last `/doc` fetched.
    let mut fetched: Option<(Option<String>, u64)> = None;
    let mut seq = 0;
    // The first tick fetches everything at once.
    let mut refresh = true;
    loop {
        if !refresh {
            match commands.recv_timeout(POLL_PERIOD) {
                Ok(PollCommand::Refresh) => refresh = true,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        if drain(commands, &mut refresh) {
            return;
        }
        // A Refresh that arrives during this tick's requests may postdate
        // its `GET /`: it makes the next tick fetch at once (`refresh`
        // counts on the tick after the current one following the command).
        let mut again = false;
        let health = client.health().map_err(|e| e.to_string());
        if drain(commands, &mut again) {
            return;
        }
        let sent = Instant::now();
        let selection = client.selection().map_err(|e| e.to_string());
        if drain(commands, &mut again) {
            return;
        }
        let mut fetch = None;
        if let Ok(h) = &health {
            let key = (h.document_id.clone(), h.revision);
            if refresh || fetched.as_ref() != Some(&key) {
                let doc = slow.doc().map_err(|e| e.to_string());
                if drain(commands, &mut again) {
                    return;
                }
                let registry = slow.commands().map_err(|e| e.to_string());
                if drain(commands, &mut again) {
                    return;
                }
                let autosave = h.gui.then(|| slow.autosave().map_err(|e| e.to_string()));
                if let Ok(d) = &doc {
                    fetched = Some((d.document_id.clone(), d.revision));
                }
                fetch = Some((doc, registry, autosave));
            }
        }
        // A Refresh pending on a tick whose `GET /` failed is kept for the next.
        refresh = again || (refresh && health.is_err());
        seq += 1;
        let mut snapshot = lock(shared);
        snapshot.seq = seq;
        snapshot.health = Some(health);
        snapshot.selection = Some((sent, selection));
        if let Some((doc, registry, autosave)) = fetch {
            match doc {
                Ok(d) => {
                    snapshot.doc_key = Some((d.document_id.clone(), d.revision));
                    snapshot.doc = Some(d);
                    snapshot.doc_error = None;
                }
                Err(e) => snapshot.doc_error = Some(e),
            }
            snapshot.commands = Some(registry);
            snapshot.autosave = autosave;
        }
    }
}

/// OnEnter(ModeScope::Cad): connect the document the switch (or the launch)
/// installed; without one, the target CAD mode last showed, else RoboCAD's
/// default URL.
pub(crate) fn enter(mut commands: Commands, doc: Option<ResMut<CadDocument>>, registry: Option<Res<DocumentRegistry>>) {
    match doc {
        Some(mut doc) => {
            if doc.connect.is_none() && doc.client.is_none() {
                start(&mut doc);
            }
        }
        None => {
            let target = registry.as_deref().and_then(|r| r.source(crate::app::ViewerMode::Cad)).and_then(crate::app::switch::sources::cad_target).unwrap_or_else(|| CadTarget::Service(sim_runtime::cad_client::DEFAULT_URL.into()));
            let mut doc = CadDocument::new(target);
            start(&mut doc);
            commands.insert_resource(doc);
        }
    }
}

/// JobResults: everything that came back from RoboCAD this frame, and the
/// shared selection's changes pushed to RoboCAD (`selection::publish_changes`).
/// The selection and the registry are borrowed mutably only by the steps
/// that change them (a new snapshot, an answered edit), so their change
/// detection is not tripped every frame.
pub(crate) fn receive(doc: Option<ResMut<CadDocument>>, mut selection: ResMut<Selection>, mut registry: ResMut<DocumentRegistry>, redraw: Option<MessageWriter<bevy::window::RequestRedraw>>, mut composition: Option<ResMut<crate::cad::composition::CadCompositionState>>) {
    let Some(mut doc) = doc else { return };
    let doc = &mut *doc;
    if super::selection::cad_id(&registry).is_none() {
        super::selection::ensure_registered(&mut registry, &mut selection, &doc.target);
    }
    // A change another writer made since (an `Act<SelectionAction>` this
    // frame) is pushed before RoboCAD's selection is read, so a push in
    // flight keeps the poll's older read from overwriting it.
    super::selection::publish_changes(doc, View { selection: &*selection, registry: &*registry });
    let busy = finish_connect(doc);
    watch_child(doc);
    if snapshot_ready(doc) {
        take_snapshot(doc, &mut Shared { selection: &mut *selection, registry: &mut *registry });
    }
    let target = doc.target.clone();
    // RoboCAD's handler cleared the selection, or selected the robot node
    // it created, after its Ops call returned (`ops::started` noted which);
    // a selection changed meanwhile is the user's newer one and is kept.
    // Set through the shared selection.
    if let Some((was, now)) = finish_edit(doc, composition.as_deref_mut())
        && (View { selection: &*selection, registry: &*registry }).items() == was
        && let Err(e) = (Shared { selection: &mut *selection, registry: &mut *registry }).set(now)
    {
        warn!("CAD: the selection was not changed after the edit: {e}");
    }
    // A Save As retargeted the document: the registry's entry names the saved file (same id).
    if doc.target != target {
        registry.set_source(crate::app::ViewerMode::Cad, super::selection::source(&doc.target));
    }
    finish_selection(doc, View { selection: &*selection, registry: &*registry });
    // A re-check after a new tree or an op's clear: pushed once.
    super::selection::publish_changes(doc, View { selection: &*selection, registry: &*registry });
    detail(doc, View { selection: &*selection, registry: &*registry });
    finish_physical(doc);
    // Keep frames coming while something is outstanding (an unfocused
    // window otherwise steps only on its low-power timer).
    if busy || doc.edit.is_some() || doc.detail_job.is_some() || doc.physical_job.is_some() || doc.selection_job.is_some() {
        if let Some(mut redraw) = redraw {
            redraw.write(bevy::window::RequestRedraw);
        }
    }
}

/// The connect job: Connected (client, health, child; the poll starts) or
/// Lost with its error. Returns whether it is still running.
fn finish_connect(doc: &mut CadDocument) -> bool {
    let Some(job) = &doc.connect else { return false };
    let Some(result) = job.poll() else {
        // The job's own progress line (a self-started service's pid and URL).
        let message = job.progress().message;
        if let Connection::Connecting { what, since } = &mut doc.connection {
            let seconds = since.elapsed().as_secs();
            if !message.is_empty() && *what != message {
                *what = message;
                doc.revision += 1;
            } else if seconds != doc.shown_seconds {
                doc.shown_seconds = seconds;
                doc.revision += 1;
            }
        }
        return true;
    };
    let generation = job.generation();
    doc.connect = None;
    if generation != doc.generation {
        // A replaced connection's result: its child was in the slot `start`
        // closed and stopped, so only the client and health are dropped.
        return false;
    }
    match result {
        Ok(Connected { client, health, self_started }) => {
            doc.url = Some(client.url());
            if self_started {
                // The process is in `doc.child` already (the job put it there).
                doc.log = Some(service::log_path(client.endpoint.port));
            }
            doc.poll = Some(spawn_poll(client.clone()));
            doc.client = Some(client);
            doc.health = Some(health);
            doc.connection = Connection::Connected;
        }
        Err(error) => doc.connection = Connection::Lost { error, since: Instant::now() },
    }
    doc.touch();
    false
}

/// A self-started service that exits once connected: Lost, naming its
/// exit, then its log tail (read on an Io job). The poll stops (it would
/// only report refused connections); `cad_refresh` starts the service
/// again. While connecting, the connect job reports an exit itself.
fn watch_child(doc: &mut CadDocument) {
    if doc.child_exit.is_none() {
        if doc.connect.is_some() || doc.client.is_none() {
            return;
        }
        let Some(exit) = doc.child.exited() else { return };
        if let Some(poll) = doc.poll.take() {
            crate::jobs::drop_off_thread(poll, "the RoboCAD poll");
        }
        doc.connection = Connection::Lost { error: format!("{exit}; reading its log…"), since: Instant::now() };
        doc.child_exit = Some(exit);
        if let Some(log) = doc.log.clone() {
            doc.exit_log = Some(Job::spawn(Pool::Io, doc.generation, "the RoboCAD service log", move |_| Ok(log_tail(&log))));
        }
        doc.touch();
        return;
    }
    let Some(job) = &doc.exit_log else { return };
    let Some(tail) = job.poll() else { return };
    doc.exit_log = None;
    let exit = doc.child_exit.clone().unwrap_or_default();
    let log = doc.log.as_ref().map(|l| l.display().to_string()).unwrap_or_default();
    let error = match tail {
        Ok(tail) => format!("{exit}; its log {log} ends: {tail}"),
        Err(e) => format!("{exit}; its log {log} could not be read: {e}"),
    };
    doc.connection = Connection::Lost { error, since: Instant::now() };
    doc.touch();
}

/// Whether the poll worker has published a snapshot not yet taken.
fn snapshot_ready(doc: &CadDocument) -> bool {
    doc.poll.as_ref().is_some_and(|poll| poll.lock().seq > doc.seen_poll)
}

/// The poll worker's newest snapshot, once.
fn take_snapshot(doc: &mut CadDocument, shared: &mut Shared) {
    let Some(poll) = &doc.poll else { return };
    let snapshot = {
        let s = poll.lock();
        if s.seq <= doc.seen_poll {
            return;
        }
        // The tree is cloned only when it is newer than the shown one.
        let doc_changed = s.doc_key != doc.doc_key;
        PollSnapshot {
            seq: s.seq,
            health: s.health.clone(),
            doc: if doc_changed { s.doc.clone() } else { None },
            doc_key: s.doc_key.clone(),
            doc_error: s.doc_error.clone(),
            commands: s.commands.clone(),
            autosave: s.autosave.clone(),
            selection: s.selection.clone(),
        }
    };
    doc.seen_poll = snapshot.seq;
    let mut changed = false;
    // `health.dirty` is known again only from a successful `GET /` sent
    // after the edit; a failed one leaves it unknown (`unsaved` stays None).
    let health_ok = matches!(snapshot.health, Some(Ok(_)));
    if health_ok && doc.dirty_known_at.is_some_and(|at| snapshot.seq >= at) {
        doc.dirty_known_at = None;
        changed = true;
    }
    match snapshot.health {
        Some(Ok(health)) => {
            if doc.health.as_ref() != Some(&health) {
                doc.health = Some(health);
                changed = true;
            }
            if doc.connection != Connection::Connected && doc.child_exit.is_none() {
                if matches!(doc.connection, Connection::Lost { .. }) {
                    // Back from Lost: meshes and a node detail that failed meanwhile are fetched again.
                    doc.mesh_retry += 1;
                    if matches!(doc.detail, Some((_, _, Err(_)))) {
                        doc.detail_key = None;
                    }
                }
                doc.connection = Connection::Connected;
                changed = true;
            }
        }
        Some(Err(error)) => {
            let same = matches!(&doc.connection, Connection::Lost { error: e, .. } if *e == error);
            if !same && doc.child_exit.is_none() {
                doc.connection = Connection::Lost { error, since: Instant::now() };
                changed = true;
            }
        }
        None => {}
    }
    if let Some(tree) = &snapshot.doc {
        // A deleted node's hover and Alt-menu items go (the selection's are
        // re-checked below, once staleness is known).
        let known = |i: &SelectionItem| tree.nodes.iter().any(|n| n.id == i.0);
        if doc.hover.as_ref().is_some_and(|h| !known(h)) {
            doc.hover = None;
        }
        if let Some(c) = &mut doc.candidates {
            c.items.retain(known);
            if c.items.is_empty() {
                doc.candidates = None;
            }
        }
    }
    let new_tree = snapshot.doc.is_some();
    if let Some(tree) = snapshot.doc {
        doc.doc = Some(tree);
        doc.doc_key = snapshot.doc_key;
        changed = true;
    }
    if doc.commands != snapshot.commands && snapshot.commands.is_some() {
        doc.commands = snapshot.commands;
        changed = true;
    }
    if doc.autosave != snapshot.autosave {
        doc.autosave = snapshot.autosave;
        changed = true;
    }
    // Stale: the shown tree is behind RoboCAD's revision (a refetch is
    // under way, or the last one failed: its error).
    let behind = match (&doc.health, &doc.doc_key) {
        (Some(h), Some((id, revision))) => h.revision != *revision || h.document_id != *id,
        (Some(_), None) => true,
        _ => false,
    };
    let stale = match (&snapshot.doc_error, behind) {
        (Some(e), true) => Some(e.clone()),
        (None, true) => doc.health.as_ref().map(|h| format!("refetching revision {}", h.revision)),
        _ => None,
    };
    if doc.stale != stale {
        doc.stale = stale;
        changed = true;
    }
    // A new tree: the registry follows RoboCAD's revision and the shared
    // selection's CAD items are re-checked (`selection::follow_tree`): body
    // items restamped, sub-body items kept at the revision they were picked
    // at, a deleted node's items dropped and named while the tree is
    // current (kept while it is behind RoboCAD's revision, for the next
    // tree to settle). The change is pushed once (`publish_changes`).
    if new_tree && let Some(tree) = &doc.doc {
        let dropped = super::selection::follow_tree(shared, doc.shown_revision(), tree, doc.stale.is_none());
        if !dropped.is_empty() {
            // Named, as every mode names the items a re-check drops.
            doc.show(Err(format!("No longer selected (not in the document now): {}", dropped.join(", "))));
            changed = true;
        }
    }
    // RoboCAD's selection (all its items) and, from a desktop window, its
    // mode: adopted when they changed there, unless our own push is in
    // flight or the read predates its answer (`adopt_selection`). A failed
    // read is kept (shown in the connection line and cad_state) until one
    // succeeds.
    let selection = match snapshot.selection {
        Some((sent, Ok(selection))) => Some((sent, selection)),
        Some((_, Err(e))) => {
            if doc.selection_error.as_ref() != Some(&e) {
                doc.selection_error = Some(e);
                changed = true;
            }
            None
        }
        None => None,
    };
    if let Some((sent, selection)) = selection {
        if doc.selection_error.take().is_some() {
            changed = true;
        }
        changed |= adopt_selection(doc, shared, sent, selection);
    }
    if changed {
        doc.touch();
    }
}

/// The edit in flight has answered: its outcome line, the REST caller's
/// answer, and a refetch (RoboCAD's dirty flag is unknown until it lands).
/// Returns, after a successful edit that changes the selection, the
/// selection when it started and the one RoboCAD's handler leaves:
/// nothing (an op that clears it) or the robot node it created (add motor,
/// joint, sensor, cable: `OpsState::selects_created`). `receive` sets the
/// shared selection to it if the selection is still the first.
fn finish_edit(doc: &mut CadDocument, composition: Option<&mut crate::cad::composition::CadCompositionState>) -> Option<(Vec<SelectionItem>, Vec<SelectionItem>)> {
    let Some(edit) = &doc.edit else { return None };
    let Some(result) = edit.job.poll() else { return None };
    let generation = edit.job.generation();
    let (clear_selection, activates_plane, retarget) = doc.edit.take().map_or((None, false, None), |e| (e.clear_selection, e.activates_plane, e.retarget));
    let seq = doc.edit_seq;
    // The note goes with its edit, whatever the outcome.
    let selects = doc.ops.selects_created.take().filter(|(at, _)| *at == seq).map(|(_, was)| was);
    if generation != doc.generation {
        if let Some(state) = composition { crate::cad::composition::port_edit_answered(state, generation, seq, Err("composition.port: document changed while the submitted source edit waited".into())); }
        crate::cad::sketch::specs::polygon_edit_done(doc, seq, false);
        return None;
    }
    let answer = result.map(|EditDone { message, result }| (message, result));
    if let Err(error) = &answer {
        if error.contains("may still apply") || error.contains("applied it, but") { doc.uncertain_edit = Some(error.clone()); }
    }
    // Resolve pending connection intent from this authoritative answer, without
    // consuming the independent REST caller's result or starting another lifecycle.
    if let Some(state) = composition { crate::cad::composition::port_edit_answered(state, generation, seq, answer.as_ref().map(|_| ()).map_err(Clone::clone)); }
    // cad-print: a print start's answer is the job it started (tracked from now on).
    crate::cad::print::edit_answered(doc, seq, answer.as_ref().ok().map(|(_, r)| r));
    // cad-organize: a thread commit lands in its source's `annotations::InFlight`;
    // a reference or system-link edit rereads what it changed.
    crate::cad::threads::edit_answered(doc, seq, answer.as_ref().map(|(_, r)| r).map_err(Clone::clone));
    crate::cad::references::edit_answered(doc, seq, answer.as_ref().map(|(_, r)| r));
    // A polygon sent with sides: its count is the remembered one once RoboCAD took it.
    crate::cad::sketch::specs::polygon_edit_done(doc, seq, answer.is_ok());
    // A plane tool's new plane node becomes the active plane (the op's
    // answer is `{"result": id, …}`); `sketch::plane::sync` applies it.
    if activates_plane
        && let Ok((_, result)) = &answer
        && let Some(id) = result.get("result").and_then(Value::as_str)
    {
        doc.ops.plane_created = Some(id.to_string());
    }
    doc.status = Some(answer.as_ref().map(|(m, _)| m.clone()).map_err(Clone::clone));
    // A save to a path (Save As, `cad_save {path}`: `files::save` marks the
    // edit it started) of a file this window opened: the window now shows
    // (and reopens, `leave_cad`) the saved file. An attached service keeps its URL.
    if answer.is_ok()
        && let Some(path) = retarget
        && matches!(doc.target, super::document::CadTarget::File(_))
    {
        doc.target = super::document::CadTarget::File(path);
    }
    // RoboCAD's handler clears the selection after its Ops call returned
    // (`ops::started` noted which); a failed edit keeps the picks. `receive`
    // clears the shared selection (a set to nothing) unless it changed
    // meanwhile, and pushes it once (`publish_changes`).
    let cleared = clear_selection.filter(|c| answer.is_ok() && !c.is_empty()).map(|c| (c, Vec::new()));
    // The created node's id is the first call's answer (`{"result": id}`;
    // a joint with damping answers both calls, the add first).
    let created = answer.as_ref().ok().and_then(|(_, r)| match r {
        Value::Array(calls) => calls.first(),
        one => Some(one),
    });
    let created = created.and_then(|r| r.get("result")).and_then(Value::as_str).map(|id| vec![SelectionItem(id.to_string(), "body".into(), 0)]);
    let changed = selects.zip(created).or(cleared);
    if std::mem::take(&mut doc.edit_waited) {
        let seq = doc.edit_seq;
        // Only recent answers are kept: one whose REST caller went away is not collected.
        doc.edit_results.retain(|s, _| *s + 8 > seq);
        doc.edit_results.insert(seq, answer.map(|(message, result)| serde_json::json!({"message": message, "result": result})));
    }
    refresh(doc, true);
    doc.touch();
    changed
}

/// Ask the poll worker for `/doc` now; the next `GET /` it sends follows
/// this call, so `health.dirty` is known again once that snapshot is read.
///
/// `after_edit`: the saved state becomes unknown until then. A plain
/// refresh (`cad_refresh`) leaves it known, unless an edit's refetch is
/// already pending, which it moves to its own later snapshot.
pub(crate) fn refresh(doc: &mut CadDocument, after_edit: bool) {
    let track = after_edit || doc.dirty_known_at.is_some();
    // Without a poll to ask, the saved state stays unknown until a reconnect
    // (`start` resets it): never fall back to a `dirty` read before an edit.
    let Some(poll) = &doc.poll else {
        if track {
            doc.dirty_known_at = Some(u64::MAX);
        }
        return;
    };
    // The snapshot being fetched now may predate this call; the one after it does not.
    let published = poll.lock().seq;
    let sent = poll.send(PollCommand::Refresh).is_ok();
    if track {
        doc.dirty_known_at = Some(if sent { published + 2 } else { u64::MAX });
    }
    doc.touch();
}

/// Start `GET /physical?flex=0` for the shown revision.
pub(crate) fn fetch_physical(doc: &mut CadDocument) -> Result<(), String> {
    let client = doc.client.clone().filter(|_| doc.connected()).ok_or_else(|| format!("not connected to RoboCAD: {}", doc.connection_line().0))?;
    doc.physical_revision = doc.doc_key.as_ref().map_or(0, |k| k.1);
    doc.physical_job = Some(Job::spawn(Pool::Dedicated, doc.generation, "cad-physical", move |_| client.physical(false).map_err(|e| e.to_string())));
    doc.touch();
    Ok(())
}

fn finish_physical(doc: &mut CadDocument) {
    let Some(job) = &doc.physical_job else { return };
    let Some(result) = job.poll() else { return };
    let generation = job.generation();
    doc.physical_job = None;
    if generation == doc.generation {
        doc.physical = Some((doc.physical_revision, result));
    }
    // The panels show the fetch in flight, so its end is a change either way.
    doc.touch();
}

/// Start a mutating request (the caller has checked `edit_refusal`).
/// Returns the edit's sequence number. The request waits up to
/// `cad_client::EDIT_TIMEOUT` (longer than a desktop RoboCAD's own 120 s
/// wait, so an edit it then applies is not reported failed); a timeout's
/// error, which the REST caller gets verbatim, says RoboCAD may still apply it.
pub(crate) fn start_edit(doc: &mut CadDocument, label: String, waited: bool, auxiliary: bool, work: impl FnOnce(&CadClient) -> Result<EditDone, sim_runtime::cad_client::CadError> + Send + 'static) -> Result<u64, String> {
    if let Some(why) = doc.edit_refusal_for(auxiliary) {
        return Err(why);
    }
    let client = doc.client.clone().ok_or("not connected to RoboCAD")?.with_timeout(EDIT_TIMEOUT);
    doc.edit_seq += 1;
    doc.edit_waited = waited;
    let job = Job::spawn(Pool::Dedicated, doc.generation, format!("RoboCAD edit: {label}"), move |_| work(&client).map_err(|e| e.to_string()));
    doc.edit = Some(super::document::Edit { label, job, started: Instant::now(), clear_selection: None, activates_plane: false, retarget: None });
    doc.touch();
    Ok(doc.edit_seq)
}

/// A RoboCAD answer as the edit's result value.
pub(crate) fn value<T: serde::Serialize>(answer: &T) -> Value {
    serde_json::to_value(answer).unwrap_or(Value::Null)
}

/// Last: at window close a self-started service is stopped (also one still
/// starting: its slot is closed), or left running (its URL logged) when it
/// may hold unsaved edits (`CadDocument::release_child`).
pub(crate) fn on_exit(mut exits: MessageReader<AppExit>, doc: Option<ResMut<CadDocument>>, mut done: Local<bool>) {
    if exits.read().count() == 0 || std::mem::replace(&mut *done, true) {
        return;
    }
    if let Some(mut doc) = doc {
        doc.release_child("the window closed");
    }
}

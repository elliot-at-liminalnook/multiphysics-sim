//! RoboCAD's threads, read (`GET /threads`, `annotations.AnnotationOps.threads`,
//! api.py:393-395) on one `Pool::Dedicated` job per (connection
//! generation, shown revision, read epoch), as `views` reads its list:
//!
//! - **When**: while the Comments dock is open, the pins are shown
//!   (`CadDisplay::comment_pins`; without a window, always) or a REST
//!   caller asked, and the document is connected and read. A job for an
//!   older key is dropped (which cancels it) when a newer one starts, so an
//!   older answer never lands as current.
//! - **Again**: the epoch moves after every landed thread commit
//!   (`threads::edit_answered`), so the threads are read again even when
//!   RoboCAD's revision did not move; a failed read is tried again at the
//!   next key (a REST list moves the epoch to retry at once).
//! - **Stale**: the list read earlier on this connection stays shown while
//!   a newer one is read, labelled with the revision it was read at
//!   ([`line`]); commits use the revision the list was read at, so a stale
//!   list's commit is refused by name (`CadDocument::commit_refusal`).
use crate::app::actions::Call;
use crate::app::{ViewerMode, ViewerSet};
use crate::cad::display::CadDisplay;
use crate::cad::document::CadDocument;
use crate::jobs::{Job, Pool};
use bevy::prelude::*;
use serde_json::{Value, json};
use sim_runtime::cad_client::CadThread;

/// (document generation, RoboCAD's shown revision, read epoch) a list was read at.
pub(crate) type Key = (u64, u64, u64);

/// The threads as read and the read in flight.
#[derive(Default)]
pub(crate) struct ReadState {
    /// The threads as last read, with the key they were read at.
    pub(crate) listed: Option<(Key, Vec<CadThread>)>,
    /// Why the read at this key failed.
    pub(crate) error: Option<(Key, String)>,
    job: Option<(Key, Job<Vec<CadThread>>)>,
    /// Moved after each landed commit (and by a REST list retrying a failed read).
    pub(crate) epoch: u64,
    /// A REST caller asked for the list (read even with the dock closed and the pins off).
    pub(crate) asked: bool,
}
impl ReadState {
    /// Read again at the next frame.
    pub(crate) fn again(&mut self) {
        self.epoch += 1;
    }
    /// A read is in flight.
    pub(crate) fn reading(&self) -> bool {
        self.job.is_some()
    }
    /// The connection was replaced: nothing read on it is kept.
    pub(crate) fn reset(&mut self) {
        *self = ReadState { epoch: self.epoch + 1, ..ReadState::default() };
    }
}

/// The key a read made now would have.
pub(crate) fn key(doc: &CadDocument) -> Key {
    (doc.generation, doc.shown_revision(), doc.threads.read.epoch)
}

/// The threads read on this connection (whatever revision), None before
/// the first read.
pub(crate) fn listed(doc: &CadDocument) -> Option<&[CadThread]> {
    doc.threads.read.listed.as_ref().filter(|(k, _)| k.0 == doc.generation).map(|(_, l)| l.as_slice())
}

/// The RoboCAD revision the shown threads were read at.
pub(crate) fn read_at(doc: &CadDocument) -> Option<u64> {
    doc.threads.read.listed.as_ref().filter(|(k, _)| k.0 == doc.generation).map(|(k, _)| k.1)
}

/// The shown threads are RoboCAD's current ones (this revision, read after the last commit).
pub(crate) fn current(doc: &CadDocument) -> bool {
    doc.threads.read.listed.as_ref().is_some_and(|(k, _)| *k == key(doc))
}

/// One thread as read.
pub(crate) fn thread<'d>(doc: &'d CadDocument, id: &str) -> Option<&'d CadThread> {
    listed(doc)?.iter().find(|t| t.id == id)
}

/// The dock's line about the list: a read under way, a stale list or a
/// failed read (None: current).
pub(crate) fn line(doc: &CadDocument) -> Option<String> {
    let r = &doc.threads.read;
    let now = key(doc);
    if let Some((_, e)) = r.error.as_ref().filter(|(k, _)| *k == now) {
        return Some(format!("RoboCAD's comments could not be read: {e}"));
    }
    if current(doc) {
        return None;
    }
    Some(match (read_at(doc), r.reading()) {
        (None, true) => "Reading RoboCAD's comments…".to_string(),
        (None, false) if !doc.connected() => format!("RoboCAD's comments cannot be read: {}", doc.connection_line().0),
        (None, false) => "RoboCAD's comments are not read yet.".to_string(),
        (Some(at), true) => format!("Comments as read at revision {at}; reading revision {}…", now.1),
        (Some(at), false) => format!("Comments as read at revision {at}; RoboCAD's document is at revision {} (not current)", now.1),
    })
}

/// A read is wanted now.
fn wanted(doc: &CadDocument, pins: bool) -> bool {
    (doc.threads.open || pins || doc.threads.read.asked) && doc.connected() && doc.doc_key.is_some()
}

/// A finished read (key, answer), taken.
type Landed = (Key, Result<Vec<CadThread>, String>);

/// What [`tick`] has to do, found through a shared borrow (no change
/// detection while a read merely runs): `Some(landed)` when a job's key is
/// superseded, a job finished (its answer, taken), or a read is to start.
fn needs_work(doc: &CadDocument, pins: bool) -> Option<Option<Landed>> {
    let r = &doc.threads.read;
    let now = key(doc);
    match &r.job {
        Some((k, _)) if *k != now => return Some(None),
        Some((k, job)) => return job.poll().map(|result| Some((*k, result))),
        None => {}
    }
    let have = r.listed.as_ref().is_some_and(|(k, _)| *k == now) || r.error.as_ref().is_some_and(|(k, _)| *k == now);
    (!have && wanted(doc, pins)).then_some(None)
}

/// A job for another key dropped, a `landed` answer kept, a read started.
/// Returns whether anything shown changed.
pub(crate) fn tick(doc: &mut CadDocument, pins: bool, landed: Option<Landed>) -> bool {
    let now = key(doc);
    let want = wanted(doc, pins);
    let client = doc.client.clone().filter(|_| doc.connected());
    let generation = doc.generation;
    let r = &mut doc.threads.read;
    let mut touched = false;
    // Superseded: dropping the job cancels it.
    if r.job.as_ref().is_some_and(|(k, _)| *k != now) {
        r.job = None;
        touched = true;
    }
    if let Some((k, result)) = landed {
        r.job = None;
        match result {
            Ok(list) => {
                r.listed = Some((k, list));
                r.error = None;
                r.asked = false;
            }
            Err(e) => r.error = Some((k, e)),
        }
        touched = true;
    }
    let have = r.listed.as_ref().is_some_and(|(k, _)| *k == now) || r.error.as_ref().is_some_and(|(k, _)| *k == now);
    if !have && r.job.is_none() && want && let Some(client) = client {
        let job = Job::spawn(Pool::Dedicated, generation, "cad-threads", move |ctx| {
            if ctx.cancelled() {
                return Err("superseded by a newer revision".to_string());
            }
            // RoboCAD's own words (the dock shows them; no route).
            client.threads(None, None, None).map_err(|e| e.message)
        });
        r.job = Some((now, job));
        touched = true;
    }
    touched
}

/// Whether a REST caller should wait for the threads at the current key
/// (Ok(false): they are here), or why they will not come. A click never waits.
pub(crate) fn wait(doc: &mut CadDocument, call: &mut Call) -> Result<bool, String> {
    if !call.rest() {
        return Ok(false);
    }
    if let Some(g) = call.continuation.get("threads_wait").and_then(Value::as_u64) {
        if g != doc.generation {
            return Err("the CAD document was replaced or reconnected while waiting for its comments".into());
        }
        if call.cancelled {
            return Err("cancelled waiting for RoboCAD's comments".into());
        }
    } else if doc.threads.read.error.as_ref().is_some_and(|(k, _)| *k == key(doc)) {
        // A fresh request retries a failed read.
        doc.threads.read.again();
    }
    if current(doc) {
        return Ok(false);
    }
    if let Some((_, e)) = doc.threads.read.error.as_ref().filter(|(k, _)| *k == key(doc)) {
        return Err(format!("RoboCAD's comments could not be read (GET /threads): {e}"));
    }
    if !doc.connected() {
        return Err(format!("not connected to RoboCAD: {}", doc.connection_line().0));
    }
    if doc.doc_key.is_none() {
        return Err("RoboCAD's document has not been read yet; try again".into());
    }
    doc.threads.read.asked = true;
    *call.continuation = json!({"threads_wait": doc.generation});
    Ok(true)
}

/// JobResults (after `sync::receive`): [`tick`]; frames keep coming while a read runs.
fn sync(doc: Option<ResMut<CadDocument>>, display: Option<Res<CadDisplay>>, redraw: Option<MessageWriter<bevy::window::RequestRedraw>>) {
    let Some(mut doc) = doc else { return };
    let pins = display.as_deref().is_none_or(|d| d.comment_pins);
    // Polled through a shared borrow: a `ResMut` deref would mark the
    // document changed every frame while a read runs.
    if let Some(landed) = needs_work(&doc, pins)
        && tick(&mut doc, pins, landed)
    {
        doc.touch();
    }
    if doc.threads.read.reading()
        && let Some(mut redraw) = redraw
    {
        redraw.write(bevy::window::RequestRedraw);
    }
}

/// CadCorePlugin: the read (JobResults, after `sync::receive`).
pub(super) fn build_core(app: &mut App) {
    app.add_systems(Update, sync.after(crate::cad::CadSet::Results).in_set(ViewerSet::JobResults).run_if(in_state(ViewerMode::Cad)));
}

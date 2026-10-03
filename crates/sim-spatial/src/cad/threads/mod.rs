//! RoboCAD's comment threads in CAD mode (cad-organize; native-viewer.md
//! §7 "one annotations service, one thread panel"): the fourth
//! `annotations::ThreadSource`, drawn by the one `ui_kit::threads` panel.
//! RoboCAD keeps the threads in its document (`annotations.py`); every
//! change is one RoboCAD call through `actions::edit_at`, one undo step there.
//!
//! - [`source`]: the anchor adapter (`CadAnchor`), `thread_detail` as a
//!   `sim_annotate::Thread`, and the commit of each `ThreadCommand` as one
//!   RoboCAD call, reported `Committed::Pending` with the edit's sequence
//!   and landed by [`edit_answered`] (`annotations::InFlight`).
//! - [`read`]: `GET /threads` on a job per (generation, revision, epoch),
//!   a stale list labelled as such.
//! - [`dock`]: the Comments section at the top of the right dock
//!   (ui/comments.py:128-229): ＋ Annotate model, the Open/All/Resolved
//!   filter and Selected parts only, the thread list, the location line,
//!   Show on model, Fit in view, Reattach…, Resolve/Reopen, Parts in this
//!   discussion with Link selected parts, Rename part label… and Show only
//!   linked parts / Return to assembly, the messages with their part links,
//!   the author, Insert part link from selection, the composer, Edit and
//!   Delete message and Delete thread.
//! - [`ops`]: the one handler of `cad_threads` and its ops; [`controls`]:
//!   the `cad:threads:<id>` controls the dock draws and `system_ui` lists.
//! - [`input`]: the dock's three kit text fields (the composer, the
//!   author, the part label), mirrored into [`ThreadsState`], and Escape
//!   for Annotate and the isolation (before the Select tool's Escape).
//! - [`annotate`]: Annotate (N) and Reattach…: a face click through
//!   `CadMeshes::face_at` at the shown revision.
//! - [`pins`]: the numbered pins over the 3D view, display only; a press
//!   on one opens its thread.
//! - [`isolation`]: Show on model, Fit in view, Show only linked parts,
//!   Return to assembly and part links, display only (RoboCAD's visibility
//!   is never written).
//!
//! Window text never names REST routes; RoboCAD's labels and messages are
//! its own (comments.py, annotations.py).
mod annotate;
mod controls;
pub(in crate::cad) mod dock;
mod input;
pub(crate) mod isolation;
mod ops;
mod pins;
mod read;
mod source;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod evidence_tests;

pub use source::CadAnchor;
pub(in crate::cad) use controls::controls;
pub(crate) use controls::{attachment, controls_of, shown_threads, submit_action};
pub(in crate::cad) use ops::handle;
pub(crate) use ops::open;
pub(crate) use source::{Request, UNDO_IS_ROBOCADS, request_on, thread_of};
pub(crate) use ops::annotate_evidence;

use crate::annotations::InFlight;
use crate::app::actions::Spec;
use crate::cad::actions::{CAD, CadAction};
use crate::cad::document::CadDocument;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sim_runtime::cad_client::CadThread;

/// RoboCAD's undo labels (annotations.py), also the edits' labels here.
pub(crate) const ADD: &str = "Add annotation";
pub(crate) const UPDATE: &str = "Update annotation";
pub(crate) const DELETE_THREAD: &str = "Delete annotation";
pub(crate) const REPLY: &str = "Reply to annotation";
pub(crate) const EDIT_COMMENT: &str = "Edit comment";
pub(crate) const DELETE_COMMENT: &str = "Delete comment";
/// RoboCAD's status line after a post (comments.py:360).
pub(crate) const SAVED: &str = "Annotation saved in document • Ctrl+S writes the file • Ctrl+Z undoes";
/// RoboCAD's Annotate hint (comments.py:100).
pub(crate) const HINT: &str = "Click a surface to place a comment • click a pin to read it • Esc cancels";
/// What the draft-guarded controls say while a draft is open (comments.py:313, 330).
pub(crate) const DRAFTING: &str = "Post or cancel your current draft first";

/// A thread another mode asked CAD mode to show (Robot mode's "Open in
/// CAD", `robot::threads`): kept here, in state rather than a message, until
/// CAD mode's document for `target` has read its threads (the switch and
/// RoboCAD's start take many frames). The Comments dock is opened once
/// (closing it afterwards drops the request); once read, the thread opens
/// (filter All) as `ops::open` opens it (an open draft on another thread
/// keeps its thread, and the status line says why), or the status line
/// says it is gone. A read that failed at the current revision, or a lost
/// connection, ends the request with a status line; a document for another
/// target drops it. The asking mode puts it on its switch request
/// (`app::switch::ModeSwitch::reveal`, [`Reveal::new`]); `app::switch::start`
/// installs it only when that switch to CAD mode is accepted (every
/// accepted switch to CAD mode replaces it, None clearing it), so a refused
/// request (a second Open in CAD while the first is entering) never changes
/// it. [`read`]'s `sync` takes it, and leaving CAD mode drops one that never
/// landed (`app::switch::leave::leave_cad`), so none stays pending.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct RevealThread(pub Option<Reveal>);
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Reveal {
    /// The CAD document it is in (the switch's target).
    pub target: crate::cad::CadTarget,
    pub thread: String,
    /// The Comments dock was opened for it (it is opened only once).
    pub opened: bool,
}
impl Reveal {
    /// A request to show `thread` of the document for `target`.
    pub(crate) fn new(target: crate::cad::CadTarget, thread: impl Into<String>) -> Self {
        Reveal { target, thread: thread.into(), opened: false }
    }
}

/// The thread list's filter (RoboCAD's combo box).
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Filter {
    #[default]
    Open,
    All,
    Resolved,
}
impl Filter {
    pub const ALL: [Filter; 3] = [Filter::Open, Filter::All, Filter::Resolved];
    pub fn name(self) -> &'static str {
        match self {
            Filter::Open => "open",
            Filter::All => "all",
            Filter::Resolved => "resolved",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Filter::Open => "Open",
            Filter::All => "All",
            Filter::Resolved => "Resolved",
        }
    }
    fn keeps(self, t: &CadThread) -> bool {
        match self {
            Filter::All => true,
            Filter::Open => !t.resolved(),
            Filter::Resolved => t.resolved(),
        }
    }
}

/// The dock's text fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    Compose,
    Author,
    Label,
}

/// A pin placed and not yet posted (`CommentsPanel.pending`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Pending {
    pub node: String,
    /// mm, RoboCAD's frame.
    pub point: [f64; 3],
    /// The face index at `revision`.
    pub face: Option<i64>,
    /// RoboCAD's camera at the click.
    pub view: Map<String, Value>,
    /// The shown revision the click was made at.
    pub revision: u64,
}

/// The Annotate tool (RoboCAD's `AnnotateTool`): `thread` when it reattaches one.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tool {
    pub thread: Option<String>,
}

/// The open "Part label for this discussion" dialog.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LabelDialog {
    pub thread: String,
    pub node: String,
    pub text: String,
}

/// RoboCAD's threads as read, the Comments dock, the composer, Annotate
/// and the isolation (on the document: reset with it).
pub struct ThreadsState {
    pub(crate) read: read::ReadState,
    /// Commits sent and not yet answered (`annotations::InFlight`).
    pub(crate) in_flight: InFlight,
    /// The Comments section is shown.
    pub(crate) open: bool,
    pub(crate) filter: Filter,
    pub(crate) selected_only: bool,
    /// The thread shown (RoboCAD's current list item).
    pub(crate) current: Option<String>,
    /// The current row of "Parts in this discussion".
    pub(crate) part: Option<String>,
    /// The message whose menu (Edit, Delete) is open.
    pub(crate) menu: Option<String>,
    /// The composer's text (mirrored from its kit field).
    pub(crate) compose: String,
    pub(crate) pending: Option<Pending>,
    /// The message being edited (Save edit).
    pub(crate) editing: Option<String>,
    /// The author field ("You", RoboCAD's default).
    pub(crate) author: String,
    pub(crate) label: Option<LabelDialog>,
    /// Which field has the kit's keyboard (mirrored by `input`).
    pub(crate) focus: Option<Field>,
    /// A field the handler asks `input` to give the keyboard to.
    pub(crate) claim: Option<Field>,
    /// The composer's keyboard is to be released (its draft was posted or cancelled).
    pub(crate) release: bool,
    /// The composer's post in flight: its edit's sequence and the body sent
    /// (the draft is kept until RoboCAD takes it, and ends then only if unchanged).
    pub(crate) sending: Option<(u64, String)>,
    /// The last post's refusal or error, under the composer.
    pub(crate) error: Option<String>,
    pub(crate) tool: Option<Tool>,
    pub(crate) isolation: Option<isolation::Isolation>,
}
impl Default for ThreadsState {
    fn default() -> Self {
        Self {
            read: Default::default(),
            in_flight: Default::default(),
            open: false,
            filter: Filter::Open,
            selected_only: false,
            current: None,
            part: None,
            menu: None,
            compose: String::new(),
            pending: None,
            editing: None,
            author: "You".into(),
            label: None,
            focus: None,
            claim: None,
            release: false,
            sending: None,
            error: None,
            tool: None,
            isolation: None,
        }
    }
}
impl ThreadsState {
    /// A draft is open (`update_send`'s `drafting`): RoboCAD disables the
    /// list, the filter and the thread actions then.
    pub(crate) fn drafting(&self) -> bool {
        !self.compose.is_empty() || self.pending.is_some() || self.editing.is_some()
    }
    /// The draft ends (`cancel_draft`).
    fn end_draft(&mut self) {
        self.compose.clear();
        self.pending = None;
        self.editing = None;
        self.error = None;
        self.release = true;
    }
}

/// Why a stale pin is not posted ([`draft_gone`]); the draft's text is kept.
pub(crate) const STALE_PIN: &str = "RoboCAD's document changed since the pin was placed: Annotate again to place it on the current model";

/// The revision a placed pin was clicked at, when the shown document has
/// moved past it (a remote edit, an undo) and the pin is not being posted:
/// its face index may name another face now, so it is never sent. Annotate
/// re-places it and keeps the text ([`may_replace_pin`]).
pub(crate) fn stale_pin(doc: &CadDocument) -> Option<u64> {
    let st = &doc.threads;
    st.pending.as_ref().filter(|p| st.sending.is_none() && p.revision != doc.shown_revision()).map(|p| p.revision)
}

/// Annotate may start, and its click land, although a draft is open: the
/// draft is a new annotation whose pin is stale ([`stale_pin`]).
pub(crate) fn may_replace_pin(doc: &CadDocument) -> bool {
    doc.threads.editing.is_none() && stale_pin(doc).is_some()
}

/// Why the composer's draft cannot be posted as it stands (it is kept):
/// its pin is stale, or, in a list read at RoboCAD's current revision, the
/// message being edited or the thread being replied to is gone (deleted or
/// undone in RoboCAD's window, or by an undo here). None while the list is
/// being read again: then nothing is claimed gone.
pub(crate) fn draft_gone(doc: &CadDocument) -> Option<String> {
    let st = &doc.threads;
    if stale_pin(doc).is_some() {
        return Some(STALE_PIN.to_string());
    }
    if st.pending.is_some() || !read::current(doc) {
        return None;
    }
    let listed = read::listed(doc)?;
    if let Some(comment) = &st.editing {
        let there = listed.iter().any(|t| t.comments.iter().any(|c| c.id == *comment));
        return (!there).then(|| "The message being edited is no longer in RoboCAD's comments (deleted or undone); nothing can be saved: copy your text, then Cancel".to_string());
    }
    let id = st.current.as_deref()?;
    let there = listed.iter().any(|t| t.id == id);
    (st.drafting() && !there).then(|| "This thread is no longer in RoboCAD's comments (deleted or undone); the reply cannot be posted: copy your text, then Cancel".to_string())
}

/// What `cad_threads` does.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThreadsOp {
    /// The threads and the dock as `cad_state.threads` shows them.
    #[default]
    State,
    /// The threads at RoboCAD's current revision (a REST caller waits for them).
    List,
    /// The Comments section shown (`open: true`), hidden (`false`) or toggled.
    Dock,
    /// Start Annotate (N): the next click on a surface places a pin.
    Annotate,
    /// Annotate's or Reattach's click: `node`, `point` (mm), `face`, `view`
    /// at `revision` (the window's clicks).
    Place,
    /// End Annotate or Reattach (Escape).
    Cancel,
    /// The composer's Cancel (`cancel_draft`).
    Discard,
    Filter,
    SelectedOnly,
    /// Open `thread` (RoboCAD's `select`).
    Open,
    /// Post annotation: the placed pin (or `node`, `point`, `face`, `view`), `body`, `author`.
    Create,
    Reply,
    /// Save edit: `comment`'s new `body`.
    Edit,
    /// Edit message: `comment`'s body into the composer.
    EditMessage,
    /// A message's menu (Edit, Delete) shown or hidden.
    Menu,
    /// Delete message `comment`.
    Delete,
    DeleteThread,
    /// Resolve or reopen (`resolved`; absent toggles).
    Resolve,
    /// Reattach…: starts Annotate for `thread`; with `node` and `point`, moves the pin now.
    Reattach,
    /// Link selected parts (or `ids`).
    Link,
    /// A linked part's row pressed: it and its children selected.
    Part,
    /// Rename part label…: without `label` the dialog opens; with it, it is saved.
    Label,
    LabelCancel,
    /// Show on model.
    Show,
    /// Fit in view.
    Fit,
    /// Show only linked parts (or `ids`).
    ShowParts,
    /// Return to assembly.
    Return,
    /// A `[label](part:ID)` link pressed: `node`.
    PartLink,
    /// Insert part link from selection.
    InsertLink,
    /// The composer's text (`text`).
    Draft,
    /// The author (`text`).
    Author,
}

/// `cad_threads`' arguments.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct ThreadsArgs {
    #[serde(default)]
    pub op: ThreadsOp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<String>>,
    /// mm, RoboCAD's frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<[f64; 3]>,
    /// A face index at `revision`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<i64>,
    /// RoboCAD's camera (target, distance, yaw, pitch, fov, orthographic, mode, rot).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Filter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<bool>,
    /// RoboCAD's revision the threads, the face or the pin were read at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
}
impl ThreadsArgs {
    pub(crate) fn of(op: ThreadsOp) -> ThreadsArgs {
        ThreadsArgs { op, ..ThreadsArgs::default() }
    }
    pub(crate) fn thread(op: ThreadsOp, thread: Option<&str>) -> CadAction {
        CadAction::CadThreads(ThreadsArgs { op, thread: thread.map(str::to_string), ..ThreadsArgs::default() })
    }
    pub(crate) fn action(self) -> CadAction {
        CadAction::CadThreads(self)
    }
}

/// The action of a RoboCAD command id that is the threads' (`surfaces::registry`'s `Do::Organize`).
pub(crate) fn command_action(id: &str) -> Option<CadAction> {
    use crate::cad::display::{DisplayArgs, DisplaySetting};
    match id {
        "tool.annotate" => Some(ThreadsArgs::of(ThreadsOp::Annotate).action()),
        // Shown and raised, never hidden (the dock's Close hides it).
        "view.comments" => Some(ThreadsArgs { op: ThreadsOp::Dock, open: Some(true), ..ThreadsArgs::default() }.action()),
        "view.comment_pins" => Some(CadAction::CadDisplay(DisplayArgs { toggle: Some(DisplaySetting::CommentPins), ..DisplayArgs::default() })),
        _ => None,
    }
}

/// Whether Annotate or Reattach takes the 3D view's clicks (the selection's click stands aside).
pub(crate) fn takes_clicks(doc: &CadDocument) -> bool {
    doc.threads.tool.is_some()
}

/// Whether node `id` is shown: false only while Show only linked parts leaves it out.
pub(crate) fn shown(doc: &CadDocument, id: &str) -> bool {
    isolation::shown(doc, id)
}

/// Whether some linked parts are shown alone.
pub(crate) fn isolating(doc: &CadDocument) -> bool {
    doc.threads.isolation.is_some()
}

/// `sync::finish_edit`: edit `seq` answered (every edit; only this
/// source's commits land). A post's draft ends when RoboCAD took it (a new
/// thread becomes the current one, as RoboCAD's `select(tid)`); an error
/// stays under the composer. The threads are read again either way.
pub(crate) fn edit_answered(doc: &mut CadDocument, seq: u64, result: Result<&Value, String>) {
    let st = &mut doc.threads;
    if !st.in_flight.waits(seq) {
        return;
    }
    let id = result.as_ref().ok().and_then(|v| v.get("id")).and_then(Value::as_str).map(str::to_string);
    let Some(landed) = st.in_flight.land(seq, result.map(|_| id)) else { return };
    st.read.again();
    let sent = st.sending.take_if(|(s, _)| *s == seq);
    let compose = sent.is_some();
    match &landed.result {
        Ok(new) => {
            // The draft ends only if it is still the text sent; a draft typed
            // further while the post was in flight is kept (as a plain reply draft).
            if let Some((_, body)) = &sent {
                if st.compose == *body {
                    st.end_draft();
                } else {
                    st.pending = None;
                    st.editing = None;
                }
            }
            if landed.label == ADD
                && let Some(new) = new
            {
                st.open = true;
                st.filter = Filter::All;
                st.selected_only = false;
                st.current = Some(new.clone());
                st.part = None;
            }
            if landed.label == DELETE_THREAD && st.current == landed.thread {
                st.current = None;
                st.part = None;
            }
            if landed.label == DELETE_COMMENT {
                st.menu = None;
            }
        }
        Err(e) => {
            if compose {
                st.error = Some(plain(e));
            }
        }
    }
    doc.touch();
}

/// An error as the window shows it: RoboCAD's words without the request
/// line (`CadError`'s "RoboCAD METHOD /route: " prefix and " (HTTP n)"
/// suffix; REST callers get the whole text).
pub(crate) fn plain(e: &str) -> String {
    let rest = e.strip_prefix("RoboCAD ").and_then(|r| r.split_once(": ")).map_or(e, |(_, m)| m);
    match rest.rfind(" (HTTP ") {
        Some(i) if rest.ends_with(')') => rest[..i].to_string(),
        _ => rest.to_string(),
    }
}

/// `sync::start` (a reconnect, a new document): commits sent on the old
/// connection will not land here, and what was read and picked on it goes.
pub(crate) fn restarted(doc: &mut CadDocument) {
    let st = &mut doc.threads;
    st.in_flight.clear();
    st.read.reset();
    st.sending = None;
    st.tool = None;
    st.pending = None;
    st.isolation = None;
    st.label = None;
    // What named the old connection's threads and messages goes; the typed text stays.
    st.editing = None;
    st.menu = None;
    st.part = None;
    st.claim = None;
}

/// `cad_state.threads`.
pub(in crate::cad) fn state_json(doc: &CadDocument) -> Value {
    let st = &doc.threads;
    let threads: Vec<Value> = read::listed(doc)
        .unwrap_or(&[])
        .iter()
        .enumerate()
        .map(|(i, t)| {
            json!({
                "id": t.id,
                "number": i + 1,
                "node_id": t.anchor.node_id,
                "node_name": t.node_name,
                "status": t.status,
                "anchor_status": t.anchor_status,
                "attachment": attachment(t.anchor_status),
                "point": t.anchor.point,
                "messages": t.comments.iter().map(|c| json!({"id": c.id, "author": c.author, "body": c.body, "created_at": c.created_at, "updated_at": c.updated_at})).collect::<Vec<_>>(),
                "linked_parts": t.linked_parts,
            })
        })
        .collect();
    json!({
        "threads": threads,
        "read_at_revision": read::read_at(doc),
        "current": read::current(doc),
        "reading": st.read.reading(),
        "line": read::line(doc),
        "dock": {"open": st.open, "filter": st.filter, "selected_only": st.selected_only, "thread": st.current, "part": st.part, "menu": st.menu},
        "draft": {"text": st.compose, "pending": st.pending.as_ref().map(|p| json!({"node": p.node, "point": p.point, "face": p.face, "revision": p.revision})), "editing": st.editing, "author": st.author, "sending": st.sending.as_ref().map(|s| s.0), "error": st.error},
        "label_dialog": st.label.as_ref().map(|l| json!({"thread": l.thread, "node": l.node, "text": l.text})),
        "annotate": st.tool.as_ref().map(|t| json!({"reattach": t.thread})),
        "isolation": isolation::state_json(doc),
        "in_flight": st.in_flight.busy(),
    })
}

/// cad-organize's threads command (`cad_threads`).
pub(in crate::cad) fn specs() -> Vec<Spec> {
    vec![crate::app::actions::spec(
        "cad_threads",
        CAD,
        json!({"op": "create", "node": "b1", "point": [10.0, 0.0, 5.0], "face": 2, "body": "Too thin here", "author": "You"}),
        "CAD mode: RoboCAD's comment threads (cad_state.threads: each thread with its number, part, status, attachment, messages and linked parts; read with GET /threads at each revision). op: state; list (waits for the threads at RoboCAD's current revision); dock (open true | false, absent toggles the Comments section); open (thread); create (node, point [x, y, z] mm, face? index at revision, view? RoboCAD's camera {target, distance, yaw, pitch, fov, orthographic, mode, rot}, body, author? default the dock's author \"You\"; without node, the pin placed by Annotate: POST /threads, undo step \"Add annotation\"); reply (thread? default the open one, body, author?: POST /threads/{id}/comments, \"Reply to annotation\"); edit (comment, body: PATCH /comments/{id}, \"Edit comment\"); delete (comment: DELETE /comments/{id}; RoboCAD refuses a thread's last message); delete_thread (thread); resolve (thread, resolved? absent toggles: PATCH status); reattach (thread, node, point, face?, view?: PATCH with the new pin; without node it starts Annotate for that thread); link (thread, ids? default the selected nodes: PATCH part_refs); label (thread, node, label: a linked part's plain-language label, at most 120 characters); annotate, place, cancel (the window's Annotate tool and its clicks); discard (the composer's Cancel); filter (filter open | all | resolved); selected_only (on); edit_message, menu, draft (text), author (text), insert_link (part links of the selected nodes into the draft); show (the thread's saved camera on the native camera, its part selected), fit (its linked parts framed and selected), show_parts (thread or ids: only those parts drawn, display only, RoboCAD's visibility unchanged), return (the view, selection and section before show_parts), part (a linked part and its children selected), part_link (node: selects it as cad_select {ids: [node]} does, then shows it alone). Commits take revision? (default the revision the threads were read at) and are refused by name when RoboCAD's document moved since, while another CAD edit is in flight or when disconnected; a REST caller waits for RoboCAD's answer. Undo is RoboCAD's (cad_undo). system_ui lists cad:threads:*.",
    )]
}

/// CadCorePlugin's windowless part: the threads' read on a job (JobResults).
pub(crate) struct CoreParts;
impl Plugin for CoreParts {
    fn build(&self, app: &mut App) {
        app.init_resource::<RevealThread>();
        read::build_core(app);
    }
}

/// CadPlugin: the dock's text fields and keys, Annotate's clicks and the pins.
pub(in crate::cad) fn build(app: &mut App) {
    input::build(app);
    annotate::build(app);
    pins::build(app);
}

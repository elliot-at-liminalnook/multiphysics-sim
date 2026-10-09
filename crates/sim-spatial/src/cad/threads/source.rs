//! RoboCAD's comment threads as the fourth `annotations::ThreadSource`
//! (native-viewer.md §7): a remote source. The threads live in RoboCAD's
//! document (`annotations.py`'s `doc.annotations`), reached over its REST
//! routes (api.py:375-426, `crate::cad::types::threads`).
//!
//! - **Anchor** ([`CadAnchor`]): a thread's first target is its surface pin
//!   (`annotations.anchor`: the part, the point in mm in RoboCAD's frame,
//!   the picked face's description, plus the face *index* and the camera
//!   of a pin being placed or reattached, and `thread_detail`'s
//!   `anchor_status`), or an experiment-evidence reference; the others are
//!   its linked parts (`thread_parts`, as `thread_detail`'s `linked_parts`:
//!   the node, the optional plain-language label, description and saved
//!   view, the part's current name and whether it exists). A comment's
//!   links are the `[label](part:ID)` links in its body
//!   (`annotations.PART_LINK`, `cad_client::part_links`).
//! - **Threads** ([`thread_of`]): the title is `node_name` (RoboCAD lists
//!   "n · node_name"); comments keep RoboCAD's ids, author and body, the
//!   time as RoboCAD's list shows it (`created_at[:16]`, `T` as a space,
//!   comments.py:276), and `edited_at` when `updated_at` differs.
//! - **Commit** ([`CadThreadSource::commit`]): each `ThreadCommand` is
//!   exactly one RoboCAD call ([`Request`]) through `actions::edit_at`
//!   (CAD mode's edit job; refused by name with nothing sent while another
//!   edit is in flight, when disconnected, or when RoboCAD's revision moved
//!   since the threads were read): a put of a thread RoboCAD does not have
//!   is `POST /threads` (`create_thread`; RoboCAD assigns the ids), a put
//!   of a known one is one `PATCH /threads/{id}` carrying only what changed
//!   (`part_refs` for linked parts and their labels; `node_id`, `point`,
//!   `face`, `view` for a reattached pin; `status`), a reply is `POST
//!   /threads/{id}/comments`, a comment edit `PATCH /comments/{id}`, a
//!   comment delete `DELETE /comments/{id}`, a resolve `PATCH` `status`, a
//!   thread delete `DELETE /threads/{id}`. Undo and redo are refused by
//!   name: RoboCAD's undo is the document's (each change is one undo step
//!   there). A sent commit is recorded in `ThreadsState::in_flight`
//!   (`annotations::InFlight`) with the edit's sequence and reported as
//!   `Committed::Pending`; `threads::edit_answered` lands it.
//! - **Limits** ([`CadThreadSource::validate`]): RoboCAD's (annotations.py
//!   `text`, `validate_part_refs`): a body and an author non-empty after
//!   trimming and at most 20000 characters, at most 200 linked parts with
//!   unique ids, a label at most 120 characters and a description at most 1000.
use super::{ADD, DELETE_COMMENT, DELETE_THREAD, EDIT_COMMENT, REPLY, UPDATE};
use crate::annotations::{Committed, ThreadSource};
use crate::app::actions::Call;
use crate::cad::document::{CadDocument, EditDone};
use crate::cad::sync::value;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sim_annotate::{Anchor, Comment, Thread, ThreadCommand};
use sim_api::Outcome;
use crate::cad::types::{AnchorStatus, CadThread, NewThread, PartRef, ThreadPatch, part_links};
use std::collections::{BTreeMap, BTreeSet};

/// RoboCAD's `validate_part_refs` limit.
pub(crate) const MAX_PARTS: usize = 200;
/// RoboCAD's `text` limit (characters).
pub(crate) const MAX_TEXT: usize = 20000;
/// RoboCAD's part label limit.
pub(crate) const MAX_LABEL: usize = 120;
/// RoboCAD's part description limit.
pub(crate) const MAX_DESCRIPTION: usize = 1000;

/// What a CAD thread points at (see the module doc).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CadAnchor {
    /// The thread's pin on a part's surface.
    Surface {
        node_id: String,
        /// mm, RoboCAD's model frame.
        point: [f64; 3],
        /// `Face.to_json()` of the face as RoboCAD stored it (None for a new pin).
        #[serde(default)]
        face: Option<Value>,
        /// The face index picked (as `GET /nodes/{id}/faces` numbers them at
        /// the shown revision): a new or reattached pin's, sent to RoboCAD.
        #[serde(default)]
        face_index: Option<i64>,
        /// RoboCAD's camera when the pin was placed (`annotations.camera_view`).
        #[serde(default)]
        view: Map<String, Value>,
        /// `thread_detail`'s `anchor_status`.
        #[serde(default)]
        state: AnchorStatus,
        /// The part's name ("Deleted part" when gone).
        #[serde(default)]
        node_name: String,
    },
    /// An experiment-evidence thread (no part, no point).
    Evidence {
        #[serde(default)]
        evidence: Value,
    },
    /// A linked part.
    Part {
        node_id: String,
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        view: Option<Value>,
        /// The part's current name ("Deleted part" when gone).
        #[serde(default)]
        name: String,
        #[serde(default)]
        available: bool,
    },
}

impl CadAnchor {
    /// The node it names (None: evidence).
    pub(crate) fn node(&self) -> Option<&str> {
        match self {
            CadAnchor::Surface { node_id, .. } | CadAnchor::Part { node_id, .. } => Some(node_id),
            CadAnchor::Evidence { .. } => None,
        }
    }
    /// A linked part as RoboCAD stores it.
    pub(crate) fn part_ref(&self) -> Option<PartRef> {
        match self {
            CadAnchor::Part { node_id, label, description, view, .. } => Some(PartRef { node_id: node_id.clone(), label: label.clone(), description: description.clone(), view: view.clone() }),
            _ => None,
        }
    }
    /// A new linked part on node `id` named `name` (`link_selection`'s `{'node_id': nid}`).
    pub(crate) fn part(id: &str, name: &str) -> CadAnchor {
        CadAnchor::Part { node_id: id.to_string(), label: None, description: None, view: None, name: name.to_string(), available: true }
    }
}

/// RoboCAD's `text(value, name)` check (annotations.py:22-27).
pub(crate) fn text(value: &str, name: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{name} must not be empty"));
    }
    if value.chars().count() > MAX_TEXT {
        return Err(format!("{name} is too long"));
    }
    Ok(())
}

impl Anchor for CadAnchor {
    /// The shown tree's nodes: id → name.
    type Index = BTreeMap<String, String>;
    fn validate(&self) -> Result<(), String> {
        match self {
            CadAnchor::Surface { node_id, point, .. } => {
                if node_id.is_empty() {
                    return Err("annotation part does not exist".into());
                }
                if !point.iter().all(|v| v.is_finite()) {
                    return Err("anchor point must contain three finite millimetre coordinates".into());
                }
                Ok(())
            }
            CadAnchor::Evidence { .. } => Ok(()),
            CadAnchor::Part { node_id, label, description, .. } => {
                if node_id.is_empty() {
                    return Err("Linked part IDs must be unique strings".into());
                }
                if label.as_ref().is_some_and(|l| l.chars().count() > MAX_LABEL) {
                    return Err(format!("Part label must be text of at most {MAX_LABEL} characters"));
                }
                if description.as_ref().is_some_and(|d| d.chars().count() > MAX_DESCRIPTION) {
                    return Err(format!("Part description must be text of at most {MAX_DESCRIPTION} characters"));
                }
                Ok(())
            }
        }
    }
    fn label(&self) -> String {
        match self {
            CadAnchor::Surface { node_name, node_id, .. } => if node_name.is_empty() { node_id.clone() } else { node_name.clone() },
            CadAnchor::Evidence { .. } => "Experiment evidence".into(),
            CadAnchor::Part { label, name, node_id, .. } => label.clone().filter(|l| !l.is_empty()).unwrap_or_else(|| if name.is_empty() { node_id.clone() } else { name.clone() }),
        }
    }
    fn missing(&self) -> bool {
        match self {
            CadAnchor::Surface { state, .. } => *state == AnchorStatus::Missing,
            CadAnchor::Evidence { .. } => false,
            CadAnchor::Part { available, .. } => !available,
        }
    }
    /// Presence only: whether the geometry under a pin changed is
    /// RoboCAD's to say (`anchor_status`), so an attached or needs-review
    /// state is kept while the part exists.
    fn refresh(&mut self, index: &BTreeMap<String, String>) -> bool {
        match self {
            CadAnchor::Surface { node_id, state, node_name, .. } => match index.get(node_id.as_str()) {
                Some(name) => {
                    *node_name = name.clone();
                    if *state == AnchorStatus::Missing {
                        *state = AnchorStatus::NeedsReview;
                    }
                    true
                }
                None => {
                    *state = AnchorStatus::Missing;
                    *node_name = "Deleted part".into();
                    false
                }
            },
            CadAnchor::Evidence { .. } => true,
            CadAnchor::Part { node_id, name, available, .. } => {
                match index.get(node_id.as_str()) {
                    Some(n) => {
                        *name = n.clone();
                        *available = true;
                    }
                    None => {
                        *name = "Deleted part".into();
                        *available = false;
                    }
                }
                *available
            }
        }
    }
}

/// A time as RoboCAD's lists show it: `created_at[:16]` with `T` as a space.
pub(crate) fn shown_time(iso: &str) -> String {
    iso.get(..16).map_or_else(|| iso.to_string(), |s| s.replace('T', " "))
}

/// A thread as `thread_detail` answers it, as the annotations service's
/// thread (see the module doc).
pub(crate) fn thread_of(t: &CadThread) -> Thread<CadAnchor> {
    let first = match (&t.anchor.node_id, t.anchor.point) {
        (Some(node_id), Some(point)) if t.anchor_status != AnchorStatus::Evidence => CadAnchor::Surface {
            node_id: node_id.clone(),
            point,
            face: t.anchor.face.clone(),
            face_index: None,
            view: t.view.as_object().cloned().unwrap_or_default(),
            state: t.anchor_status,
            node_name: t.node_name.clone(),
        },
        _ => CadAnchor::Evidence { evidence: t.evidence.clone().unwrap_or(Value::Null) },
    };
    let parts = t.linked_parts.iter().map(|p| CadAnchor::Part { node_id: p.part.node_id.clone(), label: p.part.label.clone(), description: p.part.description.clone(), view: p.part.view.clone(), name: p.name.clone(), available: p.available });
    let link = |label: String, id: String| {
        let known = t.linked_parts.iter().find(|p| p.part.node_id == id);
        CadAnchor::Part { name: known.map_or_else(|| label.clone(), |p| p.name.clone()), available: known.is_some_and(|p| p.available), node_id: id, label: Some(label), description: None, view: None }
    };
    Thread {
        id: t.id.clone(),
        title: t.node_name.clone(),
        resolved: t.resolved(),
        targets: std::iter::once(first).chain(parts).collect(),
        comments: t
            .comments
            .iter()
            .map(|c| Comment {
                id: c.id.clone(),
                author: c.author.clone(),
                body: c.body.clone(),
                created_at: shown_time(&c.created_at),
                edited_at: (!c.updated_at.is_empty() && c.updated_at != c.created_at).then(|| shown_time(&c.updated_at)),
                links: part_links(&c.body).into_iter().map(|(label, id)| link(label, id)).collect(),
            })
            .collect(),
        pin_m: None,
        view: None,
    }
}

/// One RoboCAD call a commit sends.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Request {
    Create(NewThread),
    Evidence(crate::cad::types::threads::NewEvidenceThread),
    Update { id: String, patch: ThreadPatch },
    DeleteThread { id: String },
    Reply { thread: String, body: String, author: String },
    EditComment { id: String, body: String },
    DeleteComment { id: String },
}
impl Request {
    /// Apply it to the archive being edited (`local::Workspace`): the new
    /// thread's or comment's id, the thread changed, or `{"deleted": id}`.
    /// `kind`: who writes a new thread's first comment or a reply.
    pub(crate) fn apply(self, ws: &mut crate::cad::local::Workspace, kind: sim_cad::annotations::AuthorKind) -> Result<Value, String> {
        use sim_cad::annotations as a;
        Ok(match self {
            Request::Create(t) => {
                let new = a::NewThread {
                    node_id: Some(t.node_id),
                    point: Some(serde_json::json!(t.point)),
                    face: t.face,
                    body: t.body,
                    author: t.author,
                    author_kind: Some(kind.name().into()),
                    view: t.view.map(Value::Object),
                    part_refs: t.part_refs.map(|p| value(&p)),
                    ..a::NewThread::default()
                };
                let (id, comment) = a::create(&mut ws.edit, ws.archive, ws.stamps, new)?;
                serde_json::json!({"id": id, "comment": comment})
            }
            Request::Evidence(t) => {
                if ws.archive.manifest["document_id"].as_str().is_some_and(|d| d != t.document_id) {
                    return Err("evidence.document_id: names another document".into());
                }
                let new = a::NewThread { body: t.body, author: t.author, author_kind: Some(kind.name().into()), evidence: Some(value(&t.evidence)), ..a::NewThread::default() };
                let (id, comment) = a::create(&mut ws.edit, ws.archive, ws.stamps, new)?;
                serde_json::json!({"id": id, "comment": comment})
            }
            Request::Update { id, patch } => {
                let p = a::ThreadPatch {
                    status: patch.status,
                    node_id: patch.node_id,
                    point: patch.point.map(|p| serde_json::json!(p)),
                    face: patch.face,
                    view: patch.view.map(Value::Object),
                    part_refs: patch.part_refs.map(|p| value(&p)),
                    ..a::ThreadPatch::default()
                };
                a::update(&mut ws.edit, ws.archive, ws.stamps, &id, p)?;
                serde_json::json!({"id": id})
            }
            Request::DeleteThread { id } => {
                a::delete(&mut ws.edit, &id)?;
                serde_json::json!({"deleted": id})
            }
            Request::Reply { thread, body, author } => {
                let id = a::reply(&mut ws.edit, &thread, &body, &author, kind)?;
                serde_json::json!({"id": id, "thread": thread})
            }
            Request::EditComment { id, body } => serde_json::json!({"id": id, "thread": a::change_comment(&mut ws.edit, &id, Some(&body))?}),
            Request::DeleteComment { id } => serde_json::json!({"deleted": id, "thread": a::change_comment(&mut ws.edit, &id, None)?}),
        })
    }
}

/// RoboCAD's threads as one commit sees them: the threads as read, the
/// document (its edit job and `ThreadsState::in_flight`), the call (a REST
/// caller waits for RoboCAD's answer) and the revision they were read at.
pub(crate) struct CadThreadSource<'a, 'c> {
    threads: BTreeMap<String, Thread<CadAnchor>>,
    doc: &'a mut CadDocument,
    call: &'a mut Call<'c>,
    began: Option<u64>,
    /// The edit's outcome (`Outcome::Pending` for a REST caller, which then
    /// waits for RoboCAD's answer as every edit's caller does).
    pub(crate) outcome: Option<Outcome>,
    /// The request sent (tests read it).
    #[allow(dead_code)]
    pub(crate) sent: Option<Request>,
}

impl<'a, 'c> CadThreadSource<'a, 'c> {
    /// The threads as last read (any revision of this connection; `began`
    /// is checked by `edit_at`).
    pub(crate) fn new(doc: &'a mut CadDocument, call: &'a mut Call<'c>, began: Option<u64>) -> Self {
        let threads: BTreeMap<String, Thread<CadAnchor>> = super::read::listed(doc).map(|l| l.iter().map(|t| (t.id.clone(), thread_of(t))).collect()).unwrap_or_default();
        Self { threads, doc, call, began, outcome: None, sent: None }
    }

    /// One request through `edit_at`; `thread` is what it is about.
    fn send(&mut self, label: &str, thread: Option<String>, request: Request) -> Result<Committed, String> {
        // The status line once RoboCAD answered (RoboCAD's after a post, comments.py:360).
        let message = match label {
            ADD | REPLY | EDIT_COMMENT => super::SAVED.to_string(),
            other => format!("{other} · Ctrl+Z undoes"),
        };
        self.sent = Some(request.clone());
        let kind = self.doc.threads.kind;
        let outcome = crate::cad::actions::local_edit_at(self.doc, self.call, self.began, label.to_string(), true, move |ws| request.apply(ws, kind).map(|result| EditDone { message, result }));
        if let Outcome::Done(Err(e)) = outcome {
            return Err(e);
        }
        let seq = self.doc.edit_seq;
        self.doc.threads.in_flight.submitted(seq, label, thread);
        self.doc.touch();
        self.outcome = Some(outcome);
        Ok(Committed::Pending(seq))
    }
}

/// The surface pin of a thread's first target.
fn surface(t: &Thread<CadAnchor>) -> Option<&CadAnchor> {
    t.targets.first().filter(|a| matches!(a, CadAnchor::Surface { .. }))
}

/// The linked parts (every target after the first) as RoboCAD stores them.
fn part_refs(t: &Thread<CadAnchor>) -> Vec<PartRef> {
    t.targets.iter().skip(1).filter_map(CadAnchor::part_ref).collect()
}

/// The one `PATCH /threads/{id}` that turns `old` into `new`, or why it
/// cannot be one RoboCAD call.
pub(crate) fn patch(old: &Thread<CadAnchor>, new: &Thread<CadAnchor>) -> Result<ThreadPatch, String> {
    if new.title != old.title || new.pin_m != old.pin_m || new.view != old.view {
        return Err("RoboCAD's comment threads have no title, pin or saved view of their own: the part's name is the title, and Reattach… moves the pin".into());
    }
    if new.comments != old.comments {
        return Err("a message is changed one at a time: edit, reply to or delete the message".into());
    }
    let mut p = ThreadPatch::default();
    if new.resolved != old.resolved {
        p.status = Some(if new.resolved { "resolved" } else { "open" }.into());
    }
    if new.targets.first() != old.targets.first() {
        match surface(new) {
            Some(CadAnchor::Surface { node_id, point, face_index, view, .. }) => {
                p.node_id = Some(node_id.clone());
                p.point = Some(*point);
                p.face = *face_index;
                p.view = (!view.is_empty()).then(|| view.clone());
            }
            _ => return Err("an annotation is reattached to a part's surface: Reattach…, then click a surface".into()),
        }
    }
    let parts = part_refs(new);
    if parts != part_refs(old) {
        p.part_refs = Some(parts);
    }
    if p == ThreadPatch::default() {
        return Err("nothing to change in this comment thread".into());
    }
    Ok(p)
}

impl ThreadSource for CadThreadSource<'_, '_> {
    type Anchor = CadAnchor;
    /// Never sent: RoboCAD assigns a new thread's and comment's ids.
    const THREAD_ID: &'static str = "new-thread";
    const COMMENT_ID: &'static str = "new-comment";
    fn threads(&self) -> BTreeMap<String, Thread<CadAnchor>> {
        self.threads.clone()
    }
    fn thread(&self, id: &str) -> Option<Thread<CadAnchor>> {
        self.threads.get(id).cloned()
    }
    /// A linked part is the same part whatever its label.
    fn same(a: &CadAnchor, b: &CadAnchor) -> bool {
        match (a, b) {
            (CadAnchor::Part { node_id: x, .. }, CadAnchor::Part { node_id: y, .. }) => x == y,
            _ => a == b,
        }
    }
    /// RoboCAD's limits (see the module doc).
    fn validate(&self, t: &Thread<CadAnchor>) -> Result<(), String> {
        let parts: Vec<&str> = t.targets.iter().skip(1).filter_map(|a| matches!(a, CadAnchor::Part { .. }).then(|| a.node()).flatten()).collect();
        if parts.len() > MAX_PARTS {
            return Err(format!("Linked parts must be a list of at most {MAX_PARTS} parts"));
        }
        if parts.iter().collect::<BTreeSet<_>>().len() != parts.len() {
            return Err("Linked part IDs must be unique strings".into());
        }
        for a in &t.targets {
            a.validate()?;
        }
        if t.comments.is_empty() {
            return Err("Comment must not be empty".into());
        }
        for c in &t.comments {
            text(&c.body, "Comment")?;
            text(&c.author, "Author")?;
        }
        Ok(())
    }
    fn commit(&mut self, label: &str, command: ThreadCommand<CadAnchor>) -> Result<Committed, String> {
        match command {
            ThreadCommand::PutThread { thread } if !self.threads.contains_key(&thread.id) => {
                if let Some(CadAnchor::Evidence { evidence }) = thread.targets.first() {
                    let evidence: crate::cad::types::threads::ExperimentEvidence =
                        serde_json::from_value(evidence.clone()).map_err(|e| format!("evidence: {e}"))?;
                    evidence.validate()?;
                    let first = thread.comments.first().ok_or("Comment must not be empty")?;
                    let request = crate::cad::types::threads::NewEvidenceThread {
                        body: first.body.clone(), author: first.author.clone(), evidence,
                        document_id: self.doc.doc.as_ref().and_then(|d| d.document_id.clone())
                            .ok_or("evidence.document_id: unavailable document identity")?,
                        expected_revision: self.began.ok_or("evidence.revision: required")?,
                    };
                    return self.send(if label.is_empty() { ADD } else { label }, None, Request::Evidence(request));
                }
                let Some(CadAnchor::Surface { node_id, point, face_index, view, .. }) = surface(&thread).cloned() else {
                    return Err("An annotation requires a part: Annotate model, then click a surface".into());
                };
                let Some(first) = thread.comments.first() else { return Err("Comment must not be empty".into()) };
                let parts = part_refs(&thread);
                let new = NewThread {
                    node_id,
                    point,
                    body: first.body.clone(),
                    author: first.author.clone(),
                    face: face_index,
                    view: (!view.is_empty()).then_some(view),
                    part_refs: (!parts.is_empty()).then_some(parts),
                };
                self.send(if label.is_empty() { ADD } else { label }, None, Request::Create(new))
            }
            ThreadCommand::Undo | ThreadCommand::Redo => Err(UNDO_IS_ROBOCADS.into()),
            command => {
                let (label, thread, request) = request_on(&self.threads, label, command)?;
                self.send(&label, thread, request)
            }
        }
    }
}

/// Why undo and redo are not thread commands here.
pub(crate) const UNDO_IS_ROBOCADS: &str = "RoboCAD's comments are undone and redone with the document's Undo and Redo (Ctrl+Z, Ctrl+Shift+Z): each change is one undo step there";

/// The one RoboCAD call a command on an existing thread is (shared by CAD
/// mode's [`CadThreadSource`] and Robot mode's source over the same
/// threads, `robot::threads`): its undo label, the thread it is about and
/// the request, or why not (refused before anything is sent, with
/// RoboCAD's own words where RoboCAD would refuse). `threads` are the
/// threads as last read. A new thread (a put of an unknown id) and undo or
/// redo are refused here: only CAD mode creates threads (Annotate model),
/// and undo is RoboCAD's document's.
pub(crate) fn request_on(threads: &BTreeMap<String, Thread<CadAnchor>>, label: &str, command: ThreadCommand<CadAnchor>) -> Result<(String, Option<String>, Request), String> {
    let known = |id: &str| threads.get(id).ok_or_else(|| format!("no comment thread {id} in RoboCAD's comments as last read"));
    let named = |default: &str| if label.is_empty() { default.to_string() } else { label.to_string() };
    match command {
        ThreadCommand::PutThread { thread } => {
            let old = threads.get(&thread.id).ok_or("a new comment thread is placed in CAD mode: Annotate model, then click a surface")?;
            let p = patch(old, &thread)?;
            let id = thread.id.clone();
            Ok((named(UPDATE), Some(id.clone()), Request::Update { id, patch: p }))
        }
        ThreadCommand::AddComment { thread, comment } => {
            known(&thread)?;
            text(&comment.body, "Comment")?;
            text(&comment.author, "Author")?;
            Ok((REPLY.into(), Some(thread.clone()), Request::Reply { thread, body: comment.body, author: comment.author }))
        }
        ThreadCommand::EditComment { thread, comment, body, .. } => {
            let t = known(&thread)?;
            if !t.comments.iter().any(|c| c.id == comment) {
                return Err(format!("annotation or comment not found: {comment}"));
            }
            text(&body, "Comment")?;
            Ok((EDIT_COMMENT.into(), Some(thread), Request::EditComment { id: comment, body }))
        }
        ThreadCommand::DeleteComment { thread, comment } => {
            let t = known(&thread)?;
            if !t.comments.iter().any(|c| c.id == comment) {
                return Err(format!("annotation or comment not found: {comment}"));
            }
            // RoboCAD's `_change_comment` refusal, before anything is sent.
            if t.comments.len() == 1 {
                return Err("delete the thread to remove its last comment".into());
            }
            Ok((DELETE_COMMENT.into(), Some(thread), Request::DeleteComment { id: comment }))
        }
        ThreadCommand::Resolve { thread, resolved } => {
            known(&thread)?;
            let patch = ThreadPatch { status: Some(if resolved { "resolved" } else { "open" }.into()), ..ThreadPatch::default() };
            Ok((named(UPDATE), Some(thread.clone()), Request::Update { id: thread, patch }))
        }
        ThreadCommand::DeleteThread { id } => {
            known(&id)?;
            Ok((DELETE_THREAD.into(), Some(id.clone()), Request::DeleteThread { id }))
        }
        ThreadCommand::Undo | ThreadCommand::Redo => Err(UNDO_IS_ROBOCADS.into()),
    }
}

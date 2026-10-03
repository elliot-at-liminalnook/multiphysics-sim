//! Inspect notes and saved views (`sim_inspect::annotations`, the
//! `*.annotations.json` sidecar beside a description): the notes' thread
//! adapter for `crate::annotations`, the sidecar's file store, and the
//! `annotations` request (REST and the notes panel's buttons). The panel is
//! `notes/panel.rs`.
//!
//! A note is shown as a thread: its label is the title, its target the
//! thread's anchor, its text the first message (no author or time; its id
//! is the note's) with the note's links, then its replies with theirs
//! ([`NoteAnchor`]), and its `resolved` the thread's. [`as_note`] turns the
//! thread back into the same note. Replies and resolves are the sidecar's
//! version 2 (`sim_inspect::annotations`, written only while used).
use super::*;
use crate::annotations::{Committed, ThreadOp, ThreadSource};
use crate::inspect::Owner;
use serde_json::{Value, json};
use sim_annotate::{Comment, Thread, ThreadCommand};
use sim_inspect::annotations as notes;

mod compose;
mod panel;
pub(crate) use compose::{build, compose};
pub(crate) use panel::{NotesPanel, clicks, guides, update};

/// A new note's colour (the notes panel's, as before).
const NOTE_COLOR: [u8; 3] = [30, 155, 160];

impl SpatialScene {
    /// The sidecar's store: its own worker (in `sim-annotate`) reads and
    /// writes the file, never the UI thread.
    pub fn connect_annotations(&mut self, path: std::path::PathBuf) {
        self.annotations = Some(notes::native::Store::new(std::sync::Arc::new(self.description.clone()), path));
    }
    /// Another system was opened: its own sidecar, no leftover emphasis or navigation.
    pub(crate) fn retarget_annotations(&mut self, path: std::path::PathBuf) {
        self.note_navigation = 0;
        self.note_hover = SelectionTarget::None;
        self.note_pointer_hover = SelectionTarget::None;
        self.note_error = None;
        self.connect_annotations(path);
    }
    pub(crate) fn note_document(&self) -> notes::Document {
        self.annotations.as_ref().map(|s| s.document()).unwrap_or_else(|| notes::Document::new(&self.description))
    }
}

/// What an Inspect note points at, as the thread panel shows it: the note's
/// own target (`link` None), one of its links, or one of a reply's links
/// (`reply`). A presentation of the note, never written to disk in this shape.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct NoteAnchor {
    pub label: String,
    /// The note it belongs to.
    pub note: String,
    /// Which link (None: the note's own target).
    pub link: Option<usize>,
    /// The reply whose link it is (None: the note's own).
    #[serde(default)]
    pub reply: Option<String>,
    pub target: notes::LinkTarget,
    pub missing: bool,
}
impl sim_annotate::Anchor for NoteAnchor {
    type Index = sim_inspect::SystemDescription;
    fn validate(&self) -> Result<(), String> {
        if self.label.trim().is_empty() || self.label.len() > 256 {
            return Err("link label is empty".into());
        }
        Ok(())
    }
    fn label(&self) -> String {
        self.label.clone()
    }
    fn missing(&self) -> bool {
        self.missing
    }
    fn refresh(&mut self, d: &sim_inspect::SystemDescription) -> bool {
        self.missing = match &self.target {
            notes::LinkTarget::Selection { target } => target.validate(d).is_err(),
            notes::LinkTarget::View { .. } => false,
        };
        !self.missing
    }
}

/// How a note's own target is named on its chip.
fn target_label(d: &sim_inspect::SystemDescription, target: &SelectionTarget) -> String {
    let (ids, one, many) = match target {
        SelectionTarget::None => return "nothing".into(),
        SelectionTarget::Components { ids } => (ids, ids.first().and_then(|id| d.components.get(id)).map(|c| c.label.clone()), "parts"),
        SelectionTarget::Ports { ids } => (ids, None, "ports"),
        SelectionTarget::Nets { ids } => (ids, None, "nets"),
    };
    match (ids.len(), one) {
        (1, Some(label)) => label,
        (1, None) => ids.first().cloned().unwrap_or_default(),
        (n, _) => format!("{n} {many}"),
    }
}

/// A link of a note (`reply` None) or of one of its replies, as an anchor.
fn link_anchor(note: &str, reply: Option<&str>, index: usize, link: &notes::Link) -> NoteAnchor {
    NoteAnchor { label: link.label.clone(), note: note.to_string(), link: Some(index), reply: reply.map(str::to_string), target: link.target.clone(), missing: false }
}

/// An anchor's link as stored.
fn anchor_link(a: &NoteAnchor) -> notes::Link {
    notes::Link { label: a.label.clone(), target: a.target.clone() }
}

/// A note as a thread: its text first (no author or time; the note's id
/// and links), then each reply as stored; resolved as the note is.
pub(crate) fn as_thread(note: &notes::Note, d: &sim_inspect::SystemDescription) -> Thread<NoteAnchor> {
    let own = NoteAnchor { label: target_label(d, &note.targets), note: note.id.clone(), link: None, reply: None, target: notes::LinkTarget::Selection { target: note.targets.clone() }, missing: note.targets.validate(d).is_err() };
    let links = note.links.iter().enumerate().map(|(i, l)| link_anchor(&note.id, None, i, l)).collect();
    let mut comments = vec![Comment { id: note.id.clone(), author: String::new(), body: note.text.clone(), created_at: String::new(), edited_at: None, links }];
    comments.extend(note.replies.iter().map(|r| Comment {
        id: r.id.clone(),
        author: r.author.clone(),
        body: r.body.clone(),
        created_at: r.created_at.clone(),
        edited_at: r.edited_at.clone(),
        links: r.links.iter().enumerate().map(|(i, l)| link_anchor(&note.id, Some(r.id.as_str()), i, l)).collect(),
    }));
    Thread { id: note.id.clone(), title: note.label.clone(), resolved: note.resolved, targets: vec![own], comments, pin_m: None, view: None }
}

/// A thread's comment as a stored reply.
fn as_reply(c: &Comment<NoteAnchor>) -> notes::Reply {
    Comment { id: c.id.clone(), author: c.author.clone(), body: c.body.clone(), created_at: c.created_at.clone(), edited_at: c.edited_at.clone(), links: c.links.iter().map(anchor_link).collect() }
}

/// The note a thread shows (`color`: the note's own, kept as it was): the
/// first comment is its text and links, the others its replies.
pub(crate) fn as_note(t: &Thread<NoteAnchor>, color: [u8; 3]) -> Result<notes::Note, String> {
    let targets = match t.targets.first().map(|a| &a.target) {
        Some(notes::LinkTarget::Selection { target }) => target.clone(),
        _ => return Err("a note points at parts, ports or nets".into()),
    };
    let text = t.comments.first();
    Ok(notes::Note {
        id: t.id.clone(),
        label: t.title.clone(),
        text: text.map(|c| c.body.clone()).unwrap_or_default(),
        targets,
        links: text.map(|c| c.links.iter().map(anchor_link).collect()).unwrap_or_default(),
        color,
        replies: t.comments.iter().skip(1).map(as_reply).collect(),
        resolved: t.resolved,
    })
}

/// Inspect's notes as a thread source. Saved views and navigation are not
/// threads: they stay `notes::Command`s ([`api`]).
pub(crate) struct InspectNotes<'a> {
    pub scene: &'a mut SpatialScene,
}
impl ThreadSource for InspectNotes<'_> {
    type Anchor = NoteAnchor;
    const THREAD_ID: &'static str = "note";
    const COMMENT_ID: &'static str = "note";
    fn threads(&self) -> BTreeMap<String, Thread<NoteAnchor>> {
        self.scene.note_document().notes.values().map(|n| (n.id.clone(), as_thread(n, &self.scene.description))).collect()
    }
    /// The sidecar validates the note it becomes (`notes::Document::validate`).
    fn validate(&self, _thread: &Thread<NoteAnchor>) -> Result<(), String> {
        Ok(())
    }
    fn commit(&mut self, _label: &str, command: ThreadCommand<NoteAnchor>) -> Result<Committed, String> {
        let doc = self.scene.note_document();
        let note = |id: &str| doc.notes.get(id).cloned().ok_or_else(|| "unknown annotation".to_string());
        let change = match command {
            ThreadCommand::PutThread { thread } => {
                let color = doc.notes.get(&thread.id).map_or(NOTE_COLOR, |n| n.color);
                notes::Command::PutNote { note: as_note(&thread, color)? }
            }
            ThreadCommand::DeleteThread { id } => notes::Command::DeleteNote { id },
            // The first comment is the note's text (its id is the note's).
            ThreadCommand::EditComment { thread, comment, body, .. } if comment == thread => notes::Command::PutNote { note: notes::Note { text: body, ..note(&thread)? } },
            ThreadCommand::EditComment { thread, comment, body, edited_at } => notes::Command::EditReply { note: thread, reply: comment, body, edited_at },
            ThreadCommand::AddComment { thread, comment } => notes::Command::AddReply { note: thread, reply: as_reply(&comment) },
            ThreadCommand::DeleteComment { thread, comment } => {
                if comment == thread {
                    return Err("delete the note to remove its text".into());
                }
                notes::Command::DeleteReply { note: thread, reply: comment }
            }
            ThreadCommand::Resolve { thread, resolved } => notes::Command::Resolve { note: thread, resolved },
            ThreadCommand::Undo => notes::Command::Undo,
            ThreadCommand::Redo => notes::Command::Redo,
        };
        // Reply and resolve commands touch one field of one note and
        // commute with other windows' edits: no revision check (the Store
        // applies them to the file as it is). A whole-note put, a delete,
        // undo and redo are checked against the revision shown.
        let expected = match &change {
            notes::Command::AddReply { .. } | notes::Command::EditReply { .. } | notes::Command::DeleteReply { .. } | notes::Command::Resolve { .. } => None,
            notes::Command::Undo | notes::Command::Redo | notes::Command::PutNote { .. } | notes::Command::DeleteNote { .. } | notes::Command::PutView { .. } | notes::Command::DeleteView { .. } | notes::Command::FollowView { .. } => Some(doc.revision),
        };
        let store = self.scene.annotations.as_mut().ok_or("annotation store not connected")?;
        store.submit(change, expected).map(Committed::Pending)
    }
}

/// Show `target` as Inspect's selection: checked against the assembly as
/// `set_selection` resolves it, written to the shared selection (`owner`:
/// Inspect's document), and projected to the view. Without an owner
/// (Build, Lessons) only the view shows it. Returns what is selected.
fn select(scene: &mut SpatialScene, owner: Option<&mut Owner>, target: SelectionTarget) -> Result<SelectionTarget, String> {
    // Inspect's one adapter; it leaves what is selected shown.
    crate::inspect::select(scene, owner, &target)?;
    Ok(scene.shown.clone())
}

/// Follow a saved view the sidecar navigated to (a restore here or in
/// another window): its camera, display toggles and selection.
pub(crate) fn sync(scene: &mut SpatialScene, camera: &mut Orbit, owner: Option<&mut Owner>) {
    let doc = scene.note_document();
    let Some(nav) = doc.navigation.as_ref().filter(|n| n.revision != scene.note_navigation) else { return };
    scene.note_navigation = nav.revision;
    let Some(view) = doc.views.get(&nav.view) else { return };
    if let Some(v) = &view.physical {
        // A cut: also ends a glide (which would carry on from its
        // start) and returns from the trackball.
        // A restored view stands still: a spin would turn away from it.
        camera.interrupt();
        camera.glide_to(crate::camera::Pose { focus: Vec3::from_array(v.focus), radius: v.radius, yaw: v.yaw, pitch: v.pitch }, 0.0);
        scene.state.exploded = v.exploded;
        scene.state.connections = v.connections;
        scene.state.hidden = v.hidden.clone();
    }
    if let Err(e) = select(scene, owner, view.selection.clone()) {
        scene.note_error = Some(e);
    }
}

fn store(scene: &mut SpatialScene) -> Result<&mut notes::native::Store, String> {
    scene.annotations.as_mut().ok_or_else(|| "annotation store not connected".to_string())
}

/// One thread op on the notes (`crate::annotations::apply`); the sidecar's
/// request id goes into the continuation, so the caller waits for the
/// Store's result like any other edit.
fn thread_op(scene: &mut SpatialScene, continuation: &mut Value, label: &str, op: ThreadOp<NoteAnchor>) -> Result<Option<Value>, String> {
    let applied = crate::annotations::apply(&mut InspectNotes { scene: &mut *scene }, label, op)?;
    match applied.committed {
        Committed::Pending(request) => {
            *continuation = json!(request);
            Ok(None)
        }
        Committed::Done => Ok(Some(json!(scene.note_document()))),
    }
}

/// The `annotations` request (REST, and the panel's buttons as the same
/// action). Edits go to the sidecar's worker; the continuation keeps the
/// request id until its result arrives.
pub(crate) fn api(scene: &mut SpatialScene, camera: &mut Orbit, mut owner: Option<&mut Owner>, action: notes::Request, continuation: &mut Value) -> sim_api::Outcome {
    let result = (|| -> Result<Option<Value>, String> {
        if let Some(id) = continuation.as_u64() {
            return store(scene)?.result(id).map(|r| r.map(|d| Some(json!(d)))).unwrap_or(Ok(None));
        }
        let doc = scene.note_document();
        let (change, expected) = match action {
            notes::Request::Document => return Ok(Some(json!(doc))),
            notes::Request::Emphasize { target } => {
                target.validate(&scene.description).map_err(|e| e.to_string())?;
                scene.note_hover = target;
                return Ok(Some(json!({"emphasized":scene.note_hover})));
            }
            notes::Request::SelectNote { id } => {
                let note = doc.notes.get(&id).ok_or("unknown annotation")?;
                let selected = select(scene, owner.as_deref_mut(), note.targets.clone())?;
                return Ok(Some(json!({"selection":selected})));
            }
            notes::Request::SaveView { id, label } => {
                let selection = owner.as_deref().map_or_else(|| scene.shown.clone(), |o| o.selection.target(o.document));
                let (yaw, pitch) = camera.turntable();
                let physical = notes::PhysicalView {
                    focus: camera.focus.to_array(),
                    radius: camera.radius,
                    // The heading drawn (the trackball's, when it is on).
                    yaw,
                    pitch,
                    exploded: scene.state.exploded,
                    connections: scene.state.connections,
                    hidden: scene.state.hidden.clone(),
                };
                (notes::Command::PutView { view: notes::SavedView { id, label, selection, schematic: None, physical: Some(physical) } }, None)
            }
            notes::Request::RestoreView { id } => (notes::Command::FollowView { id }, None),
            notes::Request::FollowLink { note, index, reply } => {
                let n = doc.notes.get(&note).ok_or("unknown annotation")?;
                let links = match &reply {
                    None => &n.links,
                    Some(reply) => &n.replies.iter().find(|r| &r.id == reply).ok_or("unknown annotation reply")?.links,
                };
                let link = links.get(index).ok_or("unknown annotation link")?;
                match &link.target {
                    notes::LinkTarget::Selection { target } => {
                        let selected = select(scene, owner.as_deref_mut(), target.clone())?;
                        return Ok(Some(json!({"selection":selected})));
                    }
                    notes::LinkTarget::View { id } => (notes::Command::FollowView { id: id.clone() }, None),
                }
            }
            notes::Request::Edit { change, expected_revision } => (change, expected_revision),
            // Replies, comment edits and resolves go through the one
            // annotations service, as the panel's buttons and keys do.
            notes::Request::Reply { note, body, author, id: None } => return thread_op(scene, continuation, "Reply", ThreadOp::Reply { thread: note, body, author, links: vec![] }),
            // The caller's id (the panel's draft): the same comment a new reply is, with that id.
            notes::Request::Reply { note, body, author, id: Some(id) } => {
                let comment = Comment { id, author, body, created_at: sim_annotate::stamp(), edited_at: None, links: vec![] };
                return thread_op(scene, continuation, "Reply", ThreadOp::Post { thread: note, comment });
            }
            notes::Request::EditComment { note, comment, body } => return thread_op(scene, continuation, "Edit comment", ThreadOp::EditComment { thread: note, comment, body, links: None }),
            notes::Request::DeleteComment { note, comment } => return thread_op(scene, continuation, "Delete comment", ThreadOp::DeleteComment { thread: note, comment }),
            notes::Request::Resolve { note, resolved } => return thread_op(scene, continuation, if resolved { "Resolve" } else { "Reopen" }, ThreadOp::Resolve { thread: note, resolved }),
        };
        *continuation = json!(store(scene)?.submit(change, expected)?);
        Ok(None)
    })();
    match result {
        Ok(Some(value)) => {
            scene.note_error = None;
            sync(scene, camera, owner);
            sim_api::Outcome::Done(Ok(value))
        }
        Ok(None) => sim_api::Outcome::Pending,
        Err(error) => sim_api::Outcome::Done(Err(error)),
    }
}

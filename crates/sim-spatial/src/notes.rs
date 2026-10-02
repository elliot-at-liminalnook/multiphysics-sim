//! Inspect notes and saved views (`sim_inspect::annotations`, the
//! `*.annotations.json` sidecar beside a description): the notes' thread
//! adapter for `crate::annotations`, the sidecar's file store, and the
//! `annotations` request (REST and the notes panel's buttons). The panel is
//! `notes/panel.rs`.
//!
//! A note is shown as a one-message thread: its label is the title, its
//! target the thread's anchor, its text the message and its links the
//! message's links ([`NoteAnchor`]). The note format on disk is unchanged
//! ([`as_note`] turns the thread back into the same note).
use super::*;
use crate::annotations::{Committed, ThreadSource};
use crate::inspect::Owner;
use serde_json::{Value, json};
use sim_annotate::{Comment, Thread, ThreadCommand};
use sim_inspect::annotations as notes;

mod panel;
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
/// own target (`link` None) or one of its links. A presentation of the
/// note, never written to disk in this shape.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct NoteAnchor {
    pub label: String,
    /// The note it belongs to.
    pub note: String,
    /// Which of the note's links (None: the note's own target).
    pub link: Option<usize>,
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

/// A note as a thread: one message (its text, no author or time) whose
/// links are the note's.
pub(crate) fn as_thread(note: &notes::Note, d: &sim_inspect::SystemDescription) -> Thread<NoteAnchor> {
    let own = NoteAnchor { label: target_label(d, &note.targets), note: note.id.clone(), link: None, target: notes::LinkTarget::Selection { target: note.targets.clone() }, missing: note.targets.validate(d).is_err() };
    let links = note.links.iter().enumerate().map(|(i, l)| NoteAnchor { label: l.label.clone(), note: note.id.clone(), link: Some(i), target: l.target.clone(), missing: false }).collect();
    let text = Comment { id: note.id.clone(), author: String::new(), body: note.text.clone(), created_at: String::new(), edited_at: None, links };
    Thread { id: note.id.clone(), title: note.label.clone(), resolved: false, targets: vec![own], comments: vec![text], pin_m: None, view: None }
}

/// The note a thread shows (`color`: the note's own, kept as it was).
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
        links: text.map(|c| c.links.iter().map(|a| notes::Link { label: a.label.clone(), target: a.target.clone() }).collect()).unwrap_or_default(),
        color,
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
            ThreadCommand::EditComment { thread, body, .. } => notes::Command::PutNote { note: notes::Note { text: body, ..note(&thread)? } },
            ThreadCommand::Undo => notes::Command::Undo,
            ThreadCommand::Redo => notes::Command::Redo,
            ThreadCommand::AddComment { .. } | ThreadCommand::DeleteComment { .. } | ThreadCommand::Resolve { .. } => {
                return Err("an Inspect note has one text: it takes no replies and is not resolved".into());
            }
        };
        let store = self.scene.annotations.as_mut().ok_or("annotation store not connected")?;
        store.submit(change, Some(doc.revision)).map(Committed::Pending)
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
            notes::Request::FollowLink { note, index } => {
                let link = doc.notes.get(&note).and_then(|n| n.links.get(index)).ok_or("unknown annotation link")?;
                match &link.target {
                    notes::LinkTarget::Selection { target } => {
                        let selected = select(scene, owner.as_deref_mut(), target.clone())?;
                        return Ok(Some(json!({"selection":selected})));
                    }
                    notes::LinkTarget::View { id } => (notes::Command::FollowView { id: id.clone() }, None),
                }
            }
            notes::Request::Edit { change, expected_revision } => (change, expected_revision),
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

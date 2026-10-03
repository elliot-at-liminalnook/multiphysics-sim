//! Source-bound discussion notes and saved inspection views, shared by both hosts.
//! These are authored presentation artifacts, never inputs to physics or CAD.
use crate::{DiagramState, SystemDescription, selection::SelectionTarget};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
/// A reply to a note: the shared comment shape (`sim_annotate::Comment`),
/// its links being note links.
pub type Reply = sim_annotate::Comment<Link>;
/// The newest format version this build reads. Version 2 adds a note's
/// `replies` and `resolved`; a file without them is written as version 1,
/// so older viewers keep reading it (see [`Document::apply`]).
pub const VERSION: u32 = 2;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Note {
    pub id: String,
    pub label: String,
    pub text: String,
    pub targets: SelectionTarget,
    #[serde(default)]
    pub links: Vec<Link>,
    pub color: [u8; 3],
    /// Replies after the note's own text, oldest first (version 2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replies: Vec<Reply>,
    /// The discussion is settled (version 2).
    #[serde(default, skip_serializing_if = "is_false")]
    pub resolved: bool,
}
fn is_false(value: &bool) -> bool {
    !*value
}
impl Note {
    /// Carries what only version 2 can store.
    pub fn needs_v2(&self) -> bool {
        !self.replies.is_empty() || self.resolved
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub label: String,
    pub target: LinkTarget,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LinkTarget {
    Selection { target: SelectionTarget },
    View { id: String },
}
/// Shared with system discussions and lessons (`sim-annotate`).
pub use sim_annotate::PhysicalView;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Focus {
    Component(String),
    Group(String),
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SchematicView {
    pub state: DiagramState,
    pub collapsed: BTreeSet<String>,
    pub focus: Option<Focus>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SavedView {
    pub id: String,
    pub label: String,
    pub selection: SelectionTarget,
    pub physical: Option<PhysicalView>,
    pub schematic: Option<SchematicView>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Navigation {
    pub revision: u64,
    pub view: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub version: u32,
    pub description_id: String,
    pub revision: u64,
    pub notes: BTreeMap<String, Note>,
    pub views: BTreeMap<String, SavedView>,
    pub navigation: Option<Navigation>,
    #[serde(default)]
    pub undo: Vec<Command>,
    #[serde(default)]
    pub redo: Vec<Command>,
}
impl Document {
    pub fn new(d: &SystemDescription) -> Self {
        Self {
            version: 1,
            description_id: d.id.clone(),
            revision: 0,
            notes: BTreeMap::new(),
            views: BTreeMap::new(),
            navigation: None,
            undo: vec![],
            redo: vec![],
        }
    }
    /// Refuse a format version this build cannot read, by name.
    fn check_version(version: u64) -> Result<(), String> {
        if version > u64::from(VERSION) {
            return Err(format!("annotations version {version} is newer than this viewer reads (1–{VERSION})"));
        }
        if version == 0 {
            return Err(format!("annotations version 0 is not one this viewer reads (1–{VERSION})"));
        }
        Ok(())
    }
    /// The version check on a file's raw JSON, before the typed parse: a
    /// newer file is refused for its version, not for the first field this
    /// build does not know (every struct denies unknown fields). Text that
    /// is not JSON, or has no whole-number `version` (`3` or `3.0`), is
    /// left to the typed parse.
    pub fn check_raw(bytes: &[u8]) -> Result<(), String> {
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else { return Ok(()) };
        let whole = |v: &serde_json::Value| v.as_u64().or_else(|| v.as_f64().filter(|f| f.is_finite() && *f >= 0. && f.fract() == 0.).map(|f| f as u64));
        match value.get("version").and_then(whole) {
            Some(version) => Self::check_version(version),
            None => Ok(()),
        }
    }
    /// Anything in the notes or in the undo/redo stacks that only version 2
    /// stores (an older viewer could not read the file).
    pub fn needs_v2(&self) -> bool {
        self.notes.values().any(Note::needs_v2) || self.undo.iter().chain(&self.redo).any(Command::needs_v2)
    }
    fn validate_link(&self, link: &Link, d: &SystemDescription) -> Result<(), String> {
        if link.label.trim().is_empty() || link.label.len() > 256 {
            return Err("link label is empty".into());
        }
        match &link.target {
            LinkTarget::Selection { target } => target.validate(d).map_err(|e| e.to_string()),
            LinkTarget::View { id } => {
                if !self.views.contains_key(id) {
                    return Err(format!("unknown saved-view link {id}"));
                }
                Ok(())
            }
        }
    }
    pub fn validate(&self, d: &SystemDescription) -> Result<(), String> {
        Self::check_version(u64::from(self.version))?;
        if self.description_id != d.id {
            return Err("annotations belong to another model or schema".into());
        }
        sim_annotate::history::validate(&self.undo, &self.redo, |c| {
            matches!(
                c,
                Command::Undo | Command::Redo | Command::FollowView { .. }
            )
        })?;
        if self.notes.len() > 256 || self.views.len() > 128 {
            return Err("annotation limit: 256 notes and 128 saved views".into());
        }
        for (id, v) in &self.views {
            if id != &v.id
                || id.trim().is_empty()
                || id.len() > 256
                || v.label.trim().is_empty()
                || v.label.len() > 256
                || v.physical.is_none() && v.schematic.is_none()
            {
                return Err("saved views need an ID, label and camera/layout".into());
            }
            v.selection.validate(d).map_err(|e| e.to_string())?;
            if let Some(p) = &v.physical {
                if !p
                    .focus
                    .iter()
                    .chain([p.radius, p.yaw, p.pitch].iter())
                    .all(|v| v.is_finite())
                    || p.radius <= 0.
                    || p.pitch.abs() > 1.5
                    || p.hidden.iter().any(|id| !d.components.contains_key(id))
                {
                    return Err("invalid physical saved view".into());
                }
            }
            if let Some(s) = &v.schematic {
                if !s.state.zoom.is_finite()
                    || s.state.zoom <= 0.
                    || !s.state.camera.x.is_finite()
                    || !s.state.camera.y.is_finite()
                    || s.state
                        .positions
                        .values()
                        .any(|p| !p.x.is_finite() || !p.y.is_finite())
                {
                    return Err("invalid schematic saved view".into());
                }
            }
        }
        for (id, n) in &self.notes {
            if id != &n.id
                || id.trim().is_empty()
                || id.len() > 256
                || n.label.trim().is_empty()
                || n.label.len() > 256
                || n.text.len() > 16384
                || n.links.len() > 64
            {
                return Err(
                    "notes need an ID and label; text/links exceed supported limits".into(),
                );
            }
            n.targets.validate(d).map_err(|e| e.to_string())?;
            if n.targets
                .resolve(d)
                .map_err(|e| e.to_string())?
                .components
                .is_empty()
            {
                return Err("a note must refer to at least one component, port or net".into());
            }
            for link in &n.links {
                self.validate_link(link, d)?;
            }
            if n.replies.len() > 256 {
                return Err("a note takes at most 256 replies".into());
            }
            let mut ids = BTreeSet::new();
            for r in &n.replies {
                // A reply's id never repeats the note's: the note's text is
                // shown as the comment with the note's id.
                if r.id.trim().is_empty()
                    || r.id.len() > 256
                    || r.id == n.id
                    || !ids.insert(r.id.as_str())
                    || r.author.trim().is_empty()
                    || r.author.len() > 256
                    || r.body.trim().is_empty()
                    || r.body.len() > 16384
                    || r.created_at.len() > 100
                    || r.edited_at.as_ref().is_some_and(|e| e.len() > 100)
                    || r.links.len() > 64
                {
                    return Err(
                        "replies need a unique ID, an author and text (at most 16384 bytes)".into(),
                    );
                }
                for link in &r.links {
                    self.validate_link(link, d)?;
                }
            }
        }
        if self
            .navigation
            .as_ref()
            .is_some_and(|n| !self.views.contains_key(&n.view))
        {
            return Err("active saved view no longer exists".into());
        }
        Ok(())
    }
    pub fn apply(&mut self, command: Command, d: &SystemDescription) -> Result<(), String> {
        let revision = self
            .revision
            .checked_add(1)
            .ok_or("annotation revision exhausted")?;
        let mut next = self.clone();
        let undo = matches!(command, Command::Undo);
        let redo = matches!(command, Command::Redo);
        let command = if undo {
            next.undo.pop().ok_or("nothing to undo")?
        } else if redo {
            next.redo.pop().ok_or("nothing to redo")?
        } else {
            command
        };
        let inverse = match &command {
            Command::PutNote { note } => Some(
                next.notes
                    .get(&note.id)
                    .cloned()
                    .map(|note| Command::PutNote { note })
                    .unwrap_or(Command::DeleteNote {
                        id: note.id.clone(),
                    }),
            ),
            Command::DeleteNote { id } => next
                .notes
                .get(id)
                .cloned()
                .map(|note| Command::PutNote { note }),
            Command::PutView { view } => Some(
                next.views
                    .get(&view.id)
                    .cloned()
                    .map(|view| Command::PutView { view })
                    .unwrap_or(Command::DeleteView {
                        id: view.id.clone(),
                    }),
            ),
            Command::DeleteView { id } => next
                .views
                .get(id)
                .cloned()
                .map(|view| Command::PutView { view }),
            Command::FollowView { .. } => None,
            // A reply edit or a resolve is undone by restoring the whole note.
            Command::AddReply { note, .. }
            | Command::EditReply { note, .. }
            | Command::DeleteReply { note, .. }
            | Command::Resolve { note, .. } => Some(Command::PutNote {
                note: next.notes.get(note).cloned().ok_or("unknown note")?,
            }),
            Command::Undo | Command::Redo => {
                return Err("invalid recursive annotation history".into());
            }
        };
        match command {
            Command::PutNote { note } => {
                next.notes.insert(note.id.clone(), note);
            }
            Command::DeleteNote { id } => {
                if next.notes.remove(&id).is_none() {
                    return Err("unknown note".into());
                }
            }
            Command::PutView { mut view } => {
                if let Some(old) = next.views.get(&view.id).filter(|_| !undo && !redo) {
                    if view.physical.is_none() {
                        view.physical = old.physical.clone();
                    }
                    if view.schematic.is_none() {
                        view.schematic = old.schematic.clone();
                    }
                }
                next.views.insert(view.id.clone(), view);
            }
            Command::DeleteView { id } => {
                if next.views.remove(&id).is_none() {
                    return Err("unknown saved view".into());
                }
                if next.navigation.as_ref().is_some_and(|n| n.view == id) {
                    next.navigation = None;
                }
            }
            Command::Undo | Command::Redo => unreachable!(),
            Command::FollowView { id } => {
                next.navigation = Some(Navigation { revision, view: id });
            }
            Command::AddReply { note, reply } => {
                let n = next.notes.get_mut(&note).ok_or("unknown note")?;
                if reply.id == n.id || n.replies.iter().any(|r| r.id == reply.id) {
                    return Err(format!("reply {} already exists", reply.id));
                }
                n.replies.push(reply);
            }
            Command::EditReply {
                note,
                reply,
                body,
                edited_at,
            } => {
                let n = next.notes.get_mut(&note).ok_or("unknown note")?;
                let r = n
                    .replies
                    .iter_mut()
                    .find(|r| r.id == reply)
                    .ok_or("unknown reply")?;
                r.body = body;
                r.edited_at = Some(edited_at);
            }
            Command::DeleteReply { note, reply } => {
                let n = next.notes.get_mut(&note).ok_or("unknown note")?;
                let before = n.replies.len();
                n.replies.retain(|r| r.id != reply);
                if n.replies.len() == before {
                    return Err("unknown reply".into());
                }
            }
            Command::Resolve { note, resolved } => {
                next.notes.get_mut(&note).ok_or("unknown note")?.resolved = resolved;
            }
        }
        if let Some(inverse) = inverse {
            let step = if undo {
                sim_annotate::history::Step::Undo
            } else if redo {
                sim_annotate::history::Step::Redo
            } else {
                sim_annotate::history::Step::Do
            };
            sim_annotate::history::record(&mut next.undo, &mut next.redo, inverse, step);
        }
        next.revision = revision;
        // Version 2 only while something needs it: a file without replies
        // or resolved notes (in its stacks too) stays readable by version 1.
        next.version = if next.needs_v2() { 2 } else { 1 };
        next.validate(d)?;
        *self = next;
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Undo,
    Redo,
    PutNote { note: Note },
    DeleteNote { id: String },
    PutView { view: SavedView },
    DeleteView { id: String },
    FollowView { id: String },
    /// A reply after the note's replies (version 2).
    AddReply { note: String, reply: Reply },
    /// New text for a reply (version 2).
    EditReply {
        note: String,
        reply: String,
        body: String,
        edited_at: String,
    },
    DeleteReply { note: String, reply: String },
    /// Resolve (true) or reopen a note (version 2).
    Resolve { note: String, resolved: bool },
}
impl Command {
    /// Only version 2 stores this command (in an undo or redo stack).
    pub fn needs_v2(&self) -> bool {
        match self {
            Command::PutNote { note } => note.needs_v2(),
            Command::AddReply { .. }
            | Command::EditReply { .. }
            | Command::DeleteReply { .. }
            | Command::Resolve { .. } => true,
            Command::Undo
            | Command::Redo
            | Command::DeleteNote { .. }
            | Command::PutView { .. }
            | Command::DeleteView { .. }
            | Command::FollowView { .. } => false,
        }
    }
}
impl sim_annotate::store::Revisioned for Document {
    type Command = Command;
    type Context = SystemDescription;
    fn empty(d: &SystemDescription) -> Self {
        Document::new(d)
    }
    fn revision(&self) -> u64 {
        self.revision
    }
    fn validate(&self, d: &SystemDescription) -> Result<(), String> {
        Document::validate(self, d)
    }
    fn apply(&mut self, command: Command, d: &SystemDescription) -> Result<(), String> {
        Document::apply(self, command, d)
    }
    fn check_raw(bytes: &[u8]) -> Result<(), String> {
        Document::check_raw(bytes)
    }
}
/// The locked, revisioned sidecar store shared with lessons (`sim-annotate`).
#[cfg(not(target_arch = "wasm32"))]
pub mod native {
    pub type Store = sim_annotate::store::Store<super::Document>;
}
#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> SystemDescription {
        serde_json::from_str(include_str!(
            "../../../examples/systems-viewer/spatial/motor-thermal.description.json"
        ))
        .unwrap()
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn two_stores_merge_independent_edits_reject_stale_revision_and_reopen() {
        use std::{
            sync::Arc,
            time::{Duration, Instant},
        };
        fn result(store: &mut native::Store, id: u64) -> Result<Document, String> {
            let start = Instant::now();
            loop {
                if let Some(r) = store.result(id) {
                    return r;
                }
                assert!(start.elapsed() < Duration::from_secs(5));
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let d = Arc::new(source());
        let directory = std::env::temp_dir().join(format!(
            "annotations-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("notes.json");
        let mut a = native::Store::new(d.clone(), path.clone());
        let mut b = native::Store::new(d.clone(), path.clone());
        let note = |id: &str| Note {
            id: id.into(),
            label: id.into(),
            text: "persisted".into(),
            targets: SelectionTarget::component(d.components.keys().next().unwrap().clone()),
            links: vec![],
            color: [30, 150, 160],
            replies: vec![],
            resolved: false,
        };
        let x = a
            .submit(Command::PutNote { note: note("a") }, None)
            .unwrap();
        let y = b
            .submit(Command::PutNote { note: note("b") }, None)
            .unwrap();
        result(&mut a, x).unwrap();
        result(&mut b, y).unwrap();
        let stale = a
            .submit(Command::DeleteNote { id: "a".into() }, Some(0))
            .unwrap();
        assert!(
            result(&mut a, stale)
                .unwrap_err()
                .contains("revision conflict")
        );
        drop(a);
        drop(b);
        let reopened = native::Store::new(d.clone(), path.clone());
        let start = Instant::now();
        while reopened.document().revision != 2 {
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(reopened.document().notes.len(), 2);
        let bytes = std::fs::read(&path).unwrap();
        let disk: Document = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(disk, reopened.document());
        drop(reopened);
        std::fs::remove_dir_all(directory).unwrap();
    }
    /// The shared-crate refactor and version 2 must not change a version-1
    /// sidecar: every committed Inspect sidecar (the repository's
    /// `*.annotations.json` other than the lesson's, which is a
    /// `ThreadDocument`) reads, through the store's reader too, and writes
    /// back to the same JSON with version 1 kept; an edit without replies or
    /// resolves keeps it version 1.
    #[test]
    fn committed_sidecars_round_trip_unchanged() {
        let d = source();
        let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/systems-viewer/evidence/rest-api");
        let mut checked = 0;
        for run in std::fs::read_dir(&base).unwrap().flatten() {
            let path = run.path().join("discussion.annotations.json");
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let original: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let doc: Document = serde_json::from_value(original.clone()).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!(doc.version, 1, "{}", path.display());
            assert!(!doc.needs_v2(), "{}", path.display());
            // Compare written text (f32 camera fields print as they were read).
            let written: serde_json::Value = serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
            assert_eq!(written, original, "{}", path.display());
            #[cfg(not(target_arch = "wasm32"))]
            {
                let read: Document = sim_annotate::store::read(&path, &d).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                assert_eq!(read, doc, "{}", path.display());
            }
            // A version-1 edit (a note put again as it is) stays version 1,
            // and its notes gain no `replies` or `resolved` keys.
            let mut edited = doc.clone();
            if let Some(first) = doc.notes.values().next().cloned() {
                edited.apply(Command::PutNote { note: first }, &d).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                assert_eq!(edited.version, 1, "{}", path.display());
                let json = serde_json::to_value(&edited).unwrap();
                for note in json["notes"].as_object().unwrap().values() {
                    assert!(note.get("replies").is_none() && note.get("resolved").is_none(), "{}", path.display());
                }
            }
            checked += 1;
        }
        assert!(checked >= 3, "found {checked} sidecars");
    }
    fn note(d: &SystemDescription, id: &str) -> Note {
        Note {
            id: id.into(),
            label: "Heat path".into(),
            text: "The winding heats the housing.".into(),
            targets: SelectionTarget::component(d.components.keys().next().unwrap().clone()),
            links: vec![],
            color: [30, 155, 160],
            replies: vec![],
            resolved: false,
        }
    }
    fn reply(id: &str) -> Reply {
        sim_annotate::Comment {
            id: id.into(),
            author: "Ada".into(),
            body: "It reaches 80 °C at the rated load.".into(),
            created_at: "1760000000".into(),
            edited_at: None,
            links: vec![],
        }
    }
    fn view(id: &str) -> SavedView {
        SavedView {
            id: id.into(),
            label: "View".into(),
            selection: SelectionTarget::None,
            physical: Some(PhysicalView {
                focus: [0.; 3],
                radius: 1.,
                yaw: 0.,
                pitch: 0.,
                exploded: false,
                connections: false,
                hidden: BTreeSet::new(),
            }),
            schematic: None,
        }
    }
    /// Reply, edit a reply, delete a reply, resolve and reopen: each is
    /// applied, undone and redone, and a bad one changes nothing.
    #[test]
    fn replies_and_resolve_apply_undo_and_redo() {
        let d = source();
        let mut doc = Document::new(&d);
        doc.apply(Command::PutNote { note: note(&d, "n") }, &d).unwrap();
        let first = reply("r1");
        // Reply.
        doc.apply(Command::AddReply { note: "n".into(), reply: first.clone() }, &d).unwrap();
        assert_eq!(doc.notes["n"].replies, vec![first.clone()]);
        doc.apply(Command::Undo, &d).unwrap();
        assert!(doc.notes["n"].replies.is_empty());
        doc.apply(Command::Redo, &d).unwrap();
        assert_eq!(doc.notes["n"].replies, vec![first.clone()]);
        // Refused without change: a repeated id, the note's own id, an
        // unknown note, no author, no text, a link to no saved view.
        let before = doc.clone();
        let refused = [
            Command::AddReply { note: "n".into(), reply: first.clone() },
            Command::AddReply { note: "n".into(), reply: reply("n") },
            Command::AddReply { note: "missing".into(), reply: reply("r2") },
            Command::AddReply { note: "n".into(), reply: sim_annotate::Comment { author: " ".into(), ..reply("r2") } },
            Command::AddReply { note: "n".into(), reply: sim_annotate::Comment { body: String::new(), ..reply("r2") } },
            Command::AddReply { note: "n".into(), reply: sim_annotate::Comment { links: vec![Link { label: "view".into(), target: LinkTarget::View { id: "missing".into() } }], ..reply("r2") } },
        ];
        for command in refused {
            assert!(doc.apply(command.clone(), &d).is_err(), "{command:?}");
        }
        assert_eq!(doc, before);
        // Edit a reply.
        doc.apply(Command::EditReply { note: "n".into(), reply: "r1".into(), body: "Edited".into(), edited_at: "1760000100".into() }, &d).unwrap();
        assert_eq!(doc.notes["n"].replies[0].body, "Edited");
        assert_eq!(doc.notes["n"].replies[0].edited_at.as_deref(), Some("1760000100"));
        doc.apply(Command::Undo, &d).unwrap();
        assert_eq!(doc.notes["n"].replies[0], first);
        doc.apply(Command::Redo, &d).unwrap();
        assert_eq!(doc.notes["n"].replies[0].body, "Edited");
        assert!(doc.apply(Command::EditReply { note: "n".into(), reply: "nope".into(), body: "x".into(), edited_at: "1".into() }, &d).unwrap_err().contains("unknown reply"));
        // Resolve, then reopen.
        doc.apply(Command::Resolve { note: "n".into(), resolved: true }, &d).unwrap();
        assert!(doc.notes["n"].resolved);
        doc.apply(Command::Undo, &d).unwrap();
        assert!(!doc.notes["n"].resolved);
        doc.apply(Command::Redo, &d).unwrap();
        assert!(doc.notes["n"].resolved);
        doc.apply(Command::Resolve { note: "n".into(), resolved: false }, &d).unwrap();
        assert!(!doc.notes["n"].resolved);
        doc.apply(Command::Undo, &d).unwrap();
        assert!(doc.notes["n"].resolved);
        doc.apply(Command::Redo, &d).unwrap();
        assert!(!doc.notes["n"].resolved);
        // Delete a reply.
        doc.apply(Command::DeleteReply { note: "n".into(), reply: "r1".into() }, &d).unwrap();
        assert!(doc.notes["n"].replies.is_empty());
        doc.apply(Command::Undo, &d).unwrap();
        assert_eq!(doc.notes["n"].replies.len(), 1);
        assert_eq!(doc.notes["n"].replies[0].body, "Edited");
        doc.apply(Command::Redo, &d).unwrap();
        assert!(doc.notes["n"].replies.is_empty());
        assert!(doc.apply(Command::DeleteReply { note: "n".into(), reply: "r1".into() }, &d).unwrap_err().contains("unknown reply"));
        // The whole document, stacks included, writes and reads back.
        let back: Document = serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(back, doc);
    }
    /// Version 2 while a note, or a command in the undo/redo stacks, holds
    /// a reply or a resolve; version 1 again once none does.
    #[test]
    fn version_is_two_only_while_replies_or_resolves_remain() {
        let d = source();
        let mut doc = Document::new(&d);
        doc.apply(Command::PutNote { note: note(&d, "n") }, &d).unwrap();
        assert_eq!(doc.version, 1);
        let json = serde_json::to_value(&doc).unwrap();
        assert!(json["notes"]["n"].get("replies").is_none() && json["notes"]["n"].get("resolved").is_none());
        doc.apply(Command::AddReply { note: "n".into(), reply: reply("r1") }, &d).unwrap();
        assert_eq!(doc.version, 2);
        assert_eq!(serde_json::to_value(&doc).unwrap()["version"], serde_json::json!(2));
        // The reply is gone from the note but kept in the undo stack.
        doc.apply(Command::DeleteReply { note: "n".into(), reply: "r1".into() }, &d).unwrap();
        assert!(!doc.notes["n"].needs_v2());
        assert_eq!(doc.version, 2, "the undo stack holds the note with its reply");
        // Undone twice: the note as first put, the reply only in redo.
        doc.apply(Command::Undo, &d).unwrap();
        doc.apply(Command::Undo, &d).unwrap();
        assert!(!doc.notes["n"].needs_v2());
        assert!(doc.undo.iter().all(|c| !c.needs_v2()));
        assert!(doc.redo.iter().any(Command::needs_v2));
        assert_eq!(doc.version, 2, "the redo stack holds the reply");
        // A new edit clears redo: nothing of version 2 is left.
        doc.apply(Command::PutView { view: view("v") }, &d).unwrap();
        assert_eq!(doc.version, 1);
        // A resolve alone is version 2 as well, and reopening (with its
        // history pushed out of the bounded stack) is version 1 again.
        doc.apply(Command::Resolve { note: "n".into(), resolved: true }, &d).unwrap();
        assert_eq!(doc.version, 2);
        assert_eq!(serde_json::to_value(&doc).unwrap()["notes"]["n"]["resolved"], serde_json::json!(true));
        doc.apply(Command::Resolve { note: "n".into(), resolved: false }, &d).unwrap();
        assert_eq!(doc.version, 2, "the stack still holds resolve commands' notes");
        for i in 0..sim_annotate::history::LIMIT {
            doc.apply(Command::PutView { view: view(&format!("v{i}")) }, &d).unwrap();
        }
        assert_eq!(doc.version, 1);
    }
    /// A file of a newer version is refused for its version, naming the
    /// file, even when it also has a field this build does not know.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_newer_version_is_refused_by_name_before_unknown_fields() {
        let d = source();
        let directory = std::env::temp_dir().join(format!(
            "annotations-v3-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("newer.annotations.json");
        let mut value = serde_json::to_value(Document::new(&d)).unwrap();
        value["version"] = serde_json::json!(3);
        value["layers"] = serde_json::json!({});
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let error = sim_annotate::store::read::<Document>(&path, &d).unwrap_err();
        assert_eq!(error, format!("{}: annotations version 3 is newer than this viewer reads (1–2)", path.display()));
        // A whole-number float version is refused by name too.
        let mut float = value.clone();
        float["version"] = serde_json::json!(3.0);
        std::fs::write(&path, serde_json::to_vec(&float).unwrap()).unwrap();
        assert_eq!(sim_annotate::store::read::<Document>(&path, &d).unwrap_err(), format!("{}: annotations version 3 is newer than this viewer reads (1–2)", path.display()));
        // The typed parse alone would have named the unknown field.
        assert!(serde_json::from_value::<Document>(value).unwrap_err().to_string().contains("unknown field"));
        // A version-2 file reads.
        let mut doc = Document::new(&d);
        doc.apply(Command::PutNote { note: note(&d, "n") }, &d).unwrap();
        doc.apply(Command::AddReply { note: "n".into(), reply: reply("r1") }, &d).unwrap();
        std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
        assert_eq!(sim_annotate::store::read::<Document>(&path, &d).unwrap(), doc);
        // `validate` names the version; another description keeps its message.
        let mut newer = Document::new(&d);
        newer.version = 3;
        assert!(newer.validate(&d).unwrap_err().contains("version 3"));
        let mut other = Document::new(&d);
        other.description_id = "another".into();
        assert_eq!(other.validate(&d).unwrap_err(), "annotations belong to another model or schema");
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn notes_allow_overlapping_multi_part_groups_but_reject_dangling_links() {
        let d = source();
        let ids = d.components.keys().take(2).cloned().collect();
        let mut doc = Document::new(&d);
        let note = Note {
            id: "n".into(),
            label: "Heat path".into(),
            text: "Shared discussion".into(),
            targets: SelectionTarget::Components { ids },
            links: vec![],
            color: [10, 140, 150],
            replies: vec![],
            resolved: false,
        };
        doc.apply(Command::PutNote { note: note.clone() }, &d)
            .unwrap();
        let mut bad = note;
        bad.links.push(Link {
            label: "view".into(),
            target: LinkTarget::View {
                id: "missing".into(),
            },
        });
        assert!(doc.apply(Command::PutNote { note: bad }, &d).is_err());
        assert_eq!(doc.revision, 1);
        doc.apply(Command::Undo, &d).unwrap();
        assert!(doc.notes.is_empty());
        doc.apply(Command::Redo, &d).unwrap();
        assert_eq!(doc.notes.len(), 1);
        let view = SavedView {
            id: "v".into(),
            label: "View".into(),
            selection: SelectionTarget::None,
            physical: Some(PhysicalView {
                focus: [0.; 3],
                radius: 1.,
                yaw: 0.,
                pitch: 0.,
                exploded: false,
                connections: false,
                hidden: BTreeSet::new(),
            }),
            schematic: None,
        };
        doc.apply(Command::PutView { view: view.clone() }, &d)
            .unwrap();
        let schematic = SavedView {
            physical: None,
            schematic: Some(SchematicView {
                state: DiagramState::new(&d),
                collapsed: BTreeSet::new(),
                focus: None,
            }),
            ..view.clone()
        };
        doc.apply(Command::PutView { view: schematic }, &d).unwrap();
        assert!(doc.views["v"].schematic.is_some());
        doc.apply(Command::Undo, &d).unwrap();
        assert_eq!(doc.views["v"], view);
        doc.apply(Command::Redo, &d).unwrap();
        assert!(doc.views["v"].schematic.is_some());
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Document,
    Edit {
        change: Command,
        #[serde(default)]
        expected_revision: Option<u64>,
    },
    SaveView {
        id: String,
        label: String,
    },
    RestoreView {
        id: String,
    },
    /// Follow link `index` of a note, or of its reply `reply`.
    FollowLink {
        note: String,
        index: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply: Option<String>,
    },
    SelectNote {
        id: String,
    },
    Emphasize {
        target: SelectionTarget,
    },
    /// A reply to a note, written now by `author`. `id` (optional) is the
    /// reply's id, chosen by the caller: a repeated post of the same id is
    /// refused ("reply … already exists"), so a retry never duplicates it,
    /// and the caller recognises its reply in the document.
    Reply {
        note: String,
        body: String,
        author: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// New text for a note's comment: the note's own text (`comment` is the
    /// note's id) or one of its replies.
    EditComment {
        note: String,
        comment: String,
        body: String,
    },
    /// Delete one of a note's replies (its own text goes with the note).
    DeleteComment {
        note: String,
        comment: String,
    },
    /// Resolve (true) or reopen a note.
    Resolve {
        note: String,
        resolved: bool,
    },
}

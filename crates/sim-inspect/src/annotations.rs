//! Source-bound discussion notes and saved inspection views, shared by both hosts.
//! These are authored presentation artifacts, never inputs to physics or CAD.
use crate::{DiagramState, SystemDescription, selection::SelectionTarget};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
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
    pub fn validate(&self, d: &SystemDescription) -> Result<(), String> {
        if self.version != 1 || self.description_id != d.id {
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
                if link.label.trim().is_empty() || link.label.len() > 256 {
                    return Err("link label is empty".into());
                }
                match &link.target {
                    LinkTarget::Selection { target } => {
                        target.validate(d).map_err(|e| e.to_string())?
                    }
                    LinkTarget::View { id } => {
                        if !self.views.contains_key(id) {
                            return Err(format!("unknown saved-view link {id}"));
                        }
                    }
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
    /// The shared-crate refactor must not change the sidecar format: every
    /// committed sidecar reads and writes back to the same JSON.
    #[test]
    fn committed_sidecars_round_trip_unchanged() {
        let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/systems-viewer/evidence/rest-api");
        let mut checked = 0;
        for run in std::fs::read_dir(&base).unwrap().flatten() {
            let path = run.path().join("discussion.annotations.json");
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let original: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let doc: Document = serde_json::from_value(original.clone()).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            // Compare written text (f32 camera fields print as they were read).
            let written: serde_json::Value = serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
            assert_eq!(written, original, "{}", path.display());
            checked += 1;
        }
        assert!(checked >= 3, "found {checked} sidecars");
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
    FollowLink {
        note: String,
        index: usize,
    },
    SelectNote {
        id: String,
    },
    Emphasize {
        target: SelectionTarget,
    },
}

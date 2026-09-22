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
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PhysicalView {
    pub focus: [f32; 3],
    pub radius: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub exploded: bool,
    pub connections: bool,
    pub hidden: BTreeSet<String>,
}
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
        if self.undo.len() > 32
            || self.redo.len() > 32
            || self.undo.iter().chain(&self.redo).any(|c| {
                matches!(
                    c,
                    Command::Undo | Command::Redo | Command::FollowView { .. }
                )
            })
        {
            return Err("invalid annotation history".into());
        }
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
            if undo {
                next.redo.push(inverse);
            } else {
                next.undo.push(inverse);
                if !redo {
                    next.redo.clear();
                }
            }
            if next.undo.len() > 32 {
                next.undo.remove(0);
            }
            if next.redo.len() > 32 {
                next.redo.remove(0);
            }
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
#[cfg(not(target_arch = "wasm32"))]
pub mod native {
    use super::*;
    use std::{
        fs::{self, File, OpenOptions, TryLockError},
        io::{Read, Write},
        path::{Path, PathBuf},
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    struct Request {
        id: u64,
        command: Command,
        expected: Option<u64>,
    }
    struct State {
        document: Document,
        error: Option<String>,
        results: BTreeMap<u64, Result<Document, String>>,
    }
    pub struct Store {
        state: Arc<Mutex<State>>,
        tx: mpsc::SyncSender<Request>,
        next: u64,
        stop: Arc<AtomicBool>,
        pub path: PathBuf,
    }
    fn read(path: &Path, d: &SystemDescription) -> Result<Document, String> {
        let mut bytes = Vec::new();
        match File::open(path) {
            Ok(f) => {
                f.take(4_194_305)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Document::new(d)),
            Err(e) => return Err(e.to_string()),
        }
        if bytes.len() > 4_194_304 {
            return Err("annotation file exceeds 4 MiB".into());
        }
        let doc: Document = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        doc.validate(d)?;
        Ok(doc)
    }
    fn edit(path: &Path, d: &SystemDescription, request: Request) -> Result<Document, String> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(format!("{}.lock", path.display()))
            .map_err(|e| e.to_string())?;
        let mut acquired = false;
        for _ in 0..20 {
            match lock.try_lock() {
                Ok(()) => {
                    acquired = true;
                    break;
                }
                Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(5)),
                Err(e) => return Err(e.to_string()),
            }
        }
        if !acquired {
            return Err("annotation file is busy; retry with a fresh revision".into());
        }
        let mut doc = read(path, d)?;
        if request.expected.is_some_and(|r| r != doc.revision) {
            return Err("annotation revision conflict; read annotations and merge edits".into());
        }
        doc.apply(request.command, d)?;
        let bytes = serde_json::to_vec_pretty(&doc).map_err(|e| e.to_string())?;
        if bytes.len() > 4_194_304 {
            return Err("annotation file exceeds 4 MiB".into());
        }
        let tmp = PathBuf::from(format!(
            "{}.{}.{}.tmp",
            path.display(),
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let result = (|| -> std::io::Result<()> {
            let mut f = OpenOptions::new().create_new(true).write(true).open(&tmp)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
            fs::rename(&tmp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result.map_err(|e| e.to_string())?;
        Ok(doc)
    }
    impl Store {
        pub fn new(d: Arc<SystemDescription>, path: PathBuf) -> Self {
            let state = Arc::new(Mutex::new(State {
                document: Document::new(&d),
                error: None,
                results: BTreeMap::new(),
            }));
            let worker = state.clone();
            let stop = Arc::new(AtomicBool::new(false));
            let stopping = stop.clone();
            let (tx, rx) = mpsc::sync_channel::<Request>(32);
            let file = path.clone();
            std::thread::spawn(move || {
                while !stopping.load(Ordering::Relaxed) {
                    match rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(request) => {
                            let id = request.id;
                            let result = edit(&file, &d, request);
                            let mut s = worker.lock().unwrap();
                            if let Ok(doc) = &result {
                                s.document = doc.clone();
                                s.error = None;
                            } else {
                                s.error = result.as_ref().err().cloned();
                            }
                            if s.results.len() >= 64 {
                                s.results.pop_first();
                            }
                            s.results.insert(id, result);
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => match read(&file, &d) {
                            Ok(doc) => {
                                let mut s = worker.lock().unwrap();
                                if s.document.revision != doc.revision {
                                    s.error = None;
                                }
                                s.document = doc;
                            }
                            Err(e) => worker.lock().unwrap().error = Some(e),
                        },
                    }
                }
            });
            Self {
                state,
                tx,
                next: 1,
                stop,
                path,
            }
        }
        pub fn document(&self) -> Document {
            self.state.lock().unwrap().document.clone()
        }
        pub fn error(&self) -> Option<String> {
            self.state.lock().unwrap().error.clone()
        }
        pub fn submit(&mut self, command: Command, expected: Option<u64>) -> Result<u64, String> {
            let id = self.next;
            self.tx
                .try_send(Request {
                    id,
                    command,
                    expected,
                })
                .map_err(|e| e.to_string())?;
            self.next += 1;
            Ok(id)
        }
        pub fn result(&mut self, id: u64) -> Option<Result<Document, String>> {
            self.state.lock().unwrap().results.remove(&id)
        }
    }
    impl Drop for Store {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }
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

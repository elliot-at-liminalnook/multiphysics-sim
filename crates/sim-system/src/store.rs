//! A system file shared by several editors (both viewers, REST, the CLI).
//! Every edit is a locked read-modify-write of the file, and undo/redo use a
//! shared journal beside it, so all editors see one history.
use crate::commands::{apply, Command, Outcome};
use crate::document::SystemDocument;
use crate::resolve::Resolver;
use crate::SystemError;
use serde::{Deserialize, Serialize};
use sim_core::BehaviorRegistry;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

const HISTORY_LIMIT: usize = 200;

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), SystemError> {
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Journal {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    label: String,
    /// Document to restore.
    document: SystemDocument,
    /// Content hash the file must have for this entry to apply.
    expects: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Applied {
    pub revision: u64,
    pub content_hash: String,
    pub outcomes: Vec<Outcome>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryView {
    pub undo: Vec<String>,
    pub redo: Vec<String>,
}

pub struct SystemStore {
    pub path: PathBuf,
}

struct Lock(PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl SystemStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Create a new file; refuses to overwrite.
    pub fn create(path: impl Into<PathBuf>, document: &SystemDocument) -> Result<Self, SystemError> {
        let store = Self::new(path);
        if store.path.exists() {
            return Err(SystemError::Invalid(format!("{} already exists", store.path.display())));
        }
        if let Some(parent) = store.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_atomic(&store.path, &serde_json::to_vec_pretty(document)?)?;
        Ok(store)
    }

    fn journal_path(&self) -> PathBuf {
        let name = self.path.file_name().unwrap().to_string_lossy().to_string();
        self.path.with_file_name(format!(".{name}.history.json"))
    }

    fn lock(&self) -> Result<Lock, SystemError> {
        let name = self.path.file_name().unwrap().to_string_lossy().to_string();
        let path = self.path.with_file_name(format!(".{name}.lock"));
        let start = Instant::now();
        loop {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Ok(Lock(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    // A crashed editor may leave a lock behind.
                    let stale = std::fs::metadata(&path).and_then(|m| m.modified()).map(|t| SystemTime::now().duration_since(t).unwrap_or_default() > Duration::from_secs(10)).unwrap_or(false);
                    if stale {
                        let _ = std::fs::remove_file(&path);
                        continue;
                    }
                    if start.elapsed() > Duration::from_secs(5) {
                        return Err(SystemError::Invalid(format!("{} is locked by another editor", self.path.display())));
                    }
                    std::thread::sleep(Duration::from_millis(15));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    pub fn load(&self) -> Result<SystemDocument, SystemError> {
        Ok(serde_json::from_slice(&std::fs::read(&self.path)?)?)
    }

    /// Load and fully validate against a registry.
    pub fn load_valid(&self, registry: &BehaviorRegistry) -> Result<SystemDocument, SystemError> {
        let document = self.load()?;
        Resolver::new(&document, registry).validate()?;
        Ok(document)
    }

    fn read_journal(&self) -> Journal {
        std::fs::read(self.journal_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn write_journal(&self, journal: &Journal) -> Result<(), SystemError> {
        write_atomic(&self.journal_path(), &serde_json::to_vec(journal)?)
    }

    /// Apply a batch atomically. `expected_revision` rejects edits made
    /// against an out-of-date copy.
    pub fn apply(&self, registry: &BehaviorRegistry, label: &str, commands: &[Command], expected_revision: Option<u64>) -> Result<Applied, SystemError> {
        let _lock = self.lock()?;
        let before = self.load()?;
        if let Some(expected) = expected_revision {
            if expected != before.revision {
                return Err(SystemError::Stale { expected, found: before.revision });
            }
        }
        let mut after = before.clone();
        let outcomes = apply(&mut after, registry, commands)?;
        write_atomic(&self.path, &serde_json::to_vec_pretty(&after)?)?;
        let mut journal = self.read_journal();
        journal.undo.push(Entry { label: label.into(), document: before, expects: after.content_hash() });
        if journal.undo.len() > HISTORY_LIMIT {
            journal.undo.remove(0);
        }
        journal.redo.clear();
        self.write_journal(&journal)?;
        Ok(Applied { revision: after.revision, content_hash: after.content_hash(), outcomes })
    }

    pub fn undo(&self) -> Result<Applied, SystemError> {
        self.step(true)
    }

    pub fn redo(&self) -> Result<Applied, SystemError> {
        self.step(false)
    }

    fn step(&self, undo: bool) -> Result<Applied, SystemError> {
        let _lock = self.lock()?;
        let current = self.load()?;
        let mut journal = self.read_journal();
        let (from, to) = if undo { (&mut journal.undo, &mut journal.redo) } else { (&mut journal.redo, &mut journal.undo) };
        let entry = from.pop().ok_or_else(|| SystemError::Invalid(format!("nothing to {}", if undo { "undo" } else { "redo" })))?;
        if entry.expects != current.content_hash() {
            return Err(SystemError::Invalid("the file changed outside the shared history; undo is unavailable for this edit".into()));
        }
        let mut restored = entry.document.clone();
        restored.revision = current.revision + 1;
        to.push(Entry { label: entry.label.clone(), document: current, expects: restored.content_hash() });
        write_atomic(&self.path, &serde_json::to_vec_pretty(&restored)?)?;
        self.write_journal(&journal)?;
        Ok(Applied {
            revision: restored.revision,
            content_hash: restored.content_hash(),
            outcomes: vec![Outcome { definition: None, shared_by: 0, message: format!("{} {}", if undo { "Undid" } else { "Redid" }, entry.label) }],
        })
    }

    pub fn history(&self) -> HistoryView {
        let journal = self.read_journal();
        HistoryView { undo: journal.undo.iter().rev().map(|e| e.label.clone()).collect(), redo: journal.redo.iter().rev().map(|e| e.label.clone()).collect() }
    }

    /// Modification stamp for cheap change polling by viewers.
    pub fn stamp(&self) -> Option<SystemTime> {
        std::fs::metadata(&self.path).and_then(|m| m.modified()).ok()
    }

    /// Import an image and place it as a reference in one undoable edit.
    pub fn import_reference(&self, registry: &BehaviorRegistry, at: &str, id: &str, image: &Path, view: crate::document::ReferenceView, origin: [f32; 3], width: f32) -> Result<Applied, SystemError> {
        let (asset_id, asset) = crate::assets::import_image(&self.path, image)?;
        let label = asset.original_name.clone();
        self.apply(
            registry,
            &format!("Import reference {label}"),
            &[
                Command::RegisterAsset { id: asset_id.clone(), asset },
                Command::SetReference {
                    at: at.into(),
                    id: id.into(),
                    reference: crate::document::ReferenceImage {
                        label,
                        asset: asset_id,
                        view,
                        origin,
                        normal: [0., 1., 0.],
                        x_axis: [1., 0., 0.],
                        width,
                        opacity: 0.6,
                        locked: false,
                        visible: true,
                    },
                },
            ],
            None,
        )
    }
}

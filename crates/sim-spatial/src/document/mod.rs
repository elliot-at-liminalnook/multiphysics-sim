//! The one document registry (native-viewer.md §7): one entry per mode's
//! document, open in the window, parked while another mode is shown, or
//! remembered so the mode reopens it. Each entry has an id, a kind, its
//! source (path, preset, URL, lessons folder, exhibit), a revision and the
//! mode that owns it.
//!
//! - **Ids** are unique for the window's life: a new source gets a new id,
//!   so a selection item or a job result naming an older document can
//!   never match the new one ([`DocumentRegistry::open`]).
//! - **Revisions** are the counts each mode already keeps (the builder's
//!   `document.revision`, RoboCAD's revision, a reload of the same source):
//!   the mode copies its counter with [`DocumentRegistry::set_revision`] or
//!   bumps it on a reload ([`DocumentRegistry::open`] of the same source).
//!   No edit tracking of its own.
//! - **Parked scenes** (Inspect's scene and selection link, the builder's
//!   scene) live on their mode's entry while another mode is shown
//!   ([`DocumentRegistry::park`], [`DocumentRegistry::unpark`]).
//!
//! `app::switch::Documents` keeps only the launch configuration (library,
//! models, presets, hardware); the switch, the picker, the recent documents
//! and every mode's arrival and leaving read and write this registry.
use crate::app::ViewerMode;
use crate::{SelectionLink, SpatialScene};
use bevy::prelude::*;
use serde::Serialize;
use serde_json::{Value, json};
use std::path::PathBuf;

/// A document's identity for the window's life (never reused).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct DocumentId(pub u64);

/// What a document is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    /// Inspect: a `*.description.json` with its `*.spatial.json`.
    Assembly,
    /// Build (and Lessons, over its sandbox copy): a `*.system.json`.
    System,
    /// Lessons: a folder of `<slug>/lesson.md`.
    Lessons,
    /// Robot: a `*.simrobot.json` or a preset.
    Robot,
    /// Place: a sim-place build directory.
    Place,
    /// CAD: a RoboCAD document (a `.rcad` file or a running service).
    Cad,
    /// Phenomena: the compiled-in exhibits.
    Exhibits,
}

/// Where a document comes from.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    /// A file or directory (system, place, robot file, `.rcad`).
    Path { path: PathBuf },
    /// An inspected assembly: its description and spatial files.
    Assembly { description: PathBuf, spatial: PathBuf },
    /// A lessons folder and the lesson last open in it.
    Lessons { dir: PathBuf, lesson: Option<String> },
    /// A robot preset id.
    Preset { id: String },
    /// A running RoboCAD service (loopback).
    Url { url: String },
    /// The exhibit phenomena mode opens (`--exhibit`'s rule; None: the first).
    Exhibit { exhibit: Option<String> },
}
impl Source {
    pub fn path(path: impl Into<PathBuf>) -> Self {
        Source::Path { path: path.into() }
    }
    /// Whether `other` names the same document: a lesson, an exhibit or a
    /// reopened assembly within the same folder or set is the same one.
    pub fn same_document(&self, other: &Source) -> bool {
        match (self, other) {
            (Source::Lessons { dir: a, .. }, Source::Lessons { dir: b, .. }) => a == b,
            (Source::Exhibit { .. }, Source::Exhibit { .. }) => true,
            _ => self == other,
        }
    }
    /// As a person names it.
    pub fn describe(&self) -> String {
        match self {
            Source::Path { path } => path.display().to_string(),
            Source::Assembly { description, .. } => description.display().to_string(),
            Source::Lessons { dir, lesson: Some(slug) } => format!("{} ({slug})", dir.display()),
            Source::Lessons { dir, lesson: None } => dir.display().to_string(),
            Source::Preset { id } => format!("preset {id}"),
            Source::Url { url } => format!("RoboCAD at {url}"),
            Source::Exhibit { exhibit: Some(e) } => format!("exhibit {e}"),
            Source::Exhibit { exhibit: None } => "the first exhibit".into(),
        }
    }
}

/// Whether the document is in the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    /// Its mode is shown with it (or, for the builder, it is loaded).
    Open,
    /// Another mode is shown; its scene is kept here to come back as it was.
    Parked,
    /// Closed; its mode reopens it when chosen without a document.
    Remembered,
}

/// What a parked mode keeps while another mode is shown.
pub enum Parked {
    /// Inspect's scene and its selection link.
    Inspect(Box<(SpatialScene, Option<SelectionLink>)>),
    /// The builder's scene (Build and Lessons share the builder).
    Builder(Box<SpatialScene>),
}

/// One document.
pub struct DocumentEntry {
    pub id: DocumentId,
    pub kind: DocumentKind,
    pub source: Source,
    /// The owning mode's own count of edits and reloads.
    pub revision: u64,
    /// The mode that owns it.
    pub mode: ViewerMode,
    pub presence: Presence,
    parked: Option<Parked>,
}
impl DocumentEntry {
    pub fn json(&self) -> Value {
        json!({"id": self.id, "kind": self.kind, "source": self.source, "revision": self.revision, "mode": self.mode, "presence": self.presence, "parked": self.parked.is_some()})
    }
    pub fn parked(&self) -> bool {
        self.parked.is_some()
    }
}

/// The outcome of [`DocumentRegistry::open`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opened {
    pub id: DocumentId,
    /// The same source again: the id stays and the revision advanced.
    pub reload: bool,
    /// The document this one replaced (its selection items are dropped by
    /// `Selection::forget`, called by whoever opens).
    pub replaced: Option<DocumentId>,
}

/// The registry (a resource that survives every switch).
#[derive(Resource, Default)]
pub struct DocumentRegistry {
    /// At most one entry per mode, in the order they were first opened.
    entries: Vec<DocumentEntry>,
    next: u64,
    /// Bumped whenever an entry is added, replaced, re-revisioned, parked or closed.
    pub changed: u64,
}

impl DocumentRegistry {
    /// The entry `mode` owns (open, parked or remembered).
    pub fn entry(&self, mode: ViewerMode) -> Option<&DocumentEntry> {
        self.entries.iter().find(|e| e.mode == mode)
    }
    fn entry_mut(&mut self, mode: ViewerMode) -> Option<&mut DocumentEntry> {
        self.entries.iter_mut().find(|e| e.mode == mode)
    }
    pub fn get(&self, id: DocumentId) -> Option<&DocumentEntry> {
        self.entries.iter().find(|e| e.id == id)
    }
    pub fn entries(&self) -> impl Iterator<Item = &DocumentEntry> {
        self.entries.iter()
    }
    /// The source `mode` reopens.
    pub fn source(&self, mode: ViewerMode) -> Option<&Source> {
        self.entry(mode).map(|e| &e.source)
    }
    /// `mode`'s document and its revision, while it is open (not parked or remembered).
    pub fn current(&self, mode: ViewerMode) -> Option<(DocumentId, u64)> {
        self.entry(mode).filter(|e| e.presence == Presence::Open).map(|e| (e.id, e.revision))
    }
    /// The revision of `id` (None: no such document).
    pub fn revision(&self, id: DocumentId) -> Option<u64> {
        self.get(id).map(|e| e.revision)
    }

    /// `mode` now shows `source` (a load, a reopen, an Open in the mode).
    /// The same document again is a reload: the id stays, the revision
    /// advances by one and a parked scene is kept for the caller to unpark.
    /// Another source gets a new id at revision 0 (the old entry and its
    /// parked scene go; the caller drops what was parked off the UI thread
    /// with [`DocumentRegistry::take_parked`] first if it needs to).
    pub fn open(&mut self, mode: ViewerMode, kind: DocumentKind, source: Source) -> Opened {
        self.changed += 1;
        if let Some(e) = self.entry_mut(mode) {
            if e.kind == kind && e.source.same_document(&source) {
                // Same document: a lesson or exhibit choice is kept as given.
                e.source = source;
                e.revision += 1;
                e.presence = Presence::Open;
                return Opened { id: e.id, reload: true, replaced: None };
            }
        }
        self.next += 1;
        let id = DocumentId(self.next);
        let entry = DocumentEntry { id, kind, source, revision: 0, mode, presence: Presence::Open, parked: None };
        let replaced = match self.entries.iter().position(|e| e.mode == mode) {
            Some(i) => Some(std::mem::replace(&mut self.entries[i], entry).id),
            None => {
                self.entries.push(entry);
                None
            }
        };
        Opened { id, reload: false, replaced }
    }
    /// Remember `source` for `mode` without opening it (the launch's
    /// choices, a picker's preselection). Keeps an existing entry's id when
    /// the source is the same document.
    pub fn remember(&mut self, mode: ViewerMode, kind: DocumentKind, source: Source) -> DocumentId {
        if let Some(e) = self.entry_mut(mode).filter(|e| e.kind == kind && e.source.same_document(&source)) {
            e.source = source;
            let id = e.id;
            self.changed += 1;
            return id;
        }
        let opened = self.open(mode, kind, source);
        if let Some(e) = self.entry_mut(mode) {
            e.presence = Presence::Remembered;
        }
        opened.id
    }
    /// Update the remembered source of `mode`'s document without a new id
    /// (the lesson last shown, the exhibit last shown, a self-started
    /// RoboCAD left running at its URL).
    pub fn set_source(&mut self, mode: ViewerMode, source: Source) {
        if let Some(e) = self.entry_mut(mode) {
            if e.source != source {
                e.source = source;
                self.changed += 1;
            }
        }
    }
    /// Copy the owning mode's own counter (an edit or reload it counts).
    /// Returns whether it changed.
    pub fn set_revision(&mut self, id: DocumentId, revision: u64) -> bool {
        match self.entries.iter_mut().find(|e| e.id == id) {
            Some(e) if e.revision != revision => {
                e.revision = revision;
                self.changed += 1;
                true
            }
            _ => false,
        }
    }
    /// Advance by one (a mode whose reload has no counter of its own).
    pub fn bump(&mut self, id: DocumentId) -> Option<u64> {
        let e = self.entries.iter_mut().find(|e| e.id == id)?;
        e.revision += 1;
        let revision = e.revision;
        self.changed += 1;
        Some(revision)
    }
    /// The mode was left: its document stays remembered (or parked, with
    /// [`DocumentRegistry::park`]).
    pub fn close(&mut self, mode: ViewerMode) {
        if let Some(e) = self.entry_mut(mode) {
            if e.presence != Presence::Remembered {
                e.presence = Presence::Remembered;
                self.changed += 1;
            }
        }
    }
    /// Keep `parked` on `mode`'s entry until the mode is shown again.
    /// Without an entry the scene is handed back (nothing to keep it under).
    pub fn park(&mut self, mode: ViewerMode, parked: Parked) -> Result<(), Parked> {
        match self.entry_mut(mode) {
            Some(e) => {
                e.parked = Some(parked);
                e.presence = Presence::Parked;
                self.changed += 1;
                Ok(())
            }
            None => Err(parked),
        }
    }
    /// The parked scene, taken back (the entry is open again).
    pub fn unpark(&mut self, mode: ViewerMode) -> Option<Parked> {
        let e = self.entry_mut(mode)?;
        let parked = e.parked.take()?;
        e.presence = Presence::Open;
        self.changed += 1;
        Some(parked)
    }
    /// The parked scene, taken without reopening (it is being replaced).
    pub fn take_parked(&mut self, mode: ViewerMode) -> Option<Parked> {
        let e = self.entry_mut(mode)?;
        let parked = e.parked.take();
        if parked.is_some() {
            self.changed += 1;
        }
        parked
    }
    /// Whether `mode` has a parked scene.
    pub fn is_parked(&self, mode: ViewerMode) -> bool {
        self.entry(mode).is_some_and(|e| e.parked.is_some())
    }
    /// `documents` in `viewer_mode {}` (each entry as kept here).
    pub fn json(&self) -> Value {
        Value::Array(self.entries.iter().map(DocumentEntry::json).collect())
    }
}

#[cfg(test)]
mod tests;

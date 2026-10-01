//! The document registry (`crate::document`) as the switch reads and writes
//! it: each mode's kind, the conversions between a registry [`Source`] and
//! the switch's [`Document`] or CAD's [`CadTarget`], opening and leaving a
//! mode's document, and `viewer_mode {}`'s `documents` report.
//!
//! One entry per mode, owned by that mode. Inspect's is an assembly, Build's
//! the builder's system file (open whenever a builder is in the window and
//! Build or Lessons is shown, parked with its scene otherwise), Lessons' its
//! folder and the lesson last open, Robot's a file or preset, Place's a
//! directory, CAD's a `.rcad` file or a service URL, Phenomena's the exhibit
//! it opens.
use super::{Document, Documents};
use crate::app::ViewerMode;
use crate::cad::CadTarget;
use crate::document::{DocumentKind, DocumentRegistry, Opened, Presence, Source};
use crate::selection::Selection;
use bevy::prelude::*;
use serde_json::{Value, json};
use std::path::PathBuf;

/// The kind of document `mode` owns.
pub fn kind(mode: ViewerMode) -> DocumentKind {
    match mode {
        ViewerMode::Inspect => DocumentKind::Assembly,
        ViewerMode::Build => DocumentKind::System,
        ViewerMode::Lessons => DocumentKind::Lessons,
        ViewerMode::Robot => DocumentKind::Robot,
        ViewerMode::Place => DocumentKind::Place,
        ViewerMode::Cad => DocumentKind::Cad,
        ViewerMode::Phenomena => DocumentKind::Exhibits,
    }
}

/// A switch's document as the registry keeps it.
pub fn document_source(document: &Document) -> Source {
    match document {
        Document::Path(p) => Source::path(p.clone()),
        Document::Preset(id) => Source::Preset { id: id.clone() },
        Document::Url(url) => Source::Url { url: url.clone() },
    }
}

/// The switch's document a registry source names (None: an assembly,
/// lessons or exhibit source, which no `viewer_mode` argument names alone).
pub fn source_document(source: &Source) -> Option<Document> {
    match source {
        Source::Path { path } => Some(Document::Path(path.clone())),
        Source::Preset { id } => Some(Document::Preset(id.clone())),
        Source::Url { url } => Some(Document::Url(url.clone())),
        Source::Assembly { .. } | Source::Lessons { .. } | Source::Exhibit { .. } => None,
    }
}

/// CAD's target as the registry keeps it: a file is a path, a service its URL.
pub fn cad_source(target: &CadTarget) -> Source {
    match target {
        CadTarget::File(p) => Source::path(p.clone()),
        CadTarget::Service(url) => Source::Url { url: url.clone() },
    }
}

/// The CAD target a registry source names (None for other kinds of source).
pub fn cad_target(source: &Source) -> Option<CadTarget> {
    match source {
        Source::Path { path } => Some(CadTarget::File(path.clone())),
        Source::Url { url } => Some(CadTarget::Service(url.clone())),
        _ => None,
    }
}

/// An assembly source's (description, spatial).
pub fn inspect_paths(source: &Source) -> Option<(PathBuf, PathBuf)> {
    match source {
        Source::Assembly { description, spatial } => Some((description.clone(), spatial.clone())),
        _ => None,
    }
}

/// A lessons source's (folder, lesson).
pub fn lessons_of(source: &Source) -> Option<(PathBuf, Option<String>)> {
    match source {
        Source::Lessons { dir, lesson } => Some((dir.clone(), lesson.clone())),
        _ => None,
    }
}

/// A path source's path (Place's directory, Build's file).
pub fn path_of(source: &Source) -> Option<PathBuf> {
    match source {
        Source::Path { path } => Some(path.clone()),
        _ => None,
    }
}

/// The exhibit phenomena mode opens (`--exhibit`'s rule; None: the first).
pub fn exhibit_of(registry: &DocumentRegistry) -> Option<String> {
    match registry.source(ViewerMode::Phenomena) {
        Some(Source::Exhibit { exhibit }) => exhibit.clone(),
        _ => None,
    }
}

/// `mode` now shows `source` (a load, a reopen): [`DocumentRegistry::open`]
/// (the same document again is a reload: its id kept, its revision + 1),
/// and the items of a document it replaced leave the selection.
pub(crate) fn open(world: &mut World, mode: ViewerMode, source: Source) -> Opened {
    let opened = world.resource_mut::<DocumentRegistry>().open(mode, kind(mode), source);
    if let Some(old) = opened.replaced
        && let Some(mut selection) = world.get_resource_mut::<Selection>()
    {
        selection.forget(old);
    }
    opened
}

/// `mode` shows `source` (derived from what is in the window): opened
/// unless its entry is already open on the same document, so a mode that
/// was entered without a switch (the launch) or stayed open (Build ↔
/// Lessons) gets no extra revision.
pub(crate) fn ensure_open(world: &mut World, mode: ViewerMode, source: Source) {
    let shown = world.resource::<DocumentRegistry>().entry(mode).is_some_and(|e| e.presence == Presence::Open && e.kind == kind(mode) && e.source.same_document(&source));
    if shown {
        world.resource_mut::<DocumentRegistry>().set_source(mode, source);
    } else {
        open(world, mode, source);
    }
}

/// `mode` was left: its source is updated (the lesson or exhibit last
/// shown, a self-started RoboCAD left running at its URL) without a new id,
/// or remembered if it had no entry, and the entry is closed (reopened by
/// the mode when chosen without a document).
pub(crate) fn left(world: &mut World, mode: ViewerMode, source: Option<Source>) {
    let mut registry = world.resource_mut::<DocumentRegistry>();
    if let Some(source) = source {
        if registry.entry(mode).is_some() {
            registry.set_source(mode, source);
        } else {
            registry.remember(mode, kind(mode), source);
        }
    }
    registry.close(mode);
}

impl Documents {
    /// `viewer_mode {}`'s `documents`: what each mode reopens, from the
    /// registry, and the launch configuration; `registry` lists its entries.
    pub fn json(&self, registry: &DocumentRegistry) -> Value {
        let source = move |mode: ViewerMode| registry.source(mode);
        json!({
            "inspect": source(ViewerMode::Inspect).and_then(inspect_paths).map(|(d, s)| json!({"description": d, "spatial": s})),
            "inspect_parked": registry.is_parked(ViewerMode::Inspect),
            "builder_scene_parked": registry.is_parked(ViewerMode::Build),
            "lessons": source(ViewerMode::Lessons).and_then(lessons_of).map(|(d, s)| json!({"dir": d, "lesson": s})),
            "robot": source(ViewerMode::Robot).and_then(source_document).as_ref().map(Document::json),
            "place": source(ViewerMode::Place).and_then(path_of),
            "cad": source(ViewerMode::Cad).and_then(cad_target).as_ref().map(CadTarget::json),
            "phenomena": {"exhibit": exhibit_of(registry)},
            "library": match &self.library { Ok(p) => json!(p), Err(e) => json!({"error": e}) },
            "models": self.models,
            "presets": self.presets,
            "registry": registry.json(),
        })
    }
}

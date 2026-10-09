//! `CadDocument`'s queries (tree rows, names, the connection line, the
//! selection) and the refusals every edit path shares (`commit_refusal`,
//! `edit_refusal`, `switch_blockers`), and the release of a self-started service.
use super::{CadDocument, CadTarget, Connection, TreeRow};
use crate::cad::types::SelectionItem;
use std::collections::HashMap;

impl CadDocument {
    /// The model tree in RoboCAD's walk order (`/doc` nodes), with each
    /// row's depth from its parent chain and whether any of `selection`'s
    /// items names it. Empty until the first `/doc`.
    pub(crate) fn rows(&self, selection: &[SelectionItem]) -> Vec<TreeRow> {
        let Some(doc) = &self.doc else { return Vec::new() };
        let parents: HashMap<&str, Option<&str>> = doc.nodes.iter().map(|n| (n.id.as_str(), n.parent.as_deref())).collect();
        doc.nodes
            .iter()
            .map(|n| {
                // Bounded by the node count: a malformed parent cycle cannot hang the UI.
                let mut depth = 0;
                let mut parent = n.parent.as_deref();
                while let Some(p) = parent {
                    if depth >= doc.nodes.len() {
                        break;
                    }
                    depth += 1;
                    parent = parents.get(p).copied().flatten();
                }
                TreeRow {
                    id: n.id.clone(),
                    depth,
                    kind: n.kind.clone(),
                    name: n.name.clone(),
                    effective_visible: n.effective_visible,
                    visible: n.visible,
                    locked: n.locked,
                    disabled: n.disabled,
                    selected: selection.iter().any(|s| s.0 == n.id),
                }
            })
            .collect()
    }

    /// The node's name in the shown tree (its id when unknown).
    pub(crate) fn node_name(&self, id: &str) -> String {
        self.doc.as_ref().and_then(|d| d.nodes.iter().find(|n| n.id == id)).map(|n| n.name.clone()).unwrap_or_else(|| id.to_string())
    }

    /// Whether the shown tree has node `id` (true while no tree is shown yet).
    pub(crate) fn has_node(&self, id: &str) -> bool {
        self.doc.as_ref().is_none_or(|d| d.nodes.iter().any(|n| n.id == id))
    }

    /// What the document is: an archive opened in process.
    pub(crate) fn service_line(&self) -> String {
        match &self.target {
            CadTarget::File(path) => format!("In-process Rust CAD · {} · direct OCCT", path.display()),
            CadTarget::Service(_) => "No CAD service is attached in this viewer; open a local .rcad file".into(),
        }
    }

    /// The header's connection line and whether it is an error.
    pub(crate) fn connection_line(&self) -> (String, bool) {
        if let Some(load) = &self.local_load {
            return (format!("Opening locally: {}", load.job.progress().message), false);
        }
        match &self.connection {
            Connection::Connected => (format!("In-process CAD · exact OCCT · {}", if self.unsaved() == Some(true) { "unsaved changes" } else { "saved" }), false),
            Connection::Connecting { what, .. } => (what.clone(), false),
            Connection::Lost { error, .. } => (error.clone(), true),
        }
    }

    /// The stale reason is a failed refetch (an error), not a refetch under way.
    fn doc_error_shown(&self) -> bool {
        self.stale.as_ref().is_some_and(|s| !s.starts_with("refetching revision"))
    }

    /// The document's file name (RoboCAD's path, else the target's), or
    /// "untitled" for a document RoboCAD has not saved.
    pub(crate) fn document_name(&self) -> String {
        let path = self.health.as_ref().and_then(|h| h.path.clone()).or_else(|| match &self.target {
            CadTarget::File(p) => Some(p.display().to_string()),
            CadTarget::Service(_) => None,
        });
        match path {
            Some(p) => std::path::Path::new(&p).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or(p),
            None => "untitled document".into(),
        }
    }

    /// The edit in flight, by name.
    pub(crate) fn edit_label(&self) -> Option<&str> {
        self.edit.as_ref().map(|e| e.label.as_str())
    }

    /// RoboCAD's revision the shown tree (and meshes, topology) is at.
    pub(crate) fn shown_revision(&self) -> u64 {
        self.doc_key.as_ref().map_or(0, |k| k.1)
    }

    /// Why a tool's commit cannot be sent now: an edit in flight or no
    /// connection (`edit_refusal`), the shown document behind RoboCAD's
    /// (`stale`), or RoboCAD's revision changed since `began`, when a drag
    /// or preview began, a form opened or picks were read (they were made
    /// against geometry that is gone).
    /// Nothing is sent when refused.
    pub(crate) fn commit_refusal(&self, began: Option<u64>) -> Option<String> {
        self.commit_refusal_for(began, false)
    }
    pub(crate) fn commit_refusal_for(&self, began: Option<u64>, auxiliary: bool) -> Option<String> {
        if let Some(why) = self.edit_refusal_for(auxiliary) {
            return Some(why);
        }
        if let Some(stale) = &self.stale {
            return Some(format!("the shown document is behind RoboCAD's ({stale}); nothing was sent"));
        }
        match began {
            Some(began) if began != self.shown_revision() => {
                Some(format!("the document changed since these values were taken (revision {began}, now {}); nothing was changed: redo the drag, the entry or the form", self.shown_revision()))
            }
            _ => None,
        }
    }

    /// The service answers and the window may send it requests.
    pub(crate) fn connected(&self) -> bool {
        self.local.is_some() && self.connection == Connection::Connected
    }

    /// Whether RoboCAD holds unsaved edits: `health.dirty`, or None when
    /// that cannot be confirmed: not connected (the last `health` may be
    /// old; RoboCAD may hold edits made since), an edit in flight, or just
    /// after one until a successful `GET /` sent after it has been read.
    pub(crate) fn unsaved(&self) -> Option<bool> {
        self.local.as_ref().map(|_| self.doc.as_ref().is_some_and(|d| d.dirty))
    }

    /// Why a mutating request cannot be sent now (refusals name it).
    pub(crate) fn edit_refusal(&self) -> Option<String> {
        self.edit_refusal_for(false)
    }
    pub(crate) fn edit_refusal_for(&self, auxiliary: bool) -> Option<String> {
        if let Some(error) = &self.uncertain_edit {
            return Some(format!("Unknown source edit outcome: {error}; inspect fresh source/history and explicitly acknowledge; no automatic retry"));
        }
        if self.preview_read_only && !auxiliary {
            return Some("Return to live CAD before editing the physical source; captured and kinematic previews are read-only".into());
        }
        if let Some(label) = &self.component_busy {
            return Some(format!("a component rebuild is in progress: {label}; wait or cancel it in Components"));
        }
        if let Some(label) = self.edit_label() {
            return Some(format!("another CAD edit is in flight: {label}"));
        }
        if self.local_load.is_some() {
            return Some("a document is opening; edits wait until it is shown".into());
        }
        if !self.connected() {
            return Some(format!("no CAD document is open: {}", self.connection_line().0));
        }
        None
    }

    /// What leaving CAD mode (or replacing this document) would lose: an
    /// edit in flight, or unsaved edits in a service this window started
    /// (it stops when the document closes; the viewer never saves for you),
    /// or a model export running or queued (cad-physical-inspect: it is
    /// dropped with the document, nothing written).
    pub(crate) fn switch_blockers(&self) -> Vec<String> {
        let mut blockers = Vec::new();
        if self.doc.as_ref().is_some_and(|d| d.dirty) { blockers.push("Local CAD document has unsaved work; preserve it before replacement or mode exit".into()); }
        if self.uncertain_edit.is_some() { blockers.push("An unknown source edit outcome needs inspection and explicit acknowledgment before replacing this document".into()); }
        if let Some(label) = &self.component_busy {
            blockers.push(format!("a component rebuild is in progress: {label}; wait or cancel it in Components"));
        }
        if let Some(label) = self.edit_label() {
            blockers.push(format!("a CAD edit is in flight: {label}"));
        }
        let exports = &self.results.exports;
        if let Some(running) = &exports.running {
            blockers.push(format!("a model export is running: {} to {}; wait or cancel it", running.request.label, running.request.path.display()));
        }
        if let Some(queued) = &exports.queued {
            blockers.push(format!("a model export is queued: {} to {}; wait for it, or stop the live link", queued.label, queued.path.display()));
        }
        // cad-print: a RoboCAD print job this window started and still tracks.
        blockers.extend(self.print.jobs.blockers());

        blockers
    }

    /// Leaving an attached document with unsaved edits is allowed: the
    /// switch's message says where the edits stay.
    pub(crate) fn leaving_note(&self) -> Option<String> {
        None
    }

    /// Mark something shown as changed (the panels refresh on it).
    pub(crate) fn touch(&mut self) {
        self.revision += 1;
    }

    /// Show an outcome line.
    pub(crate) fn show(&mut self, status: Result<String, String>) {
        self.status = Some(status);
        self.touch();
    }

    /// Leaving: a pending open is cancelled (its job drops).
    pub(crate) fn cancel_load(&mut self) {
        self.local_load = None;
    }
}

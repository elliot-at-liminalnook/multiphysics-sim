//! `CadDocument`'s queries (tree rows, names, the connection line, the
//! selection) and the refusals every edit path shares (`commit_refusal`,
//! `edit_refusal`, `switch_blockers`), and the release of a self-started service.
use super::{CadDocument, CadTarget, Connection, TreeRow};
use sim_runtime::cad_client::SelectionItem;
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

    /// The URL the window talks to, once known.
    pub(crate) fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    /// The service: self-started (with its pid) or attached, its URL, and
    /// RoboCAD's GUI or headless service once it has answered.
    pub(crate) fn service_line(&self) -> String {
        let url = self.url.as_deref().unwrap_or("a port not yet chosen");
        let mut line = match (&self.target, self.child.pid()) {
            (CadTarget::File(p), Some(pid)) if self.connect.is_some() => format!("Starting RoboCAD's headless service (pid {pid}) on {}", p.display()),
            (CadTarget::File(_), Some(pid)) => format!("Self-started RoboCAD (pid {pid}) at {url}"),
            (CadTarget::File(p), None) if self.connect.is_some() => format!("Starting RoboCAD's headless service on {}", p.display()),
            (CadTarget::File(p), None) => format!("RoboCAD's service for {} is not running", p.display()),
            (CadTarget::Service(_), _) => format!("Attached to RoboCAD at {url} (never stopped by this window)"),
        };
        if let Some(h) = &self.health {
            line.push_str(if h.gui { " · desktop GUI" } else { " · headless" });
            if !h.version.is_empty() {
                line.push_str(&format!(" · RoboCAD {}", h.version));
            }
        }
        line
    }

    /// The header's connection line and whether it is an error.
    pub(crate) fn connection_line(&self) -> (String, bool) {
        match &self.connection {
            Connection::Connecting { what, since } => (format!("Connecting: {what} ({} s)", since.elapsed().as_secs()), false),
            Connection::Connected => {
                let mut line = String::from("Connected");
                if let Some((_, revision)) = &self.doc_key {
                    line.push_str(&format!(" · revision {revision}"));
                }
                match self.unsaved() {
                    Some(true) => line.push_str(" · unsaved edits in RoboCAD"),
                    None => line.push_str(" · saved state being refetched"),
                    Some(false) => {}
                }
                if let Some(stale) = &self.stale {
                    line.push_str(&format!(" · tree may be behind RoboCAD: {stale}"));
                }
                if let Some(e) = &self.selection_error {
                    line.push_str(&format!(" · RoboCAD's selection could not be read: {e}"));
                }
                (line, self.doc_error_shown() || self.selection_error.is_some())
            }
            Connection::Lost { error, .. } => (format!("Not connected: {error}"), true),
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
        if let Some(why) = self.edit_refusal() {
            return Some(why);
        }
        if let Some(stale) = &self.stale {
            return Some(format!("the shown document is behind RoboCAD's ({stale}); nothing was sent"));
        }
        match (began, self.health.as_ref().map(|h| h.revision)) {
            (Some(began), Some(now)) if began != now || began != self.shown_revision() => {
                Some(format!("the document changed since these values were taken (revision {began}, now {now}); nothing was sent: redo the drag, the entry or the form"))
            }
            _ => None,
        }
    }

    /// The service answers and the window may send it requests.
    pub(crate) fn connected(&self) -> bool {
        self.client.is_some() && self.connection == Connection::Connected
    }

    /// Whether RoboCAD holds unsaved edits: `health.dirty`, or None when
    /// that cannot be confirmed: not connected (the last `health` may be
    /// old; RoboCAD may hold edits made since), an edit in flight, or just
    /// after one until a successful `GET /` sent after it has been read.
    pub(crate) fn unsaved(&self) -> Option<bool> {
        if self.dirty_known_at.is_some() || self.edit.is_some() || !self.connected() {
            return None;
        }
        self.health.as_ref().map(|h| h.dirty)
    }

    /// A self-started service that has answered in this connection (so it
    /// may hold edits) and has not exited (an exited one's edits are gone).
    fn child_may_hold_edits(&self) -> bool {
        self.client.is_some() && self.child_exit.is_none() && self.child.running()
    }

    /// Why a mutating request cannot be sent now (refusals name it).
    pub(crate) fn edit_refusal(&self) -> Option<String> {
        if let Some(label) = self.edit_label() {
            return Some(format!("another CAD edit is in flight: {label}"));
        }
        if !self.connected() {
            return Some(format!("not connected to RoboCAD: {}", self.connection_line().0));
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
        if self.edit.is_none() && self.child_may_hold_edits() {
            let name = self.document_name();
            let pid = self.child.pid().map_or_else(String::new, |p| format!(" (pid {p})"));
            match self.unsaved() {
                Some(false) => {}
                Some(true) => blockers.push(format!("{name} has unsaved edits in the RoboCAD service this window started{pid}, which stops when CAD mode closes: save first (the Save button)")),
                None if !self.connected() => blockers.push(format!(
                    "{name} may have unsaved edits in the RoboCAD service this window started{pid}, and its saved state can't be confirmed while the window is not connected to it ({}); that service stops when CAD mode closes: press Refresh to reconnect and save first, or stop that process yourself to discard its edits",
                    self.connection_line().0
                )),
                None => blockers.push(format!("{name} may have unsaved edits in the RoboCAD service this window started{pid} (an edit just finished and RoboCAD's state is being refetched), which stops when CAD mode closes: wait a moment, or save first (the Save button)")),
            }
        }
        blockers
    }

    /// Leaving an attached document with unsaved edits is allowed: the
    /// switch's message says where the edits stay.
    pub(crate) fn leaving_note(&self) -> Option<String> {
        if self.child.is_some() {
            return None;
        }
        let url = self.url.as_deref().unwrap_or("its URL");
        match self.unsaved() {
            Some(true) => Some(format!("RoboCAD at {url} keeps the unsaved edits to {}", self.document_name())),
            // Not confirmed now, but the last answer said dirty.
            None if self.health.as_ref().is_some_and(|h| h.dirty) => Some(format!("RoboCAD at {url} had unsaved edits to {} when last read; any it still holds stay there", self.document_name())),
            _ => None,
        }
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

    /// Before the document is dropped (window close, leaving CAD mode,
    /// `cad_open`): the child slot is closed (a service still starting is
    /// stopped by its connect job the moment it would be put there) and a
    /// self-started service is stopped, unless it has answered in this
    /// connection, has not exited and may hold unsaved edits (dirty, or not
    /// confirmable: not connected, an edit in flight or just finished). Then
    /// it is detached (left running, its URL logged) so the edits are not
    /// lost: unsaved edits are RoboCAD's, and the viewer never saves on its
    /// own. Synchronous and non-blocking (`ChildProcess::stop`/`detach`).
    /// Returns the URL of a service left running.
    pub(crate) fn release_child(&mut self, why: &str) -> Option<String> {
        let keep = self.child_may_hold_edits() && self.unsaved() != Some(false);
        let child = self.child.close()?;
        if keep {
            let state = if self.unsaved() == Some(true) { "holds unsaved edits" } else { "may hold unsaved edits (its saved state could not be confirmed)" };
            bevy::log::warn!(
                "{why}: the RoboCAD service this window started (pid {}) {state} to {}; it is left running at {} so they are not lost: open it there (sim-spatial --cad-url {}) and save, or stop it",
                child.id(),
                self.document_name(),
                self.url.as_deref().unwrap_or("its URL"),
                self.url.as_deref().unwrap_or("URL")
            );
            child.detach();
            self.url.clone()
        } else {
            child.stop();
            None
        }
    }
}

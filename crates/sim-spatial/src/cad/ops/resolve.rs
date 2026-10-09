//! The selection an operation runs on, resolved against its `Needs`.
//!
//! The items are the explicit ones (REST `cad_run {items}`) or the
//! selection. Nodes come in RoboCAD's `Selection.nodes()` order (each
//! item's node once, in item order; ui/viewport.py:217-222), edges and
//! faces in `Selection.edges()`/`faces()` order (:224-228). A selection
//! that does not fit is refused with the entry's message (RoboCAD's
//! `self.error(…)`), naming nothing sent.
use super::{Env, Needs, OpEntry};
use crate::cad::document::CadDocument;
use crate::cad::transform::{mm, selection_revision, view_back};
use crate::cad::types::SelectionItem;

/// What an operation runs on.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Resolved {
    /// The selected nodes, each once, in selection order; for a
    /// `Needs::Nodes` with kinds, only the nodes of those kinds (RoboCAD's
    /// handlers filter: thicken takes sheets, make unique instances).
    pub nodes: Vec<String>,
    /// The selected edges as (node, edge index), in selection order.
    pub edges: Vec<(String, i64)>,
    /// The selected faces as (node, face index), in selection order.
    pub faces: Vec<(String, i64)>,
    /// Dependent offset's target: the first selected node owning none of the selected faces.
    pub other: Option<String>,
    /// The direction the camera looks along, in RoboCAD's frame (`-camera.basis()[2]`).
    pub view_dir: Option<[f64; 3]>,
    /// The snapped point under the pointer (mm).
    pub snap: Option<[f64; 3]>,
    /// The revision the picks and values were read at: the shown one,
    /// or the run's `revision` when given (`ops::prepare` sets it).
    pub revision: u64,
}

/// Whether the entry reads face or edge indices.
pub(super) fn reads_indices(needs: Needs) -> bool {
    matches!(needs, Needs::Edges { .. } | Needs::Faces { .. } | Needs::NodesWithFaces | Needs::FaceThenNode)
}

/// The node's kind in the shown tree (None when unknown or no tree is shown).
pub(crate) fn kind_of<'a>(doc: &'a CadDocument, id: &str) -> Option<&'a str> {
    doc.doc.as_ref().and_then(|d| d.nodes.iter().find(|n| n.id == id)).map(|n| n.kind.as_str())
}

/// Resolve `entry`'s selection: `items` (explicit), else the shared
/// selection's CAD items (`env.selection`). Refused by name when a node is not in the shown tree, when
/// the selection's face or edge indices were read at an older revision,
/// when an index does not exist in the shown topology, or when the
/// selection does not fit `entry.needs` (`entry.refusal`).
pub(super) fn resolve(entry: &OpEntry, doc: &CadDocument, env: &Env, items: Option<&[SelectionItem]>) -> Result<Resolved, String> {
    let (topology, view) = (env.topology, env.view);
    let explicit = items.is_some();
    let items: &[SelectionItem] = items.unwrap_or(env.selection);
    let shown = doc.shown_revision();
    let mut r = Resolved { view_dir: view.filter(|v| v.valid).map(|v| mm(-view_back(v))), snap: doc.ops.cursor_snap.filter(|(at, _)| *at == shown).map(|(_, p)| p), revision: shown, ..Default::default() };
    for SelectionItem(node, kind, index) in items {
        if !doc.has_node(node) {
            return Err(format!("no node {node} in the shown tree"));
        }
        if !r.nodes.contains(node) {
            r.nodes.push(node.clone());
        }
        match kind.as_str() {
            "edge" => r.edges.push((node.clone(), *index)),
            "face" => r.faces.push((node.clone(), *index)),
            _ => {}
        }
    }
    if reads_indices(entry.needs) && (!r.edges.is_empty() || !r.faces.is_empty()) {
        // The pickers store only indices read at the shown revision
        // (`CadMeshes::face_at`); a selection first seen before an edit may
        // name faces RoboCAD has renumbered since.
        if !explicit {
            let seen = selection_revision(doc, env.selection);
            if seen != shown {
                return Err(format!("the selection was made at revision {seen}; RoboCAD's faces may be renumbered since (now {shown}): reselect"));
            }
        }
        // As transform/commit.rs `check_faces`: with the topology at the shown revision, each index must exist.
        if let Some(t) = topology {
            for (node, face) in &r.faces {
                if let Some(nt) = t.get(node)
                    && !nt.faces.iter().any(|f| f.index == *face)
                {
                    return Err(format!("{} has no face {face} at the shown revision ({} faces)", doc.node_name(node), nt.faces.len()));
                }
            }
            for (node, edge) in &r.edges {
                if let Some(nt) = t.get(node)
                    && !nt.edges.iter().any(|e| e.index == *edge)
                {
                    return Err(format!("{} has no edge {edge} at the shown revision ({} edges)", doc.node_name(node), nt.edges.len()));
                }
            }
        }
    }
    let refuse = || -> Result<Resolved, String> { Err(entry.refusal.to_string()) };
    match entry.needs {
        Needs::Nothing => {}
        Needs::Nodes { min, max, kinds } => {
            if !kinds.is_empty() {
                r.nodes.retain(|n| kind_of(doc, n).is_some_and(|k| kinds.contains(&k)));
            }
            if r.nodes.len() < min || max.is_some_and(|m| r.nodes.len() > m) {
                return refuse();
            }
        }
        Needs::TargetThenTools => {
            if r.nodes.len() < 2 {
                return refuse();
            }
        }
        Needs::Edges { min, max, same_node } => {
            let n = r.edges.len();
            if n < min || max.is_some_and(|m| n > m) || (same_node && r.edges.iter().any(|(node, _)| *node != r.edges[0].0)) {
                return refuse();
            }
        }
        Needs::Faces { min } => {
            if r.faces.len() < min {
                return refuse();
            }
        }
        Needs::NodesWithFaces => {
            if r.nodes.is_empty() {
                return refuse();
            }
        }
        Needs::FaceThenNode => {
            r.other = r.nodes.iter().find(|n| !r.faces.iter().any(|(f, _)| f == *n)).cloned();
            if r.faces.is_empty() || r.other.is_none() {
                return refuse();
            }
        }
        Needs::NodesThenPath => {
            // The path is the last selected node: a curve or sketch (`Ops._profile`).
            let path_ok = r.nodes.last().is_some_and(|n| kind_of(doc, n).is_none_or(|k| k == "curve" || k == "sketch"));
            if r.nodes.len() < 2 || !path_ok {
                return refuse();
            }
        }
    }
    Ok(r)
}

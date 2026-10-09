//! The outliner's display state ([`TreeState`], on `CadDocument::tree`) and
//! the pure readings of RoboCAD's tree it is drawn from: the rows shown
//! under the search and the collapse state ([`shown`], RoboCAD's
//! `OutlinerPanel.refresh`, widgets.py:271-312), the Shift range
//! ([`range`]), a drop's parent and index ([`move_plan`], `_drop`
//! widgets.py:366-381 with `Ops.move_nodes`' refusals, commands.py:377-392)
//! and the "Move to group" paths ([`group_paths`], widgets.py:416-430).
//! Nothing here talks to RoboCAD or changes its document.
use crate::cad::document::{CadDocument, TreeRow};
use crate::cad::types::{DocState, SelectionItem};
use std::collections::{BTreeSet, HashMap, HashSet};

/// The outliner's state on the document (display only; reset with the
/// document, so per connection generation and document).
#[derive(Default, Debug)]
pub struct TreeState {
    /// Monotonic local modal lifetime; drafts and focus do not change it.
    pub(crate) form_sequence: u64,
    /// The search text as typed (RoboCAD lowercases and strips it to match).
    pub(crate) search: String,
    /// Whether the search field has the keyboard (mirrored from the kit's
    /// focus for the drawn field).
    pub(crate) search_focused: bool,
    /// Collapsed nodes (RoboCAD's `_expansion` entries that are false; a
    /// node is expanded unless recorded collapsed). Kept across edits,
    /// refetches and searches; not changed while searching.
    pub(crate) collapsed: BTreeSet<String>,
    /// The Shift range's anchor: the row last pressed without Shift.
    pub(crate) anchor: Option<String>,
    /// The inline rename open on a row.
    pub(crate) rename: Option<RenameState>,
    /// The context menu, while open.
    pub(crate) menu: Option<MenuState>,
    /// The "Organize components" name dialog, while open.
    pub(crate) dialog: Option<DialogState>,
    /// A row drag in progress (display only: the window's local preview;
    /// the drop is the `move` action).
    pub(crate) drag: Option<TreeDrag>,
    /// A field the window should give the keyboard to (set by the handler
    /// or Ctrl+F, taken by `input::fields`).
    pub(crate) claim: Option<Claim>,
}

/// A field the outliner owns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Claim {
    Search,
    Rename,
    Dialog,
}

/// The inline rename on row `id`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RenameState {
    pub id: String,
    /// The field's text as typed (the kit's draft, mirrored for drawing).
    pub draft: String,
    /// The draft is selected (the next key replaces it).
    pub selected: bool,
    /// The shown revision when the rename began (the edit is refused by
    /// name if RoboCAD's document changed since).
    pub began: u64,
}

/// The open context menu.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MenuState {
    /// The row it opened on (None: the empty area under the rows).
    pub id: Option<String>,
    /// The selected nodes when it opened (the row selected first): what its
    /// entries act on. A selection change while it is open closes it.
    pub ids: Vec<String>,
    /// Where it opened (window logical px; None from automation: beside the dock).
    pub at: Option<[f32; 2]>,
    /// The shown revision it was opened at (its edits are refused by name after a change).
    pub began: u64,
}

/// The open "Organize components" dialog.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DialogState {
    /// The nodes OK groups ([] for New group: an empty group).
    pub ids: Vec<String>,
    /// "Group name:" as typed.
    pub draft: String,
    /// Why the last OK sent nothing (an empty name, edits blocked).
    pub error: Option<String>,
    /// The shown revision it opened at.
    pub began: u64,
}

/// Where a drop would put the dragged nodes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DropTarget {
    /// Into group `id`, at its end (RoboCAD: a drop on a group).
    Into(String),
    /// Before node `id`, under its parent (RoboCAD: a drop on a non-group).
    Before(String),
    /// At the end of the top level (RoboCAD: a drop on no item).
    TopLevel,
}

/// A row drag: the nodes it moves and the current target.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TreeDrag {
    pub ids: Vec<String>,
    pub target: Option<DropTarget>,
    /// The shown revision the drag started at.
    pub began: u64,
}

/// One row as the outliner shows it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Shown {
    pub row: TreeRow,
    /// The node has children in the shown tree (a disclosure control).
    pub children: bool,
    /// Its children are shown (expanded, or every row while searching).
    pub open: bool,
    pub group: bool,
    /// RoboCAD's active group (drawn in its blue).
    pub active: bool,
}

impl TreeState {
    /// The search as RoboCAD matches it (`text().lower().strip()`); empty: not searching.
    pub(crate) fn query(&self) -> String {
        self.search.trim().to_lowercase()
    }
    pub(crate) fn searching(&self) -> bool {
        !self.query().is_empty()
    }
}

/// Node `id`'s ancestors, nearest first (bounded: a malformed parent cycle cannot hang).
pub(crate) fn ancestors<'a>(state: &'a DocState, parents: &HashMap<&'a str, Option<&'a str>>, id: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut parent = parents.get(id).copied().flatten();
    while let Some(p) = parent {
        if out.len() > state.nodes.len() || out.contains(&p) {
            break;
        }
        out.push(p);
        parent = parents.get(p).copied().flatten();
    }
    out
}

fn parents(state: &DocState) -> HashMap<&str, Option<&str>> {
    state.nodes.iter().map(|n| (n.id.as_str(), n.parent.as_deref())).collect()
}

/// Node `ids` and everything under them, in walk order (RoboCAD's
/// `doc.walk(nid)` for each, with the nodes themselves).
pub(crate) fn with_descendants(state: &DocState, ids: &[String]) -> HashSet<String> {
    let mut out: HashSet<String> = ids.iter().cloned().collect();
    // Walk order lists parents before children.
    for n in &state.nodes {
        if n.parent.as_ref().is_some_and(|p| out.contains(p)) {
            out.insert(n.id.clone());
        }
    }
    out
}

/// RoboCAD's search matches (widgets.py:276-288): each node whose name
/// contains `query` (lowercase), with its descendants and its ancestors.
pub(crate) fn matches(state: &DocState, query: &str) -> HashSet<String> {
    let hits: Vec<String> = state.nodes.iter().filter(|n| n.name.to_lowercase().contains(query)).map(|n| n.id.clone()).collect();
    let parents = parents(state);
    let mut out = with_descendants(state, &hits);
    for id in &hits {
        out.extend(ancestors(state, &parents, id).into_iter().map(str::to_string));
    }
    out
}

/// The rows the outliner shows, in RoboCAD's walk order: while searching,
/// the matches with every row expanded (the collapse state is kept, not
/// changed); otherwise every row whose ancestors are all expanded.
pub(crate) fn shown(doc: &CadDocument, selection: &[SelectionItem]) -> Vec<Shown> {
    let Some(state) = &doc.doc else { return Vec::new() };
    let tree = &doc.tree;
    let query = tree.query();
    let found = (!query.is_empty()).then(|| matches(state, &query));
    let parents = parents(state);
    let with_children: HashSet<&str> = state.nodes.iter().filter_map(|n| n.parent.as_deref()).collect();
    let active = state.active_group.as_deref();
    // `rows` is built from the same nodes in the same order.
    doc.rows(selection)
        .into_iter()
        .zip(&state.nodes)
        .filter(|(row, _)| match &found {
            Some(found) => found.contains(&row.id),
            None => !ancestors(state, &parents, &row.id).iter().any(|a| tree.collapsed.contains(*a)),
        })
        .map(|(row, node)| {
            let children = with_children.contains(row.id.as_str());
            Shown { children, open: found.is_some() || !tree.collapsed.contains(&row.id), group: node.kind == "group", active: active == Some(row.id.as_str()), row }
        })
        .collect()
}

/// The nodes with children (what Collapse all records collapsed).
pub(crate) fn parents_of_children(state: &DocState) -> BTreeSet<String> {
    let ids: HashSet<&str> = state.nodes.iter().map(|n| n.id.as_str()).collect();
    state.nodes.iter().filter_map(|n| n.parent.as_deref()).filter(|p| ids.contains(p)).map(str::to_string).collect()
}

/// A Shift press on `id` (Qt's ExtendedSelection): the shown rows from
/// the anchor to `id`, in shown order; just `id` without an anchor or
/// when the anchor is not shown (collapsed away or filtered out).
pub(crate) fn range(shown: &[Shown], anchor: Option<&str>, id: &str) -> Vec<String> {
    let at = |want: &str| shown.iter().position(|s| s.row.id == want);
    match (anchor.and_then(at), at(id)) {
        (Some(a), Some(b)) => shown[a.min(b)..=a.max(b)].iter().map(|s| s.row.id.clone()).collect(),
        _ => vec![id.to_string()],
    }
}

/// RoboCAD's `Document.index_of` (document.py:415-418): `id`'s position
/// among its parent's children (or the roots).
pub(crate) fn index_of(state: &DocState, id: &str) -> Option<usize> {
    let node = state.nodes.iter().find(|n| n.id == id)?;
    let siblings = match &node.parent {
        Some(p) => &state.nodes.iter().find(|n| n.id == *p)?.children,
        None => &state.roots,
    };
    siblings.iter().position(|s| s == id)
}

/// A move as `Ops.move_nodes(ids, parent, index)` takes it, built as
/// RoboCAD's `_drop` builds it: into group `parent` (index None), before
/// sibling `before` (its parent and `index_of(before)`), or the top level
/// (neither). Refused by name before sending where RoboCAD would refuse:
/// a target that is not a group, a group into itself or its descendants,
/// unknown nodes, nothing to move.
pub(crate) fn move_plan(doc: &CadDocument, ids: &[String], parent: Option<&str>, before: Option<&str>) -> Result<(Option<String>, Option<i64>), String> {
    let state = doc.doc.as_ref().ok_or("the document has not loaded yet")?;
    if ids.is_empty() {
        return Err("nothing to move: select the rows to move first".into());
    }
    let node = |id: &str| state.nodes.iter().find(|n| n.id == id);
    if let Some(missing) = ids.iter().find(|id| node(id.as_str()).is_none()) {
        return Err(format!("no node {missing} in the shown tree"));
    }
    let (parent, index) = match (parent, before) {
        (Some(_), Some(_)) => return Err("move takes parent (a group to move into) or before (a node to move in front of), not both".into()),
        (Some(p), None) => {
            let target = node(p).ok_or_else(|| format!("no node {p} in the shown tree"))?;
            if target.kind != "group" {
                return Err(format!("{} is not a group: the move target must be a group", target.name));
            }
            (Some(p.to_string()), None)
        }
        (None, Some(b)) => {
            let target = node(b).ok_or_else(|| format!("no node {b} in the shown tree"))?;
            if ids.iter().any(|id| id == b) {
                return Err(format!("{} cannot be moved in front of itself", target.name));
            }
            let index = index_of(state, b).ok_or_else(|| format!("{} is not listed under its parent in RoboCAD's tree", target.name))?;
            (target.parent.clone(), Some(index as i64))
        }
        (None, None) => (None, None),
    };
    if let Some(p) = &parent {
        // RoboCAD's check walks the new parent's ancestors for a moved node.
        let moving = with_descendants(state, ids);
        if moving.contains(p) {
            // RoboCAD's text (commands.py `move_nodes`), with the group named.
            return Err(format!("Cannot move a group into itself or its descendants ({})", doc.node_name(p)));
        }
    }
    Ok((parent, index))
}

/// "Move to group" (widgets.py:416-430): every group in walk order except
/// the moving nodes and their descendants, as (id, "A / B" path).
pub(crate) fn group_paths(state: &DocState, moving: &[String]) -> Vec<(String, String)> {
    let excluded = with_descendants(state, moving);
    let parents = parents(state);
    let names: HashMap<&str, &str> = state.nodes.iter().map(|n| (n.id.as_str(), n.name.as_str())).collect();
    state
        .nodes
        .iter()
        .filter(|n| n.kind == "group" && !excluded.contains(&n.id))
        .map(|n| {
            let mut path: Vec<&str> = ancestors(state, &parents, &n.id).into_iter().rev().map(|a| names.get(a).copied().unwrap_or(a)).collect();
            path.push(n.name.as_str());
            (n.id.clone(), path.join(" / "))
        })
        .collect()
}

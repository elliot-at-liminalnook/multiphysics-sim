//! CAD's side of the one selection (native-viewer.md §7). CAD mode keeps no
//! selection of its own: its items are `Item::Cad` entries (RoboCAD's
//! `[node, kind, index]`) of the shared [`Selection`], under the id of CAD's
//! entry in the [`DocumentRegistry`] ([`cad_id`]).
//!
//! - **Writers** change them only through `Selection::apply` with a
//!   [`SelectionAction`] ([`Shared::apply`], [`Shared::set`],
//!   [`Shared::clear`]): the CAD handler's selection arms, a mode switch, an
//!   op that clears the selection, RoboCAD's selection adopted by the poll,
//!   and any `Act<SelectionAction>` another part of the window writes.
//! - **Readers** take the items once per system ([`CadSelection`]) or
//!   handler ([`Shared::items`]) and pass them on as `&[SelectionItem]`
//!   ([`CadItems`] has the node and kind queries).
//! - **Revisions.** The registry's revision of CAD's entry is RoboCAD's
//!   revision of the shown tree (`CadDocument::shown_revision`, the one
//!   `CadMeshes::face_at` and the tools' refusals compare against), copied
//!   when a new tree is shown ([`follow_tree`]). Body items still in the
//!   tree are restamped; face, edge, vertex, point and curve items keep the
//!   revision they were picked at (their indices belong to it; CAD's own
//!   stale-revision refusals stay as they were); an item naming a node
//!   absent from a current tree is dropped and named (while the shown tree
//!   is behind RoboCAD's revision it is kept until the next tree).
use crate::app::ViewerMode;
use crate::cad::document::CadTarget;
use crate::document::{DocumentId, DocumentKind, DocumentRegistry, Source};
use crate::selection::{Item, Op, Recheck, Selection, SelectionAction};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use sim_runtime::cad_client::{DocState, SelectionItem};

/// Why a CAD selection change cannot be applied without a registry entry.
const NOT_REGISTERED: &str = "CAD mode's document is not in the window's document registry yet; nothing was selected";

/// CAD's document in the registry (open, parked or remembered).
pub(crate) fn cad_id(registry: &DocumentRegistry) -> Option<DocumentId> {
    registry.entry(ViewerMode::Cad).map(|e| e.id)
}

/// The registry's source for a CAD target (the switch's one mapping).
pub(crate) fn source(target: &CadTarget) -> Source {
    crate::app::switch::sources::cad_source(target)
}

/// CAD's items of the shared selection, in selection order (none without
/// a CAD entry).
pub(crate) fn cad_items(selection: &Selection, registry: &DocumentRegistry) -> Vec<SelectionItem> {
    cad_id(registry).map_or_else(Vec::new, |id| selection.cad(id))
}

/// A CAD document is shown but the registry has no CAD entry (a launch
/// whose arrival did not open one): open it, so selection changes have a
/// document to name. An existing entry is left as it is.
pub(crate) fn ensure_registered(registry: &mut DocumentRegistry, selection: &mut Selection, target: &CadTarget) {
    if registry.entry(ViewerMode::Cad).is_some() {
        return;
    }
    let opened = registry.open(ViewerMode::Cad, DocumentKind::Cad, source(target));
    if let Some(old) = opened.replaced {
        selection.forget(old);
    }
}

/// `cad_open` replaced the document: the registry opens `target` (the same
/// source again is a reload, keeping its id and items); a replaced
/// document's items go.
pub(crate) fn reopen(registry: &mut DocumentRegistry, selection: &mut Selection, target: &CadTarget) -> DocumentId {
    let opened = registry.open(ViewerMode::Cad, DocumentKind::Cad, source(target));
    if let Some(old) = opened.replaced {
        selection.forget(old);
    }
    opened.id
}

/// The node and kind queries on CAD's items.
pub(crate) trait CadItems {
    /// The first selected node (the inspected one).
    fn first_node(&self) -> Option<&str>;
    /// The selected nodes, each once, in selection order (RoboCAD's `Selection.nodes`).
    fn nodes(&self) -> Vec<String>;
    /// The selected items of one kind ("face", "edge", …) as (node, index).
    fn of_kind(&self, kind: &str) -> Vec<(String, i64)>;
}
impl CadItems for [SelectionItem] {
    fn first_node(&self) -> Option<&str> {
        self.first().map(|i| i.0.as_str())
    }
    fn nodes(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for SelectionItem(node, ..) in self {
            if !out.contains(node) {
                out.push(node.clone());
            }
        }
        out
    }
    fn of_kind(&self, kind: &str) -> Vec<(String, i64)> {
        self.iter().filter(|i| i.1 == kind).map(|i| (i.0.clone(), i.2)).collect()
    }
}

/// A system's read of CAD's items (no CAD entry, or no selection resource: none).
#[derive(SystemParam)]
pub(crate) struct CadSelection<'w> {
    selection: Option<Res<'w, Selection>>,
    registry: Option<Res<'w, DocumentRegistry>>,
}
impl CadSelection<'_> {
    pub(crate) fn items(&self) -> Vec<SelectionItem> {
        match (self.selection.as_deref(), self.registry.as_deref()) {
            (Some(selection), Some(registry)) => cad_items(selection, registry),
            _ => Vec::new(),
        }
    }
    /// The shared selection's change count (`Selection::changed`; 0 without
    /// one): a display caches what it derives from the items by it.
    pub(crate) fn changed(&self) -> u64 {
        self.selection.as_deref().map_or(0, |s| s.changed)
    }
}

/// A read of the shared selection and the registry (changes nothing).
#[derive(Clone, Copy)]
pub(crate) struct View<'a> {
    pub selection: &'a Selection,
    pub registry: &'a DocumentRegistry,
}
impl View<'_> {
    pub(crate) fn id(&self) -> Option<DocumentId> {
        cad_id(self.registry)
    }
    /// CAD's items, in selection order.
    pub(crate) fn items(&self) -> Vec<SelectionItem> {
        cad_items(self.selection, self.registry)
    }
    /// The change counter (`Selection::changed`).
    pub(crate) fn changed(&self) -> u64 {
        self.selection.changed
    }
}

/// The shared selection and the registry, borrowed by CAD's handler
/// (`actions::Cx::shared`) and its job results (`sync::receive`).
pub(crate) struct Shared<'a> {
    pub selection: &'a mut Selection,
    pub registry: &'a mut DocumentRegistry,
}
impl Shared<'_> {
    pub(crate) fn view(&self) -> View<'_> {
        View { selection: &*self.selection, registry: &*self.registry }
    }
    pub(crate) fn id(&self) -> Option<DocumentId> {
        cad_id(self.registry)
    }
    /// CAD's items, in selection order.
    pub(crate) fn items(&self) -> Vec<SelectionItem> {
        cad_items(self.selection, self.registry)
    }
    /// The action for CAD's document: each item with the revision it was
    /// picked at (None: the current one).
    pub(crate) fn action(&self, op: Op, items: impl IntoIterator<Item = (SelectionItem, Option<u64>)>) -> Result<SelectionAction, String> {
        let document = self.id().ok_or_else(|| NOT_REGISTERED.to_string())?;
        Ok(SelectionAction { op, document, items: items.into_iter().map(|(i, r)| (Item::Cad(i), r)).collect() })
    }
    /// The one validated apply (`Selection::apply`). Returns whether the selection changed.
    pub(crate) fn apply(&mut self, op: Op, items: impl IntoIterator<Item = (SelectionItem, Option<u64>)>) -> Result<bool, String> {
        let action = self.action(op, items)?;
        self.selection.apply(&*self.registry, &action)
    }
    /// Replace CAD's items (duplicates dropped), at the current revision.
    pub(crate) fn set(&mut self, items: impl IntoIterator<Item = SelectionItem>) -> Result<bool, String> {
        self.apply(Op::Set, items.into_iter().map(|i| (i, None)))
    }
    /// Remove every CAD item.
    pub(crate) fn clear(&mut self) -> Result<bool, String> {
        self.apply(Op::Clear, Vec::<(SelectionItem, Option<u64>)>::new())
    }
}

/// A new tree is shown (`sync::take_snapshot`): the registry's revision
/// becomes RoboCAD's `revision` and CAD's items are re-checked (see the
/// module doc). `current`: the tree is RoboCAD's latest (`stale` is None).
/// Items already stamped with `revision` that name a node the current tree
/// lacks (another document at the same revision number, a restarted
/// service) are removed too. Returns the dropped items' labels (also in
/// `Selection::dropped`).
pub(crate) fn follow_tree(shared: &mut Shared, revision: u64, tree: &DocState, current: bool) -> Vec<String> {
    let Some(id) = shared.id() else { return Vec::new() };
    shared.registry.set_revision(id, revision);
    let known = |node: &str| tree.nodes.iter().any(|n| n.id == node);
    let mut dropped = shared.selection.revalidate(id, revision, |item| match &*item {
        Item::Cad(SelectionItem(node, ..)) if current && !known(node.as_str()) => Recheck::Drop,
        Item::Cad(SelectionItem(node, kind, _)) if kind.as_str() == "body" && known(node.as_str()) => Recheck::Restamp,
        _ => Recheck::Keep,
    });
    if current {
        let absent: Vec<SelectionItem> = shared.selection.cad_stamped(id).into_iter().filter(|(i, r)| *r == revision && !known(i.0.as_str())).map(|(i, _)| i).collect();
        if !absent.is_empty() {
            let labels: Vec<String> = absent.iter().map(|i| Item::Cad(i.clone()).label()).collect();
            if shared.apply(Op::Remove, absent.into_iter().map(|i| (i, None))) == Ok(true) {
                dropped.extend(labels);
                shared.selection.dropped = dropped.clone();
            }
        }
    }
    dropped
}

/// Tests: a registry with CAD's entry and an empty selection.
#[cfg(test)]
pub(crate) struct Fixture {
    pub selection: Selection,
    pub registry: DocumentRegistry,
}
#[cfg(test)]
impl Fixture {
    /// CAD's entry (RoboCAD at 127.0.0.1:8420, as the tests' documents) at revision 0.
    pub(crate) fn new() -> Self {
        Self::at(0)
    }
    /// CAD's entry at RoboCAD's `revision` (the shown tree's).
    pub(crate) fn at(revision: u64) -> Self {
        let mut registry = DocumentRegistry::default();
        let id = registry.open(ViewerMode::Cad, DocumentKind::Cad, Source::Url { url: "http://127.0.0.1:8420".into() }).id;
        registry.set_revision(id, revision);
        Self { selection: Selection::default(), registry }
    }
    pub(crate) fn shared(&mut self) -> Shared<'_> {
        Shared { selection: &mut self.selection, registry: &mut self.registry }
    }
    pub(crate) fn items(&self) -> Vec<SelectionItem> {
        cad_items(&self.selection, &self.registry)
    }
    /// Replace CAD's items (as a test's starting selection).
    pub(crate) fn set(&mut self, items: Vec<SelectionItem>) {
        self.shared().set(items).expect("CAD's entry is registered");
    }
    pub(crate) fn id(&self) -> DocumentId {
        cad_id(&self.registry).expect("CAD's entry is registered")
    }
}

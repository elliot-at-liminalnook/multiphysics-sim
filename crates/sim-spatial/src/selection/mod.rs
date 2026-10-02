//! The one selection model (native-viewer.md §7): one [`Selection`]
//! resource of typed items, each naming the document it belongs to and the
//! revision it refers to. Every mode reads its document's items from it; no
//! mode keeps a selection of its own.
//!
//! - **One action** ([`SelectionAction`]: set, add, toggle, remove or clear
//!   items of one document) validated and applied by one function,
//!   [`Selection::apply`]. Picks, list rows and keys write
//!   `Act<SelectionAction>`, drained by the one system [`apply_actions`] in
//!   `ViewerSet::Actions`. A mode's REST selection command (`select`,
//!   `cad_select`, `robot_select`, `system_ui` rows) stays its own command
//!   with its own name and arguments: its handler is a thin adapter that
//!   checks the mode's own rules (a component that exists, a CAD node in the
//!   tree) and applies the same [`SelectionAction`] through
//!   [`Selection::apply`] in that handler, so its answer reports the new
//!   state in the same frame. A mode's own edit that changes what is
//!   selected (grouping selects the group, a delete clears) goes through
//!   [`Selection::apply`] too.
//! - **Revisions.** An item carries the revision of its document it was
//!   picked at. [`Selection::apply`] refuses, naming it, an item picked at
//!   an older revision than the document's current one (the document
//!   changed under the pick); an item without one is stamped with the
//!   current revision. When a mode's document advances (an edit it counts,
//!   a reload), it re-checks its items with [`Selection::revalidate`]:
//!   each is restamped (it still resolves), kept as picked (CAD's sub-body
//!   items, whose stale-revision refusals stay CAD's), or dropped, and the
//!   dropped ones are named in [`Selection::dropped`].
//! - **Documents.** Items of several documents may be held at once (each
//!   mode's selection survives a switch, as before). A document replaced by
//!   another ([`crate::document::Opened::replaced`]) loses its items with
//!   [`Selection::forget`].
//! - **Change counter.** [`Selection::changed`] is bumped on every change;
//!   highlights, inspectors and RoboCAD's `/selection` push compare it with
//!   the count they last saw (a derived cache, never an owner).
use crate::app::actions::{self, Act, Action, InFlight, Replies, Spec};
use crate::app::{ViewerMode, ViewerSet};
use crate::document::{DocumentId, DocumentRegistry};
use bevy::ecs::message::Messages;
use bevy::prelude::*;
use serde::Serialize;
use serde_json::{Value, json};
use sim_api::Outcome;
use sim_inspect::selection::SelectionTarget;
use sim_runtime::cad_client::SelectionItem;

/// What can be selected.
/// Serialized adjacently tagged (`{"kind": "cad", "item": [node, kind, index]}`):
/// a CAD item is a sequence, which an internally tagged enum cannot hold.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "item", rename_all = "snake_case")]
pub enum Item {
    /// A system component: an inspected assembly's component id, or a
    /// builder instance name at the builder's level (as the builder names it).
    Component { id: String },
    /// A port of an inspected assembly.
    Port { id: String },
    /// A net of an inspected assembly.
    Net { id: String },
    /// A robot link: its index in the loaded model and its name (a reload
    /// re-finds the index by name).
    Link { index: usize, name: String },
    /// A CAD item as RoboCAD writes it: `[node, kind, index]` (kind body,
    /// face, edge, vertex, point or curve). Face indices come only from
    /// `CadMeshes::face_at`.
    Cad(SelectionItem),
}
impl Item {
    /// How a refusal names it.
    pub fn label(&self) -> String {
        match self {
            Item::Component { id } => format!("component {id}"),
            Item::Port { id } => format!("port {id}"),
            Item::Net { id } => format!("net {id}"),
            Item::Link { name, .. } => format!("link {name}"),
            Item::Cad(SelectionItem(node, kind, index)) if kind == "body" => format!("[{node}, body, {index}]"),
            Item::Cad(SelectionItem(node, kind, index)) => format!("[{node}, {kind}, {index}]"),
        }
    }
    /// Its kind's name, as [`Item`] serializes it.
    pub fn kind(&self) -> &'static str {
        match self {
            Item::Component { .. } => "component",
            Item::Port { .. } => "port",
            Item::Net { .. } => "net",
            Item::Link { .. } => "link",
            Item::Cad(_) => "cad",
        }
    }
    /// Same thing selected (a link by name: its index may differ across a reload).
    fn same(&self, other: &Item) -> bool {
        match (self, other) {
            (Item::Link { name: a, .. }, Item::Link { name: b, .. }) => a == b,
            _ => self == other,
        }
    }
}

/// An inspection target's items (`SelectionTarget::None`: none), in id order.
pub fn target_items(target: &SelectionTarget) -> Vec<Item> {
    match target {
        SelectionTarget::None => Vec::new(),
        SelectionTarget::Components { ids } => ids.iter().map(|id| Item::Component { id: id.clone() }).collect(),
        SelectionTarget::Ports { ids } => ids.iter().map(|id| Item::Port { id: id.clone() }).collect(),
        SelectionTarget::Nets { ids } => ids.iter().map(|id| Item::Net { id: id.clone() }).collect(),
    }
}

/// One selected item, of one document at one revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Selected {
    pub item: Item,
    pub document: DocumentId,
    pub revision: u64,
}

/// How [`SelectionAction`] combines its items with what is selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// Replace the document's items (duplicates dropped).
    Set,
    /// Append the ones not selected (Shift).
    Add,
    /// Toggle each (Ctrl; wins over Shift).
    Toggle,
    /// Remove these.
    Remove,
    /// Remove every item of the document (`items` is ignored).
    Clear,
}

/// The one selection action.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectionAction {
    pub op: Op,
    pub document: DocumentId,
    /// Each with the revision it was picked at (None: the current one).
    pub items: Vec<(Item, Option<u64>)>,
}
impl SelectionAction {
    /// Items picked at the document's current revision.
    pub fn new(op: Op, document: DocumentId, items: impl IntoIterator<Item = Item>) -> Self {
        Self { op, document, items: items.into_iter().map(|i| (i, None)).collect() }
    }
    pub fn set(document: DocumentId, items: impl IntoIterator<Item = Item>) -> Self {
        Self::new(Op::Set, document, items)
    }
    pub fn clear(document: DocumentId) -> Self {
        Self::new(Op::Clear, document, [])
    }
    /// Items picked at a known revision (a 3D pick against a drawn mesh).
    pub fn picked(op: Op, document: DocumentId, revision: u64, items: impl IntoIterator<Item = Item>) -> Self {
        Self { op, document, items: items.into_iter().map(|i| (i, Some(revision))).collect() }
    }
}
// No REST command of its own: each mode's selection command is the adapter.
impl<'de> serde::Deserialize<'de> for SelectionAction {
    fn deserialize<D: serde::Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
        Err(serde::de::Error::custom("the selection is changed through each mode's own selection command"))
    }
}
impl Action for SelectionAction {
    fn commands() -> Vec<Spec> {
        Vec::new()
    }
    fn accepts() -> Vec<&'static str> {
        Vec::new()
    }
}

/// What a re-check after the document advanced does with an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recheck {
    /// It still resolves: stamped with the new revision.
    Restamp,
    /// Kept at the revision it was picked at (the mode refuses stale uses itself).
    Keep,
    /// It no longer resolves: dropped and named.
    Drop,
}

/// The one selection.
#[derive(Resource, Default, Debug)]
pub struct Selection {
    items: Vec<Selected>,
    /// Bumped on every change.
    pub changed: u64,
    /// The items the last re-check or forget dropped, by name (shown by the
    /// mode until the next change).
    pub dropped: Vec<String>,
}

impl Selection {
    /// Every item, in selection order.
    pub fn all(&self) -> &[Selected] {
        &self.items
    }
    /// `document`'s items, in selection order.
    pub fn of(&self, document: DocumentId) -> impl Iterator<Item = &Selected> {
        self.items.iter().filter(move |s| s.document == document)
    }
    /// `document`'s items without their stamps.
    pub fn items(&self, document: DocumentId) -> Vec<Item> {
        self.of(document).map(|s| s.item.clone()).collect()
    }
    pub fn is_empty_for(&self, document: DocumentId) -> bool {
        self.of(document).next().is_none()
    }
    pub fn contains(&self, document: DocumentId, item: &Item) -> bool {
        self.of(document).any(|s| s.item.same(item))
    }
    /// The component ids selected in `document`, in order.
    pub fn components(&self, document: DocumentId) -> Vec<String> {
        self.of(document).filter_map(|s| if let Item::Component { id } = &s.item { Some(id.clone()) } else { None }).collect()
    }
    /// `document`'s items as an inspection target (`sim_inspect`'s shape:
    /// one kind, the first item's; None when nothing of those kinds is
    /// selected). What Inspect, the builder scene, notes and the selection
    /// link exchange.
    pub fn target(&self, document: DocumentId) -> SelectionTarget {
        let mut first = None;
        let mut ids = std::collections::BTreeSet::new();
        for s in self.of(document) {
            let (kind, id) = match &s.item {
                Item::Component { id } => ("component", id),
                Item::Port { id } => ("port", id),
                Item::Net { id } => ("net", id),
                _ => continue,
            };
            if *first.get_or_insert(kind) == kind {
                ids.insert(id.clone());
            }
        }
        match first {
            None => SelectionTarget::None,
            Some("component") => SelectionTarget::Components { ids },
            Some("port") => SelectionTarget::Ports { ids },
            Some(_) => SelectionTarget::Nets { ids },
        }
    }
    /// The robot link selected in `document` (the first).
    pub fn link(&self, document: DocumentId) -> Option<usize> {
        self.of(document).find_map(|s| if let Item::Link { index, .. } = &s.item { Some(*index) } else { None })
    }
    /// The CAD items selected in `document`, as RoboCAD writes them, in order.
    pub fn cad(&self, document: DocumentId) -> Vec<SelectionItem> {
        self.of(document).filter_map(|s| if let Item::Cad(i) = &s.item { Some(i.clone()) } else { None }).collect()
    }
    /// The CAD items with the revision each was picked at.
    pub fn cad_stamped(&self, document: DocumentId) -> Vec<(SelectionItem, u64)> {
        self.of(document).filter_map(|s| if let Item::Cad(i) = &s.item { Some((i.clone(), s.revision)) } else { None }).collect()
    }

    /// The one validated apply. Refused, naming it: a document that is not
    /// in the registry, or an item picked at a revision other than the
    /// document's current one. Returns whether the selection changed.
    pub fn apply(&mut self, registry: &DocumentRegistry, action: &SelectionAction) -> Result<bool, String> {
        let entry = registry.get(action.document).ok_or_else(|| format!("document {} is not open in this window", action.document.0))?;
        let current = entry.revision;
        let mut wanted = Vec::with_capacity(action.items.len());
        for (item, revision) in &action.items {
            match revision {
                Some(r) if *r != current => {
                    return Err(format!("{} was picked at revision {r} of {}, which is now at revision {current}; pick it again", item.label(), entry.source.describe()));
                }
                _ => wanted.push(Selected { item: item.clone(), document: action.document, revision: current }),
            }
        }
        let before = self.items.clone();
        let doc = action.document;
        match action.op {
            Op::Clear => self.items.retain(|s| s.document != doc),
            Op::Set => {
                self.items.retain(|s| s.document != doc);
                for w in wanted {
                    if !self.contains(doc, &w.item) {
                        self.items.push(w);
                    }
                }
            }
            Op::Add => {
                for w in wanted {
                    if !self.contains(doc, &w.item) {
                        self.items.push(w);
                    }
                }
            }
            Op::Toggle => {
                for w in wanted {
                    match self.items.iter().position(|s| s.document == doc && s.item.same(&w.item)) {
                        Some(i) => {
                            self.items.remove(i);
                        }
                        None => self.items.push(w),
                    }
                }
            }
            Op::Remove => self.items.retain(|s| s.document != doc || !wanted.iter().any(|w| w.item.same(&s.item))),
        }
        let changed = self.items != before;
        if changed {
            self.changed += 1;
            self.dropped.clear();
        }
        Ok(changed)
    }

    /// After `document` advanced to `revision` (an edit or reload its mode
    /// counts): each of its items is restamped, kept as picked, or dropped
    /// (named in [`Selection::dropped`]); `recheck` may also update an item
    /// (a link's index re-found by name). Returns the dropped labels.
    pub fn revalidate(&mut self, document: DocumentId, revision: u64, recheck: impl FnMut(&mut Item) -> Recheck) -> Vec<String> {
        self.recheck(document, revision, false, recheck)
    }
    /// As [`Selection::revalidate`], but every item of `document` is
    /// re-checked, even one already stamped `revision`: its source was
    /// replaced under the same document and revision (a lesson's new
    /// sandbox builder).
    pub fn revalidate_all(&mut self, document: DocumentId, revision: u64, recheck: impl FnMut(&mut Item) -> Recheck) -> Vec<String> {
        self.recheck(document, revision, true, recheck)
    }
    fn recheck(&mut self, document: DocumentId, revision: u64, all: bool, mut recheck: impl FnMut(&mut Item) -> Recheck) -> Vec<String> {
        let mut dropped = Vec::new();
        let mut changed = false;
        self.items.retain_mut(|s| {
            if s.document != document || (s.revision == revision && !all) {
                return true;
            }
            let before = s.item.clone();
            match recheck(&mut s.item) {
                Recheck::Restamp => {
                    s.revision = revision;
                    changed |= s.item != before;
                    true
                }
                Recheck::Keep => {
                    changed |= s.item != before;
                    true
                }
                Recheck::Drop => {
                    dropped.push(before.label());
                    false
                }
            }
        });
        if changed || !dropped.is_empty() {
            self.changed += 1;
            self.dropped = dropped.clone();
        }
        dropped
    }

    /// `document` was replaced or closed for good: its items go.
    pub fn forget(&mut self, document: DocumentId) {
        let before = self.items.len();
        self.items.retain(|s| s.document != document);
        if self.items.len() != before {
            self.changed += 1;
        }
    }

    /// As `state` and REST answers show it.
    pub fn json(&self, document: Option<DocumentId>) -> Value {
        let items: Vec<&Selected> = match document {
            Some(d) => self.of(d).collect(),
            None => self.items.iter().collect(),
        };
        json!({"items": items, "changed": self.changed, "dropped": self.dropped})
    }
}

/// The one apply system (`ViewerSet::Actions`): picks, list rows and keys.
/// A refusal from a click is logged (the mode's status line shows its own
/// refusals for its REST adapters); a REST origin is answered with it.
pub(crate) fn apply_actions(mut messages: ResMut<Messages<Act<SelectionAction>>>, mut in_flight: ResMut<InFlight<SelectionAction>>, mut replies: ResMut<Replies>, mut selection: ResMut<Selection>, registry: Res<DocumentRegistry>) {
    actions::apply(&mut messages, &mut in_flight, &mut replies, |action, call| {
        let result = selection.apply(&registry, action);
        if let (Err(e), false) = (&result, call.rest()) {
            warn!("selection: {e}");
        }
        Outcome::Done(result.map(|changed| json!({"changed": changed})))
    });
}

/// The selection and its action (every mode; no window needed).
pub(crate) fn build(app: &mut App) {
    actions::register::<SelectionAction>(app);
    app.init_resource::<Selection>().init_resource::<DocumentRegistry>().init_resource::<Replies>().add_systems(Update, apply_actions.in_set(ViewerSet::Actions));
}

/// The modes a selection belongs to (every mode that shows a document with items).
pub const MODES: &[ViewerMode] = &[ViewerMode::Inspect, ViewerMode::Build, ViewerMode::Lessons, ViewerMode::Robot, ViewerMode::Cad];

#[cfg(test)]
mod tests;

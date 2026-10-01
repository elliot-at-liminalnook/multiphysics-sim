//! The builder's view of the one selection (native-viewer.md §7). The
//! builder keeps no selection of its own: what is selected is the
//! `Item::Component { id }` items of the Build document in
//! [`crate::selection::Selection`] (`DocumentRegistry::current(Build)`, the
//! document the lesson screen's builder shows too), each an instance name at
//! the builder's level, as `system_state.selected` lists them.
//!
//! - Every change goes through `Selection::apply` ([`Picked::set`],
//!   [`Picked::toggle`], [`Picked::clear`]): the Outline, schematic boxes,
//!   3D picks, `system_select`, `system_ui` and the builder's own edits
//!   (grouping selects the group, a delete or a level change clears).
//! - When the builder's document advances (`document.revision`: an edit, an
//!   undo, a reload from disk) the registry gets the new revision and the
//!   items are re-checked by name ([`sync`]): restamped while the instance
//!   still exists at the builder's level, else dropped and named in
//!   `Selection::dropped`.
//! - The highlight (the builder scene's components, the schematic, the
//!   Outline, the inspector) is projected from it; [`track`] re-projects
//!   when `Selection::changed` moves.
use super::*;
use crate::document::{DocumentId, DocumentKind, Source};
use crate::selection::{Item, Op, Recheck, SelectionAction};

/// Why a selection change is refused when the Build document is not open.
const NO_DOCUMENT: &str = "the builder's system is not open in the document registry";

/// The builder's document while it is open (Build, or Lessons over the builder).
pub(crate) fn document(registry: &DocumentRegistry) -> Option<DocumentId> {
    registry.current(ViewerMode::Build).map(|(id, _)| id)
}

/// The selected instance names (sorted, as `system_state.selected` lists them).
pub(crate) fn names(selection: &Selection, registry: &DocumentRegistry) -> BTreeSet<String> {
    document(registry).map(|d| selection.components(d).into_iter().collect()).unwrap_or_default()
}

/// The one selected instance, when exactly one is.
pub(crate) fn only(names: &BTreeSet<String>) -> Option<String> {
    if names.len() == 1 { names.iter().next().cloned() } else { None }
}

/// The selection and the registry, borrowed by a builder handler that reads
/// or changes what is selected.
pub(crate) struct Picked<'a> {
    pub(crate) selection: &'a mut Selection,
    pub(crate) registry: &'a mut DocumentRegistry,
}

impl<'a> Picked<'a> {
    pub(crate) fn new(selection: &'a mut Selection, registry: &'a mut DocumentRegistry) -> Self {
        Self { selection, registry }
    }
    pub(crate) fn document(&self) -> Option<DocumentId> {
        document(&*self.registry)
    }
    /// The selected instance names, sorted.
    pub(crate) fn names(&self) -> BTreeSet<String> {
        names(&*self.selection, &*self.registry)
    }
    pub(crate) fn only(&self) -> Option<String> {
        only(&self.names())
    }
    /// The first selected name (by name order).
    pub(crate) fn first(&self) -> Option<String> {
        self.names().into_iter().next()
    }
    /// One `SelectionAction` of component items, through `Selection::apply`.
    fn act(&mut self, op: Op, names: impl IntoIterator<Item = String>) -> Result<bool, String> {
        let document = self.document().ok_or(NO_DOCUMENT)?;
        let action = SelectionAction::new(op, document, names.into_iter().map(|id| Item::Component { id }));
        self.selection.apply(&*self.registry, &action)
    }
    /// Select exactly these instances.
    pub(crate) fn set(&mut self, names: impl IntoIterator<Item = String>) -> Result<bool, String> {
        self.act(Op::Set, names)
    }
    /// Shift-click: add the instance, or remove it if selected.
    pub(crate) fn toggle(&mut self, name: String) -> Result<bool, String> {
        self.act(Op::Toggle, [name])
    }
    pub(crate) fn clear(&mut self) -> Result<bool, String> {
        self.act(Op::Clear, [])
    }
    /// Copy the builder's revision to the registry and re-check the items ([`sync`]).
    pub(crate) fn sync(&mut self, b: &Builder) -> Vec<String> {
        sync(self.selection, self.registry, b)
    }
    /// `system_state` with the current selection (re-checked first, so a REST
    /// answer after an edit never lists a removed instance).
    pub(crate) fn state(&mut self, b: &Builder) -> serde_json::Value {
        self.sync(b);
        b.state_json(&self.names())
    }
}

impl Builder {
    /// The instance names at the builder's level.
    pub(crate) fn level_instances(&self) -> BTreeSet<String> {
        self.definition_id().and_then(|id| self.document.definitions.get(&id).map(|d| d.instances.keys().cloned().collect::<BTreeSet<String>>())).unwrap_or_default()
    }

    /// `system_select`, the Outline and schematic rows: select exactly these
    /// instances of the current level. An unknown name is refused, naming
    /// it, and the selection stays.
    pub(crate) fn select(&mut self, pick: &mut Picked, names: Vec<String>) -> Result<(), String> {
        let instances = self.level_instances();
        if let Some(unknown) = names.iter().find(|n| !instances.contains(n.as_str())) {
            let level = if self.level.is_empty() { "the top level".to_string() } else { format!("level `{}`", self.level) };
            return Err(format!("no instance `{unknown}` at {level}"));
        }
        pick.set(names)?;
        self.alternatives = None;
        self.panel_dirty = true;
        Ok(())
    }

    /// Drill to `path` (`set_level`); the names selected at the old level clear.
    pub(crate) fn enter_level(&mut self, pick: &mut Picked, path: &str) -> Result<(), String> {
        self.set_level(path)?;
        // Without an open Build document there is nothing selected to clear.
        let _ = pick.clear();
        Ok(())
    }
}

/// The builder's document advanced (or not): copy `document.revision` to the
/// registry and re-check its items by name at the builder's level. Returns
/// the labels it dropped (also in `Selection::dropped`).
pub(crate) fn sync(selection: &mut Selection, registry: &mut DocumentRegistry, b: &Builder) -> Vec<String> {
    recheck(selection, registry, b, false)
}

/// [`sync`]; `all` re-checks every item, even one stamped with the current
/// revision (a builder replaced under the same document, whose revision
/// may equal the old one's).
fn recheck(selection: &mut Selection, registry: &mut DocumentRegistry, b: &Builder, all: bool) -> Vec<String> {
    let Some(document) = document(registry) else { return Vec::new() };
    let revision = b.document.revision;
    registry.set_revision(document, revision);
    if !all && selection.of(document).all(|s| s.revision == revision) {
        return Vec::new();
    }
    let instances = b.level_instances();
    let check = |item: &mut Item| match item {
        Item::Component { id } if instances.contains(id.as_str()) => Recheck::Restamp,
        Item::Component { .. } => Recheck::Drop,
        // Not the builder's kind of item: left as picked.
        _ => Recheck::Keep,
    };
    if all { selection.revalidate_all(document, revision, check) } else { selection.revalidate(document, revision, check) }
}

/// The Systems tab opened another file (`finish_open`): the Build document
/// is now that file, and the items of the one it replaced leave the
/// selection. A no-op when the registry already names this file.
pub(crate) fn follow_open(selection: &mut Selection, registry: &mut DocumentRegistry, path: &std::path::Path) {
    let source = Source::path(path);
    if registry.entry(ViewerMode::Build).is_some_and(|e| e.kind == DocumentKind::System && e.source.same_document(&source)) {
        return;
    }
    let opened = registry.open(ViewerMode::Build, DocumentKind::System, source);
    if let Some(old) = opened.replaced {
        selection.forget(old);
    }
}

/// The selected instances as the builder scene's highlight: every part at or
/// under each selected instance's full path.
pub(super) fn project(b: &Builder, scene: &mut SpatialScene, names: &BTreeSet<String>) {
    let paths: Vec<String> = names.iter().map(|n| b.full_path(n)).collect();
    let chosen: BTreeSet<String> = scene.spatial.parts.iter().map(|p| p.component.clone()).filter(|c| paths.iter().any(|n| c == n || c.starts_with(&format!("{n}/")))).collect();
    let _ = scene.set_selection(if chosen.is_empty() { SelectionTarget::None } else { SelectionTarget::Components { ids: chosen } });
}

/// SimSync, before the scene and panels: an edit made anywhere (a handler,
/// the file watch, the agent, a drop) re-checks the items ([`sync`]); a
/// selection change redraws the panels and, in Build, re-projects the
/// scene's highlight (the lesson screen draws its own in Lessons).
///
/// A builder never tracked before (a new one: a lesson's sandbox builder
/// installed under the same Build document) re-checks every item, so names
/// of the replaced builder's instances do not stay selected. Items a
/// re-check drops are named in the status line. Entering Build re-projects
/// (Lessons drew its own highlight meanwhile).
pub(super) fn track(mut builder: ResMut<Builder>, mut selection: ResMut<Selection>, mut registry: ResMut<DocumentRegistry>, mut scene: ResMut<SpatialScene>, mode: Option<Res<State<ViewerMode>>>, mut last_mode: Local<Option<ViewerMode>>) {
    let Some((doc, revision)) = registry.current(ViewerMode::Build) else { return };
    let current = builder.document.revision;
    let fresh = builder.seen_selection.is_none();
    let dropped = if fresh || revision != current || selection.of(doc).any(|s| s.revision != current) {
        recheck(&mut selection, &mut registry, &builder, fresh)
    } else {
        Vec::new()
    };
    if !dropped.is_empty() {
        builder.status = format!("No longer selected (not in the system now): {}", dropped.join(", "));
    }
    let mode = mode.map(|m| *m.get());
    let entered_build = mode == Some(ViewerMode::Build) && *last_mode != Some(ViewerMode::Build);
    *last_mode = mode;
    if builder.seen_selection == Some(selection.changed) && !entered_build {
        return;
    }
    builder.seen_selection = Some(selection.changed);
    builder.panel_dirty = true;
    if mode.is_none_or(|m| m == ViewerMode::Build) {
        let selected = names(&selection, &registry);
        project(&builder, &mut scene, &selected);
    }
}

#[cfg(test)]
mod tests;

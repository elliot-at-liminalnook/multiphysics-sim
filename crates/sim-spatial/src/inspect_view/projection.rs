//! Inspect's selection, shown: the shared [`Selection`]'s items of the
//! Inspect document projected to [`SpatialScene::shown`] (its details,
//! highlights and inspector), and the items re-checked when the document is
//! reloaded or replaced. The window runs [`project_selection`]; the
//! headless server calls [`project`] from its loop.
use super::SpatialScene;
use crate::app::ViewerMode;
use crate::document::{DocumentId, DocumentRegistry};
use crate::selection::{Item, Recheck, Selection, SelectionAction, target_items};
use bevy::prelude::*;
use sim_inspect::SystemDescription;

/// What the view last projected: the selection's change count, and the
/// Inspect document and its revision.
pub(crate) type Seen = (u64, DocumentId, u64);

/// Whether an Inspect item still names something in `description`
/// (anything else is not Inspect's and goes).
pub(crate) fn recheck(description: &SystemDescription, item: &Item) -> Recheck {
    let found = match item {
        Item::Component { id } => description.components.contains_key(id),
        Item::Port { id } => description.ports.contains_key(id),
        Item::Net { id } => description.nets.contains_key(id),
        _ => false,
    };
    if found { Recheck::Restamp } else { Recheck::Drop }
}

/// Project Inspect's items to the view when the selection or the document
/// changed since `seen`:
/// - the first projection of the window's life adopts what the scene
///   already shows (the launch's `--select`) when the document has no items;
/// - a new document or revision (a reload, a reopen) re-checks the items
///   against the scene's description: each is restamped or dropped by name
///   (`Selection::dropped`, and logged);
/// - the items' target is shown unless it already is (so a hidden selected
///   part stays hidden until it is selected again).
///
/// The scene must be the document's (installed with it). Returns the labels
/// dropped.
pub(crate) fn project(scene: &mut SpatialScene, selection: &mut Selection, registry: &DocumentRegistry, seen: &mut Option<Seen>) -> Vec<String> {
    let Some((document, revision)) = registry.current(ViewerMode::Inspect) else { return Vec::new() };
    if *seen == Some((selection.changed, document, revision)) {
        return Vec::new();
    }
    if seen.is_none() && selection.is_empty_for(document) && scene.shown != sim_inspect::selection::SelectionTarget::None {
        if let Err(e) = selection.apply(registry, &SelectionAction::set(document, target_items(&scene.shown))) {
            error!("inspect: {e}");
        }
    }
    let mut dropped = Vec::new();
    if seen.is_none_or(|(_, d, r)| d != document || r != revision) {
        let description = &scene.description;
        dropped = selection.revalidate(document, revision, |item| recheck(description, item));
        if !dropped.is_empty() {
            warn!("inspect: the reloaded assembly no longer has {}; deselected", dropped.join(", "));
        }
    }
    let target = selection.target(document);
    if target != scene.shown {
        // Every item resolves (re-checked above, validated when applied); a
        // refusal is logged, as the link's and the notes' were.
        if let Err(e) = scene.set_selection(target) {
            error!("{e}");
        }
    }
    *seen = Some((selection.changed, document, revision));
    dropped
}

/// SimSync (Inspect only): [`project`] in the window.
pub(crate) fn project_selection(mut scene: ResMut<SpatialScene>, selection: Option<ResMut<Selection>>, registry: Option<Res<DocumentRegistry>>, mut seen: Local<Option<Seen>>) {
    let (Some(mut selection), Some(registry)) = (selection, registry) else { return };
    let key = registry.current(ViewerMode::Inspect).map(|(d, r)| (selection.changed, d, r));
    // Nothing to do: leave the resources unmarked.
    if key.is_none() || *seen == key {
        return;
    }
    project(&mut scene, &mut selection, &registry, &mut seen);
}

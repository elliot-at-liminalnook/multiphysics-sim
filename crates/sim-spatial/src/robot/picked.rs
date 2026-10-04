//! Robot mode's view of the one selection (native-viewer.md §7). The view
//! keeps no selection of its own: the selected link is the
//! `Item::Link { index, name }` of the Robot document
//! (`DocumentRegistry::current(Robot)`) in [`crate::selection::Selection`]
//! (a body's index and name for a planar v2 file). The list rows, a 3D
//! click and `system_ui` `link:<index>` / `clear_selection` change it through
//! `Selection::apply` (`RobotAction::SelectLink` / `ClearSelection`); a
//! reload advances the document's revision and finds the link again by
//! name ([`reloaded`]).
use super::*;
use crate::document::{DocumentId, DocumentKind, Source};
use crate::selection::{Item, Recheck, SelectionAction};

/// Why a selection change is refused when the Robot document is not open.
const NO_DOCUMENT: &str = "the robot is not open in the document registry";

/// Robot mode's document while it is open.
pub(crate) fn document(registry: &DocumentRegistry) -> Option<DocumentId> {
    registry.current(ViewerMode::Robot).map(|(id, _)| id)
}

/// The selected link's index into the loaded model (a body's for a planar file).
pub(crate) fn link(selection: &Selection, registry: &DocumentRegistry) -> Option<usize> {
    document(registry).and_then(|d| selection.link(d))
}

/// The selected link's name as it was picked.
fn picked_name(selection: &Selection, document: DocumentId) -> Option<String> {
    selection.of(document).find_map(|s| if let Item::Link { name, .. } = &s.item { Some(name.clone()) } else { None })
}

/// Select link `index`, named `name`: the one link of the Robot document.
pub(super) fn select(selection: &mut Selection, registry: &DocumentRegistry, index: usize, name: String) -> Result<bool, String> {
    let document = document(registry).ok_or(NO_DOCUMENT)?;
    selection.apply(registry, &SelectionAction::set(document, [Item::Link { index, name }]))
}

/// Nothing selected in the Robot document.
pub(super) fn clear(selection: &mut Selection, registry: &DocumentRegistry) -> Result<bool, String> {
    let document = document(registry).ok_or(NO_DOCUMENT)?;
    selection.apply(registry, &SelectionAction::clear(document))
}

/// A model was installed. A reload (`reload`: the file changed or Reload)
/// advances the Robot document's revision; an open keeps the revision the
/// registry gave it. Items picked at another revision are found again by
/// name in the new model (`find`; the index may differ), else dropped.
/// Returns the name selected before and whether it is still selected (None:
/// nothing was selected).
pub(super) fn reloaded(selection: &mut Selection, registry: &mut DocumentRegistry, reload: bool, find: impl Fn(&str) -> Option<usize>) -> Option<(String, bool)> {
    let document = document(registry)?;
    let before = picked_name(selection, document);
    let revision = if reload { registry.bump(document)? } else { registry.revision(document)? };
    selection.revalidate(document, revision, |item| match item {
        Item::Link { index, name } => match find(name.as_str()) {
            Some(i) => {
                *index = i;
                Recheck::Restamp
            }
            None => Recheck::Drop,
        },
        _ => Recheck::Keep,
    });
    before.map(|name| (name, selection.link(document).is_some()))
}

/// REST `robot_preset` replaced the view in this mode: the Robot document is
/// now that preset (the previous document's items go) and, as a new view,
/// it starts with nothing selected.
/// A robot file opened in place (`robot_open`): the Robot document is that file.
pub(super) fn opened_file(selection: &mut Selection, registry: &mut DocumentRegistry, path: &std::path::Path) {
    let opened = registry.open(ViewerMode::Robot, DocumentKind::Robot, Source::path(path.to_path_buf()));
    if let Some(old) = opened.replaced {
        selection.forget(old);
    }
    let _ = selection.apply(registry, &SelectionAction::clear(opened.id));
}

pub(super) fn opened_preset(selection: &mut Selection, registry: &mut DocumentRegistry, id: &str) {
    let opened = registry.open(ViewerMode::Robot, DocumentKind::Robot, Source::Preset { id: id.to_string() });
    if let Some(old) = opened.replaced {
        selection.forget(old);
    }
    // The same preset again keeps its document id: its link is cleared here.
    let _ = selection.apply(registry, &SelectionAction::clear(opened.id));
}

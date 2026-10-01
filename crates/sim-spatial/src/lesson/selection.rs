//! The lesson page's pick and the shared selection. Lessons show the
//! builder's system (the scene's sandbox copy, the Build document, open in
//! Lessons mode too), so a part picked in a lesson scene is selected in the
//! Build document as the builder names it: the instance at the builder's
//! level that holds the picked part, the same item a click in Build selects
//! (`builder::rebuild::click_part`). The page keeps the picked part's full
//! path (`Learn::picked`) for what only it shows (the zoom, the margin's
//! part, a note's anchor); the 3D highlight stays the page's own display
//! (hover, pick or the script's highlight, `scene_view::playback`).
use super::Learn;
use crate::app::ViewerMode;
use crate::builder::Builder;
use crate::document::DocumentRegistry;
use crate::selection::{Item, Selection, SelectionAction};
use bevy::prelude::*;

/// The Build document's item for a picked part (None: nothing picked, or a
/// part outside the builder's level).
fn item(builder: &Builder, picked: Option<&str>) -> Option<Item> {
    picked.and_then(|p| builder.instance_for_component(p)).map(|id| Item::Component { id })
}

/// After a lesson action changed the pick (Lessons mode): the Build
/// document's selection becomes the picked part's instance, or nothing.
pub(super) fn share(picked: Option<&str>, builder: &Builder, selection: &mut Selection, registry: &DocumentRegistry) {
    let Some((document, _)) = registry.current(ViewerMode::Build) else { return };
    if let Err(e) = selection.apply(registry, &SelectionAction::set(document, item(builder, picked))) {
        warn!("lesson: {e}");
    }
}

/// SimSync (Lessons): the pick follows the shared selection. When the Build
/// document's components were selected elsewhere (the builder, while the
/// learner had it open) and none of them holds the picked part, the pick is
/// dropped. An empty selection (cleared, or a new sandbox's document) keeps
/// the pick, so a part link that opens another scene keeps its part.
pub(super) fn follow(mut learn: ResMut<Learn>, selection: Option<Res<Selection>>, registry: Option<Res<DocumentRegistry>>, builder: Option<Res<Builder>>, mut seen: Local<u64>) {
    let (Some(selection), Some(registry), Some(builder)) = (selection, registry, builder) else { return };
    if *seen == selection.changed {
        return;
    }
    *seen = selection.changed;
    let Some((document, _)) = registry.current(ViewerMode::Build) else { return };
    let Some(picked) = item(&builder, learn.picked.as_deref()) else { return };
    if !selection.components(document).is_empty() && !selection.contains(document, &picked) {
        learn.picked = None;
        learn.dirty = true;
    }
}

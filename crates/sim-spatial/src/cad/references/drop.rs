//! Files dropped on the window in CAD mode add reference images (RoboCAD:
//! the viewport's drop hands URLs to the References panel, ui/app.py:243-245
//! and 1801-1803; the panel's own drop, references.py:224-230; both call
//! `add_paths`, which imports them all in one `import_references`).
//!
//! Bevy 0.19.1 facts (`bevy_window-0.19.1/src/event.rs:372-406`):
//! `FileDragAndDrop` is a `Message` with `DroppedFile { window, path_buf }`,
//! `HoveredFile { window, path_buf }` and `HoveredFileCanceled { window }`,
//! registered by `WindowPlugin` (`bevy_window-0.19.1/src/lib.rs:123`,
//! `add_message::<FileDragAndDrop>()`); winit writes one `DroppedFile` per
//! file. The window has one drop target, so the modes share it by state:
//! this reader runs only in CAD mode, and the builder's
//! (`builder/drafts.rs:drops`) only in Build mode (`builder.rs:536`,
//! `drops.run_if(building)`, `building = in_state(ViewerMode::Build)` at
//! :527). A reader gated off keeps its cursor, so on the frame CAD mode is
//! entered the drops still buffered from the mode before (messages live two
//! updates) are skipped, not imported.
//!
//! Deliberately different: RoboCAD sends every dropped file to
//! `import_references` and Pillow refuses a non-image there; here a drop
//! naming a folder or a file without an image suffix (png, jpg, jpeg, webp,
//! bmp) is refused by name before anything is sent (`edits::image_path`).
use super::{ReferencesArgs, ReferencesOp};
use crate::app::actions::Act;
use crate::app::{ViewerMode};
use crate::cad::actions::CadAction;
use bevy::prelude::*;
use bevy::window::FileDragAndDrop;

/// The paths of this frame's drops (hovering is not a drop).
pub(crate) fn dropped(events: impl IntoIterator<Item = FileDragAndDrop>) -> Vec<String> {
    events
        .into_iter()
        .filter_map(|e| match e {
            FileDragAndDrop::DroppedFile { path_buf, .. } => Some(path_buf.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

/// The action this frame's drops write: one import of them all.
pub(crate) fn action(paths: Vec<String>) -> Option<CadAction> {
    (!paths.is_empty()).then(|| ReferencesArgs { paths: Some(paths), ..ReferencesArgs::of(ReferencesOp::Add) }.action())
}

/// Input: the frame's dropped files as one `CadReferences {op: add}`.
pub(crate) fn drops(mut events: MessageReader<FileDragAndDrop>, mode: Res<State<ViewerMode>>, mut out: MessageWriter<Act<CadAction>>) {
    let entered = mode.is_changed();
    let paths = dropped(events.read().cloned());
    if entered {
        return;
    }
    if let Some(action) = action(paths) {
        out.write(Act::ui(action));
    }
}

/// CadPlugin: [`drops`] in CAD mode only.
pub(super) fn build(app: &mut App) {
    app.add_systems(Update, drops.in_set(crate::app::InputSet::Window).run_if(in_state(ViewerMode::Cad)));
}

//! CAD mode's model tree (left dock) as RoboCAD's outliner
//! (`OutlinerPanel`, cad/robocad/ui/widgets.py:225-437; cad-organize). One
//! row per node of RoboCAD's document in its walk order, indented by depth;
//! everything below is a [`CadAction`] (`CadTree`, REST `cad_tree`:
//! [`TreeArgs`]) handled once in [`handle`], and every edit is exactly one
//! RoboCAD call through `actions::edit_at`, refused by name with nothing
//! sent while another edit is in flight, when not connected, when the shown
//! tree is behind RoboCAD's, or when RoboCAD's revision moved since the row,
//! menu, drag or dialog was read (`revision`).
//!
//! - **Rows** (`rows::draw`, `Part::Tree`): kind tag, name, a "locked" flag
//!   and the visibility chip (`CadPatch {visible}`, RoboCAD's own flag; it
//!   reads "Disabled" for a disabled node and "Hidden by parent" when an
//!   ancestor hides or disables it). A row with children has a "+"/"−"
//!   disclosure chip (`toggle`). RoboCAD's active group is drawn in its
//!   blue, `QColor(120, 200, 255)` (widgets.py:299-300; `rows::ACTIVE_GROUP`,
//!   the kit has no blue token), a node not effectively visible in grey
//!   (widgets.py:301-302). Rows are compact single lines built like robot
//!   mode's link rows (the kit's two-line `list_item` needs an icon CAD
//!   kinds have none of). A selection change restyles the rows in place
//!   (`rows::highlight`); the tree is rebuilt only when its key changes.
//! - **Search** (`rows::tools`, `Part::TreeTools`): the kit field
//!   "Search (Ctrl+F)…" (ui/strings.py:20) filters as typed (`search`): the
//!   nodes whose names contain the text (case-insensitive, stripped), with
//!   their ancestors and descendants, every row expanded (`state::matches`,
//!   widgets.py:276-288, 306). Ctrl+F (Cmd+F) with the pointer over the dock
//!   focuses it ([`search`], before `keys::keys`, consuming the key: Ctrl+F
//!   is otherwise RoboCAD's Fillet).
//! - **Expand and collapse** (`expand`, `collapse`, `toggle`, `expand_all`,
//!   `collapse_all`; the tools' "New group", "Expand all", "Collapse all",
//!   widgets.py:235-241): `TreeState::collapsed` is RoboCAD's `_expansion`
//!   (a node is expanded unless recorded collapsed), kept across edits and
//!   refetches, and not changed while a search is typed (widgets.py:267-269).
//! - **Multi-select** (`select`; Qt's ExtendedSelection, widgets.py:250):
//!   a press selects the row alone, Ctrl/Cmd toggles it, Shift selects the
//!   shown rows from the anchor (the last row pressed without Shift) to it,
//!   replacing the selection. Each is the shared selection's `CadSelect`
//!   (`selection::handle`), the action a 3D pick writes. A plain press on
//!   an already selected row selects it alone on release without a drag, so
//!   a drag moves the whole selection.
//! - **Rename** (`begin_rename`, `rename`; `_renamed` and `_double`,
//!   widgets.py:341-352): a double-click (400 ms) opens the kit field on
//!   the row with the name selected; Enter sends one `PATCH /nodes/{id}
//!   {"name"}` with the name stripped (unchanged: nothing sent; empty:
//!   refused by name, the field stays); Escape or a press elsewhere ends it.
//! - **Drag and drop** (`input::rows`, `move`; `_drop`, widgets.py:366-381):
//!   Bevy picking's `Pointer<DragStart/Drag/DragEnter/DragOver/DragLeave/
//!   DragDrop/DragEnd>` messages on the rows. A drop on a group moves into
//!   it (parent = the group, index None), on any other row in front of it
//!   (its parent, `index_of`), under the rows to the top level; it moves
//!   the selection when the dragged row is selected, else that row: one
//!   `Ops.move_nodes`. RoboCAD's refusals (a target that is not a group, a
//!   group into itself or its descendants) are checked before sending
//!   (`state::move_plan`): no drop marker is drawn there, and a drop there
//!   sends nothing and says why in the status line.
//! - **Context menu** (`menu`; `_menu`, `_context_menu`, widgets.py:383-390,
//!   403-437):
//!   a right press on an unselected row selects it first; the kit popup
//!   lists Fit in view, Isolate, Hide, Show, Lock, Unlock, Group selection…,
//!   Move to group (Top level and every group's "A / B" path except the
//!   moving nodes and their descendants), Make unique (bake instance), Set
//!   as active group (one group selected), Delete, Clear active group and
//!   Show all (`controls::menu_rows`).
//! - **Group selection… and New group** (`group_dialog`, `group`; `_group`,
//!   widgets.py:392-395): the "Organize components" dialog with
//!   "Group name:"; OK sends one `Ops.group(ids, name)` (New group: ids []);
//!   an empty name is refused by name in the dialog.
//! - **Active group** (`set_active`): one `Ops.set_active_group` (a group,
//!   or none: Clear active group; commands.py:398-400).
//! - **Lock/Unlock, Hide/Show** (`lock`, `visible`): one `Ops.set_locked` /
//!   `Ops.set_visible` on the menu's nodes (commands.py:336-340). Group and
//!   move are `Ops.group` and `Ops.move_nodes` (commands.py:354-392).
//!
//! **Pointer input: messages, not observers.** native-viewer.md §3 keeps
//! observer triggers for pointer events on entities; the outliner departs
//! from that on purpose. Its gesture (press, double-click timing, the
//! pending plain press on a selected row, the drag's slop, target and end)
//! is one `Local` spanning several events, and the rows are despawned and
//! rebuilt during a drag, so one Input system reads Bevy picking's pointer
//! messages in order instead. Every `Pointer<E>` is both an `EntityEvent`
//! and a `Message` (`bevy_picking-0.19.1/src/events.rs:71-84`), written by
//! `pointer_events` beside each trigger (events.rs:597-614 the writers;
//! 824-835 `DragEnter`, 1007-1031 `DragDrop` then `DragEnd` on release) in
//! PreUpdate's `PickingSystems::Hover` (lib.rs:428-454). Messages do not
//! bubble (the triggers propagate through `PointerTraversal`,
//! events.rs:95-125), so a hit on a row's chip resolves to the row by
//! walking `ChildOf`, and the row's labels are `Pickable::IGNORE`
//! (lib.rs:198-245) so presses land on the row itself.
//!
//! `system_ui` lists `cad:tree:<id>` ([`controls`]); the snapshot carries
//! [`state_json`]. Deliberately different from RoboCAD: a group row's top
//! quarter drops in front of the group (RoboCAD always drops into a
//! group, so a group could not be reordered by dragging); a rename ends
//! without renaming when the field loses the keyboard (Qt's delegate
//! commits on focus loss); "Move to group" is a heading over its entries,
//! not a submenu; an empty group name is refused by name (RoboCAD does
//! nothing).
mod controls;
mod handle;
mod input;
pub(in crate::cad) mod popup;
pub(in crate::cad) mod rows;
mod state;
#[cfg(test)]
mod tests;

pub use handle::{TreeArgs, TreeOp};
pub use state::TreeState;

pub(in crate::cad) use controls::controls;
pub(in crate::cad) use handle::{handle, specs, state_json, select_action};
pub(in crate::cad) use input::search;
pub(in crate::cad) use rows::{EyeChip, TreeRowId, draw, highlight, key, tools, tools_key};
pub(in crate::cad) use popup::{TreeDialogRoot, TreeMenuRoot};

use super::actions::CadAction;
use super::document::CadDocument;
use crate::app::{ViewerMode, ViewerSet};
use bevy::prelude::*;

/// The action a RoboCAD command id stands for (`surfaces::registry`'s
/// `Do::Organize`): the outliner owns no registry command (its group
/// commands are the catalogue's `group.group` and `group.set_active`).
pub(crate) fn command_action(_id: &str) -> Option<CadAction> {
    None
}

/// The outliner's context menu is open (a left press then only dismisses it).
pub(crate) fn menu_open(doc: &CadDocument) -> bool {
    doc.tree.menu.is_some()
}

/// A connection (re)start (`sync::start`): the open rename, menu, dialog,
/// drag and any pending keyboard claim end, since the revisions they began
/// at belong to the old service. The search and collapse state stay.
pub(crate) fn restarted(doc: &mut CadDocument) {
    let t = &mut doc.tree;
    let open = t.rename.is_some() || t.menu.is_some() || t.dialog.is_some() || t.drag.is_some() || t.claim.is_some();
    t.rename = None;
    t.menu = None;
    t.dialog = None;
    t.drag = None;
    t.claim = None;
    if open {
        doc.touch();
    }
}

/// CadCorePlugin's windowless outliner systems: none (its state lives on
/// the document and its handler is `actions::handle`'s).
pub(crate) struct CoreParts;
impl Plugin for CoreParts {
    fn build(&self, _app: &mut App) {}
}

/// CadPlugin: the outliner's three kit fields, its input (Ctrl+F, the
/// menu, the rows' presses and drags, the fields; before CAD's keys) and
/// its popups (Present).
pub(in crate::cad) fn build(app: &mut App) {
    use crate::ui_kit::text::TextFieldApp;
    app.add_text_field(input::SEARCH, input::search_field())
        .add_text_field(input::RENAME, input::rename_field())
        .add_text_field(input::GROUP_NAME, input::group_field())
        // Before the two-step key gate (and so before CAD's keys, which run
        // after it): the open menu's Escape is consumed first. No system of
        // this chain is ordered after the gate, so there is no cycle.
        .add_systems(
            Update,
            (popup::input, input::rows, input::fields)
                .chain()
                .in_set(crate::app::InputSet::Window)
                .before(crate::cad::CadKeySet::Gate)
                .run_if(in_state(ViewerMode::Cad)),
        )
        // Ctrl+F: after the gate (a pending two-step key owns the frame:
        // `keys::free`), before CAD's keys, which it takes the key from.
        .add_systems(Update, search.run_if(super::keys::free).after(crate::cad::CadKeySet::Gate).in_set(crate::cad::CadKeySet::Focus).run_if(in_state(ViewerMode::Cad)))
        .add_systems(Update, popup::draw.in_set(ViewerSet::Present).run_if(in_state(ViewerMode::Cad)));
}

//! The catalogue's Outliner entries (cad-organize): RoboCAD's registry
//! commands `group.set_active` and `group.group` (`ui/app.py` :427-428),
//! each the one Ops call its handler makes. The outliner's own context
//! menu ("Group selection…" with its "Organize components" name dialog,
//! "Move to group ▸", Lock/Unlock, "Clear active group") and New group are
//! `tree`'s typed actions (`CadTree`), not catalogue rows: their arguments
//! (a target group, a typed name) come from the menu, not the selection.
use super::super::kinds::*;
use super::super::*;

pub(in crate::cad::ops) const ENTRIES: &[OpEntry] = &[
    OpEntry {
        id: "group.set_active",
        label: "Set selected group as active",
        category: "Outliner",
        needs: nodes(1, None, &["group"]),
        route: "set_active_group",
        args: &[Arg::Node],
        refusal: "Select a group to make it the active group",
        source: "ui/app.py:427 (Ops.set_active_group(the first selected group, else None), commands.py:398-400: RoboCAD's active group, not an undo step; RoboCAD clears the active group when no group is selected: here that is the outliner menu's \"Clear active group\", and the command refuses by name, ours)",
        ..BASE
    },
    OpEntry {
        id: "group.group",
        label: "Group selection",
        category: "Outliner",
        route: "group",
        args: &[Arg::Nodes],
        source: "ui/app.py:428 (Ops.group(selection.nodes()) with Ops.group's default name \"Group\", commands.py:354-364: a new group under the first root's parent holding the selection's roots, one undo step \"Group\"; with nothing selected RoboCAD makes an empty group, as here)",
        ..BASE
    },
];

//! The catalogue's View entries (cad-views-export): RoboCAD's Isolate, Show
//! All and Hide (`ui/app.py` `_build_commands` :312-314) as the one Ops call
//! each handler makes on the selected nodes (`viewport.selection.nodes()`),
//! each one RoboCAD undo step. In the registry they follow Edit and come
//! before Create; `edit_create` holds both, so they are joined after it.
use super::super::kinds::*;
use super::super::*;

pub(in crate::cad::ops) const ENTRIES: &[OpEntry] = &[
    OpEntry {
        id: "view.isolate",
        label: "Isolate",
        category: "View",
        keys: &["/"],
        needs: ANY_NODES,
        route: "isolate",
        args: &[Arg::Nodes],
        refusal: "Select the nodes to isolate",
        source: "ui/app.py:312 (label ui/strings.py, key keymap.json), commands.py:402-413 (Ops.isolate(ids): every node hidden but the selected nodes, their descendants and ancestors, one undo step \"Isolate\"; RoboCAD also runs it on an empty selection, hiding everything: refusal ours)",
        ..BASE
    },
    OpEntry {
        id: "view.show_all",
        label: "Show All",
        category: "View",
        keys: &["Alt+H"],
        route: "show_all",
        source: "ui/app.py:313 (label ui/strings.py, key keymap.json), commands.py:415-416 (Ops.show_all(): every node visible, one undo step \"Show all\")",
        ..BASE
    },
    OpEntry {
        id: "view.hide",
        label: "Hide",
        category: "View",
        keys: &["H"],
        needs: ANY_NODES,
        route: "set_visible",
        args: &[Arg::Nodes, Arg::Const("false")],
        refusal: "Select the nodes to hide",
        source: "ui/app.py:314 (label ui/strings.py, key keymap.json), commands.py:336-337 (Ops.set_visible(ids, False): one undo step \"Hide\"; RoboCAD pushes an empty step on an empty selection: refusal ours)",
        ..BASE
    },
];

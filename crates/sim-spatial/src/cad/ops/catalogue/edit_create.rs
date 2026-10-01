//! The catalogue's entries: Edit (Delete, Copy and Paste with Placement) and RoboCAD's `PrimitiveTool` (box, centre box, cylinder, sphere).
//! In RoboCAD's registry order (`ui/app.py` `_build_commands`); see `super`.
use super::super::kinds::*;
use super::super::*;

pub(in crate::cad::ops) const ENTRIES: &[OpEntry] = &[
    // ---- Edit ---------------------------------------------------------------
    OpEntry {
        id: "edit.delete",
        label: "Delete",
        category: "Edit",
        keys: &["Delete", "Backspace"],
        needs: ANY_NODES,
        route: "delete",
        args: &[Arg::Nodes],
        refusal: "Select the nodes to delete",
        clears_selection: true,
        source: "ui/app.py:293 (label ui/strings.py:14), ui/app.py:1458-1463 (one Ops.delete of every selected node, then the selection is cleared; silent when empty: refusal ours), commands.py:312",
        ..BASE
    },
    OpEntry {
        id: "edit.copy",
        label: "Copy with Placement",
        category: "Edit",
        keys: &["Ctrl+C"],
        needs: ANY_NODES,
        route: "clipboard",
        shape: Shape::Copy,
        refusal: "Select the nodes to copy",
        source: "ui/app.py:294 (label ui/strings.py:14), ui/app.py:1465-1468 (Document.copy_nodes of the selected nodes; RoboCAD copies an empty clip: refusal ours)",
        ..BASE
    },
    OpEntry {
        id: "edit.paste",
        label: "Paste with Placement",
        category: "Edit",
        keys: &["Ctrl+V"],
        route: "paste",
        shape: Shape::Paste,
        refusal: "Nothing to paste: copy with placement first (Ctrl+C)",
        source: "ui/app.py:295 (label ui/strings.py:14), ui/app.py:1470-1484 (paste_nodes keep_placement, one undo step \"Paste\"; RoboCAD's refusal \"Clipboard has no robocad content\" reads the system clipboard, the viewer's is its last copy)",
        ..BASE
    },
    // ---- Create: RoboCAD's PrimitiveTool -------------------------------------
    OpEntry {
        id: "tool.box",
        label: "Box (corner)",
        category: "Create",
        keys: &["Shift+A, B"],
        params: &[WIDTH, DEPTH, HEIGHT, p("corner", "corner", POINT, "0, 0, 0")],
        flow: Flow::Place(Primitive::BoxCorner),
        route: "box",
        shape: Shape::Place(Primitive::BoxCorner),
        hint: HINT_CORNER,
        source: "ui/app.py:328, ui/tools.py:391-536 (_finish :491, _make_box :517, commit :522; the anchor is the drag's first point or the plane origin); sent as Ops.box (commands.py:419) on the XY plane where RoboCAD extrudes a sketch rectangle",
        ..BASE
    },
    OpEntry {
        id: "tool.box_center",
        label: "Box (centre)",
        category: "Create",
        params: &[WIDTH, DEPTH, HEIGHT, p("center", "centre", POINT, "0, 0, 0")],
        flow: Flow::Place(Primitive::BoxCentre),
        route: "box",
        shape: Shape::Place(Primitive::BoxCentre),
        hint: HINT_CENTRE,
        source: "ui/app.py:329, ui/tools.py:522-529 (centred in the plane only, base on the plane; not Ops.box_center, which also centres the height); sent as Ops.box (commands.py:419)",
        ..BASE
    },
    OpEntry {
        id: "tool.cylinder",
        label: "Cylinder",
        category: "Create",
        keys: &["Shift+A, C"],
        params: &[DIAMETER, HEIGHT, p("base", "base centre", POINT, "0, 0, 0")],
        flow: Flow::Place(Primitive::Cylinder),
        route: "cylinder",
        shape: Shape::Place(Primitive::Cylinder),
        hint: HINT_CORNER,
        source: "ui/app.py:330, ui/tools.py:501-505 (_finish: axis ± the plane normal by the height's sign, radius at least 1e-3), commands.py:437",
        ..BASE
    },
    OpEntry {
        id: "tool.sphere",
        label: "Sphere",
        category: "Create",
        keys: &["Shift+A, S"],
        params: &[DIAMETER, p("center", "centre", POINT, "0, 0, 0")],
        flow: Flow::Place(Primitive::Sphere),
        route: "sphere",
        shape: Shape::Place(Primitive::Sphere),
        hint: HINT_CORNER,
        source: "ui/app.py:331, ui/tools.py:506-508 and :531-532, commands.py:440",
        ..BASE
    },
];

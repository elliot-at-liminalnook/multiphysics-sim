//! The catalogue's entries: Modify: fillets, chamfer, shell, thicken, draft, delete faces.
//! In RoboCAD's registry order (`ui/app.py` `_build_commands`); see `super`.
use super::super::kinds::*;
use super::super::*;
use crate::cad::sketch::BasePlane;
use crate::ui_kit::form::Unit;

pub(in crate::cad::ops) const ENTRIES: &[OpEntry] = &[
    // ---- Modify: fillets and chamfer (EdgeTool) --------------------------------
    OpEntry {
        id: "tool.fillet",
        label: "Fillet",
        keys: &["Ctrl+F"],
        needs: EDGES,
        params: &[p("radius", "radius", LENGTH, "1.0")],
        flow: Flow::PickThenForm(SelectMode::Edge),
        route: "fillet",
        args: &[Arg::Node, Arg::Edges, Arg::Param("radius")],
        fan: Fan::PerNode,
        refusal: NO_EDGES,
        hint: "fillet: select edges (click adds) then type the size • Enter applies",
        clears_selection: true,
        source: "ui/app.py:338, ui/tools.py:937-989 (fields :948, one call per node :977-985, selection cleared :986), commands.py:630",
        ..BASE
    },
    OpEntry {
        id: "tool.fillet_variable",
        label: "Variable fillet",
        needs: EDGES,
        params: &[p("radius", "start radius", LENGTH, "1.0"), p("radius_end", "end radius", LENGTH, "2.0")],
        flow: Flow::PickThenForm(SelectMode::Edge),
        route: "fillet",
        args: &[Arg::Node, Arg::Edges, Arg::Param("radius"), Arg::Param("radius_end")],
        fan: Fan::PerNode,
        refusal: NO_EDGES,
        hint: "variable: select edges (click adds) then type the size • Enter applies",
        clears_selection: true,
        source: "ui/app.py:339, ui/tools.py:948 and :980-981, commands.py:630",
        ..BASE
    },
    OpEntry {
        id: "tool.fillet_chordal",
        label: "Chordal fillet",
        needs: EDGES,
        params: &[p("chord", "chord", LENGTH, "1.0")],
        flow: Flow::PickThenForm(SelectMode::Edge),
        route: "fillet_chordal",
        args: &[Arg::Node, Arg::Edges, Arg::Param("chord")],
        fan: Fan::PerNode,
        refusal: NO_EDGES,
        hint: "chordal: select edges (click adds) then type the size • Enter applies",
        clears_selection: true,
        source: "ui/app.py:340, ui/tools.py:948 and :982-983, commands.py:633",
        ..BASE
    },
    OpEntry {
        id: "tool.fillet_all",
        label: "Fillet all edges…",
        needs: ANY_NODES,
        params: &[p("radius", "Radius (mm):", number(Unit::Length, 0.01, 100.0, 3), "1.0")],
        flow: Flow::Form,
        route: "fillet_all",
        args: &[Arg::Node, Arg::Param("radius")],
        fan: Fan::PerNode,
        refusal: "Select the bodies to fillet",
        source: "ui/app.py:341, ui/app.py:832-837 (getDouble \"Fillet all edges\", 1.0, 0.01..100, 3 decimals; silent with nothing selected: refusal ours), commands.py:636",
        ..BASE
    },
    OpEntry {
        id: "tool.full_round",
        label: "Full round (two edges)",
        needs: Needs::Edges { min: 2, max: Some(2), same_node: true },
        route: "full_round",
        args: &[Arg::Node, Arg::EdgeA, Arg::EdgeB],
        refusal: "Select two edges of the same body",
        source: "ui/app.py:342, ui/app.py:839-846, commands.py:639",
        ..BASE
    },
    OpEntry {
        id: "tool.remove_fillets",
        label: "Remove fillets (selected faces)",
        needs: FACES,
        route: "remove_fillets",
        args: &[Arg::Node, Arg::Faces],
        fan: Fan::PerNode,
        refusal: "Select the fillet faces to remove",
        source: "ui/app.py:343, ui/app.py:848-855 (faces grouped by node; silent when none: refusal ours), commands.py:642",
        ..BASE
    },
    OpEntry {
        id: "tool.chamfer",
        label: "Chamfer",
        keys: &["Ctrl+Shift+F"],
        needs: EDGES,
        params: &[p("distance", "distance", LENGTH, "1.0"), p("angle", "angle", ANGLE, "45.0")],
        flow: Flow::PickThenForm(SelectMode::Edge),
        route: "chamfer",
        shape: Shape::Chamfer,
        args: &[Arg::Node, Arg::Edges],
        fan: Fan::PerNode,
        refusal: NO_EDGES,
        hint: "chamfer: select edges (click adds) then type the size • Enter applies",
        clears_selection: true,
        source: "ui/app.py:344, ui/tools.py:948 and :984-985 (ChamferSpec, angle only when not 45°), commands.py:645",
        ..BASE
    },
    OpEntry {
        id: "tool.shell",
        label: "Hollow / shell",
        keys: &["Ctrl+Shift+H"],
        needs: Needs::NodesWithFaces,
        params: &[p("wall", "wall", LENGTH, "2.0")],
        flow: Flow::PickThenForm(SelectMode::Face),
        route: "shell",
        args: &[Arg::Node, Arg::Param("wall"), Arg::Faces],
        fan: Fan::PerNode,
        refusal: "Select a body or its faces to open",
        hint: "Select the faces to open (click adds), then type the wall thickness",
        clears_selection: true,
        source: "ui/app.py:345, ui/tools.py:992-1017, commands.py:624",
        ..BASE
    },
    OpEntry {
        id: "tool.thicken",
        label: "Thicken sheet…",
        needs: nodes(1, None, &["sheet"]),
        params: &[p("thickness", "Thickness (mm):", number(Unit::Length, 0.01, 100.0, 3), "2.0")],
        flow: Flow::Form,
        route: "thicken",
        args: &[Arg::Node, Arg::Param("thickness")],
        fan: Fan::PerNode,
        refusal: "Select a sheet",
        source: "ui/app.py:346, ui/app.py:857-864, commands.py:627",
        ..BASE
    },
    OpEntry {
        id: "tool.draft",
        label: "Draft faces…",
        needs: FACES,
        params: &[p("angle", "Angle (degrees):", number(Unit::Angle, -45.0, 45.0, 2), "2.0"), p("neutral", "neutral plane", PLANES, "active")],
        flow: Flow::Form,
        route: "draft",
        args: &[Arg::Node, Arg::Faces, Arg::Const("[0, 0, 1]"), Arg::Param("angle"), Arg::Plane("neutral", BasePlane::Xy)],
        fan: Fan::PerNode,
        refusal: "Select faces to draft",
        source: "ui/app.py:347, ui/app.py:866-878 (pull (0, 0, 1); neutral the active plane or XY: the plane is a parameter here), commands.py:553",
        ..BASE
    },
    OpEntry {
        id: "tool.delete_face",
        label: "Delete faces (heal)",
        needs: FACES,
        route: "delete_faces",
        args: &[Arg::Node, Arg::Faces],
        fan: Fan::PerNode,
        refusal: "Select the faces to delete",
        clears_selection: true,
        source: "ui/app.py:348, ui/app.py:880-887 (silent when none: refusal ours), commands.py:556",
        ..BASE
    },
];

//! The catalogue's entries: REST-only `Ops` methods (no RoboCAD command).
//! In RoboCAD's registry order (`ui/app.py` `_build_commands`); see `super`.
use super::super::kinds::*;
use super::super::*;

pub(in crate::cad::ops) const ENTRIES: &[OpEntry] = &[
    // ---- REST-only Ops methods (no RoboCAD command) -------------------------------
    OpEntry {
        id: "ops.box",
        label: "Box (corner and size)",
        category: "Create",
        params: &[p("corner", "corner", POINT, "0, 0, 0"), p("size", "size", POINT, "20, 20, 10")],
        flow: Flow::Form,
        route: "box",
        args: &[Arg::Param("corner"), Arg::Param("size")],
        source: "commands.py:419 (defaults: PrimitiveTool's anchor and box fields, ui/tools.py:411-413 and :524)",
        ..BASE
    },
    OpEntry {
        id: "ops.box_center",
        label: "Box (centre and size)",
        category: "Create",
        params: &[p("center", "centre", POINT, "0, 0, 0"), p("size", "size", POINT, "20, 20, 10")],
        flow: Flow::Form,
        route: "box_center",
        args: &[Arg::Param("center"), Arg::Param("size")],
        source: "commands.py:422 (centred in all three axes; defaults as ops.box)",
        ..BASE
    },
    OpEntry {
        id: "ops.box_three_point",
        label: "Box (three points and height)",
        category: "Create",
        params: &[p("a", "first base corner", POINT, ""), p("b", "second corner (along x)", POINT, ""), p("c", "point setting the width", POINT, ""), p("height", "height", LENGTH, "")],
        flow: Flow::Form,
        route: "box_three_point",
        args: &[Arg::Param("a"), Arg::Param("b"), Arg::Param("c"), Arg::Param("height")],
        source: "commands.py:426 (no defaults: each is required)",
        ..BASE
    },
    OpEntry {
        id: "ops.bridge",
        label: "Bridge two curves",
        category: "Create",
        needs: nodes(2, Some(2), &["curve", "sketch"]),
        route: "bridge",
        args: &[Arg::Target, Arg::Second],
        refusal: "Select two curves or sketches to bridge",
        source: "commands.py:511 (refusal ours)",
        ..BASE
    },
    OpEntry {
        id: "ops.move_faces",
        label: "Move faces",
        needs: FACES,
        params: &[p("translation", "translation", POINT, "")],
        flow: Flow::Form,
        route: "move_faces",
        args: &[Arg::Node, Arg::Faces, Arg::Param("translation")],
        fan: Fan::PerNode,
        refusal: "Select the faces to move",
        source: "commands.py:525 (refusal ours)",
        ..BASE
    },
    OpEntry {
        id: "ops.rotate_faces",
        label: "Rotate faces",
        needs: FACES,
        params: &[p("axis_point", "axis point", POINT, ""), p("axis_dir", "axis direction", POINT, ""), p("angle_deg", "angle", ANGLE, "")],
        flow: Flow::Form,
        route: "rotate_faces",
        args: &[Arg::Node, Arg::Faces, Arg::Param("axis_point"), Arg::Param("axis_dir"), Arg::Param("angle_deg")],
        fan: Fan::PerNode,
        refusal: "Select the faces to rotate",
        source: "commands.py:528 (refusal ours)",
        ..BASE
    },
    OpEntry {
        id: "ops.set_radius",
        label: "Set cylinder radius",
        needs: FACES,
        params: &[p("radius", "radius", LENGTH, "")],
        flow: Flow::Form,
        route: "set_radius",
        args: &[Arg::Node, Arg::Face, Arg::Param("radius")],
        refusal: "Select a cylindrical face",
        source: "commands.py:531 (the first selected face; refusal ours)",
        ..BASE
    },
    OpEntry {
        id: "ops.untrim",
        label: "Untrim faces",
        needs: FACES,
        route: "untrim",
        args: &[Arg::Node, Arg::Faces],
        fan: Fan::PerNode,
        refusal: "Select the faces to untrim",
        clears_selection: true,
        source: "commands.py:559 (delete_faces; the selection cleared as tool.delete_face's; refusal ours)",
        ..BASE
    },
    OpEntry {
        id: "ops.array_curve",
        label: "Array along curve",
        needs: Needs::NodesThenPath,
        params: &[p("count", "count", count_from(2.0), ""), p("align", "align to the path", CHECK, "true"), p("as_instances", "As live instances", CHECK, "false"), p("merge", "Merge into one body", CHECK, "false")],
        flow: Flow::Form,
        route: "array_curve",
        args: &[Arg::AllButLast, Arg::Last, Arg::Param("count")],
        kwargs: &[("align", Arg::Param("align")), ("as_instances", Arg::Param("as_instances")), ("merge", Arg::Param("merge"))],
        refusal: "Select the bodies, then the path curve or sketch",
        source: "commands.py:743 (align True, as_instances False, merge False; count has no default and divides by count - 1, so at least 2; refusal ours)",
        ..BASE
    },
    OpEntry {
        id: "ops.extract_components",
        label: "Extract components",
        needs: nodes(1, Some(1), &[]),
        params: &[p("components", "components {name: [solid indices]}", JSON, "")],
        flow: Flow::Form,
        route: "extract_components",
        args: &[Arg::Node, Arg::Param("components")],
        kwargs: &[("expected_revision", Arg::Revision)],
        refusal: "Select the one body to extract components from",
        source: "commands.py:826 (indices from GET /nodes/{id}/solids at expected_revision: a REST run must pass the revision it read them at; a form sends the revision it opened at; refusal ours)",
        ..BASE
    },
    OpEntry {
        id: "ops.set_control_points",
        label: "Set control points",
        category: "Advanced",
        needs: FACES,
        params: &[p("points", "control points [[[x, y, z], …], …]", JSON, "")],
        flow: Flow::Form,
        route: "set_control_points",
        args: &[Arg::Node, Arg::Face, Arg::Param("points")],
        refusal: "Select a face",
        source: "commands.py:864 (the first selected face; the grid tool.control_points shows); the route needs api.py's ArgConverter to pass nested `points` ([[[x, y, z], …], …]) through unconverted (that fix is in cad/robocad/api.py)",
        ..BASE
    },
];

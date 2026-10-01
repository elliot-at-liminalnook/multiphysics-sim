//! The catalogue's entries: Modify: mirror, instance, array, cut, split, imprint, project, silhouette; Advanced; dependent offset; set pivot.
//! In RoboCAD's registry order (`ui/app.py` `_build_commands`); see `super`.
use super::super::kinds::*;
use super::super::*;
use crate::ui_kit::form::Unit;

pub(in crate::cad::ops) const ENTRIES: &[OpEntry] = &[
    // ---- Modify: mirror, instance, array ---------------------------------------
    OpEntry {
        id: "tool.mirror",
        label: "Mirror (about active plane)",
        keys: &["Ctrl+M"],
        needs: ANY_NODES,
        params: &[p("plane", "mirror plane", PLANES, "yz")],
        route: "mirror",
        args: &[Arg::Nodes, Arg::Param("plane")],
        kwargs: &[("live", Arg::Const("false"))],
        refusal: "Select bodies to mirror",
        source: "ui/app.py:360, ui/app.py:910-914 (the active plane or YZ), commands.py:694",
        ..BASE
    },
    OpEntry {
        id: "tool.mirror_live",
        label: "Mirror as live instance",
        needs: ANY_NODES,
        params: &[p("plane", "mirror plane", PLANES, "yz")],
        route: "mirror",
        args: &[Arg::Nodes, Arg::Param("plane")],
        kwargs: &[("live", Arg::Const("true"))],
        refusal: "Select bodies to mirror",
        source: "ui/app.py:361, ui/app.py:910-914, commands.py:694",
        ..BASE
    },
    OpEntry {
        id: "tool.instance",
        label: "Instance selected",
        needs: ANY_NODES,
        route: "instance",
        args: &[Arg::Node, Arg::Const("{\"translation\": [20.0, 0.0, 0.0]}")],
        fan: Fan::PerNode,
        refusal: "Select the bodies to instance",
        source: "ui/app.py:364, ui/app.py:916-918 (Transform((20, 0, 0)) per node; silent when none: refusal ours), commands.py:712",
        ..BASE
    },
    OpEntry {
        id: "tool.array",
        label: "Array…",
        keys: &["Ctrl+Shift+A"],
        needs: ANY_NODES,
        params: &[
            p("kind", "Kind", FieldKind::Choice { options: &["rectangular", "radial"] }, "rectangular"),
            when(p("count_x", "Count X", number(Unit::Count, 1.0, 500.0, 0), "3"), "kind", "rectangular"),
            when(p("count_y", "Count Y", number(Unit::Count, 1.0, 500.0, 0), "1"), "kind", "rectangular"),
            when(p("count_z", "Count Z", number(Unit::Count, 1.0, 500.0, 0), "1"), "kind", "rectangular"),
            when(p("mode", "Mode", FieldKind::Choice { options: &["count + spacing", "count + total extent"] }, "count + spacing"), "kind", "rectangular"),
            when(p("spacing", "Spacing or extent X / Y / Z", POINT, "10, 10, 10"), "kind", "rectangular"),
            when(p("count", "Radial count", number(Unit::Count, 2.0, 360.0, 0), "6"), "kind", "radial"),
            when(p("angle", "Radial total angle", ANGLE, "360"), "kind", "radial"),
            when(p("plane", "axis plane (its normal through its origin)", PLANES, "xy"), "kind", "radial"),
            p("as_instances", "As live instances", CHECK, "false"),
            p("merge", "Merge into one body", CHECK, "false"),
        ],
        flow: Flow::Form,
        route: "array_rect",
        shape: Shape::Array,
        refusal: "Select bodies to array",
        source: "ui/app.py:365, ui/app.py:920-941 (radial about the active plane or XY), ui/widgets.py:1024-1053 (ArrayDialog), commands.py:728 and :738",
        ..BASE
    },
    // ---- Modify: cut, split, imprint, project, silhouette ----------------------
    OpEntry {
        id: "tool.cut_plane",
        label: "Cut with active plane",
        needs: ANY_NODES,
        params: &[p("plane", "cutting plane", PLANES, "xy")],
        route: "cut",
        args: &[Arg::Node, Arg::Param("plane")],
        fan: Fan::PerNode,
        refusal: "Select the bodies to cut",
        source: "ui/app.py:366, ui/app.py:943-945 (the active plane or XY; silent when none: refusal ours), commands.py:591",
        ..BASE
    },
    OpEntry {
        id: "tool.cut_sheet",
        label: "Cut with selected sheet/curve",
        needs: nodes(2, None, &[]),
        route: "cut",
        args: &[Arg::Target, Arg::Second],
        refusal: "Select the body, then the cutter",
        source: "ui/app.py:367, ui/app.py:947-951, commands.py:591",
        ..BASE
    },
    OpEntry {
        id: "tool.split_face",
        label: "Split faces with active plane",
        needs: ANY_NODES,
        params: &[p("plane", "splitting plane", PLANES, "xy")],
        route: "split_face",
        args: &[Arg::Node, Arg::Param("plane")],
        fan: Fan::PerNode,
        refusal: "Select the bodies whose faces to split",
        source: "ui/app.py:368, ui/app.py:953-955 (the active plane or XY; refusal ours), commands.py:566",
        ..BASE
    },
    OpEntry {
        id: "tool.imprint",
        label: "Imprint selected curve/body",
        needs: nodes(2, None, &[]),
        route: "imprint",
        args: &[Arg::Target, Arg::Second],
        refusal: "Select the body, then the tool",
        source: "ui/app.py:369, ui/app.py:957-961, commands.py:562",
        ..BASE
    },
    OpEntry {
        id: "tool.project_curve",
        label: "Project curve onto body",
        needs: nodes(2, None, &[]),
        params: &[p("direction", "direction (the view's when empty)", POINT, "")],
        route: "project_curve",
        args: &[Arg::Target, Arg::Second, Arg::ViewDir],
        refusal: "Select the curve/sketch, then the body",
        source: "ui/app.py:370, ui/app.py:963-968 (along the camera's view direction, -basis()[2]), commands.py:856",
        ..BASE
    },
    OpEntry {
        id: "tool.silhouette",
        label: "Silhouette onto active plane",
        needs: ANY_NODES,
        params: &[p("plane", "projection plane", PLANES, "xy")],
        route: "silhouette",
        args: &[Arg::Node, Arg::Param("plane")],
        fan: Fan::PerNode,
        refusal: "Select the bodies to project",
        source: "ui/app.py:371, ui/app.py:970-972 (the active plane or XY; refusal ours), commands.py:860",
        ..BASE
    },
    // ---- Advanced ---------------------------------------------------------------
    OpEntry {
        id: "tool.control_points",
        label: "Show/edit control points (advanced)",
        category: "Advanced",
        needs: FACES,
        route: "control_points",
        shape: Shape::ControlPoints,
        refusal: "Select a face",
        source: "ui/app.py:372, ui/app.py:974-984 (the first selected face's kernel.control_points, drawn as points and rows)",
        ..BASE
    },
    OpEntry {
        id: "tool.raise_degree",
        label: "Raise face degree",
        category: "Advanced",
        needs: FACES,
        route: "raise_degree",
        args: &[Arg::Node, Arg::Face, Arg::Const("4"), Arg::Const("4")],
        refusal: "Select a face to raise its degree",
        source: "ui/app.py:373, ui/app.py:986-991 (the first selected face, du = dv = 4; silent without a face: refusal ours), commands.py:867",
        ..BASE
    },
    OpEntry {
        id: "tool.rebuild_face",
        label: "Rebuild face…",
        category: "Advanced",
        needs: FACES,
        params: &[p("spans", "Spans per direction:", number(Unit::Count, 1.0, 64.0, 0), "4")],
        flow: Flow::Form,
        route: "rebuild_face",
        args: &[Arg::Node, Arg::Face, Arg::Param("spans"), Arg::Param("spans")],
        refusal: "Select a face to rebuild",
        source: "ui/app.py:374, ui/app.py:993-1001 (getInt \"Rebuild face\", 4, 1..64; silent without a face: refusal ours), commands.py:870",
        ..BASE
    },
    OpEntry {
        id: "tool.dependent_offset",
        label: "Dependent offset (face to body)…",
        needs: Needs::FaceThenNode,
        params: &[p("clearance", "Clearance (mm):", number(Unit::Length, -10.0, 10.0, 2), "0.2")],
        flow: Flow::Form,
        route: "offset_face_to",
        args: &[Arg::Node, Arg::Face, Arg::OtherNode, Arg::Param("clearance")],
        refusal: "Select a face, then the body to offset it to",
        source: "ui/app.py:375, ui/app.py:1003-1013, commands.py:521",
        ..BASE
    },
    OpEntry {
        id: "tool.set_pivot",
        label: "Set pivot at cursor snap",
        category: "Tools",
        needs: ANY_NODES,
        params: &[p("point", "point (the cursor snap when empty)", POINT, "")],
        flow: Flow::AtCursorSnap,
        route: "set_pivot",
        args: &[Arg::Target, Arg::CursorSnap],
        refusal: "Select the node whose pivot to set",
        source: "ui/app.py:376, ui/app.py:1015-1020 (the first selected node, the snap under the cursor; silent when none: refusal ours), commands.py:351",
        ..BASE
    },
];

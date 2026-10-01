//! The catalogue's entries: Modify: booleans, region, join, unjoin, dissolve, make unique; Inspect (read-only overlays).
//! In RoboCAD's registry order (`ui/app.py` `_build_commands`); see `super`.
use super::super::kinds::*;
use super::super::*;

pub(in crate::cad::ops) const ENTRIES: &[OpEntry] = &[
    // ---- Modify: booleans, region, join ----------------------------------------
    OpEntry {
        id: "modify.union",
        label: "Union",
        keys: &["Ctrl+U"],
        needs: Needs::TargetThenTools,
        route: "boolean",
        args: &[Arg::Target, Arg::Tools, Arg::Const("\"union\"")],
        refusal: "Select the target body first, then the tools",
        clears_selection: true,
        source: "ui/app.py:382, ui/app.py:787-793, commands.py:573",
        ..BASE
    },
    OpEntry {
        id: "modify.subtract",
        label: "Subtract",
        keys: &["Ctrl+Shift+U"],
        needs: Needs::TargetThenTools,
        route: "boolean",
        args: &[Arg::Target, Arg::Tools, Arg::Const("\"subtract\"")],
        refusal: "Select the target body first, then the tools",
        clears_selection: true,
        source: "ui/app.py:383, ui/app.py:787-793, commands.py:573",
        ..BASE
    },
    OpEntry {
        id: "modify.intersect",
        label: "Intersect",
        keys: &["Ctrl+Alt+U"],
        needs: Needs::TargetThenTools,
        route: "boolean",
        args: &[Arg::Target, Arg::Tools, Arg::Const("\"intersect\"")],
        refusal: "Select the target body first, then the tools",
        clears_selection: true,
        source: "ui/app.py:384, ui/app.py:787-793, commands.py:573",
        ..BASE
    },
    OpEntry {
        id: "modify.region",
        label: "Region (overlap as new body)",
        needs: nodes(2, Some(2), &[]),
        route: "region",
        args: &[Arg::Target, Arg::Second],
        refusal: "Select exactly two bodies",
        source: "ui/app.py:385, ui/app.py:795-799, commands.py:588",
        ..BASE
    },
    OpEntry {
        id: "modify.join",
        label: "Join",
        keys: &["J"],
        needs: nodes(2, None, &[]),
        route: "join",
        args: &[Arg::Nodes],
        refusal: "Select two or more bodies to join",
        source: "ui/app.py:386 (Ops.join of the selected nodes, unchecked: refusal ours), commands.py:806",
        ..BASE
    },
    OpEntry {
        id: "modify.unjoin",
        label: "Unjoin",
        keys: &["Shift+J"],
        needs: ANY_NODES,
        route: "unjoin",
        args: &[Arg::Node],
        fan: Fan::PerNode,
        refusal: "Select the bodies to unjoin",
        source: "ui/app.py:387 (one call per node; refusal ours), commands.py:814",
        ..BASE
    },
    OpEntry {
        id: "modify.dissolve",
        label: "Dissolve redundant topology",
        needs: ANY_NODES,
        route: "dissolve",
        args: &[Arg::Node],
        fan: Fan::PerNode,
        refusal: "Select the bodies to dissolve",
        source: "ui/app.py:388 (one call per node; refusal ours), commands.py:823",
        ..BASE
    },
    OpEntry {
        id: "modify.make_unique",
        label: "Make instance unique",
        needs: nodes(1, None, &["instance"]),
        route: "make_unique",
        args: &[Arg::Node],
        fan: Fan::PerNode,
        refusal: "Select an instance to make unique",
        source: "ui/app.py:389 (instances only, others skipped silently: refusal ours), commands.py:719",
        ..BASE
    },
    // ---- Inspect (read-only overlays) -------------------------------------------
    OpEntry {
        id: "inspect.curvature",
        label: "Curvature comb on selected curve",
        category: "Inspect",
        needs: nodes(1, None, &["curve", "sketch"]),
        route: "curvature_comb",
        shape: Shape::CurvatureComb,
        refusal: "Select a curve or sketch",
        source: "ui/app.py:401, ui/app.py:1277-1284 (each selected curve or sketch; the last one's comb stays drawn; silent otherwise: refusal ours)",
        ..BASE
    },
    OpEntry {
        id: "inspect.continuity",
        label: "Continuity check (G0/G1/G2)",
        category: "Inspect",
        needs: ANY_NODES,
        route: "continuity",
        shape: Shape::Continuity,
        refusal: "Select a body",
        source: "ui/app.py:402, ui/app.py:1286-1300 (each selected body; the last one's report stays drawn; refusal ours)",
        ..BASE
    },
];

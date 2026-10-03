//! The catalogue's entries: Create: extrude, revolve, sweep, pipe, loft, fill (cad-sketch).
//! In RoboCAD's registry order (`ui/app.py` `_build_commands`); see `super`.
use super::super::kinds::*;
use super::super::*;
use crate::ui_kit::form::Unit;

/// RoboCAD's `ExtrudeTool.hint` (ui/tools.py:824), for extrude and revolve.
const HINT_EXTRUDE: &str = "Select a sketch (or closed curves) • drag the height • Tab for exact • Shift: subtract, Ctrl: union, Alt: intersect with the body under the cursor";

/// RoboCAD's `BooleanOp` values (kernel/base.py:258-262), picked by the
/// modifier held at the release (`ExtrudeTool._boolean_for`), else by the
/// last press's modifiers for a Tab commit; "new" when no body is under
/// the selection (`App.body_under_selection`).
const BOOLEAN: Param = p("boolean", "boolean (Shift subtract, Ctrl union, Alt intersect at release)", FieldKind::Choice { options: &["new", "union", "subtract", "intersect"] }, "new");

/// Sketches and curves: RoboCAD's sweep, pipe, loft and fill handlers keep
/// the selected nodes of these kinds (ui/app.py:802, :812, :821, :827).
const PROFILES: &[&str] = &["sketch", "curve"];

pub(in crate::cad::ops) const ENTRIES: &[OpEntry] = &[
    OpEntry {
        id: "tool.extrude",
        label: "Extrude",
        category: "Create",
        keys: &["X"],
        params: &[p("distance", "distance", LENGTH, "10.0"), p("taper", "taper", ANGLE, "0.0"), BOOLEAN],
        flow: Flow::Extrude { revolve: false },
        route: "extrude",
        shape: Shape::Extrude { revolve: false },
        hint: HINT_EXTRUDE,
        source: "ui/app.py:332, keymap.json tool.extrude, ui/tools.py:822-931 (ExtrudeTool: source :837-842, fields :843-846, press :855-857, drag :859-875, release :877-880, _boolean_for :882-890, _apply :910-924, commit :926-931), ui/app.py:647-652 (body_under_selection), commands.py:480-487",
        ..BASE
    },
    OpEntry {
        id: "tool.revolve",
        label: "Revolve",
        category: "Create",
        keys: &["Shift+R"],
        params: &[p("angle", "angle", ANGLE, "360.0"), BOOLEAN],
        flow: Flow::Extrude { revolve: true },
        route: "revolve",
        shape: Shape::Extrude { revolve: true },
        hint: HINT_EXTRUDE,
        source: "ui/app.py:333, keymap.json tool.revolve, ui/tools.py:822-931 (ExtrudeTool(revolve=True): field :843-844, _plane :849-853, press :855-857, release :877-880 (angle None: 360), _apply :916-918 angle or 360, commit :926-929), commands.py:489-491",
        ..BASE
    },
    OpEntry {
        id: "tool.sweep",
        label: "Sweep (profile + path from selection)",
        category: "Create",
        needs: nodes(2, None, PROFILES),
        params: &[p("twist", "Twist (degrees):", number(Unit::Angle, -3600.0, 3600.0, 1), "0.0")],
        flow: Flow::Form,
        route: "sweep",
        // `sweep(profile, path, options)`: ArgConverter turns the third
        // positional argument into `SweepOptions(**v)` by its name `options` (api.py:261-262).
        args: &[Arg::Target, Arg::Second, Arg::Keyed("twist_deg", "twist")],
        refusal: "Select the profile sketch, then the path sketch",
        source: "ui/app.py:334, ui/app.py:801-809 (getDouble \"Sweep\", \"Twist (degrees):\" 0.0, -3600..3600, 1 decimal; sweep(ids[0], ids[1], SweepOptions(twist_deg=twist))), commands.py:493-496, api.py:261-262",
        ..BASE
    },
    OpEntry {
        id: "tool.pipe",
        label: "Pipe along selected curve…",
        category: "Create",
        needs: nodes(1, None, PROFILES),
        params: &[p("diameter", "Diameter (mm):", number(Unit::Length, 0.01, 1000.0, 3), "4.0")],
        flow: Flow::Form,
        route: "pipe",
        args: &[Arg::Node, Arg::Param("diameter")],
        fan: Fan::PerNode,
        refusal: "Select a curve or sketch",
        source: "ui/app.py:335, ui/app.py:811-818 (getDouble \"Pipe\", \"Diameter (mm):\" 4.0, 0.01..1000, 3 decimals; one pipe per id), commands.py:498-500",
        ..BASE
    },
    OpEntry {
        id: "tool.loft",
        label: "Loft selected sketches",
        category: "Create",
        needs: nodes(2, None, PROFILES),
        route: "loft",
        args: &[Arg::Nodes],
        refusal: "Select two or more sketches to loft",
        source: "ui/app.py:336, ui/app.py:820-824 (loft(ids)), commands.py:502-505",
        ..BASE
    },
    OpEntry {
        id: "tool.fill",
        label: "Fill / patch selected curve",
        category: "Create",
        needs: nodes(1, None, PROFILES),
        route: "fill",
        args: &[Arg::Node],
        refusal: "Select a closed curve",
        source: "ui/app.py:337, ui/app.py:826-830 (fill(ids[0])), commands.py:507-509",
        ..BASE
    },
];

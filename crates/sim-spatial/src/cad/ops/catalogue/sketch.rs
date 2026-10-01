//! The catalogue's entries: Sketch: the 13 sketch tools, offset, fillet corners, join (cad-sketch).
//! In RoboCAD's registry order (`ui/app.py` `_build_commands`); see `super`.
//!
//! The tools (ui/app.py:377-378, ui/tools.py:638-817): labels "Sketch:
//! <Label>", category Sketch, keys from keymap.json; each is `Flow::Sketch`
//! driven by its `sketch::specs` row, its Tab fields RoboCAD's `_fields`
//! (names and defaults; the polygon's `sides` has no default: its form
//! opens with the remembered count, `sketch::interact::begin`, and a run
//! without it uses that count, `sketch::specs::from_values`) plus one optional `anchor` (RoboCAD's
//! `commit` anchors at the first clicked point, else the plane origin; the
//! form's submit fills it from the first click). The text tool's first
//! field is RoboCAD's "Text to sketch:" dialog (app.py:743-750), focused
//! when the tool starts.
//!
//! Recorded decisions:
//! - keymap.json binds `sketch.arc` (A), a command RoboCAD never registers
//!   (its arc tool is `sketch.arc_3pt`, with no key); the native viewer
//!   binds A to the three-point arc, the only arc tool there is.
//! - The polygon's sides are a whole count of at least 3 (RoboCAD's field
//!   takes any number and `int()`s it; 0 falls back to the remembered
//!   count and 1 or 2 draw a point or a segment).
//! - The spiral's turns are a plain number (`Unit::Plain`, fractional, no
//!   unit), as RoboCAD's `NumericField("turns", 3.0, unit="")`.
//! - Offset and fillet open their form with no selection check
//!   (`Needs::Nothing`): RoboCAD's `_selected_sketch` falls back to any
//!   visible sketch, which no selection need expresses, so the sketch is
//!   checked on OK (`sketch::edits::calls`) where RoboCAD checks before
//!   its dialog. Join's "Select a sketch" is ours (RoboCAD is silent).
use super::super::kinds::*;
use super::super::*;
use crate::ui_kit::form::Unit;

/// RoboCAD's `SketchTool.hint` (tools.py:648) for `shape`.
macro_rules! hint {
    ($shape:literal) => {
        concat!($shape, ": click points on the plane • Enter/double-click to finish • Tab for exact values • Esc cancels")
    };
}

/// RoboCAD's `commit` anchor: the first clicked point, else the plane origin.
const ANCHOR: Param = p("anchor", "anchor (the first click; the plane origin when empty)", POINT, "");
/// A plain number (RoboCAD's `NumericField(…, unit="")`), fractional.
const PLAIN: FieldKind = FieldKind::Number { unit: Unit::Plain, min: None, max: None, decimals: 6 };
/// RoboCAD's `_selected_sketch` finds none (ui/app.py:761, :769).
const NO_SKETCH: &str = "Select a sketch";

const fn tool(id: &'static str, label: &'static str, keys: &'static [&'static str], shape: SketchShape, params: &'static [Param], hint: &'static str, source: &'static str) -> OpEntry {
    OpEntry { id, label, category: "Sketch", keys, params, flow: Flow::Sketch(shape), route: "sketch", shape: Shape::Sketch(shape), hint, source, ..BASE }
}

const fn edit(id: &'static str, label: &'static str, flow: Flow, params: &'static [Param], edit: SketchEdit, source: &'static str) -> OpEntry {
    OpEntry { id, label, category: "Sketch", params, flow, route: "sketch", shape: Shape::SketchEdit(edit), refusal: NO_SKETCH, source, ..BASE }
}

pub(in crate::cad::ops) const ENTRIES: &[OpEntry] = &[
    tool(
        "sketch.line",
        "Sketch: Line",
        &["L"],
        SketchShape::Line,
        &[p("length", "length", LENGTH, "20.0"), p("angle", "angle", ANGLE, "0.0"), ANCHOR],
        hint!("line"),
        "ui/app.py:377-378, ui/tools.py:638-817 (_fields :664, needed 2 :695, _build :731, lines chain :768, commit :790-792), keymap.json L",
    ),
    tool(
        "sketch.rectangle",
        "Sketch: Rectangle",
        &["Shift+L"],
        SketchShape::Rectangle,
        &[WIDTH, HEIGHT, ANCHOR],
        hint!("rectangle"),
        "ui/app.py:377-378, ui/tools.py:638-817 (_fields :665, _build :733, commit :793-794), keymap.json Shift+L",
    ),
    tool(
        "sketch.rectangle_center",
        "Sketch: Rectangle (centre)",
        &[],
        SketchShape::RectangleCenter,
        &[WIDTH, HEIGHT, ANCHOR],
        hint!("rectangle_center"),
        "ui/app.py:377-378, ui/tools.py:638-817 (_fields :666, _build :735, commit :795-796); no key in keymap.json",
    ),
    tool(
        "sketch.circle",
        "Sketch: Circle",
        &["C"],
        SketchShape::Circle,
        &[DIAMETER, ANCHOR],
        hint!("circle"),
        "ui/app.py:377-378, ui/tools.py:638-817 (_fields :667, _build :737, commit :797-798 radius = diameter / 2), keymap.json C",
    ),
    tool(
        "sketch.circle_2pt",
        "Sketch: Circle (two points)",
        &[],
        SketchShape::Circle2pt,
        &[ANCHOR],
        hint!("circle_2pt"),
        "ui/app.py:377-378, ui/tools.py:638-817 (no Tab fields; _build :739 circle_two_point); no key",
    ),
    tool(
        "sketch.circle_3pt",
        "Sketch: Circle (three points)",
        &[],
        SketchShape::Circle3pt,
        &[ANCHOR],
        hint!("circle_3pt"),
        "ui/app.py:377-378, ui/tools.py:638-817 (no Tab fields; _build :741 circle_three_point); no key",
    ),
    tool(
        "sketch.arc_3pt",
        "Sketch: Arc (three points)",
        &["A"],
        SketchShape::Arc3pt,
        &[ANCHOR],
        hint!("arc_3pt"),
        "ui/app.py:377-378, ui/tools.py:638-817 (no Tab fields; _build :743 arc_three_point); keymap.json's dead sketch.arc A bound here (module doc)",
    ),
    tool(
        "sketch.polygon",
        "Sketch: Polygon",
        &["Shift+P"],
        SketchShape::Polygon,
        &[p("radius", "radius", LENGTH, "10.0"), p("sides", "sides", count_from(3.0), ""), ANCHOR],
        hint!("polygon"),
        "ui/app.py:377-378, ui/tools.py:638-817 (_fields :668 sides = Sketch.last_polygon_sides, _build :745 rotation toward the second click, commit :799-800 int(sides)), kernel/sketch.py:259-265, keymap.json Shift+P",
    ),
    tool(
        "sketch.slot",
        "Sketch: Slot",
        &["Shift+S"],
        SketchShape::Slot,
        &[p("length", "length", LENGTH, "20.0"), p("width", "width", LENGTH, "4.0"), ANCHOR],
        hint!("slot"),
        "ui/app.py:377-378, ui/tools.py:638-817 (_fields :669, needed 3, _build :747-749 width from the third click, commit :801-802 along +u), keymap.json Shift+S",
    ),
    tool(
        "sketch.spline",
        "Sketch: Spline",
        &["Shift+C"],
        SketchShape::Spline,
        &[ANCHOR],
        hint!("spline"),
        "ui/app.py:377-378, ui/tools.py:638-817 (no Tab fields; _build :754, Enter or double-click with two or more points :772-782), keymap.json Shift+C",
    ),
    tool(
        "sketch.ellipse",
        "Sketch: Ellipse",
        &[],
        SketchShape::Ellipse,
        &[p("radius_x", "radius x", LENGTH, "10.0"), p("radius_y", "radius y", LENGTH, "5.0"), ANCHOR],
        hint!("ellipse"),
        "ui/app.py:377-378, ui/tools.py:638-817 (_fields :670, needed 3, _build :750-753, commit :803-804); no key",
    ),
    tool(
        "sketch.spiral",
        "Sketch: Spiral",
        &[],
        SketchShape::Spiral,
        &[p("start_radius", "start radius", LENGTH, "2.0"), p("end_radius", "end radius", LENGTH, "10.0"), p("turns", "turns", PLAIN, "3.0"), ANCHOR],
        hint!("spiral"),
        "ui/app.py:377-378, ui/tools.py:638-817 (_fields :671, _build :757-758 start radius 0.15 r, 3 turns, commit :805-806), kernel/sketch.py:278-286; no key",
    ),
    tool(
        "sketch.text",
        "Sketch: Text",
        &["T"],
        SketchShape::Text,
        &[p("text", "Text to sketch:", TEXT, ""), p("height", "height", LENGTH, "10.0"), ANCHOR],
        hint!("text"),
        "ui/app.py:377-378, :743-750 (getText \"Text to sketch:\" before the tool), ui/tools.py:638-817 (_fields :672, a click places at 10 mm :759-760 since text_height is never set, commit :807-808), keymap.json T",
    ),
    edit(
        "sketch.offset",
        "Sketch: offset selected curve…",
        Flow::Form,
        &[p("distance", "Distance (mm):", number(Unit::Length, -1000.0, 1000.0, 3), "1.0")],
        SketchEdit::Offset,
        "ui/app.py:379, :758-764 (getDouble \"Offset\", every curve offset, one edit \"Offset curves\"), kernel/sketch.py:422-428",
    ),
    edit(
        "sketch.fillet",
        "Sketch: fillet corner…",
        Flow::Form,
        &[p("radius", "Radius (mm):", number(Unit::Length, 0.01, 1000.0, 3), "2.0")],
        SketchEdit::FilletCorners,
        "ui/app.py:380, :766-780 (getDouble \"Corner fillet\", every corner of every closed polyline from the last vertex down, KernelError skipped per corner, one edit \"Fillet corners\"), kernel/sketch.py:389-420",
    ),
    edit(
        "sketch.join",
        "Sketch: join curves",
        Flow::Immediate,
        &[],
        SketchEdit::Join,
        "ui/app.py:381, :782-785 (all curves joined when more than one, one edit \"Join curves\"; silent without a sketch: refusal ours), kernel/sketch.py:430-450",
    ),
];

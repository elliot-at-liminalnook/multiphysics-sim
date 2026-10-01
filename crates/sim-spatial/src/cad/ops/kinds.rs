//! The catalogue's building blocks: RoboCAD's field kinds, the shared
//! parameters, needs and hints, and `BASE`, the entry every catalogue row
//! starts from (split from `catalogue` to keep it under 700 lines).
use super::*;
use crate::ui_kit::form::Unit;

/// A tool's `NumericField(name, value)` (ui/tools.py:25-29): a length, no range.
pub(super) const LENGTH: FieldKind = FieldKind::Number { unit: Unit::Length, min: None, max: None, decimals: 6 };
/// A tool's `NumericField(name, value, angle=True)`: degrees, no range.
pub(super) const ANGLE: FieldKind = FieldKind::Number { unit: Unit::Angle, min: None, max: None, decimals: 6 };
/// A point or vector `[x, y, z]` in mm.
pub(super) const POINT: FieldKind = FieldKind::Vector { unit: Unit::Length };
/// A plane `ArgConverter.plane` names (api.py:180-185): RoboCAD's handlers
/// use the active plane, else the plane named here; the native viewer has
/// no active plane until cad-sketch.
pub(super) const PLANES: FieldKind = FieldKind::Choice { options: &["xy", "xz", "yz"] };
pub(super) const CHECK: FieldKind = FieldKind::Check;
pub(super) const JSON: FieldKind = FieldKind::Json;

/// A dialog's number: `QInputDialog.getDouble(…, value, min, max, decimals)` or `getInt`.
pub(super) const fn number(unit: Unit, min: f64, max: f64, decimals: u8) -> FieldKind {
    FieldKind::Number { unit, min: Some(min), max: Some(max), decimals }
}
/// A whole count with only a lower bound.
pub(super) const fn count_from(min: f64) -> FieldKind {
    FieldKind::Number { unit: Unit::Count, min: Some(min), max: None, decimals: 0 }
}
pub(super) const fn p(name: &'static str, label: &'static str, kind: FieldKind, default: &'static str) -> Param {
    Param { name, label, kind, default, when: None }
}
/// Shown and sent only while parameter `on` reads `is`.
pub(super) const fn when(param: Param, on: &'static str, is: &'static str) -> Param {
    Param { when: Some((on, is)), ..param }
}
pub(super) const fn nodes(min: usize, max: Option<usize>, kinds: &'static [&'static str]) -> Needs {
    Needs::Nodes { min, max, kinds }
}
pub(super) const ANY_NODES: Needs = nodes(1, None, &[]);
pub(super) const EDGES: Needs = Needs::Edges { min: 1, max: None, same_node: false };
pub(super) const FACES: Needs = Needs::Faces { min: 1 };

/// What every entry starts from; each entry sets what it uses.
pub(super) const BASE: OpEntry = OpEntry {
    id: "",
    label: "",
    category: "Modify",
    keys: &[],
    needs: Needs::Nothing,
    params: &[],
    flow: Flow::Immediate,
    route: "",
    shape: Shape::Plain,
    args: &[],
    kwargs: &[],
    fan: Fan::Once,
    refusal: "",
    hint: "",
    clears_selection: false,
    source: "",
};

/// RoboCAD's `PrimitiveTool` hint (ui/tools.py:400).
pub(super) const HINT_CORNER: &str = "Click-drag the base on the plane, then drag the height • Tab for exact sizes • corner mode";
pub(super) const HINT_CENTRE: &str = "Click-drag the base on the plane, then drag the height • Tab for exact sizes • centre mode";
/// RoboCAD's `PrimitiveTool._fields` (ui/tools.py:411-416): box 20 × 20 × 10, cylinder Ø10 × 10, sphere Ø10.
pub(super) const WIDTH: Param = p("width", "width", LENGTH, "20.0");
pub(super) const DEPTH: Param = p("depth", "depth", LENGTH, "20.0");
pub(super) const HEIGHT: Param = p("height", "height", LENGTH, "10.0");
pub(super) const DIAMETER: Param = p("diameter", "diameter", LENGTH, "10.0");
/// RoboCAD's `EdgeTool.commit` message (ui/tools.py:975).
pub(super) const NO_EDGES: &str = "Select one or more edges first";

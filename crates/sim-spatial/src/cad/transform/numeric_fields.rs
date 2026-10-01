//! The numeric bar's fields (RoboCAD's `NumericField`s per tool): how each
//! is read ([`FieldKind`]), what it commits ([`FieldCommit`]), the active
//! tool's list ([`fields`]) and the axis a typed rotation turns about
//! ([`numeric_axis`]). The bar itself is `super::super::numeric`.
use super::{AXES, ToolState, fa, fl, g};
use crate::cad::actions::Dimension;
use crate::cad::document::{CadDocument, CadTool};
use crate::cad::mesh::CadMeshes;
use crate::cad::topology::CadTopology;
use bevy::prelude::Vec3;

/// How a numeric field is read: a length (bare numbers mm), an angle
/// (degrees) or a factor (no unit), as RoboCAD's `NumericField`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Length,
    Angle,
    Factor,
}
impl FieldKind {
    /// RoboCAD's `NumericBar.values`: `evaluate(text, angle, None if angle
    /// or not unit else "mm")`. The error names the token and its position.
    pub fn evaluate(self, text: &str) -> Result<f64, String> {
        let r = match self {
            FieldKind::Length => sim_runtime::units::evaluate(text, false, Some("mm")),
            FieldKind::Angle => sim_runtime::units::evaluate(text, true, None),
            FieldKind::Factor => sim_runtime::units::evaluate(text, false, None),
        };
        r.map_err(|e| e.to_string())
    }
    /// A value as the field shows it (RoboCAD's `set_fields`).
    pub fn show(self, v: f64) -> String {
        match self {
            FieldKind::Length => fl(v),
            FieldKind::Angle => fa(v),
            FieldKind::Factor => g(v),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            FieldKind::Length => "length",
            FieldKind::Angle => "angle",
            FieldKind::Factor => "factor",
        }
    }
}

/// What a numeric field commits.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldCommit {
    Dx,
    Dy,
    Dz,
    Angle,
    Factor,
    Distance,
    /// A live dimension: `CadSetDimension`.
    Dimension { node: String, dimension: Dimension, faces: Vec<i64> },
    /// Shown, not editable: RoboCAD's message.
    ReadOnly(String),
}

/// One numeric field: its name, how it is read, the value it opens with.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: String,
    pub kind: FieldKind,
    pub value: f64,
    pub commit: FieldCommit,
}
impl Field {
    pub fn new(name: impl Into<String>, kind: FieldKind, value: f64, commit: FieldCommit) -> Self {
        Field { name: name.into(), kind, value, commit }
    }
    /// The text the field opens with.
    pub fn text(&self) -> String {
        self.kind.show(self.value)
    }
}

/// The active tool's numeric fields (RoboCAD's `ctx.numeric(fields)`):
/// move dx dy dz, rotate angle, scale factor, push/pull or offset distance;
/// in the Select tool a double-clicked face's dimension, else the selected
/// faces' and edges' live dimensions; none for measure. None while a
/// catalogue parameter form is open (`CadDocument::ops.form`): the form
/// holds the keyboard and Tab then (RoboCAD's `ctx.numeric` is replaced by
/// the operation's fields while its tool or dialog is up).
pub fn fields(doc: &CadDocument, topology: Option<&CadTopology>, meshes: Option<&CadMeshes>) -> Vec<Field> {
    if doc.ops.form.is_some() {
        return Vec::new();
    }
    match doc.tool {
        CadTool::Move => vec![Field::new("dx", FieldKind::Length, 0.0, FieldCommit::Dx), Field::new("dy", FieldKind::Length, 0.0, FieldCommit::Dy), Field::new("dz", FieldKind::Length, 0.0, FieldCommit::Dz)],
        CadTool::Rotate => vec![Field::new("angle", FieldKind::Angle, 0.0, FieldCommit::Angle)],
        CadTool::Scale => vec![Field::new("factor", FieldKind::Factor, 1.0, FieldCommit::Factor)],
        CadTool::PushPull | CadTool::OffsetFace => vec![Field::new("distance", FieldKind::Length, 0.0, FieldCommit::Distance)],
        CadTool::Measure => Vec::new(),
        CadTool::Select => match &doc.tool_state.dimension {
            Some(entry) => vec![entry.field.clone()],
            None => super::dimensions::live(doc, topology, meshes),
        },
    }
}

/// The axis a typed rotation turns about: the last dragged one, else Z
/// (RoboCAD: `axes[self.axis_index or 2]`, but handle 0 counts; see the module doc).
pub(in crate::cad) fn numeric_axis(state: &ToolState) -> Vec3 {
    match state.axis {
        Some(3) => state.free_axis.unwrap_or(Vec3::Z),
        Some(i) if i < 3 => AXES[i],
        _ => Vec3::Z,
    }
}

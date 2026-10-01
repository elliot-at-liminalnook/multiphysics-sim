//! The current image's placement form (references.py:45-63, 152-164,
//! 186-193): RoboCAD's `QFormLayout` of spin boxes as kit text fields, the
//! plane choice as chips and the lock as a chip.
//!
//! - **Rows** ([`ROWS`]): Width (mm, 0.001–1e7), Origin X/Y/Z (mm, ±1e7),
//!   Rotation (°, ±360), Opacity (%, 0–100), RoboCAD's `spin(lo, hi,
//!   suffix, decimals)`: two decimals, opacity none. Values are read with
//!   RoboCAD's unit expressions (`ui_kit::form::evaluate`) and rounded to the
//!   spin box's decimals, as a `QDoubleSpinBox` holds them.
//! - **Load** ([`PlacementForm::load`], RoboCAD's `load`): from the image's
//!   placement and the node's lock, the plane choice back to "Keep current
//!   plane"; again whenever the placement is read at a newer revision
//!   (RoboCAD's `refresh` reloads on every document change).
//! - **Apply** ([`PlacementForm::apply`]): the action RoboCAD's `commit`
//!   sends: every value (width, opacity / 100, origin, rotation, lock) and
//!   the plane choice, with the revision the form was loaded at (`began`):
//!   an Apply after RoboCAD moved on is refused by name.
use super::{PlaneChoice, ReferencesArgs, ReferencesOp};
use crate::cad::document::CadDocument;
use crate::cad::sketch::{BasePlane, CadActivePlane};
use crate::ui_kit::form::{FieldKind, FieldValue, Unit, evaluate};
use serde_json::{Value, json};
use sim_runtime::cad_client::{ImagePlacement, ReferenceUpdate};

/// One row: label, unit suffix, kind (with RoboCAD's range) and decimals.
pub(crate) struct Row {
    pub label: &'static str,
    pub suffix: &'static str,
    pub kind: FieldKind,
    pub decimals: i32,
}

const fn length(lo: f64, hi: f64) -> FieldKind {
    FieldKind::Number { unit: Unit::Length, min: Some(lo), max: Some(hi), decimals: 2 }
}

/// RoboCAD's rows, in its order (references.py:49-57).
pub(crate) const ROWS: [Row; 6] = [
    Row { label: "Width", suffix: "mm", kind: length(0.001, 1e7), decimals: 2 },
    Row { label: "Origin X", suffix: "mm", kind: length(-1e7, 1e7), decimals: 2 },
    Row { label: "Origin Y", suffix: "mm", kind: length(-1e7, 1e7), decimals: 2 },
    Row { label: "Origin Z", suffix: "mm", kind: length(-1e7, 1e7), decimals: 2 },
    Row { label: "Rotation", suffix: "°", kind: FieldKind::Number { unit: Unit::Angle, min: Some(-360.0), max: Some(360.0), decimals: 2 }, decimals: 2 },
    Row { label: "Opacity", suffix: "%", kind: FieldKind::Number { unit: Unit::Plain, min: Some(0.0), max: Some(100.0), decimals: 0 }, decimals: 0 },
];

/// `x` as a spin box with `decimals` holds it.
pub(crate) fn round_to(x: f64, decimals: i32) -> f64 {
    let s = 10f64.powi(decimals);
    let r = (x * s).round() / s;
    if r == 0.0 { 0.0 } else { r }
}

/// `x` as the spin box shows it.
fn text(x: f64, decimals: i32) -> String {
    format!("{:.*}", decimals.max(0) as usize, round_to(x, decimals))
}

/// The open form of image `id`.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacementForm {
    pub id: String,
    /// RoboCAD's revision the placement was read at (Apply's `began`).
    pub began: u64,
    /// The rows' texts, as typed.
    pub texts: [String; 6],
    pub plane: PlaneChoice,
    pub locked: bool,
    /// Why the last Apply sent nothing, or RoboCAD's refusal.
    pub error: Option<String>,
}

/// The six values in RoboCAD's units (rounded as the spin boxes hold them).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Values {
    pub width: f64,
    pub origin: [f64; 3],
    pub rotation_deg: f64,
    pub opacity_pct: f64,
}

/// Row `i` of `text` as RoboCAD's spin box takes it, or why not.
fn row_value(i: usize, text: &str) -> Result<f64, String> {
    let row = &ROWS[i];
    match evaluate(&row.kind, text) {
        Ok(FieldValue::Number(v)) => Ok(round_to(v, row.decimals)),
        Ok(_) => Err(format!("{}: not a number", row.label)),
        Err(e) => Err(format!("{}: {e}", row.label)),
    }
}

impl PlacementForm {
    /// RoboCAD's `load`: the placement's values and the node's lock; plane "Keep current plane".
    pub(crate) fn load(id: &str, began: u64, p: &ImagePlacement, locked: bool) -> Self {
        let o = p.plane.origin;
        let texts = [text(p.width, 2), text(o[0], 2), text(o[1], 2), text(o[2], 2), text(p.rotation_deg, 2), text(p.opacity * 100.0, 0)];
        PlacementForm { id: id.to_string(), began, texts, plane: PlaneChoice::Keep, locked, error: None }
    }

    /// The rows' values, or the first row that does not read.
    pub(crate) fn values(&self) -> Result<Values, String> {
        let mut v = [0.0; 6];
        for (i, slot) in v.iter_mut().enumerate() {
            *slot = row_value(i, &self.texts[i])?;
        }
        Ok(Values { width: v[0], origin: [v[1], v[2], v[3]], rotation_deg: v[4], opacity_pct: v[5] })
    }

    /// Apply placement's action (RoboCAD's `commit`), or why it cannot be sent.
    pub(crate) fn apply(&self) -> Result<ReferencesArgs, String> {
        let v = self.values()?;
        Ok(ReferencesArgs {
            plane: Some(self.plane),
            width: Some(v.width),
            origin: Some(v.origin),
            rotation_deg: Some(v.rotation_deg),
            opacity_pct: Some(v.opacity_pct),
            locked: Some(self.locked),
            revision: Some(self.began),
            ..ReferencesArgs::on(ReferencesOp::Placement, &self.id)
        })
    }

    pub(crate) fn json(&self) -> Value {
        let rows: Vec<Value> = ROWS.iter().zip(&self.texts).map(|(r, t)| json!({"label": r.label, "unit": r.suffix, "text": t})).collect();
        json!({"id": self.id, "began": self.began, "rows": rows, "plane": self.plane, "locked": self.locked, "error": self.error, "ready": self.values().err().map_or(Value::Bool(true), Value::String)})
    }
}

/// The plane value `update_reference` gets for `choice` (references.py:189):
/// none to keep it, "xz", "yz", "xy", or the active plane ("xy", "xz",
/// "yz" or its node id), else XY.
pub(crate) fn plane_value(choice: PlaneChoice, active: &CadActivePlane) -> Option<Value> {
    match choice {
        PlaneChoice::Keep => None,
        PlaneChoice::Front => Some(Value::from(BasePlane::Xz.arg())),
        PlaneChoice::Side => Some(Value::from(BasePlane::Yz.arg())),
        PlaneChoice::Top => Some(Value::from(BasePlane::Xy.arg())),
        PlaneChoice::Active => Some(active.arg_or(BasePlane::Xy)),
    }
}

/// Checked against RoboCAD's spin box range (`row`), rounded as it holds it.
fn ranged(row: usize, v: f64) -> Result<f64, String> {
    let r = &ROWS[row];
    let FieldKind::Number { min: Some(lo), max: Some(hi), .. } = r.kind else { return Ok(v) };
    if !v.is_finite() || v < lo || v > hi {
        return Err(format!("{}: {v} is outside {lo}…{hi} {}", r.label, r.suffix));
    }
    Ok(round_to(v, r.decimals))
}

/// The one `update_reference` of Apply placement: every value RoboCAD's
/// `commit` sends (`opacity_pct` / 100), each checked against its spin box's
/// range, the plane from the choice.
pub(crate) fn update(v: Values, choice: PlaneChoice, locked: bool, active: &CadActivePlane) -> Result<ReferenceUpdate, String> {
    let origin = [ranged(1, v.origin[0])?, ranged(2, v.origin[1])?, ranged(3, v.origin[2])?];
    Ok(ReferenceUpdate {
        width: Some(ranged(0, v.width)?),
        opacity: Some(ranged(5, v.opacity_pct)? / 100.0),
        origin: Some(origin),
        plane: plane_value(choice, active),
        rotation_deg: Some(ranged(4, v.rotation_deg)?),
        locked: Some(locked),
        visible: None,
        name: None,
    })
}

/// The values a REST `placement` leaves out: the image's current ones.
pub(crate) fn current_values(p: &ImagePlacement) -> Values {
    Values { width: round_to(p.width, 2), origin: p.plane.origin.map(|x| round_to(x, 2)), rotation_deg: round_to(p.rotation_deg, 2), opacity_pct: round_to(p.opacity * 100.0, 0) }
}

/// `op: form`: the plane choice or the lock of the open form (display state).
pub(crate) fn set(doc: &mut CadDocument, plane: Option<PlaneChoice>, locked: Option<bool>) -> Result<Value, String> {
    if plane.is_none() && locked.is_none() {
        return Err("form takes plane (keep | front | side | top | active) or locked".into());
    }
    let f = doc.references.form.as_mut().ok_or("no placement form is shown: select a reference image (its placement is read first)")?;
    if let Some(p) = plane {
        f.plane = p;
    }
    if let Some(l) = locked {
        f.locked = l;
    }
    f.error = None;
    let shown = f.json();
    doc.touch();
    Ok(json!({"form": shown}))
}

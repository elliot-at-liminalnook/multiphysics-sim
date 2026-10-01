//! `cad_inspector` (cad-physical-inspect): the inspector's physical rows as
//! one action, following RoboCAD's properties panel (ui/widgets.py:420-739):
//!
//! - **Colour**: RoboCAD opens a colour dialog; here an "r, g, b" field (0–1
//!   each) and "Use material colour", each one `PATCH /nodes/{id}
//!   {"color": [r, g, b] | null}` (api.py:592-593, one undo step).
//! - **Joint physics overrides** (widgets.py:510-544, 635-679): each Enter
//!   is one `set_joint_physics(joint, {key: value})` in RoboCAD's SI units,
//!   the payload RoboCAD's panel builds ([`joint_override`]): clearance and
//!   the flex patch radius mm → m, wobble ° → rad, drive backlash ° → rad
//!   with its provenance ("estimated" with RoboCAD's reference; empty
//!   declares it "unmeasured"), friction mN·m → N·m, radial stiffness N/m.
//! - **Exact measurements** (widgets.py:566-633): [`super::exact`].
//!
//! Every edit goes through `actions::edit_at` with the revision its row was
//! read at, so a value typed against an older document is refused by name.
//! The typed rows are not `system_ui` controls (as the name field): their
//! text is typed, so REST sends `cad_inspector` with the value.
use super::exact::{self, ExactState};
use crate::app::actions::{Call, Spec, spec};
use crate::app::{ViewerMode, ViewerSet};
use crate::cad::actions::{CAD, CadAction, Cx, edit_at};
use crate::cad::document::{CadDocument, EditDone};
use crate::cad::materials::{MaterialsArgs, MaterialsOp};
use crate::cad::selection::CadItems;
use crate::cad::sync::value;
use crate::ui_kit::form::TextDraft;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::SelectionItem;

/// A joint physics override row (RoboCAD's labels, widgets.py:513-541).
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JointField {
    Clearance,
    Wobble,
    DriveBacklash,
    Coulomb,
    Viscous,
    RadialStiffness,
    FlexPatchRadius,
}

impl JointField {
    pub const ALL: [JointField; 7] = [JointField::Clearance, JointField::Wobble, JointField::DriveBacklash, JointField::Coulomb, JointField::Viscous, JointField::RadialStiffness, JointField::FlexPatchRadius];
    /// RoboCAD's row label (the drive backlash's carries its provenance, added by the row).
    pub fn label(self) -> &'static str {
        match self {
            JointField::Clearance => "Radial clearance (mm)",
            JointField::Wobble => "Wobble (°)",
            JointField::DriveBacklash => "Drive backlash (°)",
            JointField::Coulomb => "Coulomb friction (mN·m)",
            JointField::Viscous => "Viscous (mN·m·s)",
            JointField::RadialStiffness => "Radial stiffness (N/m)",
            JointField::FlexPatchRadius => "Flex patch radius (mm)",
        }
    }
}

/// Which physical row is typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowField {
    /// The node's colour, "r, g, b".
    Color,
    Joint(JointField),
}

/// A physical row being typed: the node, the row, the draft, why its last
/// Enter sent nothing, and RoboCAD's revision shown when it opened.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RowDraft {
    pub node: String,
    pub field: RowField,
    pub draft: TextDraft,
    pub error: Option<String>,
    pub began: u64,
}

/// The inspector's physical rows' state on the document (reset with it):
/// the row being typed and the exact measurement.
#[derive(Default)]
pub struct PhysicalEdit {
    pub(crate) draft: Option<RowDraft>,
    pub(crate) exact: ExactState,
}

/// What `cad_inspector` does.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum InspectorOp {
    /// RoboCAD's "Calculate exact measurements" over the selected nodes.
    #[default]
    Exact,
    /// Stop the exact measurement in flight.
    ExactCancel,
    /// `PATCH /nodes/{id} {"color": [r, g, b]}` (0–1 each).
    Color,
    /// `PATCH /nodes/{id} {"color": null}`: back to the material's colour.
    MaterialColor,
    /// `set_joint_physics(id, {…})` from one row's typed `value`.
    JointPhysics,
}

/// `cad_inspector`'s arguments.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct InspectorArgs {
    #[serde(default)]
    pub op: InspectorOp,
    /// The node (color, material_color) or joint (joint_physics); the
    /// first selected node when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The colour, RGB 0–1 (color).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[f64; 3]>,
    /// The joint physics row (joint_physics).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<JointField>,
    /// The row's text as typed, read as RoboCAD's field reads it (joint_physics).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// RoboCAD's revision the value was read at; refused by name when it changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
}

impl InspectorArgs {
    pub(crate) fn of(op: InspectorOp, id: Option<&str>) -> CadAction {
        CadAction::CadInspector(InspectorArgs { op, id: id.map(str::to_string), ..InspectorArgs::default() })
    }
}

/// Python's `format(v, ".{precision}g")`: `precision` significant digits,
/// trailing zeros dropped, exponent form ("1.5e+08") below 1e-4 or from
/// 10^precision.
pub(crate) fn py_g(v: f64, precision: usize) -> String {
    if v.is_nan() {
        return "nan".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let p = precision.max(1);
    let strip = |s: &str| if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s.to_string() };
    let sci = format!("{:.*e}", p - 1, v);
    let (mantissa, exp) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    if exp < -4 || exp >= p as i32 {
        format!("{}e{}{:02}", strip(mantissa), if exp < 0 { '-' } else { '+' }, exp.abs())
    } else {
        let decimals = (p as i32 - 1 - exp).max(0) as usize;
        strip(&format!("{v:.decimals$}"))
    }
}

/// A colour as typed or sent: three numbers 0–1.
pub(crate) fn check_color(c: [f64; 3]) -> Result<[f64; 3], String> {
    if c.iter().all(|x| x.is_finite() && (0.0..=1.0).contains(x)) { Ok(c) } else { Err(format!("Colour: r, g and b are each 0–1 (got {}, {}, {})", c[0], c[1], c[2])) }
}

/// "r, g, b" as typed (plain numbers, 0–1 each).
pub(crate) fn parse_color(text: &str) -> Result<[f64; 3], String> {
    use crate::ui_kit::form::{FieldKind, FieldValue, Unit, evaluate};
    if text.trim().is_empty() {
        return Err("Colour: type r, g, b (0–1 each), or press Use material colour".into());
    }
    match evaluate(&FieldKind::Vector { unit: Unit::Plain }, text) {
        Ok(FieldValue::Vector(c)) => check_color(c),
        Ok(_) => Err("Colour: type r, g, b (0–1 each)".into()),
        Err(e) => Err(format!("Colour: {e}")),
    }
}

/// The `set_joint_physics` keyword arguments RoboCAD's panel sends for
/// `field` typed as `text` (widgets.py:513-541, 653-672), SI.
pub(crate) fn joint_override(field: JointField, text: &str) -> Result<Map<String, Value>, String> {
    use sim_runtime::units::evaluate;
    // RoboCAD's `evaluate(text)`: bare numbers as typed (its unit table scales a named length).
    let plain = |t: &str| evaluate(t, false, None).map_err(|e| format!("{}: {e}", field.label()));
    let empty = text.trim().is_empty();
    let v = match field {
        JointField::Clearance => json!({"clearance": plain(text)? / 1e3}),
        JointField::Wobble => json!({"wobble": plain(text)? / (180.0 / std::f64::consts::PI)}),
        JointField::DriveBacklash if empty => json!({"drive_backlash": {"width_rad": null, "provenance": "unmeasured", "reference": "Not measured"}}),
        JointField::DriveBacklash => json!({"drive_backlash": {"width_rad": plain(text)?.to_radians(), "provenance": "estimated", "reference": "Estimate authored in the CAD joint inspector"}}),
        JointField::Coulomb => json!({"friction": {"coulomb": plain(text)? * 1e-3}}),
        JointField::Viscous => json!({"friction": {"viscous": plain(text)? * 1e-3}}),
        JointField::RadialStiffness => json!({"stiffness": {"radial": plain(text)?}}),
        // Blank restores RoboCAD's inference.
        JointField::FlexPatchRadius if empty => json!({"flex_patch_radius": null}),
        JointField::FlexPatchRadius => json!({"flex_patch_radius": evaluate(text, false, Some("mm")).map_err(|e| format!("{}: {e}", field.label()))? * 1e-3}),
    };
    match v {
        Value::Object(map) => Ok(map),
        _ => Err("internal: not an object".into()),
    }
}

/// The node an op names (`id`, else the first selected), checked against the shown tree.
fn target(doc: &CadDocument, args: &InspectorArgs, selection: &[SelectionItem]) -> Result<String, String> {
    let id = args.id.clone().or_else(|| selection.first_node().map(str::to_string)).ok_or("Nothing selected: select a node (or pass id)")?;
    if !doc.has_node(&id) {
        return Err(format!("no node {id} in the shown tree"));
    }
    Ok(id)
}

/// `CadInspector`, from any entry point.
pub(in crate::cad) fn handle_physical(action: &CadAction, call: &mut Call, cx: &mut Cx) -> Outcome {
    let CadAction::CadInspector(args) = action else { return Outcome::Done(Err("not an inspector action".into())) };
    let selection = cx.shared.items();
    let doc = &mut *cx.doc;
    let began = Some(args.revision.unwrap_or_else(|| doc.shown_revision()));
    match args.op {
        InspectorOp::Exact => Outcome::Done(exact::start(doc, &selection)),
        InspectorOp::ExactCancel => Outcome::Done(exact::cancel(doc)),
        InspectorOp::Color | InspectorOp::MaterialColor => {
            let id = match target(doc, args, &selection) {
                Ok(id) => id,
                Err(e) => return Outcome::Done(Err(e)),
            };
            let color = match (args.op, args.color) {
                (InspectorOp::MaterialColor, _) => None,
                (_, Some(c)) => match check_color(c) {
                    Ok(c) => Some(c),
                    Err(e) => return Outcome::Done(Err(e)),
                },
                (_, None) => return Outcome::Done(Err("color needs color [r, g, b] (0–1 each); op material_color restores the material's colour".into())),
            };
            let name = doc.node_name(&id);
            let mut attrs = Map::new();
            attrs.insert("color".into(), json!(color));
            let message = match color {
                Some(c) => format!("Set {name}'s colour to {}, {}, {}", c[0], c[1], c[2]),
                None => format!("{name} uses its material's colour"),
            };
            edit_at(doc, call, began, format!("Colour of {name}"), move |c| c.patch(&id, &attrs).map(|d| EditDone { message, result: value(&d) }))
        }
        InspectorOp::JointPhysics => {
            let id = match target(doc, args, &selection) {
                Ok(id) => id,
                Err(e) => return Outcome::Done(Err(e)),
            };
            let name = doc.node_name(&id);
            if super::node(doc, &id).is_none_or(|n| n.kind != "joint") {
                return Outcome::Done(Err(format!("{name} is not a joint")));
            }
            let (Some(field), Some(text)) = (args.field, args.value.as_deref()) else {
                return Outcome::Done(Err("joint_physics needs field (clearance | wobble | drive_backlash | coulomb | viscous | radial_stiffness | flex_patch_radius) and value (the row's text)".into()));
            };
            let overrides = match joint_override(field, text) {
                Ok(o) => o,
                Err(e) => return Outcome::Done(Err(e)),
            };
            let message = format!("Set {name}'s {}", field.label());
            edit_at(doc, call, began, format!("Joint physics of {name}"), move |c| c.set_joint_physics(&id, &overrides).map(|r| EditDone { message, result: value(&r) }))
        }
    }
}

/// The inspector's physical buttons as `system_ui` lists them and the rows
/// draw them: (id, label, action, ready).
pub(in crate::cad) fn physical_controls(cx: &Cx) -> Vec<(String, String, CadAction, Result<(), String>)> {
    controls_of(cx.doc, &cx.shared.items())
}

/// The controls for `doc` and the selection.
pub(crate) fn controls_of(doc: &CadDocument, selection: &[SelectionItem]) -> Vec<(String, String, CadAction, Result<(), String>)> {
    let edit = doc.edit_refusal().map_or(Ok(()), Err);
    let mut out = vec![("cad:inspect:exact".to_string(), "Calculate exact measurements".to_string(), InspectorArgs::of(InspectorOp::Exact, None), exact::ready(doc, selection))];
    if doc.physical_edit.exact.running() {
        out.push(("cad:inspect:exact-cancel".into(), "Cancel exact measurements".into(), InspectorArgs::of(InspectorOp::ExactCancel, None), Ok(())));
    }
    let Some(n) = selection.first_node().and_then(|id| super::node(doc, id)) else { return out };
    let colour = if n.color.is_some() { edit.clone() } else { Err(format!("{} already uses its material's colour", n.name)) };
    out.push(("cad:inspect:material-colour".into(), "Use material colour".into(), InspectorArgs::of(InspectorOp::MaterialColor, Some(&n.id)), colour));
    if n.kind == "joint" {
        // Part D's catalogue form: RoboCAD's Edit joint dialog (set_joint, rename).
        out.push(("cad:inspect:edit-joint".into(), "Edit joint…".into(), CadAction::CadInvoke { id: "ops.set_joint".into() }, edit.clone()));
    }
    let known = n.material.as_deref().filter(|m| doc.doc.as_ref().is_some_and(|d| d.materials.iter().any(|v| v.get("id").and_then(Value::as_str) == Some(*m))));
    let props = known.map_or_else(|| Err(format!("{} has no material in RoboCAD's list", n.name)), |_| Ok(()));
    let action = CadAction::CadMaterials(MaterialsArgs { op: MaterialsOp::Properties, material: known.map(str::to_string), ..MaterialsArgs::default() });
    out.push(("cad:inspect:material-props".into(), "Material properties…".into(), action, props));
    out
}

/// `cad_state.inspector_physical`: the row being typed and the exact measurement.
pub(in crate::cad) fn physical_state_json(doc: &CadDocument) -> Value {
    let e = &doc.physical_edit;
    let draft = e.draft.as_ref().map(|d| {
        let field = match d.field {
            RowField::Color => "color".to_string(),
            RowField::Joint(f) => serde_json::to_value(f).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(),
        };
        json!({"node": d.node, "field": field, "text": d.draft.text, "error": d.error, "began": d.began})
    });
    json!({"typing": draft, "exact": exact::state_json(&e.exact)})
}

/// This part's REST command.
pub(in crate::cad) fn physical_specs() -> Vec<Spec> {
    vec![spec(
        "cad_inspector",
        CAD,
        json!({"op": "joint_physics", "id": "j1", "field": "clearance", "value": "0.2"}),
        "CAD mode: the inspector's physical rows, as RoboCAD's properties panel (cad_state.inspector_physical). op: exact (RoboCAD's \"Calculate exact measurements\": GET /nodes/{id} per selected node on a job, combined as RoboCAD combines them: size, volume, area, mass, centroid; 60 s limit; cancelled by any edit or selection change), exact_cancel, color (id?, color [r, g, b] 0–1: PATCH /nodes/{id} {\"color\"}), material_color (id?: colour null, the material's), joint_physics (id? a joint, field clearance | wobble | drive_backlash | coulomb | viscous | radial_stiffness | flex_patch_radius, value the row's text in its unit: mm, °, mN·m, mN·m·s, N/m; empty drive_backlash declares it unmeasured, empty flex_patch_radius restores RoboCAD's inference: one POST /ops/set_joint_physics in SI). id defaults to the first selected node; revision (optional) is RoboCAD's revision the value was read at. Each edit is one RoboCAD undo step; refused by name while one is in flight. system_ui lists cad:inspect:<id>.",
    )]
}

/// CadPlugin: the physical rows' typing (Input). After the name field (it
/// resets `CadInputFocus` each frame) and before the editors, so the
/// command surfaces, the picks and the CAD keys (all after the editors)
/// see the focus set here.
pub(in crate::cad) fn build_physical(app: &mut App) {
    app.add_systems(
        Update,
        super::entry::entry
            .after(crate::app::actions::serve)
            .after(crate::cad::panel::name_entry)
            .before(super::editor_entry)
            .before(crate::cad::numeric::entry)
            .before(crate::cad::keys::keys)
            .in_set(ViewerSet::Input)
            .run_if(in_state(ViewerMode::Cad)),
    );
}

/// CadCorePlugin's windowless part: the exact measurement's poll and its
/// cancel rule (`exact::sync`).
pub(crate) struct CoreParts;
impl Plugin for CoreParts {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, exact::sync.after(crate::cad::sync::receive).in_set(ViewerSet::JobResults).run_if(in_state(ViewerMode::Cad)));
    }
}

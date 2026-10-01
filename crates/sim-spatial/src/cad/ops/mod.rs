//! The op catalogue (cad-modify; native-viewer.md "CAD modify"): every
//! RoboCAD operation CAD mode offers, as data. One [`OpEntry`] per
//! operation names RoboCAD's command id, label, menu category and keys,
//! what must be selected ([`Needs`]), its typed parameters ([`Param`],
//! with RoboCAD's prompts, defaults and ranges), how it is started
//! ([`Flow`]), the `Ops` route it calls, and how its arguments are built
//! ([`Shape`] with [`Arg`] lists). Nothing in this module is code per
//! operation: [`args::build`] is keyed by shape, [`resolve::resolve`] by
//! selection need.
//!
//! The toolbar, the context menu, the menus by category, the palette, the
//! keys, REST (`cad_invoke`, `cad_run`, `cad_form_*`) and `system_ui`
//! (`cad:op:<id>`) all write the same [`CadAction`]s; [`handle`] applies
//! them inside the one CAD apply system (`actions::handle`). A run is one
//! edit through `actions::edit` (a Dedicated job): refused by name, with
//! nothing sent, while an edit is in flight, when not connected, when the
//! shown document is behind RoboCAD's or when RoboCAD's revision changed
//! since the form opened or the selection was made
//! (`CadDocument::commit_refusal`). A run sends one Ops call, or, where
//! RoboCAD's own handler loops over the selected nodes ([`Fan::PerNode`]),
//! one call per node in RoboCAD's order inside that one job, each its own
//! RoboCAD undo step as in RoboCAD.
//!
//! Face and edge indices come only from the selection, which the pickers
//! fill through `CadMeshes::face_at` and the topology at the shown
//! revision; [`resolve::resolve`] refuses a selection first seen at an
//! older revision.
mod args;
mod catalogue;
pub(super) mod interact;
mod resolve;
#[cfg(test)]
mod tests;

pub(crate) use catalogue::CATALOGUE;

use super::actions::{CadAction, Cx};
use super::document::SelectMode;
use crate::app::actions::Call;
use crate::ui_kit::form::{FieldKind, FieldValue};
use serde_json::{Map, Value};
use sim_api::Outcome;

/// What an operation needs selected (RoboCAD's handler reads
/// `selection.nodes()`, `.edges()` or `.faces()`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Needs {
    /// Nothing: the operation places new geometry from its parameters.
    Nothing,
    /// The selected nodes (RoboCAD's `selection.nodes()`, in selection
    /// order), at least `min`, at most `max`; only nodes whose kind is in
    /// `kinds` count when it is not empty (RoboCAD's filters: thicken takes
    /// sheets, make unique instances).
    Nodes { min: usize, max: Option<usize>, kinds: &'static [&'static str] },
    /// The first selected node is the target, the rest are tools (at least one).
    TargetThenTools,
    /// Selected edges, at least `min`, at most `max`; `same_node`: all of one body.
    Edges { min: usize, max: Option<usize>, same_node: bool },
    /// Selected faces, at least `min`.
    Faces { min: usize },
    /// Shell: the selected nodes, each with its selected faces (the faces to open).
    NodesWithFaces,
    /// Dependent offset: a selected face, then a selected node that owns
    /// none of the selected faces.
    FaceThenNode,
    /// Bodies, then a curve or sketch node (the last selected) as the path.
    NodesThenPath,
}

/// How an operation starts when invoked from a menu, the toolbar, the
/// palette or its key (RoboCAD's command handler).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flow {
    /// Runs at once on the selection with the parameter defaults.
    Immediate,
    /// A parameter form (RoboCAD's `QInputDialog` or `ArrayDialog`), then runs.
    Form,
    /// RoboCAD's `EdgeTool`/`ShellTool`: switches the selection mode,
    /// clicks toggle picks, the form stays open beside the view; Enter or
    /// OK runs on the picks; the tool stays active.
    PickThenForm(SelectMode),
    /// RoboCAD's `PrimitiveTool`: drag the base on the plane, then the
    /// height; Tab opens the form's exact sizes.
    Place(Primitive),
    /// RoboCAD's "Set pivot at cursor snap": the snap under the pointer.
    AtCursorSnap,
}

/// RoboCAD's `PrimitiveTool` kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Primitive {
    BoxCorner,
    BoxCentre,
    Cylinder,
    Sphere,
}

/// Whether RoboCAD's handler makes one call or one per selected node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fan {
    Once,
    /// One call per node (with that node's edges or faces), in selection order.
    PerNode,
}

/// One argument of the route, as the builder fills it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Arg {
    /// The node of this call (the per-node node, else the first selected).
    Node,
    /// Every selected node id, as a list.
    Nodes,
    /// The first selected node (the target).
    Target,
    /// The selected nodes after the first, as a list.
    Tools,
    /// The second selected node.
    Second,
    /// The last selected node (a path).
    Last,
    /// The selected nodes except the last, as a list.
    AllButLast,
    /// This call's node's selected edges as `[{node, edge}, …]`.
    Edges,
    /// The first and second selected edges as `{node, edge}`.
    EdgeA,
    EdgeB,
    /// This call's node's selected faces as `[{node, face}, …]`.
    Faces,
    /// The first selected face as `{node, face}`.
    Face,
    /// The node owning none of the selected faces (dependent offset's target).
    OtherNode,
    /// A parameter's value (by name).
    Param(&'static str),
    /// A fixed JSON value, written as JSON text ("[0, 0, 1]", "4", "\"union\"").
    Const(&'static str),
    /// The direction the camera looks along (RoboCAD's `-camera.basis()[2]`).
    ViewDir,
    /// The snapped point under the pointer.
    CursorSnap,
}

/// How the arguments are built (keyed by route shape, not by operation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    /// Positional `args`, keyword `kwargs`, as listed.
    Plain,
    /// Chamfer: `[node, edges, {distance, angle_deg}]`, the angle sent only
    /// when it is not 45° (tools.py:985-986).
    Chamfer,
    /// The Array dialog: `array_rect` (count, spacing or extent by the
    /// mode) or `array_radial` (about the plane's normal through its origin).
    Array,
    /// A primitive placed from the anchor (drag or plane origin) and the sizes.
    Place(Primitive),
    /// Not an Ops call: the copy read (`GET /clipboard`).
    Copy,
    /// Not an Ops call: `POST /paste` with the copied items (one undo step "Paste").
    Paste,
    /// Read-only analysis reads, drawn as display-only overlays.
    ControlPoints,
    CurvatureComb,
    Continuity,
}

/// One typed parameter: its argument name, RoboCAD's label or prompt,
/// its kind (unit and range as RoboCAD's tool or dialog sets them) and
/// its default as RoboCAD's field opens with it (typed text: "1",
/// "360", "rectangular", "true", "0, 0, 0").
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Param {
    pub name: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    pub default: &'static str,
    /// Shown and sent only when another parameter has this text (the Array
    /// dialog's rectangular or radial rows).
    pub when: Option<(&'static str, &'static str)>,
}

/// One operation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct OpEntry {
    /// RoboCAD's command id (`tool.fillet`, `modify.union`), or
    /// `ops.<name>` for an Ops method with no command (REST-only in RoboCAD).
    pub id: &'static str,
    /// RoboCAD's label.
    pub label: &'static str,
    /// RoboCAD's menu category.
    pub category: &'static str,
    /// RoboCAD's keys (keymap.json), as written there.
    pub keys: &'static [&'static str],
    pub needs: Needs,
    pub params: &'static [Param],
    pub flow: Flow,
    /// The Ops method (`POST /ops/{route}`); the Array shape picks between
    /// `array_rect` and `array_radial`; non-Ops shapes name their route.
    pub route: &'static str,
    pub shape: Shape,
    pub args: &'static [Arg],
    pub kwargs: &'static [(&'static str, Arg)],
    pub fan: Fan,
    /// RoboCAD's message when the selection does not fit (its `self.error(…)`),
    /// or a plain one where RoboCAD says nothing (recorded in the ledger).
    pub refusal: &'static str,
    /// RoboCAD's hint while the operation's interaction is active, or "".
    pub hint: &'static str,
    /// RoboCAD clears the selection after the run.
    pub clears_selection: bool,
    /// Where RoboCAD implements it (`ui/app.py:832-837`, `commands.py:636`).
    pub source: &'static str,
}

/// The catalogue entry with RoboCAD command id `id`.
pub(crate) fn entry(id: &str) -> Option<&'static OpEntry> {
    CATALOGUE.iter().find(|e| e.id == id)
}

/// A parameter's value from a REST value or a form draft: a string is
/// evaluated as the field reads it (`ui_kit::form::evaluate`, RoboCAD's
/// unit expressions); a number, bool or array is range-checked. The JSON
/// sent to RoboCAD: numbers, `[x, y, z]`, the choice's option text, a bool,
/// or the JSON value.
pub(crate) fn param_value(param: &Param, input: &Value) -> Result<Value, String> {
    let text = match input {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(a) if matches!(param.kind, FieldKind::Vector { .. }) => a.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "),
        other if matches!(param.kind, FieldKind::Json) => other.to_string(),
        other => return Err(format!("{}: expected a value as its field takes it, got {other}", param.name)),
    };
    let value = crate::ui_kit::form::evaluate(&param.kind, &text).map_err(|e| format!("{} ({}): {e}", param.label, param.name))?;
    Ok(match (value, param.kind) {
        (FieldValue::Number(v), FieldKind::Number { unit: crate::ui_kit::form::Unit::Count, .. }) => Value::from(v.round() as i64),
        (FieldValue::Number(v), _) => Value::from(v),
        (FieldValue::Vector(v), _) => Value::from(v.to_vec()),
        (FieldValue::Choice(i), FieldKind::Choice { options }) => Value::from(options[i]),
        (FieldValue::Choice(i), _) => Value::from(i),
        (FieldValue::Check(b), _) => Value::from(b),
        (FieldValue::Json(v), _) => v,
    })
}

/// Every parameter's value: `given` (REST or the form) over the defaults.
/// Unknown names are refused by name; a parameter whose `when` does not
/// hold is left out, as is one with an empty default that was not given
/// (an optional one, such as set pivot's `point`, which then comes from
/// the cursor snap).
pub(crate) fn values(entry: &OpEntry, given: &Map<String, Value>) -> Result<Map<String, Value>, String> {
    if let Some(unknown) = given.keys().find(|k| !entry.params.iter().any(|p| p.name == k.as_str())) {
        let names: Vec<&str> = entry.params.iter().map(|p| p.name).collect();
        return Err(format!("{} takes no parameter {unknown} (its parameters: {})", entry.id, if names.is_empty() { "none".to_string() } else { names.join(", ") }));
    }
    let text = |p: &Param| -> String {
        match given.get(p.name) {
            Some(Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
            None => p.default.to_string(),
        }
    };
    let mut out = Map::new();
    for p in entry.params {
        if let Some((other, want)) = p.when {
            let Some(q) = entry.params.iter().find(|q| q.name == other) else { continue };
            if text(q) != want {
                continue;
            }
        }
        if p.default.is_empty() && given.get(p.name).is_none_or(|v| v.as_str() == Some("")) {
            continue;
        }
        let input = given.get(p.name).cloned().unwrap_or_else(|| Value::String(p.default.to_string()));
        out.insert(p.name.to_string(), param_value(p, &input)?);
    }
    Ok(out)
}

// ---- State --------------------------------------------------------------------

/// The open parameter form (`CadDocument::ops.form`).
#[derive(Clone, Debug, PartialEq)]
pub struct FormState {
    /// The catalogue id it runs.
    pub op: &'static str,
    /// Each parameter's draft, in `OpEntry::params` order.
    pub texts: Vec<String>,
    /// The field with the keyboard.
    pub focus: Option<usize>,
    /// The focused text is selected (the next character replaces it).
    pub select_all: bool,
    /// The shown revision when the form opened: OK is refused by name if
    /// RoboCAD's document changed since (its picks and values were made
    /// against that geometry).
    pub began: u64,
    /// The last refusal or error of OK, shown in the form.
    pub error: Option<String>,
}

/// CAD mode's catalogue state on the document (reset with it).
#[derive(Default)]
pub struct OpsState {
    pub form: Option<FormState>,
    /// The `PickThenForm`, `Place` or `AtCursorSnap` op whose interaction is
    /// active (its form stays open while it is).
    pub active: Option<&'static str>,
    /// A primitive being placed (`interact`).
    pub place: Option<interact::Place>,
    /// The command surface open now (palette, menus, radials, context menu).
    pub surface: Option<super::surfaces::Open>,
    /// The snapped point under the pointer when it was last over the 3D
    /// view (mm; `interact` keeps it): "Set pivot at cursor snap".
    pub cursor_snap: Option<[f64; 3]>,
    /// The last copy: RoboCAD's clipboard JSON with the revision it was read at.
    pub clipboard: Option<(u64, Value)>,
    /// Read-only analysis results drawn as overlays (control points, comb, continuity).
    pub analysis: super::analysis_overlay::Analysis,
}

// ---- The handler ----------------------------------------------------------------

/// The catalogue arms of `CadAction` (`CadInvoke`, `CadRun`, `CadFormSet`,
/// `CadFormSubmit`, `CadFormCancel`), from `actions::handle`.
pub(super) fn handle(action: &CadAction, call: &mut Call, cx: &mut Cx) -> Outcome {
    let _ = (action, call, cx);
    todo!("ops::handle")
}

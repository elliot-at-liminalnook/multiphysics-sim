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
//! older revision, and REST items naming faces or edges are refused without
//! the `revision` they were read at. Where RoboCAD's handler clears the
//! selection, it is cleared once the edit succeeds (`sync::finish_edit`).
mod args;
mod catalogue;
mod kinds;
pub(super) mod interact;
mod resolve;
#[cfg(test)]
mod tests;

pub(crate) use catalogue::CATALOGUE;

use super::actions::{CadAction, Cx};
use super::document::{CadDocument, CadTool, EditDone, SelectMode};
use super::sync::value;
use super::topology::CadTopology;
use super::view::CadView;
use crate::app::actions::Call;
use crate::ui_kit::form::{FieldKind, FieldValue};
use args::Built;
use serde_json::{Map, Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::SelectionItem;
use std::sync::OnceLock;

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
    /// The direction the camera looks along (RoboCAD's `-camera.basis()[2]`),
    /// or the entry's `direction` parameter when given (REST without a view).
    ViewDir,
    /// The snapped point under the pointer, or the entry's `point`
    /// parameter when given.
    CursorSnap,
    /// The revision the caller's values were read at (`extract_components`'s
    /// `expected_revision`): a REST run's `revision` (required), a form's
    /// opening revision, else the shown one.
    Revision,
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
    /// Not an Ops call: the copy read (`POST /clipboard/copy`; read-only).
    Copy,
    /// Not an Ops call: `POST /clipboard/paste` with the copied items (one undo step "Paste").
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
        Value::Array(a) if matches!(param.kind, FieldKind::Vector { .. }) => a.iter().map(|x| x.as_str().map_or_else(|| x.to_string(), str::to_string)).collect::<Vec<_>>().join(", "),
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

/// Whether `p`'s `when` gate holds: the gating parameter's value, as its
/// field reads it (`param_value`: the canonical option, so "Radial" reads
/// as "radial"), is the gate's option. `input` gives a parameter's raw
/// value. An unreadable gating value is its own error.
fn gate(entry: &OpEntry, p: &Param, input: impl Fn(&Param) -> Value) -> Result<bool, String> {
    let Some((on, want)) = p.when else { return Ok(true) };
    let Some(q) = entry.params.iter().find(|q| q.name == on) else { return Ok(false) };
    Ok(param_value(q, &input(q))?.as_str() == Some(want))
}

/// Every parameter's value: `given` (REST or the form) over the defaults.
/// Unknown names are refused by name, as is a given parameter whose `when`
/// does not hold ("count applies only when kind is radial"); a parameter
/// whose `when` does not hold is otherwise left out, as is one with an
/// empty default that was not given (an optional one, such as set pivot's
/// `point`, which then comes from the cursor snap).
pub(crate) fn values(entry: &OpEntry, given: &Map<String, Value>) -> Result<Map<String, Value>, String> {
    if let Some(unknown) = given.keys().find(|k| !entry.params.iter().any(|p| p.name == k.as_str())) {
        let names: Vec<&str> = entry.params.iter().map(|p| p.name).collect();
        return Err(format!("{} takes no parameter {unknown} (its parameters: {})", entry.id, if names.is_empty() { "none".to_string() } else { names.join(", ") }));
    }
    let input = |q: &Param| given.get(q.name).cloned().unwrap_or_else(|| Value::String(q.default.to_string()));
    let mut out = Map::new();
    for p in entry.params {
        if !gate(entry, p, input)? {
            if let (Some((on, want)), true) = (p.when, given.contains_key(p.name)) {
                return Err(format!("{} applies only when {on} is {want}", p.name));
            }
            continue;
        }
        if p.default.is_empty() && given.get(p.name).is_none_or(|v| v.as_str() == Some("")) {
            continue;
        }
        out.insert(p.name.to_string(), param_value(p, &input(p))?);
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
    /// view (mm), with the shown revision it was snapped at (`interact`
    /// keeps it, and clears it when the pointer leaves the window, the snap
    /// finds nothing or the shown revision moves on; `resolve` uses it
    /// only at the shown revision): "Set pivot at cursor snap".
    pub cursor_snap: Option<(u64, [f64; 3])>,
    /// The last copy: RoboCAD's clipboard JSON with the revision it was read at.
    pub clipboard: Option<(u64, Value)>,
    /// Read-only analysis results drawn as overlays (control points, comb, continuity).
    pub analysis: super::analysis_overlay::Analysis,
}


// ---- The handler ----------------------------------------------------------------

/// The refusal for an id that is not in the catalogue.
fn unknown(id: &str) -> String {
    format!("unknown operation {id}; see cad_state.ops or GET /v1/capabilities")
}

/// The catalogue arms of `CadAction` (`CadInvoke`, `CadRun`, `CadFormSet`,
/// `CadFormSubmit`, `CadFormCancel`), from `actions::handle`.
pub(super) fn handle(action: &CadAction, call: &mut Call, cx: &mut Cx) -> Outcome {
    match action {
        CadAction::CadInvoke { id } => invoke(id, call, cx),
        CadAction::CadRun { id, params, items, revision } => match entry(id) {
            None => Outcome::Done(Err(unknown(id))),
            Some(e) => run(e, params, items.as_deref(), *revision, call, cx),
        },
        CadAction::CadFormSet { name, value } => Outcome::Done(form_set(cx.doc, name, value)),
        CadAction::CadFormSubmit => submit(call, cx),
        CadAction::CadFormCancel => Outcome::Done(Ok(form_cancel(cx.doc))),
        _ => Outcome::Done(Err("not a CAD catalogue action".into())),
    }
}

/// `CadInvoke`: what RoboCAD's command does when its menu entry, button or
/// key fires (its handler in ui/app.py, or `set_tool`).
fn invoke(id: &str, call: &mut Call, cx: &mut Cx) -> Outcome {
    let Some(entry) = entry(id) else { return super::surfaces::invoke_command(id, call, cx) };
    match entry.flow {
        Flow::Immediate | Flow::AtCursorSnap => run(entry, &Map::new(), None, None, call, cx),
        Flow::Form => {
            // RoboCAD's handlers check the selection before their dialog opens.
            if let Err(e) = resolve::resolve(entry, cx.doc, cx.topology.as_deref(), cx.view, None) {
                return Outcome::Done(Err(e));
            }
            // The dialog replaces an active pick or place tool's form, so
            // that tool ends as its Cancel ends it (`form_cancel`, less the
            // status line): a tool left active without its form would keep
            // taking clicks with no form to run them.
            let doc = &mut *cx.doc;
            doc.ops.active = None;
            doc.ops.place = None;
            Outcome::Done(Ok(open_form(doc, entry)))
        }
        Flow::PickThenForm(mode) => {
            end_tool(call, cx);
            let doc = &mut *cx.doc;
            // As RoboCAD's EdgeTool/ShellTool.activate (ui/tools.py:946, :999):
            // the mode is set directly and the selection is kept.
            if doc.select_mode != mode {
                doc.select_mode = mode;
                super::selection::publish(doc);
            }
            doc.ops.active = Some(entry.id);
            doc.ops.place = None;
            let answer = open_form(doc, entry);
            doc.show(Ok(entry.hint.to_string()));
            Outcome::Done(Ok(answer))
        }
        Flow::Place(_) => {
            end_tool(call, cx);
            let doc = &mut *cx.doc;
            doc.ops.active = Some(entry.id);
            doc.ops.place = None;
            let answer = open_form(doc, entry);
            doc.show(Ok(entry.hint.to_string()));
            Outcome::Done(Ok(answer))
        }
    }
}

/// RoboCAD's `set_tool` replaces the active tool: a transform tool's live
/// work ends and Select becomes the tool before a pick or place operation starts.
fn end_tool(call: &mut Call, cx: &mut Cx) {
    if cx.doc.tool != CadTool::Select {
        let _ = super::transform::handle(&CadAction::CadTool { tool: CadTool::Select }, call, cx);
    }
}

/// `CadRun`: refused by name with nothing sent, else one edit job (or a read).
fn run(entry: &'static OpEntry, params: &Map<String, Value>, items: Option<&[SelectionItem]>, revision: Option<u64>, call: &mut Call, cx: &mut Cx) -> Outcome {
    match prepare(cx.doc, cx.topology.as_deref(), cx.view, entry, params, items, revision) {
        Ok(built) => start(entry, built, items.is_some(), call, cx.doc),
        Err(e) => Outcome::Done(Err(e)),
    }
}

/// Everything a run checks before sending: the commit refusal (an edit in
/// flight, not connected, the shown document stale, RoboCAD's revision
/// changed since `revision`), the selection against the entry's needs, the
/// parameters, then the arguments.
fn prepare(doc: &CadDocument, topology: Option<&CadTopology>, view: Option<&CadView>, entry: &OpEntry, params: &Map<String, Value>, items: Option<&[SelectionItem]>, revision: Option<u64>) -> Result<Built, String> {
    if let Some(why) = doc.commit_refusal(revision) {
        return Err(why);
    }
    // Explicit face and edge indices are RoboCAD's numbering at some
    // revision: the caller names it, and `commit_refusal` checked it above.
    if let Some(items) = items
        && revision.is_none()
        && resolve::reads_indices(entry.needs)
        && items.iter().any(|i| i.1 == "face" || i.1 == "edge")
    {
        return Err("pass revision: the RoboCAD revision the face and edge indices in items were read at".into());
    }
    // An entry sending a revision (`extract_components`'s
    // `expected_revision`) sends the one its other values were read at:
    // a REST caller names it (the solid indices it read); a form sends
    // the revision it opened at. `commit_refusal` checked it above.
    let sends_revision = entry.args.iter().chain(entry.kwargs.iter().map(|(_, a)| a)).any(|a| *a == Arg::Revision);
    if sends_revision && items.is_some() && revision.is_none() {
        return Err(format!("{}: pass revision: the RoboCAD revision the indices in the parameters were read at", entry.id));
    }
    let mut r = resolve::resolve(entry, doc, topology, view, items)?;
    if let Some(revision) = revision {
        r.revision = revision;
    }
    let values = values(entry, params)?;
    args::build(entry, &r, &values, doc)
}

/// Send what was built: the calls in order inside one edit job (RoboCAD's
/// handler loop; each call its own RoboCAD undo step; the first error stops
/// the rest and is reported verbatim with how many had run), a paste, or a read.
/// `explicit`: the items were given (REST), not the selection.
fn start(entry: &'static OpEntry, built: Built, explicit: bool, call: &mut Call, doc: &mut CadDocument) -> Outcome {
    let outcome = match built {
        Built::Edit { calls, label } => {
            let n = calls.len();
            let message = if n > 1 { format!("{label} ({n} calls, each its own RoboCAD undo step)") } else { label.clone() };
            super::actions::edit(doc, call, label, move |c| {
                let mut results = Vec::with_capacity(n);
                for (i, op) in calls.iter().enumerate() {
                    match c.op(op.name, &op.args, &op.kwargs) {
                        Ok(r) => results.push(value(&r)),
                        Err(mut e) => {
                            if n > 1 {
                                e.message = format!("{} (call {} of {n}: {}; the {i} before it ran, each its own RoboCAD undo step)", e.message, i + 1, op.label);
                            }
                            return Err(e);
                        }
                    }
                }
                let result = if n == 1 { results.pop().unwrap_or(Value::Null) } else { Value::Array(results) };
                Ok(EditDone { message, result })
            })
        }
        Built::Paste { clip, label } => super::actions::edit(doc, call, label, move |c| {
            c.paste(&clip).map(|p| EditDone { message: format!("Pasted {} item(s)", p.pasted.len()), result: value(&p) })
        }),
        Built::Read(read) => {
            // The shown revision: stale face picks were already refused by
            // `resolve` (the selection's revision, for entries reading
            // indices) or `prepare` (explicit items need `revision`); the
            // node-only reads (copy, curvature, continuity) stay valid
            // across an edit that kept the selection.
            let revision = doc.shown_revision();
            return Outcome::Done(super::analysis_overlay::start(doc, read, revision));
        }
    };
    if !matches!(outcome, Outcome::Done(Err(_))) {
        started(doc, entry, explicit);
    }
    outcome
}

/// After an edit started: where RoboCAD's handler clears the selection,
/// the edit notes the selection now, and `sync::finish_edit` clears it once
/// the edit succeeds (RoboCAD clears after its Ops call returns: a failed
/// fillet keeps the picks), unless the run named its items (`explicit`,
/// REST): the user's selection was not what it ran on; a form-flow
/// operation's form closes (RoboCAD's dialog has returned); a pick or place
/// tool keeps its form and stays active.
fn started(doc: &mut CadDocument, entry: &OpEntry, explicit: bool) {
    if entry.clears_selection && !explicit && !doc.selection.is_empty() {
        let selection = doc.selection.clone();
        if let Some(edit) = doc.edit.as_mut() {
            edit.clear_selection = Some(selection);
        }
    }
    // A placed primitive is finished (RoboCAD's `commit` resets the stage).
    if matches!(entry.flow, Flow::Place(_)) {
        doc.ops.place = None;
    }
    if doc.ops.form.as_ref().is_some_and(|f| f.op == entry.id) {
        if entry.flow == Flow::Form {
            doc.ops.form = None;
        } else if let Some(f) = doc.ops.form.as_mut() {
            f.error = None;
        }
    }
    doc.touch();
}

/// `CadFormSubmit`: the open form's drafts as `CadRun` parameters. A form
/// flow's run is refused when RoboCAD's revision changed since the form
/// opened; a pick or place tool's form stays open across its runs, so its
/// picks carry their own guard (the selection's revision) instead.
fn submit(call: &mut Call, cx: &mut Cx) -> Outcome {
    let Some(form) = cx.doc.ops.form.clone() else { return Outcome::Done(Err("no form is open".into())) };
    let Some(entry) = entry(form.op) else {
        cx.doc.ops.form = None;
        return Outcome::Done(Err(unknown(form.op)));
    };
    // Only the shown fields are sent: a hidden one (its `when` does not
    // hold) would be refused as not applying.
    let text_of = |q: &Param| Value::String(entry.params.iter().position(|x| x.name == q.name).and_then(|i| form.texts.get(i)).cloned().unwrap_or_default());
    let params: Map<String, Value> = entry.params.iter().zip(&form.texts).filter(|(p, _)| gate(entry, p, text_of).unwrap_or(true)).map(|(p, t)| (p.name.to_string(), Value::String(t.clone()))).collect();
    let revision = if entry.flow == Flow::Form { Some(form.began) } else { None };
    let outcome = run(entry, &params, None, revision, call, cx);
    if let Outcome::Done(Err(e)) = &outcome {
        if let Some(f) = cx.doc.ops.form.as_mut().filter(|f| f.op == entry.id) {
            f.error = Some(e.clone());
        }
        cx.doc.touch();
    }
    outcome
}

/// Open `entry`'s form with RoboCAD's defaults (or the drafts of the same
/// form, when it is open already); a number field first takes the keyboard
/// with its text selected (RoboCAD's dialog).
fn open_form(doc: &mut CadDocument, entry: &'static OpEntry) -> Value {
    let texts = match &doc.ops.form {
        Some(f) if f.op == entry.id && f.texts.len() == entry.params.len() => f.texts.clone(),
        _ => entry.params.iter().map(|p| p.default.to_string()).collect(),
    };
    // A dialog's first number field takes the keyboard; a pick or place
    // tool's fields wait for Tab, as RoboCAD's numeric bar does.
    let focus = match entry.params.first() {
        Some(p) if entry.flow == Flow::Form && matches!(p.kind, FieldKind::Number { .. }) => Some(0),
        _ => None,
    };
    let began = doc.shown_revision();
    doc.ops.form = Some(FormState { op: entry.id, texts, focus, select_all: true, began, error: None });
    doc.touch();
    json!({"opened": entry.id, "form": form_json(doc)})
}

/// `CadFormSet`: one draft (a string as typed; a bool or number as its
/// text; `[x, y, z]` as "x, y, z").
fn form_set(doc: &mut CadDocument, name: &str, value: &Value) -> Result<Value, String> {
    {
        let Some(form) = doc.ops.form.as_mut() else { return Err("no form is open".into()) };
        let entry = entry(form.op).ok_or_else(|| unknown(form.op))?;
        let Some(i) = entry.params.iter().position(|p| p.name == name) else {
            let names: Vec<&str> = entry.params.iter().map(|p| p.name).collect();
            return Err(format!("{} has no parameter {name} (its parameters: {})", entry.id, if names.is_empty() { "none".to_string() } else { names.join(", ") }));
        };
        let text = match value {
            Value::String(s) => s.clone(),
            Value::Array(a) => a.iter().map(|x| x.as_str().map_or_else(|| x.to_string(), str::to_string)).collect::<Vec<_>>().join(", "),
            other => other.to_string(),
        };
        form.texts.resize(entry.params.len(), String::new());
        form.texts[i] = text;
        if form.focus == Some(i) {
            form.select_all = false;
        }
        form.error = None;
    }
    doc.touch();
    Ok(form_json(doc))
}

/// `CadFormCancel` (Cancel, Escape): close the form and end its
/// interaction (also transform's `CadCancel` while a pick or place op is active).
pub(in crate::cad) fn form_cancel(doc: &mut CadDocument) -> Value {
    let form = doc.ops.form.take().map(|f| f.op);
    let active = doc.ops.active.take();
    let place = doc.ops.place.take().is_some();
    if form.is_none() && active.is_none() && !place {
        return json!({"closed": null});
    }
    let what = active.or(form).and_then(entry).map_or("", |e| e.label);
    doc.show(Ok(format!("Cancelled {what}")));
    json!({"closed": {"form": form, "active": active, "place": place}})
}

/// The open form as `cad_state.ops.form` shows it: each field's draft,
/// whether it is shown (its `when`), and its evaluation or error.
fn form_json(doc: &CadDocument) -> Value {
    let Some(form) = &doc.ops.form else { return Value::Null };
    let Some(entry) = entry(form.op) else { return Value::Null };
    let text_of = |q: &Param| Value::String(entry.params.iter().position(|x| x.name == q.name).and_then(|i| form.texts.get(i)).cloned().unwrap_or_default());
    let fields: Vec<Value> = entry
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let text = form.texts.get(i).map_or("", String::as_str);
            // Compared as the gating field reads (its canonical option).
            let shown = gate(entry, p, text_of).unwrap_or(false);
            let evaluation = if !shown {
                Value::Null
            } else if text.is_empty() && p.default.is_empty() {
                json!({"ok": true, "optional_or_required": "empty"})
            } else {
                match param_value(p, &Value::String(text.to_string())) {
                    Ok(v) => json!({"ok": true, "value": v}),
                    Err(e) => json!({"ok": false, "error": e}),
                }
            };
            let mut f = Map::new();
            f.insert("name".into(), json!(p.name));
            f.insert("label".into(), json!(p.label));
            f.insert("kind".into(), json!(format!("{:?}", p.kind)));
            f.insert("text".into(), json!(text));
            f.insert("default".into(), json!(p.default));
            f.insert("shown".into(), json!(shown));
            f.insert("evaluation".into(), evaluation);
            Value::Object(f)
        })
        .collect();
    let mut out = Map::new();
    out.insert("op".into(), json!(entry.id));
    out.insert("label".into(), json!(entry.label));
    out.insert("fields".into(), Value::Array(fields));
    out.insert("focus".into(), json!(form.focus));
    out.insert("began".into(), json!(form.began));
    out.insert("error".into(), json!(form.error));
    Value::Object(out)
}

/// Every catalogue entry as `cad_state.ops.catalogue` lists it (built once).
fn catalogue_json() -> &'static Value {
    static JSON: OnceLock<Value> = OnceLock::new();
    JSON.get_or_init(|| {
        Value::Array(
            CATALOGUE
                .iter()
                .map(|e| {
                    let params: Vec<Value> = e.params.iter().map(|p| json!({"name": p.name, "label": p.label, "default": p.default, "when": p.when})).collect();
                    let mut m = Map::new();
                    m.insert("id".into(), json!(e.id));
                    m.insert("label".into(), json!(e.label));
                    m.insert("category".into(), json!(e.category));
                    m.insert("keys".into(), json!(e.keys));
                    m.insert("flow".into(), json!(format!("{:?}", e.flow)));
                    m.insert("route".into(), json!(e.route));
                    m.insert("params".into(), Value::Array(params));
                    Value::Object(m)
                })
                .collect(),
        )
    })
}

/// `cad_state.ops`: the open form with its fields and evaluations, the
/// active operation, the primitive being placed, the open command surface,
/// the cursor snap, the clipboard and the catalogue.
pub(super) fn state_json(doc: &CadDocument) -> Value {
    let ops = &doc.ops;
    let mut out = Map::new();
    out.insert("form".into(), form_json(doc));
    out.insert("active".into(), json!(ops.active));
    out.insert("place".into(), ops.place.as_ref().map_or(Value::Null, |p| json!(format!("{p:?}"))));
    out.insert("surface".into(), ops.surface.as_ref().map_or(Value::Null, |o| json!({"surface": o.surface, "highlight": o.highlight})));
    out.insert("cursor_snap".into(), json!(ops.cursor_snap.filter(|(revision, _)| *revision == doc.shown_revision()).map(|(_, p)| p)));
    out.insert(
        "clipboard".into(),
        ops.clipboard.as_ref().map_or(Value::Null, |(revision, clip)| json!({"revision": revision, "items": clip.get("items").and_then(Value::as_array).map_or(0, Vec::len)})),
    );
    out.insert("analysis".into(), super::analysis_overlay::state_json(&ops.analysis));
    out.insert("catalogue".into(), catalogue_json().clone());
    Value::Object(out)
}

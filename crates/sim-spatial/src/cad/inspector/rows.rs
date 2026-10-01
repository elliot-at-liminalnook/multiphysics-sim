//! The inspector's physical rows, under the physical link, following
//! RoboCAD's properties panel (ui/widgets.py:420-552) for the inspected
//! node (the first selected):
//!
//! - **Properties**: the facts line ("2 item(s)", or the exact
//!   measurement's text, RoboCAD's "Calculating exact measurements… You can
//!   keep working." while it runs, "Measurements unavailable: …") and
//!   "Calculate exact measurements" ([`super::exact`]).
//! - **Colour**: an "r, g, b" field (RoboCAD opens a colour dialog) and
//!   "Use material colour".
//! - **Joint** (a joint node): its type, bodies, pivot, axis, limits, motor,
//!   gear ratio, damping and friction as RoboCAD's `GET /robot` lists them,
//!   and "Edit joint…" (the catalogue's `ops.set_joint` form).
//! - **Joint physics**: RoboCAD's rows and labels, values from its physical
//!   model (`GET /physical?flex=0`'s joint `physics`, which already merges
//!   the declared overrides and an identified drive backlash, as RoboCAD's
//!   `inspect_joint_physics`), each overridden one marked " *" from the
//!   joint's declared `robot.physics` (its `GET /nodes/{id}`); "Drive
//!   backlash (°; provenance)" with "Unmeasured" when RoboCAD has no width,
//!   and its reference; RoboCAD's source line. A value the physical model
//!   lacks is left empty, never filled in (RoboCAD's panel shows 0 then).
//!   The model is fetched again for each shown revision while a joint is
//!   inspected ([`super::refresh`]).
//! - **Results**: RoboCAD's "Results: key value, …" from the node's loaded
//!   results block (`GET /results/nodes`), and "Material properties…".
//!
//! Buttons take their action and enabled state from
//! `physical_edit::controls_of` (what `system_ui` lists).
use super::exact::Stamp;
use super::physical_edit::{JointField, RowField, controls_of, py_g};
use super::{current_revision, field, node, numbers};
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::cad::panel::CadButton;
use crate::cad::selection::CadItems;
use crate::cad::transform::num;
use crate::ui_kit::{DANGER, Kit, Look, SUBTLE, VALUE, WARN, size, wrap};
use bevy::prelude::*;
use serde_json::{Map, Value};
use sim_runtime::cad_client::{NodeSummary, RobotJoint, SelectionItem};

/// A physical row's field: a press opens its draft (`entry`).
#[derive(Component, Clone, Debug)]
pub(in crate::cad) struct PhysicalField {
    pub node: String,
    pub field: RowField,
}

/// The keys RoboCAD's "Results:" line shows, in its order (widgets.py:547).
const RESULT_KEYS: [&str; 9] = ["peak_stress_pa", "yield_margin", "max_deflection_m", "peak_temperature_c", "peak_reaction_force_n", "bearing_margin", "peak_current_a", "stall_margin", "peak_winding_c"];

/// RoboCAD's "Results: key value, …" of a node's results block (None
/// when it has none of the keys).
pub(crate) fn results_line(results: &Value) -> Option<String> {
    let parts: Vec<String> = RESULT_KEYS.iter().filter_map(|k| results.get(*k).and_then(Value::as_f64).map(|v| format!("{k} {}", py_g(v, 3)))).collect();
    (!parts.is_empty()).then(|| format!("Results: {}", parts.join(", ")))
}

/// Joint `id`'s `physics` in RoboCAD's physical model for the shown revision.
pub(crate) fn physics<'a>(doc: &'a CadDocument, id: &str) -> Option<&'a Map<String, Value>> {
    let (revision, result) = doc.physical.as_ref()?;
    if Some(*revision) != current_revision(doc) {
        return None;
    }
    let joints = result.as_ref().ok()?.get("joints")?.as_array()?;
    joints.iter().find(|j| j.get("id").and_then(Value::as_str) == Some(id))?.get("physics")?.as_object()
}

/// Joint `id`'s declared overrides (`Node.robot["physics"]`, from its
/// `GET /nodes/{id}` at the shown revision): None while that is not loaded.
fn overrides<'a>(doc: &'a CadDocument, id: &str) -> Option<Option<&'a Map<String, Value>>> {
    let (detail_id, revision, result) = doc.detail.as_ref()?;
    if detail_id != id || *revision != doc.shown_revision() {
        return None;
    }
    let d = result.as_ref().ok()?;
    Some(d.robot.as_ref().and_then(|r| r.get("physics")).and_then(Value::as_object))
}

/// A row as RoboCAD shows it: its label (with " *" when overridden), its
/// text, and a note under it (the drive backlash's reference).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    pub label: String,
    pub text: String,
    pub note: Option<String>,
}

fn number(m: Option<&Map<String, Value>>, key: &str) -> Option<f64> {
    m?.get(key)?.as_f64()
}

fn sub<'a>(m: Option<&'a Map<String, Value>>, key: &str) -> Option<&'a Map<String, Value>> {
    m?.get(key)?.as_object()
}

/// Row `field` of a joint from its physics `phys` and declared overrides
/// `over` (None: not known yet, so no " *"), as RoboCAD's panel computes
/// it (widgets.py:511-541), without its 0.0 fill-in.
pub(crate) fn row(field: JointField, phys: &Map<String, Value>, over: Option<Option<&Map<String, Value>>>) -> Row {
    let o = over.flatten();
    let star = |set: bool| if set { " *" } else { "" };
    let label = field.label();
    let text = |v: Option<f64>, scale: f64, digits: usize| v.map(|v| py_g(v * scale, digits)).unwrap_or_default();
    match field {
        JointField::Clearance | JointField::Wobble => {
            let (key, scale) = if field == JointField::Clearance { ("clearance", 1e3) } else { ("wobble", 180.0 / std::f64::consts::PI) };
            let v = number(o, key).or_else(|| number(Some(phys), key));
            Row { label: format!("{label}{}", star(o.is_some_and(|o| o.contains_key(key)))), text: text(v, scale, 4), note: None }
        }
        JointField::DriveBacklash => {
            let drive = sub(o, "drive_backlash").or_else(|| sub(Some(phys), "drive_backlash"));
            // A drive block's width_rad, even null (declared unmeasured),
            // wins; the scalar `backlash` only when the block has no key.
            let width = match drive.and_then(|d| d.get("width_rad")) {
                Some(v) => v.as_f64(),
                None => number(o, "backlash"),
            };
            let provenance = drive.and_then(|d| d.get("provenance")).and_then(Value::as_str).map_or_else(|| if width.is_some() { "estimated".to_string() } else { "unmeasured".to_string() }, str::to_string);
            let reference = drive.and_then(|d| d.get("reference")).and_then(Value::as_str).unwrap_or("No drive-backlash provenance supplied");
            Row { label: format!("Drive backlash (°; {provenance})"), text: width.map(|w| py_g(w.to_degrees(), 5)).unwrap_or_default(), note: Some(reference.to_string()) }
        }
        JointField::Coulomb | JointField::Viscous => {
            let key = if field == JointField::Coulomb { "coulomb" } else { "viscous" };
            let v = number(sub(o, "friction"), key).or_else(|| number(sub(Some(phys), "friction"), key));
            Row { label: format!("{label}{}", star(sub(o, "friction").is_some_and(|f| f.contains_key(key)))), text: text(v, 1e3, 4), note: None }
        }
        JointField::RadialStiffness => {
            let v = number(sub(o, "stiffness"), "radial").or_else(|| number(sub(Some(phys), "stiffness"), "radial"));
            Row { label: format!("{label}{}", star(sub(o, "stiffness").is_some_and(|s| s.contains_key("radial")))), text: text(v, 1.0, 4), note: None }
        }
        JointField::FlexPatchRadius => {
            let set = o.and_then(|o| o.get("flex_patch_radius")).is_some_and(|v| !v.is_null());
            Row { label: format!("{label}{}", star(set)), text: text(number(Some(phys), "flex_patch_radius"), 1e3, 4), note: None }
        }
    }
}

/// RoboCAD's source line (widgets.py:544).
pub(crate) fn source_line(phys: &Map<String, Value>) -> String {
    let at = |key: &str, scale: f64, decimals: usize| number(Some(phys), key).map_or_else(|| "?".to_string(), |v| format!("{:.*}", decimals, v * scale));
    let source = phys.get("source").and_then(Value::as_str).unwrap_or("?");
    format!("source: {source}, pin Ø{} mm in Ø{} mm over {} mm; * = overridden", at("pin_radius", 2e3, 2), at("hole_radius", 2e3, 2), at("contact_length", 1e3, 1))
}

/// A node's colour as the field shows it ("" for the material's).
fn colour_text(n: &NodeSummary) -> String {
    n.color.as_deref().map(|c| c.iter().map(|x| num(*x)).collect::<Vec<_>>().join(", ")).unwrap_or_default()
}

/// The text a row's draft opens with.
pub(crate) fn current_text(doc: &CadDocument, id: &str, field: RowField) -> String {
    match field {
        RowField::Color => node(doc, id).map(colour_text).unwrap_or_default(),
        RowField::Joint(f) => physics(doc, id).map(|p| row(f, p, overrides(doc, id)).text).unwrap_or_default(),
    }
}

/// What the rows show, for the part's key.
pub(crate) fn key(doc: &CadDocument, selection: &[SelectionItem]) -> String {
    let now = Stamp::of(doc, selection);
    let id = selection.first_node();
    let n = id.and_then(|id| node(doc, id)).map(|n| (&n.name, &n.kind, &n.color, &n.material));
    let joint = id.and_then(|id| doc.robot.data.summary().and_then(|s| s.joints.iter().find(|j| j.id == id)));
    let phys = id.and_then(|id| physics(doc, id)).map(|p| Value::Object(p.clone()).to_string());
    let over = id.map(|id| overrides(doc, id).map(|o| o.map(|o| Value::Object(o.clone()).to_string())));
    let results = id.and_then(|id| doc.robot.data.node_results(id)).map(|r| r.results.to_string());
    let stale = doc.robot.data.results().and_then(|r| r.stale);
    let controls: Vec<(String, String, bool)> = controls_of(doc, selection).into_iter().map(|c| (c.0, c.1, c.3.is_ok())).collect();
    let fetch = (doc.physical.as_ref().map(|(r, res)| (*r, res.as_ref().err())), doc.physical_job.is_some(), doc.robot.data.reading(), doc.robot.data.summary_error());
    let facts = doc.physical_edit.exact.facts(&now);
    format!("{:?}", (now.nodes, facts, n, joint, phys, over, results, stale, &doc.physical_edit.draft, controls, fetch, doc.connected()))
}

/// The controls (`controls_of`): (id, label, action, ready).
type Controls = [(String, String, CadAction, Result<(), String>)];

/// The button of control `id`, labelled and enabled as listed.
fn button(p: &mut ChildSpawnerCommands, k: &Kit, controls: &Controls, id: &str, look: Look) {
    if let Some((_, label, action, ready)) = controls.iter().find(|c| c.0 == id) {
        p.spawn(k.button(label, CadButton(action.clone()), look, ready.is_ok()));
    }
}

/// One typed row: its label, its field (the draft while typed) and under
/// it the draft's error or how to commit.
#[allow(clippy::too_many_arguments)]
fn input_row(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, id: &str, which: RowField, label: &str, text: &str, placeholder: &str) {
    let draft = doc.physical_edit.draft.as_ref().filter(|d| d.node == id && d.field == which);
    let shown = draft.map_or(text, |d| d.draft.text.as_str());
    p.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), padding: UiRect::vertical(Val::Px(2.0)), flex_shrink: 0.0, ..default() }).with_children(|c| {
        c.spawn(k.text(label, size::BODY, SUBTLE, 0));
        c.spawn(k.input_selectable(shown, placeholder, PhysicalField { node: id.to_string(), field: which }, draft.is_some(), draft.is_some_and(|d| d.draft.select_all)));
    });
    match draft {
        Some(d) if d.error.is_some() => {
            p.spawn(k.text(d.error.clone().unwrap_or_default(), size::SMALL, DANGER, 0));
        }
        Some(_) => {
            p.spawn(k.note("Enter sets it in RoboCAD (one undo step); Escape cancels."));
        }
        None => {}
    }
}

/// The physical rows for the selection (see the module doc).
pub(crate) fn draw(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, selection: &[SelectionItem]) {
    let now = Stamp::of(doc, selection);
    if now.nodes.is_empty() {
        return;
    }
    let controls = controls_of(doc, selection);
    p.spawn(k.section("Properties"));
    let facts = doc.physical_edit.exact.facts(&now).unwrap_or_else(|| format!("{} item(s)\nExact measurements available on request.", now.nodes.len()));
    p.spawn(k.text(facts, size::BODY, VALUE, 0));
    p.spawn(wrap()).with_children(|r| {
        button(r, k, &controls, "cad:inspect:exact", Look::Secondary);
        button(r, k, &controls, "cad:inspect:exact-cancel", Look::Ghost);
    });
    p.spawn(k.caption("Volume, area and mass from RoboCAD's exact geometry, off this window's thread (up to 60 s). Cancel, an edit or a selection change stops waiting; the request already sent finishes in RoboCAD."));
    let Some(n) = selection.first_node().and_then(|id| node(doc, id)) else { return };
    p.spawn(k.section("Colour"));
    input_row(p, k, doc, &n.id, RowField::Color, "Colour (r, g, b; 0–1)", &colour_text(n), "material colour: type r, g, b");
    if n.color.is_none() {
        p.spawn(k.caption("Drawn in its material's colour."));
    }
    p.spawn(wrap()).with_children(|r| button(r, k, &controls, "cad:inspect:material-colour", Look::Ghost));
    if n.kind == "joint" {
        joint(p, k, doc, n, &controls);
    }
    if let Some(line) = doc.robot.data.node_results(&n.id).and_then(|r| results_line(&r.results)) {
        p.spawn(k.text(line, size::BODY, VALUE, 0));
        if doc.robot.data.results().and_then(|r| r.stale) == Some(true) {
            p.spawn(k.text("These results were computed for another state of the document (stale).", size::SMALL, WARN, 0));
        }
    }
    p.spawn(wrap()).with_children(|r| button(r, k, &controls, "cad:inspect:material-props", Look::Secondary));
}

/// The joint as RoboCAD's `GET /robot` lists it, Edit joint…, and its physics rows.
fn joint(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument, n: &NodeSummary, controls: &Controls) {
    p.spawn(k.section("Joint"));
    match doc.robot.data.summary().and_then(|s| s.joints.iter().find(|j| j.id == n.id)) {
        Some(j) => joint_fields(p, k, j),
        None => {
            if let Some(e) = doc.robot.data.summary_error() {
                p.spawn(k.text(e.to_string(), size::SMALL, DANGER, 0));
            } else if doc.robot.data.reading() || doc.robot.data.summary().is_none() {
                p.spawn(k.caption("Reading RoboCAD's robot description…"));
            } else {
                p.spawn(k.caption("RoboCAD's robot description does not list this joint."));
            }
        }
    }
    p.spawn(wrap()).with_children(|r| button(r, k, controls, "cad:inspect:edit-joint", Look::Secondary));
    p.spawn(k.section("Joint physics"));
    let Some(phys) = physics(doc, &n.id) else {
        physics_missing(p, k, doc);
        return;
    };
    let over = overrides(doc, &n.id);
    for f in JointField::ALL {
        let r = row(f, phys, over);
        let placeholder = if f == JointField::DriveBacklash { "Unmeasured" } else { "not in RoboCAD's physical model" };
        input_row(p, k, doc, &n.id, RowField::Joint(f), &r.label, &r.text, placeholder);
        if let Some(note) = r.note {
            p.spawn(k.caption(format!("Full rotational lost-motion width at the drive connection, additional to motor gearbox backlash. Editing declares an estimate; clearing marks it unmeasured. Radial bearing clearance is separate. {note}")));
        }
    }
    p.spawn(k.caption(source_line(phys)));
    if over.is_none() {
        p.spawn(k.caption("The * marks show once RoboCAD's node detail for this joint has loaded."));
    }
}

/// Why the joint physics rows are not shown, and the fetch.
fn physics_missing(p: &mut ChildSpawnerCommands, k: &Kit, doc: &CadDocument) {
    if doc.physical_job.is_some() {
        p.spawn(k.caption("Fetching RoboCAD's physical model…"));
        return;
    }
    match &doc.physical {
        Some((revision, Err(e))) if Some(*revision) == current_revision(doc) => {
            p.spawn(k.text(e.clone(), size::SMALL, DANGER, 0));
        }
        Some((revision, Ok(_))) if Some(*revision) == current_revision(doc) => {
            p.spawn(k.caption("RoboCAD's physical model has no record of this joint."));
            return;
        }
        _ => {
            p.spawn(k.caption("RoboCAD infers joint physics in its physical model, fetched for each revision while a joint is inspected; fetch it again if it did not arrive."));
        }
    }
    let ready = doc.connected();
    p.spawn(wrap()).with_children(|r| {
        r.spawn(k.button("Fetch physical model", CadButton(CadAction::CadPhysical), Look::Secondary, ready));
    });
}

/// A joint's fields from RoboCAD's robot description (rad and mm as
/// RoboCAD stores them; limits shown in degrees for rotary joints, as its
/// pose panel shows them).
fn joint_fields(p: &mut ChildSpawnerCommands, k: &Kit, j: &RobotJoint) {
    let prismatic = j.kind == "prismatic";
    field(p, k, "Type", &j.kind, "");
    field(p, k, "Parent", j.parent_name.as_deref().or(j.parent.as_deref()).unwrap_or("world"), "");
    field(p, k, "Child", j.child_name.as_deref().unwrap_or(&j.child), "");
    field(p, k, "Pivot", &numbers(&j.pivot), "mm");
    field(p, k, "Axis", &numbers(&j.axis), "");
    let (factor, unit) = if prismatic { (1.0, "mm") } else { (180.0 / std::f64::consts::PI, "°") };
    let limits = match (j.lower, j.upper) {
        (Some(lo), Some(hi)) => format!("{:.1} to {:.1} {unit}", lo * factor, hi * factor),
        (lo, hi) => format!("unset (lower {}, upper {})", lo.map_or("none".to_string(), |v| format!("{:.1} {unit}", v * factor)), hi.map_or("none".to_string(), |v| format!("{:.1} {unit}", v * factor))),
    };
    field(p, k, "Limits", &limits, "");
    field(p, k, "Motor", j.motor_name.as_deref().or(j.motor.as_deref()).unwrap_or("none"), "");
    field(p, k, "Gear ratio", &j.gear_ratio.to_string(), "");
    field(p, k, "Damping", &j.damping.to_string(), if prismatic { "" } else { "N·m·s/rad" });
    field(p, k, "Friction", &j.friction.to_string(), if prismatic { "" } else { "N·m" });
}

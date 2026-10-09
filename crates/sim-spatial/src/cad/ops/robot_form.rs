//! The robot dialogs' combo boxes and presets (cad-physical-inspect):
//! [`picks`] resolves a `FieldKind::Pick` field's choices from the
//! document as RoboCAD's dialogs fill their `QComboBox`es, [`seed`] presets
//! a newly opened form as RoboCAD's handlers preset their dialogs (from the
//! selection, the active plane, the selected joint, the document's battery,
//! control and uncertainty, the last motor), [`precheck`] is RoboCAD's
//! refusal before a dialog opens, and [`note`] the Add motor dialog's notes
//! line. Every choice is read from RoboCAD's answers (the shown tree,
//! `GET /robot`, `GET /motors`); nothing is computed or filled in.
//!
//! A combo box always has a current entry: a preset that is not in its
//! list leaves it on its first entry ("(world)", "(none)", or the first
//! body), and so does [`seed`]. Values are shown as RoboCAD's dialogs show
//! them (`f"{v:g}"` line edits, [`g`]; spin boxes rounded to their
//! decimals), so an untouched field sends what RoboCAD's would.
use super::resolve::{Resolved, kind_of};
use super::{Env, OpEntry};
use crate::cad::document::CadDocument;
use crate::cad::transform::num;
use crate::ui_kit::form::FieldKind;
use serde_json::{Map, Value, json};
use crate::cad::types::{RobotJoint, RobotSummary, SelectionItem};

/// RoboCAD's `JOINT_TYPES` with `JOINT_TYPE_HINTS` (robotics.py:22, ui/widgets.py:1065-1071).
const JOINT_TYPES: [(&str, &str); 7] = [
    ("revolute", "hinge with angle limits (servo, geared motor)"),
    ("continuous", "hinge without limits (wheel, stepper)"),
    ("prismatic", "slider along the axis (linear actuator)"),
    ("fixed", "rigid attachment: the child moves with the parent"),
    ("ball", "3-DoF spherical joint (no motor)"),
    ("loop_revolute", ""),
    ("loop_spherical", ""),
];

/// SensorDialog's kinds and hints (ui/widgets.py:1376).
const SENSOR_KINDS: [(&str, &str); 4] = [
    ("imu", "IMU: accelerometer + gyro with noise, bias and quantisation"),
    ("encoder", "encoder on a joint"),
    ("current", "motor current sense"),
    ("force", "load cell / foot force"),
];

/// Python's `format(v, "g")`: six significant digits, trailing zeros
/// dropped, exponent form below 1e-4 or from 1e6 ("1e-05", "1.23457e+06").
pub(in crate::cad) fn g(v: f64) -> String {
    if !v.is_finite() {
        return if v.is_nan() { "nan".into() } else if v > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let strip = |s: String| -> String { if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s } };
    // The exponent after rounding to six significant digits.
    let sci = format!("{v:.5e}");
    let (mantissa, exp) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    if (-4..6).contains(&exp) {
        strip(format!("{:.*}", (5 - exp) as usize, v))
    } else {
        format!("{}e{}{:02}", strip(mantissa.to_string()), if exp < 0 { '-' } else { '+' }, exp.abs())
    }
}

/// Three values as RoboCAD's three line edits show them.
fn g3(v: [f64; 3]) -> String {
    v.map(g).join(", ")
}

/// A spin box's value: rounded to its decimals.
fn fixed(v: f64, decimals: i32) -> String {
    let k = 10f64.powi(decimals);
    num((v * k).round() / k)
}

/// A pick field's choices (key, label), in RoboCAD's order, from the
/// document now: "motors" (`GET /motors`, MotorDialog's
/// "name   kind  stall N·m  mass g"), "bodies" (`_robot_bodies`: the tree's
/// bodies and sheets that are not motors), "bodies_or_world",
/// "bodies_or_pick" (MotorDialog's "(pick by clicking a face)"),
/// "bodies_or_none", "joints" (the tree's joints), "joints_or_none",
/// "motors_placed" (`_robot_motors`), "motors_placed_or_none",
/// "joint_types", "sensor_kinds"; cad-print's "printers" and "filaments"
/// (`print::picks`). Empty for an unknown source.
pub(in crate::cad) fn picks(source: &str, doc: &CadDocument) -> Vec<(String, String)> {
    let nodes = doc.doc.as_ref().map_or(&[][..], |d| d.nodes.as_slice());
    let summary = doc.robot.data.summary();
    let is_motor = |id: &str| summary.is_some_and(|s| s.motors.iter().any(|m| m.id == id));
    let bodies = || -> Vec<(String, String)> { nodes.iter().filter(|n| (n.kind == "body" || n.kind == "sheet") && !is_motor(n.id.as_str())).map(|n| (n.id.clone(), n.name.clone())).collect() };
    let joints = || -> Vec<(String, String)> { nodes.iter().filter(|n| n.kind == "joint").map(|n| (n.id.clone(), n.name.clone())).collect() };
    let placed = || -> Vec<(String, String)> { summary.map_or_else(Vec::new, |s| s.motors.iter().map(|m| (m.id.clone(), m.name.clone())).collect()) };
    let first = |label: &str, mut rest: Vec<(String, String)>| {
        rest.insert(0, (String::new(), label.to_string()));
        rest
    };
    match source {
        "motors" => doc.robot.data.motor_library(doc).map_or_else(Vec::new, |lib| lib.iter().map(|(id, m)| (id.clone(), format!("{}   {}  {} N·m  {} g", m.name, m.kind, g(m.stall_torque), g(m.mass_g)))).collect()),
        "bodies" => bodies(),
        "bodies_or_world" => first("(world)", bodies()),
        "bodies_or_pick" => first("(pick by clicking a face)", bodies()),
        "bodies_or_none" => first("(none)", bodies()),
        "joints" => joints(),
        "joints_or_none" => first("(none)", joints()),
        "motors_placed" => placed(),
        "motors_placed_or_none" => first("(none)", placed()),
        "joint_types" => hinted(&JOINT_TYPES),
        "sensor_kinds" => hinted(&SENSOR_KINDS),
        // cad-print's lists: the registry's printers and filaments.
        _ => crate::cad::print::picks(source, doc),
    }
}

/// A list with RoboCAD's hints: "revolute: hinge with angle limits (…)".
fn hinted(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(k, hint)| (k.to_string(), format!("{k}: {hint}").trim_end().to_string())).collect()
}

/// "The robot description is still being read" at the shown revision.
fn reading(doc: &CadDocument) -> String {
    format!("the robot description is still being read (revision {}); try again in a moment", doc.shown_revision())
}

/// RoboCAD's robot description as read at the shown revision, or why it
/// cannot be relied on now (still being read, not connected, or the read
/// failed): a dialog prefilled from an older or failed read would send
/// that revision's values, or RoboCAD's defaults, over the current ones.
pub(super) fn description(doc: &CadDocument) -> Result<&RobotSummary, String> {
    let data = &doc.robot.data;
    if !data.current(doc) {
        return Err(if doc.connected() { reading(doc) } else { "the robot description cannot be read: no CAD document is open".to_string() });
    }
    match data.bundle.as_ref().map(|b| &b.summary) {
        Some(Ok(s)) => Ok(s),
        Some(Err(e)) => Err(format!("RoboCAD's robot description could not be read: {e}")),
        None => Err(reading(doc)),
    }
}

/// One of the description's reads beside the summary (`field`: the
/// bundle's, e.g. its battery), at the shown revision, or why not: a
/// failed read is not "none set", so nothing is filled in from it.
pub(super) fn read<'a, T>(doc: &'a CadDocument, what: &str, field: Option<&'a Result<T, String>>) -> Result<&'a T, String> {
    description(doc)?;
    match field {
        Some(Ok(v)) => Ok(v),
        Some(Err(e)) => Err(format!("RoboCAD's {what} could not be read, so the current one is not known: {e}")),
        None => Err(reading(doc)),
    }
}

/// RoboCAD's refusal before a dialog opens: "add a motor and a joint
/// first" (ui/app.py:1607-1609), or why the values the dialog is filled
/// from are not known (`selection`: the shared selection's CAD items).
/// The power dialog shows the document's battery, control loop and
/// uncertainty, and OK sends all three: a read that failed or predates the
/// shown revision would delete the battery or reset the targets. The Edit
/// joint dialog shows the selected joint as the description has it.
pub(super) fn precheck(entry: &OpEntry, doc: &CadDocument, selection: &[SelectionItem]) -> Option<String> {
    let bundle = || doc.robot.data.bundle.as_ref();
    let ready = match entry.id {
        "robot.assign_motor" => description(doc).map(|_| ()),
        "robot.power" => description(doc).and_then(|_| {
            read(doc, "battery setting", bundle().map(|b| &b.battery))?;
            read(doc, "control loop setting", bundle().map(|b| &b.control))?;
            read(doc, "uncertainty setting", bundle().map(|b| &b.uncertainty)).map(|_| ())
        }),
        "ops.set_joint" => description(doc).and_then(|s| match selection.iter().map(|i| i.0.as_str()).find(|id| kind_of(doc, id) == Some("joint")) {
            Some(id) if !s.joints.iter().any(|j| j.id == id) => Err(format!("{} is not in RoboCAD's robot description at revision {}, so its values are not known", doc.node_name(id), doc.shown_revision())),
            // No joint selected: `resolve` refused it already.
            _ => Ok(()),
        }),
        _ => return None,
    };
    if let Err(why) = ready {
        return Some(why);
    }
    (entry.id == "robot.assign_motor" && (picks("motors_placed", doc).is_empty() || picks("joints", doc).is_empty())).then(|| "add a motor and a joint first".to_string())
}

/// The forms preset from the selection or the description (`seed`): each
/// opening starts from RoboCAD's presets again, as RoboCAD builds its
/// dialog anew, so Edit joint opened for another joint never shows the
/// previous one's values.
pub(super) fn reseeds(id: &str) -> bool {
    matches!(id, "ops.set_joint" | "robot.joint_dialog" | "robot.assign_motor" | "robot.add_sensor" | "robot.add_cable" | "robot.power")
}

/// Parameter `name`'s draft index.
fn index(entry: &OpEntry, name: &str) -> Option<usize> {
    entry.params.iter().position(|p| p.name == name)
}

fn put(entry: &OpEntry, texts: &mut [String], name: &str, text: String) {
    if let Some(i) = index(entry, name).filter(|i| *i < texts.len()) {
        texts[i] = text;
    }
}

/// RoboCAD's presets for a newly opened robot form (see the module doc):
/// the last motor's spec, rotation and cut (`self.last_motor`); the joint
/// dialog's parent and child from the selected bodies and pivot and axis
/// from the active plane (`robot_joint_dialog`); the Edit joint dialog's
/// values from the selected joint (`robot_edit_joint`: `j.to_json()`);
/// the selected motor and joint (`robot_assign_motor`); the sensor's body
/// and the cable's ends from the selected bodies; the power dialog's
/// battery, control loop, targets and uncertainty from the document. Then
/// every pick field not on one of its choices is on its first.
pub(super) fn seed(entry: &OpEntry, doc: &CadDocument, env: &Env, texts: &mut [String]) {
    // RoboCAD's `selection.nodes()` order; its handlers take the bodies (`kind == "body"`).
    let mut selected: Vec<&str> = Vec::new();
    for item in env.selection {
        if !selected.contains(&item.0.as_str()) {
            selected.push(item.0.as_str());
        }
    }
    let bodies: Vec<&str> = selected.iter().copied().filter(|id| kind_of(doc, id) == Some("body")).collect();
    match entry.id {
        "robot.add_motor" => {
            for (name, text) in doc.robot.tools.last_motor.iter().flatten() {
                put(entry, texts, name, text.clone());
            }
        }
        "robot.joint_dialog" => {
            match bodies.as_slice() {
                [parent, child, ..] => {
                    put(entry, texts, "parent", parent.to_string());
                    put(entry, texts, "child", child.to_string());
                }
                [child] => put(entry, texts, "child", child.to_string()),
                [] => {}
            }
            if let Some(f) = env.plane.and_then(|p| p.frame().ok().flatten()) {
                put(entry, texts, "pivot", g3(f.origin));
                put(entry, texts, "axis", g3(f.normal));
            }
        }
        "ops.set_joint" => {
            let joint = selected.iter().copied().find(|id| kind_of(doc, id) == Some("joint")).and_then(|id| doc.robot.data.summary()?.joints.iter().find(|j| j.id == id));
            if let Some(j) = joint {
                joint_texts(entry, j, texts);
            }
        }
        "robot.assign_motor" => {
            let placed = picks("motors_placed", doc);
            if let Some(m) = selected.iter().copied().find(|id| placed.iter().any(|(k, _)| k == id)) {
                put(entry, texts, "motor", m.to_string());
            }
            if let Some(j) = selected.iter().copied().find(|id| kind_of(doc, id) == Some("joint")) {
                put(entry, texts, "joint", j.to_string());
            }
        }
        "robot.add_sensor" => {
            if let Some(b) = bodies.first() {
                put(entry, texts, "body", b.to_string());
            }
        }
        "robot.add_cable" => {
            if let Some(b) = bodies.first() {
                put(entry, texts, "from_body", b.to_string());
            }
            if let Some(b) = bodies.get(1) {
                put(entry, texts, "to_body", b.to_string());
            }
        }
        "robot.power" => power(entry, doc, texts),
        // cad-print: the remembered Fastener hole, Clearance and wall check values.
        _ => crate::cad::print::seed(entry, doc, env, texts),
    }
    // A combo box is always on one of its entries.
    for (i, p) in entry.params.iter().enumerate() {
        if let (FieldKind::Pick { source }, Some(text)) = (p.kind, texts.get(i)) {
            let list = picks(source, doc);
            if !list.iter().any(|(k, _)| k == text) {
                texts[i] = list.first().map(|(k, _)| k.clone()).unwrap_or_default();
            }
        }
    }
}

/// The Edit joint dialog's values for joint `j` (`robot_edit_joint`:
/// `j.to_json()`): limits in degrees unless prismatic, line edits as
/// [`g`], spin boxes rounded to their decimals.
fn joint_texts(entry: &OpEntry, j: &RobotJoint, texts: &mut [String]) {
    let degrees = j.kind != "prismatic";
    let limit = |v: Option<f64>| v.map_or_else(String::new, |v| g(if degrees { v.to_degrees() } else { v }));
    put(entry, texts, "type", j.kind.clone());
    put(entry, texts, "parent", j.parent.clone().unwrap_or_default());
    put(entry, texts, "child", j.child.clone());
    put(entry, texts, "pivot", g3(j.pivot));
    put(entry, texts, "axis", g3(j.axis));
    put(entry, texts, "lower", limit(j.lower));
    put(entry, texts, "upper", limit(j.upper));
    put(entry, texts, "motor", j.motor.clone().unwrap_or_default());
    put(entry, texts, "gear_ratio", fixed(j.gear_ratio, 2));
    put(entry, texts, "damping", fixed(j.damping, 4));
    put(entry, texts, "name", j.name.clone());
}

/// `ops.set_joint`'s parameters for a run (`ops::prepare`): `given` over
/// the resolved joint's current values ([`joint_values`]: exact, as JSON
/// numbers, not the form's rounded display text), so a REST `cad_run
/// ops.set_joint` naming only `lower` changes only the lower limit and
/// sends every other field as the description has it, instead of
/// resetting it to the dialog's default or rounding it to the dialog's
/// digits. The window form sends every field (all its drafts), so `given`
/// is returned unchanged and its OK behaves as before. Unlike the form, a
/// filled-in pick is not moved onto the combo box's first entry when the
/// dialog would not list it: the field keeps the joint's own value.
/// Refused by name, with nothing sent, when the description is not read at
/// the shown revision ([`description`]), when the joint is not in it, when
/// `type` changes between prismatic and a rotary kind while a limit the
/// joint has is left out (its value is in the old kind's unit: mm against
/// degrees), and when a filled-in value is one the dialog's field refuses
/// (a gear ratio below 0.01, damping above 1000): the refusal says it is
/// the joint's current value and to pass that parameter.
pub(super) fn fill_from_joint(entry: &OpEntry, doc: &CadDocument, r: &Resolved, given: &Map<String, Value>) -> Result<Map<String, Value>, String> {
    let missing: Vec<&super::Param> = entry.params.iter().filter(|p| !given.contains_key(p.name)).collect();
    if missing.is_empty() {
        return Ok(given.clone());
    }
    let jid = r.nodes.first().ok_or_else(|| entry.refusal.to_string())?;
    let name = doc.node_name(jid);
    let unknown = |why: String| format!("{}: {name}'s current values fill the parameters not given ({}), and {why}", entry.label, missing.iter().map(|p| p.name).collect::<Vec<_>>().join(", "));
    let s = description(doc).map_err(unknown)?;
    let j = s.joints.iter().find(|j| j.id == *jid).ok_or_else(|| unknown(format!("{name} is not in RoboCAD's robot description at revision {}, so its values are not known", doc.shown_revision())))?;
    if let Some(kind) = given.get("type").and_then(Value::as_str).map(str::trim)
        && (kind == "prismatic") != (j.kind == "prismatic")
    {
        let left: Vec<&str> = [("lower", j.lower), ("upper", j.upper)].into_iter().filter(|(n, v)| v.is_some() && !given.contains_key(*n)).map(|(n, _)| n).collect();
        if !left.is_empty() {
            return Err(format!("{}: {name} changes from {} to {kind}: pass {} (the current limits are in {}, the new type's in {})", entry.label, j.kind, left.join(" and "), if j.kind == "prismatic" { "mm" } else { "degrees" }, if kind == "prismatic" { "mm" } else { "degrees" }));
        }
    }
    let current = joint_values(j);
    let mut out = given.clone();
    for p in missing {
        let Some(v) = current.get(p.name) else { continue };
        // An empty text is a field left empty (no parent, no motor, no
        // limit): `values` leaves it out and the call sends null, as the form does.
        if v.as_str() != Some("") {
            super::param_value(p, v).map_err(|e| format!("{}: {name}'s current {} ({v}) fills the parameter not given, and the dialog's field refuses it ({e}); pass {}", entry.label, p.name, p.name))?;
        }
        out.insert(p.name.to_string(), v.clone());
    }
    Ok(out)
}

/// Joint `j`'s values as `ops.set_joint`'s parameters, exact (what
/// [`fill_from_joint`] fills in): pivot and axis as three numbers, limits
/// in degrees unless prismatic (the dialog's unit; a rotary limit makes the
/// dialog's own degree round trip, so it may differ from the stored radians
/// by one ulp, as RoboCAD's `radians(float(text))` does) or empty when unset,
/// gear ratio and damping as stored, type, parent, child, motor and name
/// as their keys and text (empty when unset).
fn joint_values(j: &RobotJoint) -> Map<String, Value> {
    let degrees = j.kind != "prismatic";
    let limit = |v: Option<f64>| v.map_or_else(|| json!(""), |v| Value::from(if degrees { v.to_degrees() } else { v }));
    let mut m = Map::new();
    m.insert("type".into(), json!(j.kind));
    m.insert("parent".into(), json!(j.parent.clone().unwrap_or_default()));
    m.insert("child".into(), json!(j.child));
    m.insert("pivot".into(), json!(j.pivot));
    m.insert("axis".into(), json!(j.axis));
    m.insert("lower".into(), limit(j.lower));
    m.insert("upper".into(), limit(j.upper));
    m.insert("motor".into(), json!(j.motor.clone().unwrap_or_default()));
    m.insert("gear_ratio".into(), json!(j.gear_ratio));
    m.insert("damping".into(), json!(j.damping));
    m.insert("name".into(), json!(j.name));
    m
}

/// PowerDialog's values (ui/widgets.py:1463-1505): the document's
/// battery, control loop and uncertainty over RoboCAD's defaults (the
/// catalogue's), each motion joint's target in degrees.
fn power(entry: &OpEntry, doc: &CadDocument, texts: &mut [String]) {
    let data = &doc.robot.data;
    if let Some(b) = data.battery() {
        put(entry, texts, "cells", b.cells.to_string());
        if ["lipo", "liion", "lifepo4", "nimh", "alkaline"].contains(&b.chemistry.as_str()) {
            put(entry, texts, "chemistry", b.chemistry.clone());
        }
        put(entry, texts, "capacity_ah", fixed(b.capacity_ah, 2));
    }
    let current = data.control().map(|c| c.targets.clone()).unwrap_or_default();
    if let Some(c) = data.control() {
        put(entry, texts, "period_s", fixed(c.period_s, 4));
        put(entry, texts, "latency_s", fixed(c.latency_s, 4));
    }
    if let Some(joints) = super::robot_args::motion_joints(doc) {
        let targets: Map<String, Value> = joints.into_iter().map(|j| {
            let deg: f64 = g(current.get(&j).copied().unwrap_or(0.0).to_degrees()).parse().unwrap_or(0.0);
            (j, json!(deg))
        }).collect();
        put(entry, texts, "targets", Value::Object(targets).to_string());
    }
    if let Some(u) = data.uncertainty() {
        let sigma = |key: &str, field: &str| u.get(key).and_then(|v| v.get(field)).and_then(Value::as_f64);
        if let Some(s) = sigma("dimension_m", "sigma") {
            put(entry, texts, "dimension", fixed(s * 1e3, 3));
        }
        if let Some(s) = sigma("friction", "sigma_fraction") {
            put(entry, texts, "friction", fixed(s, 2));
        }
    }
}

/// The Add motor dialog's notes line for the chosen motor
/// (`MotorDialog._update_notes`, ui/widgets.py:1117-1122), or why the
/// library is not shown; None for other forms.
pub(in crate::cad) fn note(entry: &OpEntry, texts: &[String], doc: &CadDocument) -> Option<String> {
    if entry.id != "robot.add_motor" {
        return None;
    }
    let data = &doc.robot.data;
    let Some(lib) = data.motor_library(doc) else {
        return Some(match data.motor_library_error() {
            Some(e) => format!("RoboCAD's motor library could not be read: {e}"),
            None => "Reading RoboCAD's motor library…".to_string(),
        });
    };
    let spec = texts.get(index(entry, "spec")?)?.trim();
    let m = lib.get(spec)?;
    let holes = if m.mount_holes.is_empty() { "no mount holes".to_string() } else { format!("{} mount holes", m.mount_holes.len()) };
    Some(format!(
        "{}, {}×{}×{} mm, shaft Ø{}×{} mm, {holes}, {} rad/s no-load, {} V. {}",
        m.shape,
        g(m.size[0]),
        g(m.size[1]),
        g(m.size[2]),
        g(m.shaft_diameter),
        g(m.shaft_length),
        g(m.no_load_speed),
        g(m.voltage),
        m.notes
    ))
}

/// Open catalogue form `id` anew, seeded ([`seed`]), with `preset` drafts
/// over it where each is one of its field's choices (a combo box's
/// `findData`): the joint tool's parent, child, pivot and axis. A pick
/// that is not one of its field's choices (a motor clicked as a body: a
/// motor is not a link) leaves the field as seeded, and the status line
/// and the answer's `dropped` name it (RoboCAD's dialog drops it silently).
/// As `cad_state.ops.form` shows it.
pub(in crate::cad) fn open_preset(doc: &mut CadDocument, env: &Env, id: &str, preset: &[(&str, String)]) -> Result<Value, String> {
    let entry = super::entry(id).ok_or_else(|| format!("{id} is not in the catalogue"))?;
    doc.ops.form = None;
    super::form::open_form_with(doc, entry, Some(env));
    let mut dropped: Vec<String> = Vec::new();
    let allowed: Vec<(usize, String)> = preset
        .iter()
        .filter_map(|(name, text)| {
            let i = index(entry, name)?;
            let ok = match entry.params[i].kind {
                FieldKind::Pick { source } => picks(source, doc).iter().any(|(k, _)| k == text),
                _ => true,
            };
            if !ok {
                // Why, from what the pick is: a motor (once the robot description names it), or not a body here.
                let motor = doc.robot.data.summary().is_some_and(|s| s.motors.iter().any(|m| m.id == *text));
                let why = if motor { "a motor is not a link" } else { "not a body or sheet in the shown tree" };
                dropped.push(format!("{} is not one of the dialog's choices ({why}), so the {} was not preset: choose it in the dialog", doc.node_name(text), entry.params[i].label.to_lowercase()));
            }
            ok.then(|| (i, text.clone()))
        })
        .collect();
    if let Some(form) = doc.ops.form.as_mut().filter(|f| f.op == entry.id) {
        for (i, text) in allowed {
            if let Some(t) = form.texts.get_mut(i) {
                *t = text;
            }
        }
    }
    if !dropped.is_empty() {
        doc.show(Err(format!("{}: {}", entry.label, dropped.join("; "))));
    }
    doc.touch();
    Ok(json!({"opened": entry.id, "form": super::form::form_json(doc), "dropped": dropped}))
}

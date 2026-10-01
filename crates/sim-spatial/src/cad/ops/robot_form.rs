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
use super::resolve::kind_of;
use super::{Env, OpEntry};
use crate::cad::document::CadDocument;
use crate::cad::transform::num;
use crate::ui_kit::form::FieldKind;
use serde_json::{Map, Value, json};

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
/// "joint_types", "sensor_kinds". Empty for an unknown source.
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
        _ => Vec::new(),
    }
}

/// A list with RoboCAD's hints: "revolute: hinge with angle limits (…)".
fn hinted(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(k, hint)| (k.to_string(), format!("{k}: {hint}").trim_end().to_string())).collect()
}

/// RoboCAD's refusal before a dialog opens: "add a motor and a joint
/// first" (ui/app.py:1607-1609), or why the lists are not known yet.
pub(super) fn precheck(entry: &OpEntry, doc: &CadDocument) -> Option<String> {
    if entry.id != "robot.assign_motor" {
        return None;
    }
    if doc.robot.data.summary().is_none() {
        return Some(match doc.robot.data.summary_error() {
            Some(e) => format!("RoboCAD's robot description could not be read: {e}"),
            None => "RoboCAD's robot description is still being read; try again in a moment".to_string(),
        });
    }
    (picks("motors_placed", doc).is_empty() || picks("joints", doc).is_empty()).then(|| "add a motor and a joint first".to_string())
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
        _ => {}
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
/// `findData`): the joint tool's parent, child, pivot and axis. As
/// `cad_state.ops.form` shows it.
pub(in crate::cad) fn open_preset(doc: &mut CadDocument, env: &Env, id: &str, preset: &[(&str, String)]) -> Result<Value, String> {
    let entry = super::entry(id).ok_or_else(|| format!("{id} is not in the catalogue"))?;
    doc.ops.form = None;
    super::form::open_form_with(doc, entry, Some(env));
    let allowed: Vec<(usize, String)> = preset
        .iter()
        .filter_map(|(name, text)| {
            let i = index(entry, name)?;
            let ok = match entry.params[i].kind {
                FieldKind::Pick { source } => picks(source, doc).iter().any(|(k, _)| k == text),
                _ => true,
            };
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
    doc.touch();
    Ok(json!({"opened": entry.id, "form": super::form::form_json(doc)}))
}

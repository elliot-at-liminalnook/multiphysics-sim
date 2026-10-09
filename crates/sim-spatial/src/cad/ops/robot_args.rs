//! The robot dialogs' and tools' calls (`Shape::Robot`), as RoboCAD's
//! handlers make them (ui/app.py:1519-1674, ui/tools.py:1244-1361): the
//! arguments each handler passes, in its order, with the unit conversions
//! its dialog's `values()` does (joint limits typed in degrees sent in
//! radians unless prismatic, cable mass typed in g sent in kg, the power
//! dialog's targets in radians and dimension σ in metres). Where a field
//! is "(world)" or "(none)" the argument is `null`, as the combo box's
//! `None` data.
//!
//! A plan is sent in one edit job (`actions::edit`), each call its own
//! RoboCAD undo step as RoboCAD's handler makes them: the joint dialog's
//! `add_joint` then `set_joint(jid, damping=…)` on the new joint, fix
//! together's one `connect_fixed` per child, toggle ground's one
//! `set_ground` per body (each body's flag read first, as RoboCAD reads
//! `node.robot["ground"]`), the power dialog's battery, control and
//! uncertainty calls. The status line is RoboCAD's message ("joint
//! revolute Leg added", "2 joint(s) inferred …", "battery, control and
//! uncertainty updated"); a created node's name is read back as RoboCAD
//! reads `doc.nodes[id].name`.
use super::OpEntry;
use super::args::Built;
use super::resolve::Resolved;
use crate::app::actions::Call;
use crate::cad::document::{CadDocument, EditDone};
use crate::cad::sync::value;
use crate::cad::transform::{OpCall, num, round6};
use serde_json::{Map, Value, json};
use sim_api::Outcome;
use crate::cad::types::RobotSummary;

/// Which robot handler a catalogue entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RobotCall {
    /// `robot.add_motor`: MotorDialog's values and MotorTool's face point and shaft direction.
    AddMotor,
    /// `robot.add_joint`: the joint tool only picks; its third click opens the joint dialog.
    JointTool,
    /// `robot.joint_dialog`: the joint dialog's OK.
    AddJoint,
    /// `ops.set_joint`: the Edit joint dialog's OK.
    EditJoint,
    Infer,
    AssignMotor,
    Fixed,
    Ground,
    Sensor,
    Cable,
    Power,
    MountMotor,
    ConfigureRobot,
}

/// What a robot run sends and says.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Plan {
    /// The Ops calls, in order (each its own RoboCAD undo step).
    pub calls: Vec<OpCall>,
    /// The edit's label (the header's and the refusal's name for it).
    pub label: String,
    pub done: Done,
}

/// The status line once the calls succeeded, and the calls that depend on
/// RoboCAD's answers.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Done {
    /// RoboCAD's message as built.
    Say(String),
    /// "{what} {name} added": the first call answers the new node's id,
    /// whose name is read back; a joint with `damping` then gets
    /// `set_joint(id, damping=…)` (RoboCAD's second call).
    Added { what: &'static str, damping: Option<f64> },
    /// `infer_joints`: RoboCAD's two messages by how many it made.
    Infer,
    /// Toggle ground: per body, its `robot.ground` flag read, then
    /// `set_ground(id, not flag)`; `names` for the message.
    Ground { ids: Vec<String>, names: String },
}

/// RoboCAD's MotorTool takes the point from a face click; typed instead, both are needed (ours).
pub(crate) const NO_FACE: &str = "Click a face to mount the motor there (or type the mount point and the shaft direction)";

fn required(entry: &OpEntry, name: &str) -> String {
    let label = entry.params.iter().find(|p| p.name == name).map_or(name, |p| p.label);
    format!("{label} ({name}) is required: {} has no default for it", entry.id)
}

/// A non-empty text or pick value.
fn text(values: &Map<String, Value>, name: &str) -> Option<String> {
    values.get(name).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn number(entry: &OpEntry, values: &Map<String, Value>, name: &str) -> Result<f64, String> {
    let v = values.get(name).ok_or_else(|| required(entry, name))?;
    v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| format!("{name} must be a finite number (got {v})"))
}

fn opt_number(entry: &OpEntry, values: &Map<String, Value>, name: &str) -> Result<Option<f64>, String> {
    if values.contains_key(name) { number(entry, values, name).map(Some) } else { Ok(None) }
}

fn point(entry: &OpEntry, values: &Map<String, Value>, name: &str) -> Result<[f64; 3], String> {
    let v = values.get(name).ok_or_else(|| required(entry, name))?;
    match v.as_array().map(|a| a.iter().map(Value::as_f64).collect::<Option<Vec<f64>>>()) {
        Some(Some(a)) if a.len() == 3 && a.iter().all(|x| x.is_finite()) => Ok([a[0], a[1], a[2]]),
        _ => Err(format!("{name} must be three finite numbers [x, y, z] (got {v})")),
    }
}

fn opt_point(entry: &OpEntry, values: &Map<String, Value>, name: &str) -> Result<Option<[f64; 3]>, String> {
    if values.contains_key(name) { point(entry, values, name).map(Some) } else { Ok(None) }
}

fn or_null(v: Option<String>) -> Value {
    v.map_or(Value::Null, Value::from)
}

/// "(x, y, z)" as a label shows it.
fn pt(v: [f64; 3]) -> String {
    format!("({}, {}, {})", num(v[0]), num(v[1]), num(v[2]))
}

fn op(name: &'static str, args: Vec<Value>, kwargs: Map<String, Value>, label: String) -> OpCall {
    OpCall { name, args, kwargs, label }
}

fn kw(pairs: Vec<(&str, Value)>) -> Map<String, Value> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// A joint limit as `JointDialog.values()` sends it: none when empty,
/// else as typed for a prismatic joint, in radians for the others.
fn limit(entry: &OpEntry, values: &Map<String, Value>, name: &str, prismatic: bool) -> Result<Value, String> {
    Ok(match opt_number(entry, values, name)? {
        None => Value::Null,
        Some(v) if prismatic => json!(v),
        Some(v) => json!(v.to_radians()),
    })
}

/// The joint dialog's values as `JointDialog.values()` returns them (less the name).
fn joint_fields(entry: &OpEntry, values: &Map<String, Value>) -> Result<Map<String, Value>, String> {
    let kind = text(values, "type").unwrap_or_else(|| "revolute".to_string());
    let prismatic = kind == "prismatic";
    Ok(kw(vec![
        ("type", json!(kind)),
        ("parent", or_null(text(values, "parent"))),
        ("child", or_null(text(values, "child"))),
        ("pivot", json!(point(entry, values, "pivot")?)),
        ("axis", json!(point(entry, values, "axis")?)),
        ("lower", limit(entry, values, "lower", prismatic)?),
        ("upper", limit(entry, values, "upper", prismatic)?),
        ("motor", or_null(text(values, "motor"))),
        ("gear_ratio", json!(number(entry, values, "gear_ratio")?)),
        ("damping", json!(number(entry, values, "damping")?)),
    ]))
}

/// What `entry` sends (`args::build` for `Shape::Robot`).
pub(super) fn build(entry: &OpEntry, which: RobotCall, r: &Resolved, values: &Map<String, Value>, doc: &CadDocument) -> Result<Built, String> {
    let name_of = |id: &str| doc.node_name(id);
    let plan = match which {
        RobotCall::AddMotor => {
            let spec = text(values, "spec").ok_or_else(|| "Motor: choose a motor from the library".to_string())?;
            let (Some(at), Some(shaft)) = (opt_point(entry, values, "point")?, opt_point(entry, values, "shaft_dir")?) else { return Err(NO_FACE.to_string()) };
            let rotation = number(entry, values, "rotation")?;
            let cut = values.get("cut").and_then(Value::as_bool).unwrap_or(true);
            let mount = text(values, "mount_on");
            let spec_name = doc.robot.data.motor_library(doc).and_then(|lib| lib.get(&spec)).map_or_else(|| spec.clone(), |m| m.name.clone());
            let on = mount.as_deref().map(name_of);
            let label = format!("Add motor {spec_name} at {} mm{}", pt(at), on.as_ref().map_or(String::new(), |n| format!(" on {n}")));
            // ui/tools.py:1284: "motor placed on {clicked body}; …".
            let message = match &on {
                Some(n) => format!("motor placed on {n}; Assign motor… links it to a joint"),
                None => format!("motor placed at {} mm; Assign motor… links it to a joint", pt(at)),
            };
            let args = vec![json!(spec), json!(at.map(round6)), json!(shaft.map(round6)), json!(rotation), or_null(mount), json!(cut), or_null(text(values, "name"))];
            Plan { calls: vec![op("add_motor", args, Map::new(), label.clone())], label, done: Done::Say(message) }
        }
        RobotCall::JointTool => return Err("Robot: add joint picks in the 3D view: click the parent body (Ctrl-click for the world), the child, then a cylindrical or flat face for the axis; the joint dialog (robot.joint_dialog) then adds it".to_string()),
        RobotCall::AddJoint => {
            // ui/app.py:1555-1557: RoboCAD's refusal before anything is sent.
            let Some(child) = text(values, "child") else { return Err("a joint needs a child body".to_string()) };
            let f = joint_fields(entry, values)?;
            let damping = number(entry, values, "damping")?;
            let parent = text(values, "parent");
            let get = |k: &str| f.get(k).cloned().unwrap_or(Value::Null);
            let args = vec![get("type"), get("parent"), json!(child), get("pivot"), get("axis"), get("lower"), get("upper"), get("motor"), get("gear_ratio"), or_null(text(values, "name"))];
            let label = format!("Joint {}: {} → {}", get("type").as_str().unwrap_or("revolute"), parent.as_deref().map_or_else(|| "world".to_string(), name_of), name_of(child.as_str()));
            // `if v["damping"]:` (ui/app.py:1559): the second call only when it is not zero.
            Plan { calls: vec![op("add_joint", args, Map::new(), label.clone())], label, done: Done::Added { what: "joint", damping: (damping != 0.0).then_some(damping) } }
        }
        RobotCall::EditJoint => {
            let jid = r.nodes.first().cloned().ok_or_else(|| entry.refusal.to_string())?;
            let fields = joint_fields(entry, values)?;
            let current = name_of(jid.as_str());
            let mut calls = vec![op("set_joint", vec![json!(jid)], fields, format!("Edit joint {current}"))];
            // ui/app.py:1572-1573: renamed only when a name is given and it changed.
            let name = text(values, "name").filter(|n| *n != current);
            if let Some(n) = &name {
                calls.push(op("rename", vec![json!(jid), json!(n)], Map::new(), format!("Rename {current} to {n}")));
            }
            let shown = name.unwrap_or_else(|| current.clone());
            Plan { calls, label: format!("Edit joint {current}"), done: Done::Say(format!("joint {shown} updated")) }
        }
        RobotCall::Infer => Plan { calls: vec![op("infer_joints", Vec::new(), Map::new(), "Infer joints".into())], label: "Infer joints".into(), done: Done::Infer },
        RobotCall::AssignMotor => {
            let missing = || "add a motor and a joint first".to_string();
            let motor = text(values, "motor").ok_or_else(missing)?;
            let joint = text(values, "joint").ok_or_else(missing)?;
            let gear = number(entry, values, "gear_ratio")?;
            let message = format!("{} now drives {}", name_of(motor.as_str()), name_of(joint.as_str()));
            let label = format!("Attach motor {} to {}", name_of(motor.as_str()), name_of(joint.as_str()));
            Plan { calls: vec![op("attach_motor", vec![json!(joint), json!(motor), json!(gear)], Map::new(), label.clone())], label, done: Done::Say(message) }
        }
        RobotCall::Fixed => {
            let Some((parent, children)) = r.nodes.split_first().filter(|(_, c)| !c.is_empty()) else { return Err(entry.refusal.to_string()) };
            let calls: Vec<OpCall> = children.iter().map(|c| op("connect_fixed", vec![json!(parent), json!(c)], Map::new(), format!("Fix {} to {}", name_of(c.as_str()), name_of(parent.as_str())))).collect();
            let n = children.len();
            let message = format!("{n} bod{} fixed to {}", if n == 1 { "y" } else { "ies" }, name_of(parent.as_str()));
            Plan { label: format!("Fix {n} bod{} to {}", if n == 1 { "y" } else { "ies" }, name_of(parent.as_str())), calls, done: Done::Say(message) }
        }
        RobotCall::Ground => {
            if r.nodes.is_empty() {
                return Err(entry.refusal.to_string());
            }
            let names = r.nodes.iter().map(|n| name_of(n.as_str())).collect::<Vec<_>>().join(", ");
            Plan { calls: Vec::new(), label: format!("Toggle ground on {names}"), done: Done::Ground { ids: r.nodes.clone(), names } }
        }
        RobotCall::Sensor => {
            let kind = text(values, "kind").unwrap_or_else(|| "imu".to_string());
            let body = text(values, "body").ok_or_else(|| required(entry, "body"))?;
            let at = point(entry, values, "point")?;
            let rate = number(entry, values, "rate_hz")?;
            let args = vec![json!(kind), json!(body), json!(at), Value::Null, or_null(text(values, "name")), or_null(text(values, "joint"))];
            let label = format!("Sensor {kind} on {}", name_of(body.as_str()));
            Plan { calls: vec![op("add_sensor", args, kw(vec![("rate_hz", json!(rate))]), label.clone())], label, done: Done::Added { what: "sensor", damping: None } }
        }
        RobotCall::Cable => {
            let from = text(values, "from_body").ok_or_else(|| required(entry, "from_body"))?;
            let to = text(values, "to_body").ok_or_else(|| required(entry, "to_body"))?;
            let (a, b) = (point(entry, values, "from_point")?, point(entry, values, "to_point")?);
            let length = opt_number(entry, values, "length")?;
            // CableDialog.values(): the mass is typed in g and sent in kg.
            let mass = opt_number(entry, values, "mass")?.map(|g| g * 1e-3);
            let args = vec![json!(from), json!(a), json!(to), json!(b), json!(length), json!(mass), Value::Null, or_null(text(values, "name"))];
            let label = format!("Cable {} → {}", name_of(from.as_str()), name_of(to.as_str()));
            Plan { calls: vec![op("add_cable", args, Map::new(), label.clone())], label, done: Done::Added { what: "cable", damping: None } }
        }
        RobotCall::Power => power(entry, values, doc)?,
        RobotCall::MountMotor => {
            let motor = text(values, "motor").ok_or_else(|| required(entry, "motor"))?;
            let body = text(values, "body");
            let label = format!("Mount motor {} on {}", name_of(motor.as_str()), body.as_deref().map_or_else(|| "nothing".to_string(), name_of));
            Plan { calls: vec![op("mount_motor", vec![json!(motor), or_null(body)], Map::new(), label.clone())], label: label.clone(), done: Done::Say(label) }
        }
        RobotCall::ConfigureRobot => {
            // Only the given parts (RoboCAD's defaults are None).
            let kwargs: Map<String, Value> = ["updates", "joints", "groups", "moves"].iter().filter_map(|k| values.get(*k).map(|v| ((*k).to_string(), v.clone()))).collect();
            let label = format!("Configure robot at revision {}", r.revision);
            Plan { calls: vec![op("configure_robot", vec![json!(r.revision)], kwargs, label.clone())], label: label.clone(), done: Done::Say(label) }
        }
    };
    Ok(Built::Robot(plan))
}

/// The revolute, continuous and prismatic joints by name (`robot_power`,
/// ui/app.py:1670), from RoboCAD's robot description; None before it is read.
pub(super) fn motion_joints(doc: &CadDocument) -> Option<Vec<String>> {
    doc.robot.data.summary().map(motion_of)
}

fn motion_of(s: &RobotSummary) -> Vec<String> {
    s.joints.iter().filter(|j| matches!(j.kind.as_str(), "revolute" | "continuous" | "prismatic")).map(|j| j.name.clone()).collect()
}

/// `PowerDialog.apply` (ui/widgets.py:1515-1523): the battery (or none),
/// the control loop with every motion joint's target in radians, and the
/// uncertainty, in that order. A joint the targets leave out keeps its
/// current target as the dialog's prefilled line would send it; a name
/// that is not a motion joint is refused.
fn power(entry: &OpEntry, values: &Map<String, Value>, doc: &CadDocument) -> Result<Plan, String> {
    let cells = values.get("cells").and_then(Value::as_i64).ok_or_else(|| required(entry, "cells"))?;
    let mut calls = Vec::new();
    if cells > 0 {
        let chemistry = text(values, "chemistry").unwrap_or_else(|| "lipo".to_string());
        let capacity = number(entry, values, "capacity_ah")?;
        let k = kw(vec![("cells", json!(cells)), ("chemistry", json!(chemistry)), ("capacity_ah", json!(capacity))]);
        calls.push(op("set_battery", Vec::new(), k, format!("Battery {cells} × {chemistry}, {} Ah", num(capacity))));
    } else {
        calls.push(op("set_robot_setting", vec![json!("battery"), Value::Null], Map::new(), "No battery (motor supply voltage)".into()));
    }
    let given = match values.get("targets") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(m)) => m.clone(),
        Some(other) => return Err(format!("targets must be {{\"joint name\": degrees, …}} (got {other})")),
    };
    let mut targets = Map::new();
    for (name, v) in &given {
        let deg = v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| format!("targets: {name} must be a number of degrees (got {v})"))?;
        targets.insert(name.clone(), json!(deg.to_radians()));
    }
    // `set_control` replaces every target: the ones left out are sent as
    // the dialog's prefilled lines, so the joints and their current targets
    // must be RoboCAD's at the shown revision. Before that read has landed
    // (or when it failed) nothing is sent, rather than resetting them to 0.
    let summary = super::robot_form::description(doc)?;
    let control = super::robot_form::read(doc, "control loop setting", doc.robot.data.bundle.as_ref().map(|b| &b.control))?;
    let joints = motion_of(summary);
    if let Some(bad) = given.keys().find(|k| !joints.contains(k)) {
        return Err(format!("targets: {bad} is not a revolute, continuous or prismatic joint (the joints: {})", if joints.is_empty() { "none".to_string() } else { joints.join(", ") }));
    }
    for j in joints {
        if !targets.contains_key(&j) {
            // The dialog's prefilled line: f"{math.degrees(target):g}", read back.
            let target = control.as_ref().and_then(|c| c.targets.get(&j).copied()).unwrap_or(0.0);
            let deg: f64 = super::robot_form::g(target.to_degrees()).parse().unwrap_or(0.0);
            targets.insert(j, json!(deg.to_radians()));
        }
    }
    let (period, latency) = (number(entry, values, "period_s")?, number(entry, values, "latency_s")?);
    calls.push(op("set_control", Vec::new(), kw(vec![("period_s", json!(period)), ("latency_s", json!(latency)), ("targets", Value::Object(targets))]), format!("Control loop {} s, latency {} s", num(period), num(latency))));
    let (dimension, friction) = (number(entry, values, "dimension")?, number(entry, values, "friction")?);
    calls.push(op("set_uncertainty", Vec::new(), kw(vec![("dimension_m", json!(dimension * 1e-3)), ("friction", json!(friction))]), format!("Uncertainty: dimension σ {} mm, friction σ {}", num(dimension), num(friction))));
    Ok(Plan { calls, label: "Battery, control and uncertainty".into(), done: Done::Say("battery, control and uncertainty updated".into()) })
}

/// Python's truthiness of a JSON value (`not (n.robot or {}).get("ground", False)`).
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// Send `plan` as one edit job (the caller checked `commit_refusal`): its
/// calls in order, then the calls that depend on RoboCAD's answers; the
/// first error stops the rest and says which call it was and how many had
/// run (each its own RoboCAD undo step).
pub(super) fn send(doc: &mut CadDocument, call: &mut Call, plan: Plan) -> Outcome {
    let Plan { calls, label, done } = plan;
    crate::cad::actions::edit(doc, call, label, move |c| {
        let n = calls.len();
        let mut results: Vec<Value> = Vec::with_capacity(n);
        for (i, o) in calls.iter().enumerate() {
            match c.op(o.name, &o.args, &o.kwargs) {
                Ok(r) => results.push(value(&r)),
                Err(mut e) => {
                    if n > 1 {
                        e.message = format!("{} (call {} of {n}: {}; the {i} before it ran, each its own RoboCAD undo step)", e.message, i + 1, o.label);
                    }
                    return Err(e);
                }
            }
        }
        let first_result = results.first().and_then(|r| r.get("result")).cloned().unwrap_or(Value::Null);
        let message = match done {
            Done::Say(m) => m,
            Done::Infer => match first_result.as_array().map_or(0, Vec::len) {
                0 => "no new coaxial hole/pin pairs found (each pair needs a hole in the parent and a matching pin in the child)".to_string(),
                made => format!("{made} joint(s) inferred from coaxial hole/pin pairs"),
            },
            Done::Added { what, damping } => {
                let id = first_result.as_str().map(str::to_string);
                if let (Some(d), Some(id)) = (damping, id.as_deref()) {
                    match c.op("set_joint", &[json!(id)], &kw(vec![("damping", json!(d))])) {
                        Ok(r) => results.push(value(&r)),
                        Err(mut e) => {
                            e.message = format!("{} (the joint {id} was added, its own RoboCAD undo step; setting its damping, the second call, failed)", e.message);
                            return Err(e);
                        }
                    }
                }
                // RoboCAD's `self.doc.nodes[id].name` (a read; the id when it fails).
                let name = id.as_deref().map(|id| c.node(id).map_or_else(|_| id.to_string(), |d| d.summary.name)).unwrap_or_default();
                format!("{what} {name} added")
            }
            Done::Ground { ids, names } => {
                for (i, id) in ids.iter().enumerate() {
                    let step = |mut e: crate::cad::types::CadError| {
                        if ids.len() > 1 {
                            e.message = format!("{} (body {} of {}; the {i} before it were toggled, each its own RoboCAD undo step)", e.message, i + 1, ids.len());
                        }
                        e
                    };
                    let on = truthy(c.node(id).map_err(step)?.robot.as_ref().and_then(|r| r.get("ground")));
                    results.push(value(&c.op("set_ground", &[json!(id), json!(!on)], &Map::new()).map_err(step)?));
                }
                format!("ground toggled on {names}")
            }
        };
        let result = if results.len() == 1 { results.pop().unwrap_or(Value::Null) } else { Value::Array(results) };
        Ok(EditDone { message, result })
    })
}

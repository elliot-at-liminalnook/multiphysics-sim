//! RoboCAD's robot click tools, "Robot: validate" and the motor library
//! (cad-physical-inspect; ui/tools.py:1244-1361, ui/app.py:1635-1647).
//! The dialogs themselves are op-catalogue forms (`ops::catalogue::robot`).
//!
//! - **Click tools** (`Flow::RobotPick`, started by `robot.add_motor` and
//!   `robot.add_joint`): [`click`] turns a left press in the 3D view into
//!   `CadRobot {op: pick, item, world, picked_at}` (a face through
//!   `CadMeshes::face_at` at the shown revision, a body by the first
//!   unlocked hit), noting the hit point and, for the motor tool,
//!   RoboCAD's vertex, midpoint, centre or endpoint snap there. [`handle`]
//!   applies a pick, from a click or REST alike: a face item must carry the
//!   revision it was read at and that must be the shown one; the face's
//!   kind, normal and axis come from the topology at the shown revision
//!   (`CadTopology`). A pick from REST has no hit point: the face's own
//!   point (`FaceInfo::point`, else its centroid) stands for it.
//!   - The motor tool (`MotorTool.press`): the point is the snap, else the
//!     hit; the normal is the face's (on a cylinder, radial from its axis
//!     through the point); the shaft points into the body (`-normal`); the
//!     motor is mounted on the dialog's body, else the clicked one. The
//!     click is one `CadRun robot.add_motor` with the form's values and
//!     the pick's revision (refused by name when RoboCAD's document changed
//!     since); the tool stays active (Escape ends it).
//!   - The joint tool (`JointTool.press`): the parent body (Ctrl-click: the
//!     world), the child (not the parent), then a face: a cylinder gives
//!     its axis and the click projected onto it, a flat face its normal
//!     through the click. Then the tool ends and the joint dialog opens
//!     preset with them (`ops::open_preset`, RoboCAD's `_robot_joint_from`).
//!     Each step's prompt is RoboCAD's status text.
//! - **Validate** (`robot_validate`): RoboCAD's description at the current
//!   revision (`data`): "robot valid: N bodies, N joints, N DoF" on the
//!   status line, or RoboCAD's warning box as the status line naming each
//!   "[severity] message" (the Robot panel lists the issues). A REST
//!   caller waits for the description at the current revision; a click
//!   before it is read shows the result once it lands ([`settle`]).
//! - **Motor library** (`robot_motor_library`): a floating kit panel
//!   (`library`) listing `GET /motors` as RoboCAD's text rows.
#[path = "tools_click.rs"]
mod click;
#[path = "tools_library.rs"]
mod library;
#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;

use super::{RobotArgs, RobotOp};
use crate::app::actions::Call;
use crate::app::{ViewerMode, ViewerSet};
use crate::cad::actions::{CadAction, Cx};
use crate::cad::document::{CadDocument, SelectMode};
use crate::cad::ops::{self, Flow, RobotTool};
use bevy::prelude::*;
use serde_json::{Map, Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::{FaceInfo, RobotSummary, SelectionItem};

/// The joint tool's picks so far (`JointTool.stage`, `parent`, `child`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct JointPicks {
    /// 0: the parent next; 1: the child; 2: the axis face.
    pub stage: u8,
    /// None: the world (or not picked yet).
    pub parent: Option<String>,
    pub child: Option<String>,
}

/// A 3D click's hit, noted by [`click`] for the pick it writes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Click {
    pub item: SelectionItem,
    pub picked_at: u64,
    /// The hit point (mm, RoboCAD's frame: `result["world"]`).
    pub point: [f64; 3],
    /// RoboCAD's snap there when it is a vertex, midpoint, centre or endpoint.
    pub snap: Option<[f64; 3]>,
}

/// The last "Robot: validate", as the status line showed it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Validation {
    pub revision: u64,
    pub valid: bool,
    pub message: String,
    pub issues: Vec<String>,
}

/// The tools' state on the document (reset with it).
#[derive(Default)]
pub struct ToolsState {
    pub(crate) joint: JointPicks,
    pub(crate) click: Option<Click>,
    /// The Add motor dialog's last motor, rotation and cut (RoboCAD's
    /// `self.last_motor`), as drafts by parameter name (`ops::robot_form::seed`).
    pub(crate) last_motor: Option<Vec<(String, String)>>,
    /// The motor library panel is shown.
    pub(crate) library_open: bool,
    pub(crate) validation: Option<Validation>,
    /// A click's validate waits for the description of this document generation.
    pub(crate) validate_asked: Option<u64>,
}
impl ToolsState {
    /// The click tools start over (a tool started, cancelled or ended).
    pub(crate) fn reset_picks(&mut self) {
        self.joint = JointPicks::default();
        self.click = None;
    }
}

/// The active robot click tool: its catalogue id and kind.
pub(crate) fn active_tool(doc: &CadDocument) -> Option<(&'static str, RobotTool)> {
    let id = doc.ops.active?;
    match ops::entry(id)?.flow {
        Flow::RobotPick(tool) => Some((id, tool)),
        _ => None,
    }
}

/// `CadRobot` validate, library and pick (`robot::handle`).
pub(in crate::cad) fn handle(args: &RobotArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    match args.op {
        RobotOp::Validate => validate(call, cx.doc),
        RobotOp::Library => {
            let tools = &mut cx.doc.robot.tools;
            let open = args.open.unwrap_or(!tools.library_open);
            if tools.library_open != open {
                tools.library_open = open;
                cx.doc.touch();
            }
            Outcome::Done(Ok(json!({"library_open": open, "library": library::json(cx.doc)})))
        }
        RobotOp::Pick => pick(args, call, cx),
        RobotOp::Panel | RobotOp::Refresh => Outcome::Done(Err("not a robot tools action".into())),
    }
}

/// RoboCAD's validation of `summary` (ui/app.py:1635-1642): valid and the
/// status text, or the "[severity] message" lines.
pub(crate) fn verdict(summary: &RobotSummary) -> (bool, String, Vec<String>) {
    if summary.issues.is_empty() {
        let mobility = summary.dof.map_or_else(|| "closed-loop mobility requires constraint analysis".to_string(), |d| format!("{d} DoF"));
        (true, format!("robot valid: {} bodies, {} joints, {mobility}", summary.links, summary.joints.len()), Vec::new())
    } else {
        let issues: Vec<String> = summary.issues.iter().map(|i| format!("[{}] {}", i.severity, i.message)).collect();
        (false, format!("Robot validation: {}", issues.join("; ")), issues)
    }
}

/// The verdict at the current revision once the description is read:
/// shown on the status line and kept; None while it is being read.
fn settle_now(doc: &mut CadDocument) -> Option<Result<Value, String>> {
    let data = &doc.robot.data;
    if !data.current(doc) {
        return None;
    }
    let revision = doc.shown_revision();
    let outcome = match (data.summary(), data.summary_error()) {
        (Some(summary), _) => {
            let (valid, message, issues) = verdict(summary);
            Ok(Validation { revision, valid, message, issues })
        }
        (None, Some(e)) => Err(format!("Robot: validate: RoboCAD's robot description could not be read: {e}")),
        (None, None) => return None,
    };
    Some(match outcome {
        Ok(v) => {
            doc.show(if v.valid { Ok(v.message.clone()) } else { Err(v.message.clone()) });
            let answer = json!({"valid": v.valid, "message": v.message, "issues": v.issues, "revision": v.revision});
            doc.robot.tools.validation = Some(v);
            doc.touch();
            Ok(answer)
        }
        Err(e) => {
            doc.show(Err(e.clone()));
            Err(e)
        }
    })
}

/// "Robot: validate": the verdict now, or (REST) a wait for the
/// description at the current revision, or (a click) the verdict once it lands.
fn validate(call: &mut Call, doc: &mut CadDocument) -> Outcome {
    let generation = doc.generation;
    if let Some(g) = call.continuation.get("robot_validate").and_then(Value::as_u64) {
        if g != generation {
            return Outcome::Done(Err("the CAD document was replaced or reconnected while waiting for RoboCAD's robot description".into()));
        }
        if call.cancelled {
            return Outcome::Done(Err("cancelled waiting for RoboCAD's robot description".into()));
        }
    }
    if let Some(done) = settle_now(doc) {
        doc.robot.tools.validate_asked = None;
        return Outcome::Done(done);
    }
    if !doc.connected() {
        return Outcome::Done(Err("Robot: validate: not connected to RoboCAD".into()));
    }
    if call.rest() {
        *call.continuation = json!({"robot_validate": generation});
        return Outcome::Pending;
    }
    doc.robot.tools.validate_asked = Some(generation);
    doc.show(Ok("Robot: validate: reading RoboCAD's robot description…".to_string()));
    Outcome::Done(Ok(json!({"waiting": "the robot description at the current revision"})))
}

/// JobResults (after the description's reads): a click's validate shows
/// its verdict once the description at the current revision has landed.
fn settle(doc: Option<ResMut<CadDocument>>) {
    let Some(mut doc) = doc else { return };
    let Some(g) = doc.robot.tools.validate_asked else { return };
    if g != doc.generation {
        doc.robot.tools.validate_asked = None;
        return;
    }
    // Read through `Deref` first: a `ResMut` deref would mark the document changed.
    if !doc.robot.data.current(&doc) {
        return;
    }
    if settle_now(&mut doc).is_some() {
        doc.robot.tools.validate_asked = None;
    }
}

/// Node `item`'s face at the shown revision (`CadTopology`), or why not.
fn face_info(cx: &Cx, item: &SelectionItem) -> Result<FaceInfo, String> {
    let name = cx.doc.node_name(&item.0);
    let Some(topology) = cx.topology.as_deref() else { return Err("the faces are not available without CAD mode's 3D view".into()) };
    let Some(t) = topology.get(&item.0) else {
        return Err(match topology.error(&item.0) {
            Some(e) => format!("the faces of {name} could not be read: {e}"),
            None => format!("the faces of {name} are still being read from RoboCAD; click again in a moment"),
        });
    };
    t.faces.iter().find(|f| f.index == item.2).cloned().ok_or_else(|| format!("{name} has no face {} at the shown revision ({} faces)", item.2, t.faces.len()))
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
    a.map(|x| x * k)
}
/// RoboCAD's `v_unit` (None for a zero vector, which it cannot normalise).
fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let n = dot(a, a).sqrt();
    (n > 1e-12 && n.is_finite()).then(|| scale(a, 1.0 / n))
}

/// The axis of a cylindrical face (`face.kind == CYLINDER` with its axis point and direction).
fn cylinder_axis(face: &FaceInfo) -> Option<([f64; 3], [f64; 3])> {
    match (face.kind.as_str(), face.axis_point, face.axis_dir) {
        ("cylinder", Some(p), Some(d)) => Some((p, unit(d)?)),
        _ => None,
    }
}

/// `MotorTool.press` (ui/tools.py:1266-1279): the mount point and the
/// shaft direction (into the body) for a click at `point` on `face`.
pub(crate) fn motor_mount(face: &FaceInfo, point: [f64; 3]) -> Result<[f64; 3], String> {
    let n = match cylinder_axis(face) {
        // On a curved face the local normal is radial from the axis.
        Some((p, a)) => {
            let d = sub(point, p);
            unit(sub(d, scale(a, dot(d, a))))
        }
        None => face.normal.and_then(unit),
    };
    n.map(|n| scale(n, -1.0)).ok_or_else(|| "the clicked face has no normal there to mount a motor along".to_string())
}

/// `JointTool.press`'s third click (ui/tools.py:1345-1353): a cylinder's
/// axis with the click projected onto it, else the face's normal through
/// the click. (pivot, axis).
pub(crate) fn joint_axis(face: &FaceInfo, point: [f64; 3]) -> Result<([f64; 3], [f64; 3]), String> {
    if let Some((p, a)) = cylinder_axis(face) {
        let d = sub(point, p);
        let k = dot(d, a);
        return Ok(([p[0] + a[0] * k, p[1] + a[1] * k, p[2] + a[2] * k], a));
    }
    face.normal.and_then(unit).map(|n| (point, n)).ok_or_else(|| "the clicked face has no normal to give the joint axis".to_string())
}

/// The point a pick stands for: the click's hit (and snap), or, for a pick
/// with no click (REST), the face's own point or centroid.
fn picked_point(cx: &mut Cx, item: &SelectionItem, picked_at: u64, face: &FaceInfo) -> Result<(Option<Click>, [f64; 3]), String> {
    if let Some(c) = cx.doc.robot.tools.click.take().filter(|c| c.item == *item && c.picked_at == picked_at) {
        let at = c.point;
        return Ok((Some(c), at));
    }
    face.point.or(face.centroid).map(|p| (None, p)).ok_or_else(|| "the face has no point to pick at: click it in the 3D view".to_string())
}

/// A pick's item names a node of the shown tree, and a face item the
/// shown revision's numbering: it carries the revision it was read at
/// (`picked_at`), which must be the shown one.
pub(crate) fn check_pick(doc: &CadDocument, item: &SelectionItem, picked_at: Option<u64>) -> Result<(), String> {
    if !doc.has_node(&item.0) {
        return Err(format!("no node {} in the shown tree", item.0));
    }
    let shown = doc.shown_revision();
    match (item.1.as_str(), picked_at) {
        ("face", None) => Err("pass picked_at: the RoboCAD revision the face index was read at".into()),
        ("face", Some(r)) if r != shown => Err(format!("the face was picked at revision {r}; RoboCAD's faces may be renumbered since (now {shown}): click again")),
        _ => Ok(()),
    }
}

/// One pick of the active click tool (see the module doc).
fn pick(args: &RobotArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    let Some((id, tool)) = active_tool(cx.doc) else { return Outcome::Done(Err("no robot click tool is active: start Robot: add motor from library… or Robot: add joint first".into())) };
    if let Some(item) = &args.item
        && let Err(e) = check_pick(cx.doc, item, args.picked_at)
    {
        return Outcome::Done(Err(e));
    }
    match tool {
        RobotTool::Motor => motor_pick(id, args, call, cx),
        RobotTool::Joint => joint_pick(args, cx),
    }
}

/// The motor tool's face click: one `CadRun robot.add_motor`.
fn motor_pick(id: &'static str, args: &RobotArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    // RoboCAD returns on anything but a face.
    let Some(item) = args.item.as_ref().filter(|i| i.1 == "face") else { return Outcome::Done(Ok(json!({"ignored": "the motor tool takes a face"}))) };
    let picked_at = args.picked_at.unwrap_or_else(|| cx.doc.shown_revision());
    let face = match face_info(cx, item) {
        Ok(f) => f,
        Err(e) => return Outcome::Done(Err(e)),
    };
    let (click, hit) = match picked_point(cx, item, picked_at, &face) {
        Ok(p) => p,
        Err(e) => return Outcome::Done(Err(e)),
    };
    // RoboCAD's `s.point if s.kind in ("vertex", "midpoint", "center", "endpoint") else result["world"]`.
    let point = click.as_ref().and_then(|c| c.snap).unwrap_or(hit);
    let shaft = match motor_mount(&face, point) {
        Ok(s) => s,
        Err(e) => return Outcome::Done(Err(e)),
    };
    let Some(entry) = ops::entry(id) else { return Outcome::Done(Err(format!("{id} is not in the catalogue"))) };
    // The form's values as its OK sends them (`ops::form::submit`).
    let texts: Vec<String> = match &cx.doc.ops.form {
        Some(f) if f.op == id => f.texts.clone(),
        _ => entry.params.iter().map(|p| p.default.to_string()).collect(),
    };
    let text = |name: &str| entry.params.iter().position(|p| p.name == name).and_then(|i| texts.get(i)).cloned().unwrap_or_default();
    let mut params: Map<String, Value> = entry.params.iter().zip(&texts).map(|(p, t)| (p.name.to_string(), Value::String(t.clone()))).collect();
    params.insert("point".into(), json!(point));
    params.insert("shaft_dir".into(), json!(shaft));
    // `p.get("mount_on") or nid`: the dialog's body, else the clicked one.
    if text("mount_on").trim().is_empty() {
        params.insert("mount_on".into(), Value::String(item.0.clone()));
    }
    cx.doc.robot.tools.last_motor = Some(["spec", "rotation", "cut"].into_iter().map(|n| (n.to_string(), text(n))).collect());
    let outcome = ops::run_entry(id, &params, Some(picked_at), call, cx);
    // A refusal shows in the tool's form too, as its OK's does.
    if let Outcome::Done(Err(e)) = &outcome
        && let Some(f) = cx.doc.ops.form.as_mut().filter(|f| f.op == id)
    {
        f.error = Some(e.clone());
        cx.doc.touch();
    }
    outcome
}

/// The joint tool's clicks: parent, child, then the axis face.
fn joint_pick(args: &RobotArgs, cx: &mut Cx) -> Outcome {
    let picks = cx.doc.robot.tools.joint.clone();
    let ignored = |why: &str| Outcome::Done(Ok(json!({"ignored": why, "stage": picks.stage})));
    match picks.stage {
        0 => {
            // Ctrl-click: the world; else the clicked body; empty space does nothing.
            let parent = match (&args.item, args.world) {
                (_, true) => None,
                (Some(item), false) => Some(item.0.clone()),
                (None, false) => return ignored("click the parent body, or Ctrl-click for the world"),
            };
            let shown = parent.as_deref().map_or_else(|| "world".to_string(), |p| cx.doc.node_name(p));
            cx.doc.robot.tools.joint = JointPicks { stage: 1, parent: parent.clone(), child: None };
            cx.doc.show(Ok(format!("Joint: parent = {shown}; now click the child body")));
            cx.doc.touch();
            Outcome::Done(Ok(json!({"stage": 1, "parent": parent})))
        }
        1 => {
            let Some(item) = args.item.as_ref().filter(|i| Some(&i.0) != picks.parent.as_ref()) else { return ignored("click the child body (not the parent)") };
            let child = item.0.clone();
            cx.doc.robot.tools.joint.stage = 2;
            cx.doc.robot.tools.joint.child = Some(child.clone());
            // RoboCAD's `vp.selection_mode = "face"` for the axis face.
            if cx.doc.select_mode != SelectMode::Face {
                cx.doc.select_mode = SelectMode::Face;
                crate::cad::selection::publish(cx.doc, cx.shared.view());
            }
            let name = cx.doc.node_name(&child);
            cx.doc.show(Ok(format!("Joint: child = {name}; click a cylindrical face (axis) or a flat face (normal) for the joint axis")));
            cx.doc.touch();
            Outcome::Done(Ok(json!({"stage": 2, "parent": picks.parent, "child": child})))
        }
        _ => {
            let Some(item) = args.item.as_ref().filter(|i| i.1 == "face") else { return ignored("click a cylindrical or flat face for the axis") };
            let picked_at = args.picked_at.unwrap_or_else(|| cx.doc.shown_revision());
            let face = match face_info(cx, item) {
                Ok(f) => f,
                Err(e) => return Outcome::Done(Err(e)),
            };
            // `result["world"]`: the hit itself (RoboCAD does not snap here).
            let point = match picked_point(cx, item, picked_at, &face) {
                Ok((_, p)) => p,
                Err(e) => return Outcome::Done(Err(e)),
            };
            let (pivot, axis) = match joint_axis(&face, point) {
                Ok(v) => v,
                Err(e) => return Outcome::Done(Err(e)),
            };
            // `done(preset)`: Select becomes the tool, then the joint dialog opens preset.
            let selection = cx.shared.items();
            let (doc, env) = cx.split(&selection);
            doc.ops.active = None;
            doc.ops.form = None;
            doc.robot.tools.reset_picks();
            let three = |v: [f64; 3]| v.map(ops::g).join(", ");
            let preset = [("parent", picks.parent.clone().unwrap_or_default()), ("child", picks.child.clone().unwrap_or_default()), ("pivot", three(pivot)), ("axis", three(axis))];
            let opened = ops::open_preset(doc, &env, "robot.joint_dialog", &preset);
            if opened.is_ok() {
                doc.show(Ok("Joint: check the joint dialog and press OK to add it".to_string()));
            }
            Outcome::Done(opened)
        }
    }
}

/// `cad_state.robot.tools`.
pub(crate) fn state_json(doc: &CadDocument) -> Value {
    let t = &doc.robot.tools;
    json!({
        "active": active_tool(doc).map(|(id, _)| id),
        "joint": {"stage": t.joint.stage, "parent": t.joint.parent, "child": t.joint.child},
        "last_motor": t.last_motor,
        "library_open": t.library_open,
        "library": library::json(doc),
        "validation": t.validation.as_ref().map(|v| json!({"revision": v.revision, "valid": v.valid, "message": v.message, "issues": v.issues})),
        "validate_waiting": t.validate_asked.is_some(),
    })
}

/// The tools' `system_ui` controls: `cad:robot:validate`, `cad:robot:library`.
pub(crate) fn controls(doc: &CadDocument) -> Vec<(String, String, CadAction, Result<(), String>)> {
    let connected = if doc.connected() { Ok(()) } else { Err("not connected to RoboCAD".to_string()) };
    let open = doc.robot.tools.library_open;
    vec![
        ("cad:robot:validate".into(), "Robot: validate".into(), RobotArgs::of(RobotOp::Validate, None), connected),
        ("cad:robot:library".into(), if open { "Close the motor library".into() } else { "Robot: motor library…".into() }, RobotArgs::of(RobotOp::Library, Some(!open)), Ok(())),
    ]
}

/// CadPlugin: the click tools' 3D clicks, a click's validate, the library panel.
pub(crate) fn build(app: &mut App) {
    app.add_systems(Update, click::click.after(crate::app::actions::serve).in_set(ViewerSet::Input).run_if(in_state(ViewerMode::Cad)))
        .add_systems(Update, settle.after(super::data::sync).in_set(ViewerSet::JobResults).run_if(in_state(ViewerMode::Cad)))
        .add_systems(Update, library::draw.in_set(ViewerSet::Present).run_if(in_state(ViewerMode::Cad)));
}

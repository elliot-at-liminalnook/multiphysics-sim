//! The tools' commits: each is exactly one RoboCAD Ops call
//! (`POST /ops/{name}`) through `actions::edit`, refused with nothing sent
//! when `CadDocument::commit_refusal` names a reason. The pure builders
//! ([`transform_call`], [`push_pull_call`], [`offset_call`],
//! [`dimension_call`]) give each call's exact arguments, as RoboCAD's
//! `ArgConverter` reads them (cad/robocad/api.py:150-240: a face is
//! `{"node": id, "face": i}`, vectors are `[x, y, z]`, a measurement is
//! `{"kind", "points", "value", "label"}`), and the label its header and
//! RoboCAD's history show. Locked nodes: a transform leaves them out (as
//! RoboCAD's `Ops.transform` skips them) and is refused by name when every
//! node is locked; a face edit of a locked node is refused by name, as
//! RoboCAD's `Ops._edit` refuses it.
use super::push_pull::resolve;
use super::{Field, FieldCommit, Phase, PushTarget, fa, face_target, fields, fl, mm, num, numeric_axis, pivot};
use crate::app::actions::Call;
use crate::cad::actions::{CadAction, Cx, Dimension, MeasurePick};
use crate::cad::document::{CadDocument, CadTool, EditDone};
use crate::cad::selection::CadItems;
use crate::cad::sync::value;
use crate::cad::topology::CadTopology;
use serde_json::{Map, Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::SelectionItem;

/// One Ops call: its name, positional args, keyword args and the label the
/// header and RoboCAD's history name it by.
#[derive(Clone, Debug, PartialEq)]
pub struct OpCall {
    pub name: &'static str,
    pub args: Vec<Value>,
    pub kwargs: Map<String, Value>,
    pub label: String,
}

/// A face as RoboCAD's `ArgConverter.face` takes it.
pub fn face_ref(node: &str, face: i64) -> Value {
    json!({"node": node, "face": face})
}

fn finite(name: &str, v: f64) -> Result<f64, String> {
    if v.is_finite() { Ok(v) } else { Err(format!("{name} must be a finite number (got {v})")) }
}

fn finite3(name: &str, v: [f64; 3]) -> Result<[f64; 3], String> {
    for x in v {
        finite(name, x)?;
    }
    Ok(v)
}

fn subject(names: &[String]) -> String {
    match names {
        [one] => one.clone(),
        many => format!("{} nodes", many.len()),
    }
}

fn point(v: [f64; 3]) -> String {
    format!("({}, {}, {})", num(v[0]), num(v[1]), num(v[2]))
}

/// `Ops.transform(ids, translation, axis, angle_deg, center, scale)`
/// (commands.py:648, uniform scale): only the given keywords are sent.
pub fn transform_call(ids: &[String], names: &[String], translation: Option<[f64; 3]>, axis: Option<[f64; 3]>, angle_deg: Option<f64>, center: Option<[f64; 3]>, scale: Option<f64>) -> Result<OpCall, String> {
    if ids.is_empty() {
        return Err("nothing is selected: select the nodes to transform (or pass ids)".into());
    }
    if translation.is_none() && axis.is_none() && angle_deg.is_none() && scale.is_none() {
        return Err("a transform needs translation [dx, dy, dz], or axis [x, y, z] with angle_deg, or scale".into());
    }
    if axis.is_some() != angle_deg.is_some() {
        return Err("a rotation needs both axis [x, y, z] and angle_deg".into());
    }
    let who = subject(names);
    let mut kwargs = Map::new();
    let mut parts = Vec::new();
    let at = match center {
        Some(c) => Some(point(finite3("center", c)?)),
        None => None,
    };
    if let Some(t) = translation {
        let t = finite3("translation", t)?;
        kwargs.insert("translation".into(), json!(t));
        parts.push(format!("Move {who} by {} mm", point(t)));
    }
    if let (Some(a), Some(angle)) = (axis, angle_deg) {
        let a = finite3("axis", a)?;
        let angle = finite("angle_deg", angle)?;
        if a.iter().all(|x| x.abs() < 1e-12) {
            return Err("axis must not be the zero vector".into());
        }
        kwargs.insert("axis".into(), json!(a));
        kwargs.insert("angle_deg".into(), json!(angle));
        parts.push(format!("Rotate {who} by {} about axis {}{}", fa(angle), point(a), at.as_ref().map_or(String::new(), |c| format!(" through {c}"))));
    }
    if let Some(s) = scale {
        let s = finite("scale", s)?;
        if s <= 0.0 {
            return Err(format!("scale must be a positive uniform factor (got {s})"));
        }
        kwargs.insert("scale".into(), json!(s));
        parts.push(format!("Scale {who} by ×{}{}", num(s), at.as_ref().map_or(String::new(), |c| format!(" about {c}"))));
    }
    if let Some(c) = center {
        kwargs.insert("center".into(), json!(c));
    }
    Ok(OpCall { name: "transform", args: vec![json!(ids)], kwargs, label: parts.join(", then ") })
}

/// `Ops.push_pull(node_id, face, distance)` (commands.py:515).
pub fn push_pull_call(node: &str, name: &str, face: i64, distance: f64) -> Result<OpCall, String> {
    if face < 0 {
        return Err(format!("a face index is 0 or more (got {face})"));
    }
    let distance = finite("distance", distance)?;
    Ok(OpCall { name: "push_pull", args: vec![json!(node), face_ref(node, face), json!(distance)], kwargs: Map::new(), label: format!("Push/pull {name} face {face} by {}", fl(distance)) })
}

/// `Ops.offset_faces(node_id, faces, distance)` (commands.py:518).
pub fn offset_call(node: &str, name: &str, faces: &[i64], distance: f64) -> Result<OpCall, String> {
    if faces.is_empty() {
        return Err("an offset needs at least one face".into());
    }
    for (i, f) in faces.iter().enumerate() {
        if *f < 0 {
            return Err(format!("a face index is 0 or more (got {f})"));
        }
        if faces[..i].contains(f) {
            return Err(format!("face {f} is listed twice"));
        }
    }
    let distance = finite("distance", distance)?;
    let label = match faces {
        [one] => format!("Offset {name} face {one} by {}", fl(distance)),
        many => format!("Offset {} faces of {name} by {}", many.len(), fl(distance)),
    };
    let refs: Vec<Value> = faces.iter().map(|f| face_ref(node, *f)).collect();
    Ok(OpCall { name: "offset_faces", args: vec![json!(node), Value::Array(refs), json!(distance)], kwargs: Map::new(), label })
}

/// `Ops.set_diameter` / `set_distance` / `set_angle` (commands.py:534-552).
pub fn dimension_call(node: &str, name: &str, dimension: Dimension, faces: &[i64], value: f64) -> Result<OpCall, String> {
    let value = finite("value", value)?;
    if faces.iter().any(|f| *f < 0) {
        return Err("a face index is 0 or more".into());
    }
    let pair = |what: &str| -> Result<(i64, i64), String> {
        match faces {
            [a, b] if a != b => Ok((*a, *b)),
            _ => Err(format!("{what} takes two different faces (faces [a, b], b moves); got {faces:?}")),
        }
    };
    match dimension {
        Dimension::Diameter => {
            let [f] = faces else { return Err(format!("a diameter takes one cylindrical face (faces [f]); got {faces:?}")) };
            if value <= 0.0 {
                return Err(format!("a diameter must be positive (got {value})"));
            }
            Ok(OpCall { name: "set_diameter", args: vec![json!(node), face_ref(node, *f), json!(value)], kwargs: Map::new(), label: format!("Set Ø of {name} face {f} to {}", fl(value)) })
        }
        Dimension::Distance => {
            let (a, b) = pair("a distance")?;
            if value < 0.0 {
                return Err(format!("a distance must not be negative (got {value})"));
            }
            Ok(OpCall { name: "set_distance", args: vec![json!(node), face_ref(node, a), face_ref(node, b), json!(value)], kwargs: Map::new(), label: format!("Set distance of {name} faces {a}–{b} to {}", fl(value)) })
        }
        Dimension::Angle => {
            let (a, b) = pair("an angle")?;
            Ok(OpCall { name: "set_angle", args: vec![json!(node), face_ref(node, a), face_ref(node, b), json!(value)], kwargs: Map::new(), label: format!("Set angle of {name} faces {a}–{b} to {}", fa(value)) })
        }
    }
}

fn check_node(doc: &CadDocument, node: &str) -> Result<(), String> {
    if doc.has_node(node) { Ok(()) } else { Err(format!("no node {node} in the shown tree")) }
}

/// Whether node `id` is locked in the shown tree.
fn locked(doc: &CadDocument, id: &str) -> bool {
    doc.doc.as_ref().and_then(|d| d.nodes.iter().find(|n| n.id == id)).is_some_and(|n| n.locked)
}

/// A face edit of a locked node is refused by name with nothing sent, in
/// RoboCAD's words (`Ops._edit`, commands.py:285-286, raises "{name} is locked").
fn check_unlocked(doc: &CadDocument, node: &str) -> Result<(), String> {
    if locked(doc, node) { Err(format!("{} is locked (unlock it first); nothing was sent", doc.node_name(node))) } else { Ok(()) }
}

/// With the node's topology loaded, every face must exist at the shown
/// revision (else RoboCAD checks the index itself).
fn check_faces(doc: &CadDocument, topology: Option<&CadTopology>, node: &str, faces: &[i64]) -> Result<(), String> {
    let Some(t) = topology.and_then(|t| t.get(node)) else { return Ok(()) };
    match faces.iter().find(|f| !t.faces.iter().any(|x| x.index == **f)) {
        Some(f) => Err(format!("{} has no face {f} at the shown revision ({} faces)", doc.node_name(node), t.faces.len())),
        None => Ok(()),
    }
}

/// The Ops call a commit action sends (`selection`: the shared selection's
/// CAD items, the nodes a transform without `ids` moves).
pub(super) fn op_for(doc: &CadDocument, selection: &[SelectionItem], topology: Option<&CadTopology>, action: &CadAction) -> Result<OpCall, String> {
    match action {
        CadAction::CadTransform { ids, translation, axis, angle_deg, center, scale, .. } => {
            let ids = ids.clone().unwrap_or_else(|| selection.nodes());
            for id in &ids {
                check_node(doc, id)?;
            }
            // RoboCAD's `Ops.transform` skips locked nodes (commands.py:661-662)
            // after its component checks (650-655), which see every id, so with a
            // component all are sent. Else left out here: the label names only what moves, and all locked is refused.
            let component = ids.iter().any(|id| doc.doc.as_ref().and_then(|d| d.nodes.iter().find(|n| n.id == *id)).is_some_and(|n| n.component_member.is_some() || n.component_instance.is_some()));
            let (held, ids): (Vec<String>, Vec<String>) = ids.into_iter().partition(|id| !component && locked(doc, id));
            if ids.is_empty() && !held.is_empty() {
                let names: Vec<String> = held.iter().map(|id| doc.node_name(id)).collect();
                let verb = if names.len() == 1 { "is" } else { "are" };
                return Err(format!("{} {verb} locked; RoboCAD's transform skips locked nodes, so nothing was sent (unlock first)", names.join(", ")));
            }
            let names: Vec<String> = ids.iter().map(|id| doc.node_name(id)).collect();
            transform_call(&ids, &names, *translation, *axis, *angle_deg, *center, *scale)
        }
        CadAction::CadPushPull { node, face, distance, .. } => {
            check_node(doc, node)?;
            check_unlocked(doc, node)?;
            check_faces(doc, topology, node, &[*face])?;
            push_pull_call(node, &doc.node_name(node), *face, *distance)
        }
        CadAction::CadOffsetFaces { node, faces, distance, .. } => {
            check_node(doc, node)?;
            check_unlocked(doc, node)?;
            check_faces(doc, topology, node, faces)?;
            offset_call(node, &doc.node_name(node), faces, *distance)
        }
        CadAction::CadSetDimension { node, dimension, faces, value, .. } => {
            check_node(doc, node)?;
            check_unlocked(doc, node)?;
            check_faces(doc, topology, node, faces)?;
            dimension_call(node, &doc.node_name(node), *dimension, faces, *value)
        }
        _ => Err("not a commit".into()),
    }
}

/// Send one Ops call through the one edit path. `extra` is added to the
/// answer (a kept measurement).
fn send(doc: &mut CadDocument, call: &mut Call, op: OpCall, extra: Option<(&'static str, Value)>) -> Outcome {
    let OpCall { name, args, kwargs, label } = op;
    let message = label.clone();
    // The transform runs in process (`sim_cad::nodes::transform`, RoboCAD's
    // `Ops.transform`: bodies baked about their centroid or `center`).
    if name == "transform" {
        let ids: Vec<String> = args.first().and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
        let get3 = |k: &str| kwargs.get(k).and_then(|v| serde_json::from_value::<[f64; 3]>(v.clone()).ok());
        let (translation, axis, center) = (get3("translation").unwrap_or([0.; 3]), get3("axis"), get3("center"));
        let angle = kwargs.get("angle_deg").and_then(Value::as_f64).unwrap_or(0.);
        let scale = kwargs.get("scale").and_then(Value::as_f64).unwrap_or(1.);
        return crate::cad::actions::local_edit_at(doc, call, None, label, false, move |ws| {
            let centers: Vec<[f64; 3]> = ids.iter().map(|id| center.or_else(|| ws.centroid(id)).unwrap_or([0.; 3])).collect();
            let (archive, stamps) = (ws.archive, ws.stamps);
            let moved = sim_cad::nodes::transform(&mut ws.edit, archive, stamps, &ids, &centers, translation, axis, angle, scale)?;
            let mut result = json!({"moved": moved});
            if let (Some((key, v)), Some(object)) = (extra, result.as_object_mut()) {
                object.insert(key.to_string(), v);
            }
            Ok(EditDone { message, result })
        });
    }
    crate::cad::actions::edit(doc, call, label, move |c| {
        c.op(name, &args, &kwargs).map(|r| {
            let mut result = value(&r);
            if let (Some((key, v)), Some(object)) = (extra, result.as_object_mut()) {
                object.insert(key.to_string(), v);
            }
            EditDone { message, result }
        })
    })
}

/// A commit (`CadTransform`, `CadPushPull`, `CadOffsetFaces`,
/// `CadSetDimension`): refused with nothing sent when `commit_refusal`
/// names a reason, else exactly one Ops call. A released drag's preview
/// follows its commit (kept while the edit runs, dropped when refused).
pub(super) fn commit(doc: &mut CadDocument, selection: &[SelectionItem], topology: Option<&CadTopology>, call: &mut Call, action: &CadAction) -> Outcome {
    let revision = match action {
        CadAction::CadTransform { revision, .. } | CadAction::CadPushPull { revision, .. } | CadAction::CadOffsetFaces { revision, .. } | CadAction::CadSetDimension { revision, .. } => *revision,
        _ => None,
    };
    let before = doc.edit_seq;
    let outcome = match doc.commit_refusal(revision) {
        Some(why) => Outcome::Done(Err(why)),
        None => match op_for(doc, selection, topology, action) {
            Ok(op) => send(doc, call, op, None),
            Err(e) => Outcome::Done(Err(e)),
        },
    };
    let started = doc.edit.is_some() && doc.edit_seq != before;
    let ours = doc.tool_state.preview.as_ref().is_some_and(|p| matches!(&p.phase, Phase::Released { action: a, .. } if a == action));
    if ours && started {
        let seq = doc.edit_seq;
        if let Some(p) = doc.tool_state.preview.as_mut() {
            p.phase = Phase::Committed { seq, ended: None };
        }
    } else if ours {
        doc.tool_state.preview = None;
    }
    outcome
}

/// Each typed value read as its field reads it (RoboCAD's `NumericBar.values`);
/// an error names the field, then the evaluator's token and position.
pub fn evaluate_fields(tool: CadTool, list: &[Field], values: &[String]) -> Result<Vec<f64>, String> {
    if values.len() != list.len() {
        let names: Vec<&str> = list.iter().map(|f| f.name.as_str()).collect();
        return Err(format!("the {} tool takes {} value(s) ({}); got {}", tool.label(), list.len(), names.join(", "), values.len()));
    }
    list.iter().zip(values).map(|(field, text)| field.kind.evaluate(text).map_err(|e| format!("{}: {e}", field.name))).collect()
}

/// `CadNumeric`: the active tool's fields evaluated, then its one commit.
pub(super) fn numeric(cx: &mut Cx, call: &mut Call, values: &[String]) -> Outcome {
    let refuse = |e: String| Outcome::Done(Err(e));
    let topology = cx.topology.as_deref();
    let meshes = cx.meshes.as_deref();
    let selection = cx.shared.items();
    let doc = &mut *cx.doc;
    let list = fields(doc, &selection, topology, meshes);
    if list.is_empty() {
        return refuse(match doc.tool {
            CadTool::Select => "nothing to type: select a cylindrical face (its diameter), two planar faces of one node (their distance or angle) or a circular edge, or double-click a face".into(),
            tool => format!("the {} tool has no numeric fields", tool.label()),
        });
    }
    let v = match evaluate_fields(doc.tool, &list, values) {
        Ok(v) => v,
        Err(e) => return refuse(e),
    };
    // The bar's Enter: the revision the entry gained focus at (values typed against
    // that geometry); a REST caller's values are read against the shown revision now.
    // A REST commit leaves the bar's focus revision alone (the user's typed values keep their stale check).
    let revision = Some(if call.rest() { doc.shown_revision() } else { doc.tool_state.numeric.began.take().unwrap_or_else(|| doc.shown_revision()) });
    let transform = |translation, axis, angle_deg, center, scale| CadAction::CadTransform { ids: None, translation, axis, angle_deg, center, scale, revision };
    let action = match doc.tool {
        CadTool::Move => transform(Some([v[0], v[1], v[2]]), None, None, None, None),
        CadTool::Rotate | CadTool::Scale => {
            let Some((center, _)) = pivot(doc, &selection, meshes) else { return refuse("Select something to transform".into()) };
            if doc.tool == CadTool::Rotate {
                transform(None, Some(mm(numeric_axis(&doc.tool_state))), Some(v[0]), Some(mm(center)), None)
            } else {
                transform(None, None, None, Some(mm(center)), Some(v[0]))
            }
        }
        CadTool::PushPull | CadTool::OffsetFace => {
            let target = doc.tool_state.push.clone().or_else(|| face_target(doc, &selection, topology));
            let Some(target) = target else { return refuse("no face is targeted: select a face (or press on one in the 3D view) first".into()) };
            // A face index from an older revision is found again (RoboCAD's find_face) or refused by name.
            let faces = topology.and_then(|t| t.get(&target.node)).map(|t| &**t);
            let PushTarget { node, face, .. } = match resolve(&target, doc.shown_revision(), faces, &doc.node_name(&target.node)) {
                Ok(t) => t,
                Err(e) => return refuse(e),
            };
            let kind = topology.and_then(|t| t.get(&node)).and_then(|t| t.faces.iter().find(|f| f.index == face)).map(|f| f.kind.clone());
            let Some(kind) = kind else { return refuse(format!("the faces of {} are not loaded yet; try again in a moment", doc.node_name(&node))) };
            // PushPullTool._apply: offset for the offset tool or a face that is not planar.
            if doc.tool == CadTool::OffsetFace || kind != "plane" {
                CadAction::CadOffsetFaces { node, faces: vec![face], distance: v[0], revision }
            } else {
                CadAction::CadPushPull { node, face, distance: v[0], revision }
            }
        }
        CadTool::Select => {
            // One dimension per Enter (each is one RoboCAD edit): the field whose value changed.
            let changed: Vec<usize> = (0..list.len()).filter(|&i| values[i].trim() != list[i].text() && (v[i] - list[i].value).abs() > 1e-9).collect();
            let i = match changed.as_slice() {
                [] => return refuse("nothing changed: type a new value in one field".into()),
                [i] => *i,
                _ => return refuse("change one dimension at a time (each is one RoboCAD edit)".into()),
            };
            match &list[i].commit {
                FieldCommit::Dimension { node, dimension, faces } => CadAction::CadSetDimension { node: node.clone(), dimension: *dimension, faces: faces.clone(), value: v[i], revision },
                FieldCommit::ReadOnly(why) => return refuse(why.clone()),
                other => return refuse(format!("{} is not a dimension ({other:?})", list[i].name)),
            }
        }
        CadTool::Measure => return refuse("the measure tool has no numeric fields".into()),
    };
    commit(doc, &selection, topology, call, &action)
}

/// `CadMeasure`: computed here over the shown topology; kept as one
/// `POST /ops/add_measurement` when `keep`.
pub(super) fn measure(cx: &mut Cx, call: &mut Call, a: &MeasurePick, b: &MeasurePick, keep: bool) -> Outcome {
    let topology = cx.topology.as_deref();
    let doc = &mut *cx.doc;
    let measured = crate::cad::measure::between(a, b, |id| topology.and_then(|t| t.get(id)).map(|t| &**t), |id| doc.node_name(id));
    let m = match measured {
        Ok(m) => m,
        Err(e) => return Outcome::Done(Err(e)),
    };
    doc.tool_state.measure.first = None;
    doc.tool_state.measure.last = Some(m.clone());
    doc.tool_state.readout = Some(m.label.clone());
    if !keep {
        doc.show(Ok(m.label.clone()));
        return Outcome::Done(Ok(json!({"measurement": m.json(), "kept": false})));
    }
    // Measured over the shown revision's topology and snaps: kept only while
    // that is RoboCAD's current revision (a stale tree or a document moved on
    // since would record points of geometry that is gone).
    if let Some(why) = doc.commit_refusal(Some(doc.shown_revision())) {
        return Outcome::Done(Err(format!("measured {}, but it was not kept: {why}", m.label)));
    }
    let op = OpCall { name: "add_measurement", args: vec![m.json()], kwargs: Map::new(), label: format!("Add measurement {}", m.label) };
    send(doc, call, op, Some(("measurement", m.json())))
}

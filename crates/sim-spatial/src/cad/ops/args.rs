//! The argument builders, keyed by route shape (`Shape`), never by operation.
//!
//! Each call's arguments are what RoboCAD's `ArgConverter` reads
//! (cad/robocad/api.py:146-244): node ids as strings, a face
//! `{"node", "face"}`, an edge `{"node", "edge"}`, vectors `[x, y, z]`, a
//! plane by name ("xy" | "xz" | "yz"), a boolean op by its value
//! ("union"), a chamfer spec `{"distance"[, "angle_deg"]}`, a transform
//! `{"translation"}`, counts as integers. Labels are RoboCAD's history
//! label (the Ops method's `_edit`/`_new`/`Composite` label) and the subject.
//! A plane parameter's "active" is the active plane (`CadActivePlane`),
//! sent by name or plane node id ([`Arg::Plane`]), or read as its frame
//! (the radial array's origin and normal, a primitive's plane).
use super::resolve::{Resolved, kind_of};
use super::{Arg, Env, Fan, Needs, OpEntry, Param, Primitive, Shape};
use crate::cad::sketch::{BasePlane, SketchTarget, ViewAct};
use sim_runtime::cad_client::{PlaneFrame, SketchCall};
use crate::cad::analysis_overlay::Read;
use crate::cad::document::CadDocument;
use crate::cad::mesh::BODY_KINDS;
use crate::cad::transform::{OpCall, fa, face_ref, fl, num, round6};
use crate::ui_kit::form::{FieldKind, Unit};
use serde_json::{Map, Value, json};

/// What a run sends: Ops calls in one edit job, a paste, a sketch's
/// calls, a read-only read, or viewer state (no RoboCAD call).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Built {
    /// The Ops calls, in order (one per node for `Fan::PerNode`), and the
    /// edit's label (the header's and the refusal's name for it).
    Edit { calls: Vec<OpCall>, label: String },
    /// `POST /paste` with the copied clipboard JSON (RoboCAD's one undo step "Paste").
    Paste { clip: Value, label: String },
    /// A read drawn as an overlay, or the copy.
    Read(Read),
    /// One `POST /nodes/{id}/sketch` on `Node`, or one `POST /nodes
    /// {"kind": "sketch", "plane", "calls"}` for `New` (cad-sketch).
    Sketch { target: SketchTarget, calls: Vec<SketchCall>, label: String },
    /// Viewer state: the active plane or 2D snapping (`Flow::View`).
    View(ViewAct),
    /// A robot dialog's or tool's calls (`robot_args`).
    Robot(super::robot_args::Plan),
}

/// An edge as RoboCAD's `ArgConverter.edge` takes it (api.py:166-176).
pub(crate) fn edge_ref(node: &str, edge: i64) -> Value {
    json!({"node": node, "edge": edge})
}

/// RoboCAD's history label for an Ops route (its `_edit`/`_new`/`Composite`
/// label, commands.py), or "" for a route not listed.
pub(crate) fn history(route: &str) -> &'static str {
    match route {
        "delete" => "Delete",                                   // commands.py:331 RemoveNodes("Delete")
        "isolate" => "Isolate",                                 // :413 (cad-views-export)
        "show_all" => "Show all",                               // :416
        "set_visible" => "Hide",                                // :337 (the catalogue sends visible=false only)
        "set_pivot" => "Pivot",                                 // :352
        "box" | "box_center" | "box_three_point" => "Box",      // :420 (box_center calls box), :435
        "cylinder" => "Cylinder",                               // :438
        "sphere" => "Sphere",                                   // :441
        "bridge" => "Bridge",                                   // :512
        "offset_face_to" => "Dependent offset",                 // :523
        "move_faces" => "Move face",                            // :526
        "rotate_faces" => "Rotate face",                        // :529
        "set_radius" => "Set radius",                           // :532
        "draft" => "Draft",                                     // :554
        "delete_faces" | "untrim" => "Delete face",             // :557, :560
        "imprint" => "Imprint",                                 // :564
        "split_face" => "Split face",                           // :571
        "region" => "Region",                                   // :589
        "cut" => "Cut",                                         // :611
        "shell" => "Shell",                                     // :625
        "thicken" => "Thicken",                                 // :628
        "fillet" => "Fillet",                                   // :631
        "fillet_chordal" => "Chordal fillet",                   // :634
        "fillet_all" => "Fillet all",                           // :637
        "full_round" => "Full round",                           // :640
        "remove_fillets" => "Remove fillets",                   // :643
        "chamfer" => "Chamfer",                                 // :646
        "mirror" => "Mirror",                                   // :708
        "instance" => "Instance",                               // :716
        "make_unique" => "Make unique",                         // :724
        "array_rect" => "Rectangular array",                    // :736
        "array_radial" => "Radial array",                       // :741
        "array_curve" => "Curve array",                         // :776
        "join" => "Join",                                       // :811
        "unjoin" => "Unjoin",                                   // :820
        "dissolve" => "Dissolve",                               // :824
        "extract_components" => "Extract components",           // :853
        "project_curve" => "Project",                           // :858
        "silhouette" => "Silhouette",                           // :861
        "set_control_points" => "Move control points",          // :865
        "raise_degree" => "Raise degree",                       // :868
        "rebuild_face" => "Rebuild face",                       // :871
        "extrude" => "Extrude",                                 // :487 (_apply_boolean label)
        "revolve" => "Revolve",                                 // :491
        "sweep" => "Sweep",                                     // :496
        "pipe" => "Pipe",                                       // :500
        "loft" => "Loft",                                       // :505
        "fill" => "Fill",                                       // :509
        "plane_from_face" | "plane_three_points" | "plane_two_points_camera" | "plane_midplane" => "Plane", // :892-895 (_add_plane)
        "set_robot_setting" => "Robot setting",                 // :1095 (SetSetting.label)
        _ => "",
    }
}

/// The nodes of `items`, each once, in first-appearance order (RoboCAD's
/// `by.setdefault(nid, …)` dict order).
fn owners(items: &[(String, i64)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (node, _) in items {
        if !out.contains(node) {
            out.push(node.clone());
        }
    }
    out
}

/// One call's subject: its node, and that node's selected edges and faces.
struct Group {
    node: Option<String>,
    edges: Vec<i64>,
    faces: Vec<i64>,
}

fn group(r: &Resolved, node: Option<String>) -> Group {
    let of = |items: &[(String, i64)]| -> Vec<i64> { items.iter().filter(|(n, _)| Some(n) == node.as_ref()).map(|(_, i)| *i).collect() };
    let (edges, faces) = (of(&r.edges), of(&r.faces));
    Group { node, edges, faces }
}

/// The calls' subjects: one per node for `Fan::PerNode` (the nodes owning
/// the selected edges or faces for an edge or face operation, else the
/// selected nodes), else one: the first selected edge's or face's node for
/// an edge or face operation, else the first selected node; none for an
/// operation that needs nothing selected (a primitive is not about the selection).
fn groups(entry: &OpEntry, r: &Resolved) -> Vec<Group> {
    match entry.fan {
        Fan::PerNode => {
            let nodes = match entry.needs {
                Needs::Edges { .. } => owners(&r.edges),
                Needs::Faces { .. } => owners(&r.faces),
                _ => r.nodes.clone(),
            };
            nodes.into_iter().map(|n| group(r, Some(n))).collect()
        }
        Fan::Once => {
            let node = match entry.needs {
                Needs::Nothing => None,
                Needs::Edges { .. } => r.edges.first().map(|e| e.0.clone()),
                Needs::Faces { .. } | Needs::FaceThenNode => r.faces.first().map(|f| f.0.clone()),
                _ => r.nodes.first().cloned(),
            };
            vec![group(r, node)]
        }
    }
}

fn required(entry: &OpEntry, name: &str) -> String {
    let label = entry.params.iter().find(|p| p.name == name).map_or(name, |p| p.label);
    format!("{label} ({name}) is required: {} has no default for it", entry.id)
}

fn param<'a>(entry: &OpEntry, values: &'a Map<String, Value>, name: &str) -> Result<&'a Value, String> {
    values.get(name).ok_or_else(|| required(entry, name))
}

fn number(entry: &OpEntry, values: &Map<String, Value>, name: &str) -> Result<f64, String> {
    let v = param(entry, values, name)?;
    v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| format!("{name} must be a finite number (got {v})"))
}

fn count(entry: &OpEntry, values: &Map<String, Value>, name: &str) -> Result<i64, String> {
    let v = param(entry, values, name)?;
    v.as_i64().or_else(|| v.as_f64().filter(|x| x.is_finite()).map(|x| x.round() as i64)).ok_or_else(|| format!("{name} must be a whole number (got {v})"))
}

fn flag(entry: &OpEntry, values: &Map<String, Value>, name: &str) -> Result<bool, String> {
    let v = param(entry, values, name)?;
    v.as_bool().ok_or_else(|| format!("{name} must be true or false (got {v})"))
}

fn vec3(entry: &OpEntry, values: &Map<String, Value>, name: &str) -> Result<[f64; 3], String> {
    let v = param(entry, values, name)?;
    match v.as_array().map(|a| a.iter().map(Value::as_f64).collect::<Option<Vec<f64>>>()) {
        Some(Some(a)) if a.len() == 3 && a.iter().all(|x| x.is_finite()) => Ok([a[0], a[1], a[2]]),
        _ => Err(format!("{name} must be three finite numbers [x, y, z] (got {v})")),
    }
}

/// An optional point parameter: None when it was not given (`super::values`
/// leaves out a parameter with an empty default that is empty).
fn opt_vec3(entry: &OpEntry, values: &Map<String, Value>, name: &str) -> Result<Option<[f64; 3]>, String> {
    if values.contains_key(name) { vec3(entry, values, name).map(Some) } else { Ok(None) }
}

fn pt(v: [f64; 3]) -> String {
    format!("({}, {}, {})", num(v[0]), num(v[1]), num(v[2]))
}

/// A parameter's value as a label shows it.
fn shown(p: &Param, v: &Value) -> String {
    match (p.kind, v) {
        (FieldKind::Number { unit: Unit::Length, .. }, Value::Number(n)) => n.as_f64().map_or_else(|| n.to_string(), fl),
        (FieldKind::Number { unit: Unit::Angle, .. }, Value::Number(n)) => n.as_f64().map_or_else(|| n.to_string(), fa),
        (FieldKind::Vector { .. }, Value::Array(a)) if a.len() == 3 => match (a[0].as_f64(), a[1].as_f64(), a[2].as_f64()) {
            (Some(x), Some(y), Some(z)) => format!("{} mm", pt([x, y, z])),
            _ => v.to_string(),
        },
        (_, Value::String(s)) => s.clone(),
        (_, Value::Bool(b)) => (if *b { "yes" } else { "no" }).to_string(),
        (FieldKind::Json, _) => "as given".to_string(),
        _ => v.to_string(),
    }
}

/// The names of the nodes, or "N nodes".
fn subject(doc: &CadDocument, ids: &[String]) -> String {
    match ids {
        [one] => doc.node_name(one),
        many => format!("{} nodes", many.len()),
    }
}

fn plural(n: usize, one: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") }
}

/// The parameters an entry's arguments use, each once, shown as "name
/// value"; a plane parameter as the plane it sends ("plane xy", "plane Plane 2").
fn param_details(entry: &OpEntry, values: &Map<String, Value>, doc: &CadDocument, env: &Env) -> Vec<String> {
    let mut used: Vec<(&str, Option<BasePlane>)> = Vec::new();
    for a in entry.args.iter().chain(entry.kwargs.iter().map(|(_, a)| a)) {
        let (name, fallback) = match a {
            Arg::Param(name) => (*name, None),
            Arg::Plane(name, fallback) => (*name, Some(*fallback)),
            _ => continue,
        };
        if !used.iter().any(|(n, _)| *n == name) {
            used.push((name, fallback));
        }
    }
    used.iter()
        .filter_map(|(name, fallback)| {
            let p = entry.params.iter().find(|p| p.name == *name)?;
            let v = values.get(*name)?;
            let text = match (fallback, plane_arg(v, *fallback, env)) {
                (Some(_), Some(Value::String(sent))) if doc.has_node(&sent) && doc.doc.is_some() => doc.node_name(&sent),
                (Some(_), Some(Value::String(sent))) => sent,
                _ => shown(p, v),
            };
            Some(format!("{} {text}", p.name.replace('_', " ")))
        })
        .collect()
}

/// What a plane parameter sends (`Arg::Plane`): "active" is the active
/// plane's argument ("xy" or its plane node id), else `fallback` (RoboCAD's
/// `active_plane or Plane.yz()`); "xy", "xz" or "yz" is sent as it is. None
/// when the value is not text, or without a fallback.
fn plane_arg(v: &Value, fallback: Option<BasePlane>, env: &Env) -> Option<Value> {
    let fallback = fallback?;
    Some(match v.as_str()? {
        "active" => env.plane.map_or_else(|| Value::from(fallback.arg()), |p| p.arg_or(fallback)),
        named => Value::from(named),
    })
}

/// A plane parameter's frame: "active" is the active plane's (an error
/// naming a plane node still being read), else XY's (RoboCAD's
/// `active_plane or Plane.xy()`); "xy", "xz" or "yz" that plane's. With
/// the plane's name as a label shows it.
fn plane_frame(name: &str, value: &str, env: &Env, doc: &CadDocument) -> Result<(PlaneFrame, String), String> {
    let named = |b: BasePlane| (b.frame(), b.arg().to_string());
    match value {
        "active" => match env.plane {
            Some(p) if p.plane.is_some() => Ok((p.frame_or_xy()?, p.label(doc))),
            _ => Ok(named(BasePlane::Xy)),
        },
        "xy" => Ok(named(BasePlane::Xy)),
        "xz" => Ok(named(BasePlane::Xz)),
        "yz" => Ok(named(BasePlane::Yz)),
        other => Err(format!("{name} must be active, xy, xz or yz (got {other})")),
    }
}

fn joined(head: String, parts: &[String]) -> String {
    if parts.is_empty() { head } else { format!("{head}: {}", parts.join(", ")) }
}

/// One argument's JSON.
fn arg(entry: &OpEntry, a: &Arg, r: &Resolved, g: &Group, values: &Map<String, Value>, env: &Env) -> Result<Value, String> {
    let missing = || entry.refusal.to_string();
    Ok(match a {
        Arg::Node => json!(g.node.as_ref().ok_or_else(missing)?),
        Arg::Nodes => json!(r.nodes),
        Arg::Target => json!(r.nodes.first().ok_or_else(missing)?),
        Arg::Tools => json!(r.nodes.get(1..).ok_or_else(missing)?),
        Arg::Second => json!(r.nodes.get(1).ok_or_else(missing)?),
        Arg::Last => json!(r.nodes.last().ok_or_else(missing)?),
        Arg::AllButLast => json!(r.nodes.split_last().map(|(_, rest)| rest).ok_or_else(missing)?),
        Arg::Edges => {
            let node = g.node.as_ref().ok_or_else(missing)?;
            Value::Array(g.edges.iter().map(|e| edge_ref(node, *e)).collect())
        }
        Arg::EdgeA => r.edges.first().map(|(n, e)| edge_ref(n, *e)).ok_or_else(missing)?,
        Arg::EdgeB => r.edges.get(1).map(|(n, e)| edge_ref(n, *e)).ok_or_else(missing)?,
        Arg::Faces => {
            let node = g.node.as_ref().ok_or_else(missing)?;
            Value::Array(g.faces.iter().map(|f| face_ref(node, *f)).collect())
        }
        Arg::Face => {
            let node = g.node.as_ref().ok_or_else(missing)?;
            face_ref(node, *g.faces.first().ok_or_else(missing)?)
        }
        // RoboCAD's midplane takes exactly two picks (ui/tools.py:1096-1098).
        Arg::FaceB => match r.faces.as_slice() {
            [_, (n, f)] => face_ref(n, *f),
            [] | [_] => return Err(missing()),
            more => return Err(format!("{} takes exactly two faces ({} selected): select two faces", entry.label, more.len())),
        },
        Arg::Keyed(key, name) => {
            let mut m = Map::new();
            m.insert((*key).to_string(), param(entry, values, name)?.clone());
            Value::Object(m)
        }
        Arg::Plane(name, fallback) => plane_arg(param(entry, values, name)?, Some(*fallback), env).ok_or_else(|| format!("{name} must be active, xy, xz or yz"))?,
        Arg::OtherNode => json!(r.other.as_ref().ok_or_else(missing)?),
        Arg::Param(name) => param(entry, values, name)?.clone(),
        Arg::Const(text) => serde_json::from_str::<Value>(text).map_err(|e| format!("{}: constant {text} is not JSON: {e}", entry.id))?,
        Arg::ViewDir => match (values.get("direction"), r.view_dir) {
            (Some(v), _) => v.clone(),
            (None, Some(d)) => json!(d),
            (None, None) => return Err("the 3D view's direction is not known (no view is drawn): pass direction [x, y, z]".into()),
        },
        Arg::CursorSnap => match (values.get("point"), r.snap) {
            (Some(v), _) => v.clone(),
            (None, Some(p)) => json!(p),
            (None, None) => return Err("no snapped point under the cursor: point at the model in the 3D view, or pass point [x, y, z]".into()),
        },
        Arg::Revision => json!(r.revision),
    })
}

/// The calls of a `Plain` or `Chamfer` entry.
fn plain(entry: &OpEntry, r: &Resolved, values: &Map<String, Value>, doc: &CadDocument, env: &Env) -> Result<Built, String> {
    let groups = groups(entry, r);
    if groups.is_empty() {
        return Err(entry.refusal.to_string());
    }
    // Chamfer's spec (ui/tools.py:984-985): the angle only when it is not 45°.
    let spec = match entry.shape {
        Shape::Chamfer => {
            let distance = number(entry, values, "distance")?;
            let angle = number(entry, values, "angle")?;
            let mut spec = Map::new();
            spec.insert("distance".into(), json!(distance));
            if (angle - 45.0).abs() > 1e-9 {
                spec.insert("angle_deg".into(), json!(angle));
            }
            Some((Value::Object(spec), distance, angle))
        }
        _ => None,
    };
    let mut details = param_details(entry, values, doc, env);
    if let Some((_, distance, angle)) = &spec {
        details.push(format!("distance {}", fl(*distance)));
        if (angle - 45.0).abs() > 1e-9 {
            details.push(format!("angle {}", fa(*angle)));
        }
    }
    let uses = |want: fn(&Arg) -> bool| entry.args.iter().any(want);
    let mut history_name = history(entry.route).to_string();
    let mut calls = Vec::with_capacity(groups.len());
    for g in &groups {
        let mut args = entry.args.iter().map(|a| arg(entry, a, r, g, values, env)).collect::<Result<Vec<Value>, String>>()?;
        if let Some((spec, ..)) = &spec {
            args.push(spec.clone());
        }
        let mut kwargs = Map::new();
        for (name, a) in entry.kwargs {
            kwargs.insert((*name).to_string(), arg(entry, a, r, g, values, env)?);
        }
        // Boolean's history label is its op, capitalised (commands.py:585).
        if entry.route == "boolean"
            && let Some(op) = args.get(2).and_then(Value::as_str)
        {
            let mut c = op.chars();
            history_name = c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default();
        }
        // An operation that needs nothing selected names no subject (the
        // selection is not what it acts on).
        let who = match &g.node {
            _ if entry.needs == Needs::Nothing => String::new(),
            Some(node) if uses(|a| matches!(a, Arg::Node)) => doc.node_name(node),
            _ if uses(|a| matches!(a, Arg::Nodes)) => subject(doc, &r.nodes),
            _ => r.nodes.first().map(|n| doc.node_name(n)).unwrap_or_default(),
        };
        let mut parts = Vec::new();
        if uses(|a| matches!(a, Arg::Edges)) {
            parts.push(plural(g.edges.len(), "edge"));
        }
        if uses(|a| matches!(a, Arg::EdgeA)) {
            parts.push("2 edges".to_string());
        }
        if uses(|a| matches!(a, Arg::Faces)) {
            parts.push(plural(g.faces.len(), "face"));
        }
        if uses(|a| matches!(a, Arg::Face)) {
            parts.push(format!("face {}", g.faces.first().copied().unwrap_or_default()));
        }
        if uses(|a| matches!(a, Arg::Tools | Arg::Second | Arg::OtherNode)) {
            let others: Vec<String> = match entry.args.iter().find(|a| matches!(a, Arg::Tools | Arg::Second | Arg::OtherNode)) {
                Some(Arg::Tools) => r.nodes.iter().skip(1).cloned().collect(),
                Some(Arg::Second) => r.nodes.get(1).cloned().into_iter().collect(),
                _ => r.other.clone().into_iter().collect(),
            };
            if !others.is_empty() {
                parts.push(format!("with {}", subject(doc, &others)));
            }
        }
        parts.extend(details.iter().cloned());
        let head = if who.is_empty() { history_name.clone() } else { format!("{history_name} {who}") };
        calls.push(OpCall { name: entry.route, args, kwargs, label: joined(head, &parts) });
    }
    let label = match calls.as_slice() {
        [one] => one.label.clone(),
        many => joined(format!("{history_name} on {} nodes", many.len()), &details),
    };
    Ok(Built::Edit { calls, label })
}

/// The Array dialog (ui/app.py:920-941): `array_rect(ids, count, spacing=|extent=, as_instances, merge)`
/// or `array_radial(ids, count, axis_point, axis_dir, total_angle, as_instances, merge)`.
/// The radial array's axis is the plane's normal through its origin: the
/// active plane's ("active", RoboCAD's `active_plane or Plane.xy()`), or
/// the named plane's (kernel/base.py:102-111).
fn array(entry: &OpEntry, r: &Resolved, values: &Map<String, Value>, doc: &CadDocument, env: &Env) -> Result<Built, String> {
    if r.nodes.is_empty() {
        return Err(entry.refusal.to_string());
    }
    let as_instances = flag(entry, values, "as_instances")?;
    let merge = flag(entry, values, "merge")?;
    let who = subject(doc, &r.nodes);
    let kind = param(entry, values, "kind")?.as_str().unwrap_or_default().to_string();
    let mut kwargs = Map::new();
    let call = if kind == "radial" {
        let n = count(entry, values, "count")?;
        let angle = number(entry, values, "angle")?;
        let value = param(entry, values, "plane")?.as_str().unwrap_or_default().to_string();
        let (frame, plane) = plane_frame("plane", &value, env, doc)?;
        let r3 = |v: [f64; 3]| v.map(round6);
        kwargs.insert("total_angle".into(), json!(angle));
        kwargs.insert("as_instances".into(), json!(as_instances));
        kwargs.insert("merge".into(), json!(merge));
        let label = format!("{} {who}: {n} over {} about the {plane} normal", history("array_radial"), fa(angle));
        OpCall { name: "array_radial", args: vec![json!(r.nodes), json!(n), json!(r3(frame.origin)), json!(r3(frame.normal))], kwargs, label }
    } else if kind == "rectangular" {
        let c = [count(entry, values, "count_x")?, count(entry, values, "count_y")?, count(entry, values, "count_z")?];
        let s = vec3(entry, values, "spacing")?;
        let mode = param(entry, values, "mode")?.as_str().unwrap_or_default().to_string();
        // RoboCAD's `d.mode.currentIndex() == 0`: "count + spacing".
        let key = if mode == "count + spacing" { "spacing" } else { "extent" };
        kwargs.insert(key.into(), json!(s));
        kwargs.insert("as_instances".into(), json!(as_instances));
        kwargs.insert("merge".into(), json!(merge));
        let label = format!("{} {who}: {} × {} × {}, {key} {} mm", history("array_rect"), c[0], c[1], c[2], pt(s));
        OpCall { name: "array_rect", args: vec![json!(r.nodes), json!(c)], kwargs, label }
    } else {
        return Err(format!("kind must be rectangular or radial (got {kind})"));
    };
    let label = call.label.clone();
    Ok(Built::Edit { calls: vec![call], label })
}

/// RoboCAD's `PrimitiveTool` on the active plane (ui/tools.py:491-532;
/// `ctx.active_plane()`: the active plane, else XY). The anchor parameter
/// is a model point, projected onto the plane (`plane.to_local(anchor)`,
/// tools.py:522-529); the sphere keeps its centre. Without an anchor it is
/// the plane's origin (`anchor = self.p0 or plane.origin`, tools.py:524). The box as `_make_box`
/// (width and depth at least 1e-3; a height within 1e-6 of zero is 1; a
/// negative height extrudes |h| along −normal), the centre box centred in
/// the plane only, the cylinder as `_finish` (axis ± the normal by the
/// height's sign, radius at least 1e-3), the sphere with radius at least 1e-3.
///
/// On XY the calls are `Ops.box(corner, size)` and `Ops.cylinder` with
/// z = 0, as before cad-sketch. On another plane RoboCAD extrudes a sketch
/// rectangle (`_make_box`), which no Ops route takes; the box is sent as
/// `Ops.box_three_point(a, b, c, height)` (commands.py:426-435: x =
/// unit(b − a), y the in-plane direction towards c, z = x × y) with a, b, c
/// chosen so z is the plane's normal (h ≥ 0) or its reverse (h < 0): the
/// same solid. Deliberately different: the history label is
/// `box_three_point`'s "Box", as `_make_box`'s extrude names it "Box".
fn place(entry: &OpEntry, primitive: Primitive, values: &Map<String, Value>, env: &Env) -> Result<Built, String> {
    let frame = env.plane.map_or(Ok(PlaneFrame::XY), |p| p.frame_or_xy())?;
    let on_xy = frame.same(&PlaneFrame::XY, 1e-9);
    let r3 = |v: [f64; 3]| [round6(v[0]), round6(v[1]), round6(v[2])];
    // The anchor in the plane (u, v, w): the plane's origin when not given.
    let local = |anchor: Option<[f64; 3]>| match anchor {
        Some(a) if on_xy => a,
        Some(a) => frame.to_local(a),
        None => [0.0; 3],
    };
    let call = match primitive {
        Primitive::BoxCorner | Primitive::BoxCentre => {
            let anchor = opt_vec3(entry, values, if primitive == Primitive::BoxCorner { "corner" } else { "center" })?;
            let (w, d, h) = (number(entry, values, "width")?, number(entry, values, "depth")?, number(entry, values, "height")?);
            let [u, v, _] = local(anchor);
            let (x0, y0) = if primitive == Primitive::BoxCentre { (u - w / 2.0, v - d / 2.0) } else { (u, v) };
            let height = if h.abs() > 1e-6 { h.abs() } else { 1.0 };
            let (w, d) = (w.max(1e-3), d.max(1e-3));
            if on_xy {
                let z0 = if h >= 0.0 { 0.0 } else { -height };
                let size = r3([w, d, height]);
                let corner = r3([x0, y0, z0]);
                let label = format!("Box {} × {} × {} at {}", fl(size[0]), fl(size[1]), fl(size[2]), pt(corner));
                OpCall { name: entry.route, args: vec![json!(corner), json!(size)], kwargs: Map::new(), label }
            } else {
                // The base rectangle's corners: a → b along the plane's x, c across. For a
                // negative height the rectangle is walked from its far v edge, so y = −plane y
                // and z = x × y = −normal: `_make_box`'s extrusion along −normal.
                let at = |uu: f64, vv: f64| r3(frame.to_world(uu, vv, 0.0));
                let (a, b, c) = if h >= 0.0 { (at(x0, y0), at(x0 + w, y0), at(x0, y0 + d)) } else { (at(x0, y0 + d), at(x0 + w, y0 + d), at(x0, y0)) };
                let label = format!("Box {} × {} × {} at {} on the plane", fl(round6(w)), fl(round6(d)), fl(round6(height)), pt(at(x0, y0)));
                OpCall { name: "box_three_point", args: vec![json!(a), json!(b), json!(c), json!(round6(height))], kwargs: Map::new(), label }
            }
        }
        Primitive::Cylinder => {
            let [u, v, _] = local(opt_vec3(entry, values, "base")?);
            let (dia, h) = (number(entry, values, "diameter")?, number(entry, values, "height")?);
            let (base, normal) = if on_xy { ([u, v, 0.0], [0.0, 0.0, 1.0]) } else { (frame.to_world(u, v, 0.0), frame.normal) };
            let base = r3(base);
            let axis = if h > 0.0 { r3(normal) } else { r3(normal.map(|x| -x)) };
            let radius = round6((dia / 2.0).max(1e-3));
            let label = format!("Cylinder Ø{} × {} at {}", fl(2.0 * radius), fl(h.abs()), pt(base));
            OpCall { name: entry.route, args: vec![json!(base), json!(axis), json!(radius), json!(round6(h.abs()))], kwargs: Map::new(), label }
        }
        Primitive::Sphere => {
            let center = r3(opt_vec3(entry, values, "center")?.unwrap_or(frame.origin));
            let radius = round6((number(entry, values, "diameter")? / 2.0).max(1e-3));
            let label = format!("Sphere Ø{} at {}", fl(2.0 * radius), pt(center));
            OpCall { name: entry.route, args: vec![json!(center), json!(radius)], kwargs: Map::new(), label }
        }
    };
    let label = call.label.clone();
    Ok(Built::Edit { calls: vec![call], label })
}

/// The last resolved node whose kind in the shown tree passes `has` (a node
/// of unknown kind, with no tree shown yet, passes; RoboCAD answers for it).
fn last_of(doc: &CadDocument, r: &Resolved, has: impl Fn(&str) -> bool) -> Option<String> {
    r.nodes.iter().rev().find(|n| kind_of(doc, n).is_none_or(&has)).cloned()
}

/// Build what `entry` sends on the resolved selection with `values`
/// (`super::values`), keyed by `entry.shape`.
pub(super) fn build(entry: &OpEntry, r: &Resolved, values: &Map<String, Value>, doc: &CadDocument, env: &Env) -> Result<Built, String> {
    match entry.shape {
        Shape::Plain | Shape::Chamfer => plain(entry, r, values, doc, env),
        Shape::Array => array(entry, r, values, doc, env),
        Shape::Place(primitive) => place(entry, primitive, values, env),
        Shape::Sketch(shape) => crate::cad::sketch::specs::calls(entry, shape, values, doc, env),
        Shape::SketchEdit(edit) => crate::cad::sketch::edits::calls(entry, edit, r, values, doc, env),
        Shape::Extrude { revolve } => crate::cad::sketch::extrude::calls(entry, revolve, r, values, doc, env),
        Shape::View(act) => Ok(Built::View(act)),
        Shape::Robot(which) => super::robot_args::build(entry, which, r, values, doc),
        Shape::Copy => {
            if r.nodes.is_empty() {
                return Err(entry.refusal.to_string());
            }
            Ok(Built::Read(Read::Copy { ids: r.nodes.clone() }))
        }
        Shape::Paste => {
            let Some((_, clip)) = &doc.ops.clipboard else { return Err(entry.refusal.to_string()) };
            let n = clip.get("items").and_then(Value::as_array).map_or(0, Vec::len);
            Ok(Built::Paste { clip: clip.clone(), label: format!("Paste {}", plural(n, "item")) })
        }
        Shape::ControlPoints => {
            let (node, face) = r.faces.first().cloned().ok_or_else(|| entry.refusal.to_string())?;
            Ok(Built::Read(Read::ControlPoints { node, face }))
        }
        // RoboCAD draws each selected node's in turn; the last one's stays
        // drawn. Its comb skips a node with no body: a sketch (app.py:1277-1284).
        Shape::CurvatureComb => {
            let node = last_of(doc, r, |k| k == "curve").ok_or_else(|| "Select a curve: a sketch has no body to comb (RoboCAD's comb skips it)".to_string())?;
            Ok(Built::Read(Read::CurvatureComb { node }))
        }
        // The continuity report skips a node with no body (app.py:1287-1290):
        // bodies, sheets, instances, meshes and curves have one.
        Shape::Continuity => {
            let node = last_of(doc, r, |k| BODY_KINDS.contains(&k) || k == "curve").ok_or_else(|| "Select a body: none of the selected nodes has one (a sketch, group or plane has none; RoboCAD's check skips it)".to_string())?;
            Ok(Built::Read(Read::Continuity { node }))
        }
    }
}

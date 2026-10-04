//! RoboCAD's `Ops` command layer in process: `run(cx, name, args, kwargs)`
//! applies one operation, by RoboCAD's method name and Python argument
//! binding (positional, then keyword), to the archive being edited and
//! answers what RoboCAD's method returns (a node id, a list of ids, a dict).
//! Reference: RoboCAD's commands.py (`Ops`) and api.py (`ArgConverter`:
//! faces `{node, face}`, edges `{node, edge}`, planes "xy" | "xz" | "yz" | a
//! plane node id | `{origin, normal[, x_axis]}` | `{axis, offset}`).
//!
//! Bodies are world placed (RoboCAD's convention): an edit replaces the
//! node's `brep/<id>.brep`; an instance keeps `source`, `transform` and
//! `mirror_plane`. Face and edge indices are the current archive's.
use crate::annotations::Stamps;
use crate::edit::{Edit, new_id};
use crate::kernel::{self, Built, Kind, Op, Shape};
use crate::sketch::{Plane, Sketch};
use crate::ArchiveDocument;
use serde_json::{Map, Value, json};
use std::collections::HashSet;

/// What an operation works on.
pub struct Ctx<'a> {
    /// The archive before this edit.
    pub doc: &'a ArchiveDocument,
    /// The current stamps of its pinned nodes (`annotations::Stamps`).
    pub stamps: &'a Stamps,
    /// The change being made.
    pub edit: &'a mut Edit,
    /// A body's volume centroid (mm) from the exact geometry.
    pub centroid: &'a dyn Fn(&str) -> Option<[f64; 3]>,
    pub cancelled: &'a dyn Fn() -> bool,
}

/// A method's arguments bound to its parameter names as Python binds them.
pub struct A<'v> {
    method: &'v str,
    params: &'static [&'static str],
    args: &'v [Value],
    kwargs: &'v Map<String, Value>,
}
impl<'v> A<'v> {
    pub fn new(method: &'v str, params: &'static [&'static str], args: &'v [Value], kwargs: &'v Map<String, Value>) -> Result<A<'v>, String> {
        if args.len() > params.len() {
            return Err(format!("{method}: takes at most {} arguments ({}), got {}", params.len(), params.join(", "), args.len()));
        }
        if let Some(k) = kwargs.keys().find(|k| !params.contains(&k.as_str())) {
            return Err(format!("{method}: unexpected argument {k} (takes {})", params.join(", ")));
        }
        Ok(A { method, params, args, kwargs })
    }
    pub fn get(&self, name: &str) -> Option<&'v Value> {
        let i = self.params.iter().position(|p| *p == name)?;
        self.args.get(i).or_else(|| self.kwargs.get(name)).filter(|v| !v.is_null())
    }
    fn need(&self, name: &str) -> Result<&'v Value, String> {
        self.get(name).ok_or_else(|| format!("{}: missing {name}", self.method))
    }
    pub fn str(&self, name: &str) -> Result<&'v str, String> {
        self.need(name)?.as_str().ok_or_else(|| format!("{}: {name} must be text", self.method))
    }
    pub fn opt_str(&self, name: &str) -> Option<&'v str> {
        self.get(name).and_then(Value::as_str)
    }
    pub fn num(&self, name: &str) -> Result<f64, String> {
        self.need(name)?.as_f64().filter(|v| v.is_finite()).ok_or_else(|| format!("{}: {name} must be a finite number", self.method))
    }
    pub fn num_or(&self, name: &str, default: f64) -> Result<f64, String> {
        if self.get(name).is_some() { self.num(name) } else { Ok(default) }
    }
    pub fn opt_num(&self, name: &str) -> Result<Option<f64>, String> {
        self.get(name).map(|_| self.num(name)).transpose()
    }
    pub fn flag(&self, name: &str, default: bool) -> bool {
        self.get(name).and_then(Value::as_bool).unwrap_or(default)
    }
    pub fn v3(&self, name: &str) -> Result<[f64; 3], String> {
        v3(self.need(name)?).ok_or_else(|| format!("{}: {name} must be [x, y, z] (finite mm)", self.method))
    }
    pub fn opt_v3(&self, name: &str) -> Result<Option<[f64; 3]>, String> {
        self.get(name).map(|_| self.v3(name)).transpose()
    }
    pub fn ids(&self, name: &str) -> Result<Vec<String>, String> {
        match self.need(name)? {
            Value::String(s) => Ok(vec![s.clone()]),
            Value::Array(a) => a.iter().map(|v| v.as_str().map(str::to_string).ok_or_else(|| format!("{}: {name} must be node ids", self.method))).collect(),
            _ => Err(format!("{}: {name} must be node ids", self.method)),
        }
    }
}
pub fn v3(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array().filter(|a| a.len() == 3)?;
    Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?]).filter(|p| p.iter().all(|x| x.is_finite()))
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unit(a: [f64; 3]) -> [f64; 3] {
    let l = dot(a, a).sqrt();
    if l < 1e-12 { [0., 0., 1.] } else { scale(a, 1. / l) }
}
fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    dot(sub(a, b), sub(a, b)).sqrt()
}

/// `Plane.from_normal`.
pub fn plane_from_normal(origin: [f64; 3], normal: [f64; 3]) -> Plane {
    let n = unit(normal);
    let helper = if n[2].abs() < 0.9 { [0., 0., 1.] } else { [1., 0., 0.] };
    Plane { origin, normal: n, x_axis: unit(cross(helper, n)) }
}

impl Ctx<'_> {
    /// A node as it stands in this edit.
    pub fn node(&self, id: &str) -> Result<&Value, String> {
        self.edit.node(id).ok_or_else(|| format!("node {id} does not exist"))
    }
    fn name(&self, id: &str) -> String {
        self.edit.node(id).and_then(|n| n["name"].as_str()).unwrap_or(id).to_string()
    }
    /// RoboCAD's `unique_name`.
    pub fn unique_name(&self, base: &str) -> String {
        let names: HashSet<&str> = self.edit.manifest["nodes"].as_array().into_iter().flatten().filter_map(|n| n["name"].as_str()).collect();
        if !names.contains(base) {
            return base.to_string();
        }
        (2..).map(|k| format!("{base} {k}")).find(|n| !names.contains(n.as_str())).expect("a free name")
    }
    /// Body `id`'s B-rep as this edit has it (a plain body), RoboCAD's `body_of`
    /// for instances (the source placed, mirrored).
    pub fn body(&self, id: &str) -> Result<Vec<u8>, String> {
        let n = self.node(id)?.clone();
        if n["kind"] == "instance" {
            let src = n["source"].as_str().ok_or("instance missing source")?;
            let mut b = self.body(src)?;
            if let Some(m) = n.get("mirror_plane").filter(|m| !m.is_null()) {
                let p = Plane::parse(m)?;
                b = kernel::op1(Op::Mirror, &[&b], &[p.origin, p.normal].concat(), &[], self.cancelled)?.brep;
            }
            let t = &n["transform"];
            let tr = v3(&t["translation"]).unwrap_or([0.; 3]);
            let ax = v3(&t["axis"]).unwrap_or([0., 0., 1.]);
            let ang = t["angle_deg"].as_f64().unwrap_or(0.);
            let sc = t["scale"].as_f64().unwrap_or(1.);
            if ang.abs() > 1e-12 || tr.iter().any(|v| v.abs() > 1e-12) || (sc - 1.).abs() > 1e-12 {
                let m = kernel::placement(tr, Some(ax), ang, [0.; 3], sc)?;
                b = kernel::build(&Shape::Transform { body: &b, matrix: m }, self.cancelled)?;
            }
            return Ok(b);
        }
        if !n["component_member"].is_null() {
            return Err("component member geometry is derived: edit the component's parameters".into());
        }
        let key = format!("brep/{id}.brep");
        if let Some(entry) = self.edit.entries.get(&key) {
            return entry.clone().ok_or_else(|| format!("node {id} has no geometry"));
        }
        self.doc.entry(&key).map(<[u8]>::to_vec).ok_or_else(|| format!("node {id} has no geometry"))
    }
    /// RoboCAD's `_edit`: refusals for component members, locked nodes and
    /// instances, then the new B-rep (and the kind it makes the node).
    pub fn replace(&mut self, id: &str, built: Built) -> Result<(), String> {
        let n = self.node(id)?;
        if !n["component_member"].is_null() {
            return Err("Edit component parameters or detach the occurrence before changing its geometry".into());
        }
        if n["locked"] == true {
            return Err(format!("{} is locked", self.name(id)));
        }
        if n["kind"] == "instance" {
            return Err("edit the source body; an instance follows it".into());
        }
        let node = self.edit.node_mut(id)?;
        node["kind"] = json!(node_kind(built.kind));
        node["body_kind"] = json!(built.kind.name());
        self.edit.entries.insert(format!("brep/{id}.brep"), Some(built.brep));
        Ok(())
    }
    fn edit_body(&mut self, id: &str, f: impl FnOnce(&Self, Vec<u8>) -> Result<Built, String>) -> Result<Value, String> {
        let b = self.body(id)?;
        let built = f(self, b)?;
        self.replace(id, built)?;
        Ok(json!(id))
    }
    /// RoboCAD's `Node` record for a new node of `kind` named `name`,
    /// under `parent` (else the active group).
    pub fn add_node(&mut self, kind: &str, name: &str, parent: Option<&str>, extra: Map<String, Value>) -> Result<String, String> {
        let id = new_id();
        let parent = parent.map(str::to_string).or_else(|| self.edit.manifest["active_group"].as_str().filter(|g| self.edit.node(g).is_some()).map(str::to_string));
        let name = self.unique_name(name);
        let mut node = json!({
            "id": id, "kind": kind, "name": name, "parent": parent, "children": [],
            "visible": true, "locked": false, "disabled": false, "material": if kind == "body" { json!("pla") } else { Value::Null },
            "color": null, "pivot": null,
            "transform": {"translation": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0], "angle_deg": 0.0, "scale": 1.0},
            "source": null, "tessellation_tolerance": 0.05,
        });
        for (k, v) in extra {
            node[k.as_str()] = v;
        }
        self.edit.manifest["nodes"].as_array_mut().ok_or("manifest nodes must be an array")?.push(node);
        let list = match &parent {
            Some(p) => {
                let pn = self.edit.node_mut(p)?;
                if !pn["children"].is_array() {
                    pn["children"] = json!([]);
                }
                pn["children"].as_array_mut().expect("array")
            }
            None => {
                if !self.edit.manifest["roots"].is_array() {
                    self.edit.manifest["roots"] = json!([]);
                }
                self.edit.manifest["roots"].as_array_mut().expect("array")
            }
        };
        list.push(json!(id));
        Ok(id)
    }
    /// RoboCAD's `_new`: a body, sheet or curve node from a built shape.
    pub fn add_built(&mut self, built: Built, name: &str, material: Option<&str>, parent: Option<&str>) -> Result<String, String> {
        let kind = node_kind(built.kind);
        let mut extra = Map::new();
        extra.insert("body_kind".into(), json!(built.kind.name()));
        extra.insert("material".into(), json!(material.map(str::to_string).or_else(|| (kind == "body").then(|| "pla".to_string()))));
        let id = self.add_node(kind, name, parent, extra)?;
        self.edit.entries.insert(format!("brep/{id}.brep"), Some(built.brep));
        Ok(id)
    }
    /// The profile of `source` (a sketch node, a curve or a body), RoboCAD's
    /// `_profile`: a sketch's closed curves as a face (outer + holes), else its
    /// curves as wires; and the sketch's plane.
    pub fn profile(&self, source: &Value) -> Result<(Vec<u8>, Option<Plane>), String> {
        let id = source.as_str().ok_or("a profile is a sketch, curve or body node id")?;
        let n = self.node(id)?;
        if let Some(s) = n.get("sketch").filter(|s| !s.is_null()) {
            let sk = Sketch::from_json(s)?;
            let face = sk.profile(&[], self.cancelled).or_else(|_| sk.wires(&[], self.cancelled))?;
            return Ok((face, Some(sk.plane)));
        }
        Ok((self.body(id)?, None))
    }
    /// A plane argument: RoboCAD's forms, or a plane node's `plane`.
    pub fn plane(&self, v: &Value) -> Result<Plane, String> {
        if let Some(id) = v.as_str().filter(|s| !matches!(*s, "xy" | "xz" | "yz")) {
            let n = self.node(id).map_err(|_| format!("unknown plane {id:?} (xy/xz/yz or a plane node id)"))?;
            return Plane::parse(n.get("plane").filter(|p| !p.is_null()).ok_or_else(|| format!("{} is not a plane", self.name(id)))?);
        }
        Plane::parse(v)
    }
    /// A face argument `{node, face}` of `node`: its index.
    pub fn face(&self, v: &Value, node: &str) -> Result<i32, String> {
        let o = v.as_object().ok_or("a face is {node, face}")?;
        if o.get("node").and_then(Value::as_str).is_some_and(|n| n != node) {
            return Err("the face belongs to another part".into());
        }
        o.get("face").and_then(Value::as_i64).map(|i| i as i32).ok_or_else(|| "a face is {node, face}".to_string())
    }
    pub fn faces(&self, v: &Value, node: &str) -> Result<Vec<i32>, String> {
        v.as_array().ok_or("faces must be a list of {node, face}")?.iter().map(|f| self.face(f, node)).collect()
    }
    pub fn edge(&self, v: &Value, node: &str) -> Result<i32, String> {
        let o = v.as_object().ok_or("an edge is {node, edge}")?;
        if o.get("node").and_then(Value::as_str).is_some_and(|n| n != node) {
            return Err("the edge belongs to another part".into());
        }
        o.get("edge").and_then(Value::as_i64).map(|i| i as i32).ok_or_else(|| "an edge is {node, edge}".to_string())
    }
    pub fn edges(&self, v: &Value, node: &str) -> Result<Vec<i32>, String> {
        v.as_array().ok_or("edges must be a list of {node, edge}")?.iter().map(|e| self.edge(e, node)).collect()
    }
    fn set(&mut self, id: &str, key: &str, value: Value) -> Result<(), String> {
        self.edit.node_mut(id)?[key] = value;
        Ok(())
    }
    fn robot_meta(&self, id: &str) -> Map<String, Value> {
        self.edit.node(id).and_then(|n| n["robot"].as_object().cloned()).unwrap_or_default()
    }
    fn is_body(&self, id: &str) -> bool {
        self.edit.node(id).is_some_and(|n| matches!(n["kind"].as_str(), Some("body" | "instance")))
    }
    fn centroid_of(&self, id: &str) -> Result<[f64; 3], String> {
        if let Some(c) = (self.centroid)(id).filter(|_| !self.edit.entries.contains_key(&format!("brep/{id}.brep"))) {
            return Ok(c);
        }
        let b = self.body(id)?;
        let bb = kernel::measure(kernel::Measure::Bounds, &[&b], &[], &[])?;
        Ok([(bb[0] + bb[3]) / 2., (bb[1] + bb[4]) / 2., (bb[2] + bb[5]) / 2.])
    }
}

/// RoboCAD's node kind for a body kind (`EditBodies`, `_new`).
pub fn node_kind(k: Kind) -> &'static str {
    match k {
        Kind::Solid => "body",
        Kind::Sheet => "sheet",
        Kind::Wire => "curve",
    }
}

fn boolean_code(v: &str) -> Result<i32, String> {
    match v {
        "union" => Ok(0),
        "subtract" => Ok(1),
        "intersect" => Ok(2),
        "new" => Ok(-1),
        other => Err(format!("boolean op must be new, union, subtract or intersect, not {other}")),
    }
}

/// Every method name [`run`] implements.
pub const METHODS: &[&str] = &[
    "rename", "set_visible", "set_locked", "set_disabled", "set_material", "set_color", "set_pivot", "group", "move_nodes", "move_node", "set_active_group", "isolate", "show_all", "delete",
    "box", "box_center", "box_three_point", "cylinder", "sphere", "new_sketch",
    "extrude", "revolve", "sweep", "pipe", "loft", "fill", "bridge",
    "push_pull", "offset_faces", "offset_face_to", "move_faces", "rotate_faces", "set_radius", "set_diameter", "set_distance", "set_angle", "draft", "delete_faces", "untrim", "imprint", "split_face",
    "boolean", "region", "cut", "shell", "thicken", "fillet", "fillet_chordal", "fillet_all", "full_round", "remove_fillets", "chamfer",
    "transform", "mirror", "instance", "make_unique", "array_rect", "array_radial", "array_curve", "join", "unjoin", "dissolve", "extract_components", "project_curve", "silhouette",
    "set_control_points", "raise_degree", "rebuild_face", "plane_from_face", "plane_three_points", "plane_two_points_camera", "plane_midplane", "add_measurement",
    "clearance", "fastener_hole",
    "add_joint", "set_joint", "connect_fixed", "add_motor", "mount_motor", "attach_motor", "set_ground", "infer_joints", "add_sensor", "add_cable",
    "set_robot_setting", "set_battery", "set_control", "set_uncertainty", "save_motion", "delete_motion", "set_material_props", "set_joint_physics",
];

/// Apply `Ops.<name>(*args, **kwargs)`: RoboCAD's return value.
pub fn run(cx: &mut Ctx, name: &str, args: &[Value], kwargs: &Map<String, Value>) -> Result<Value, String> {
    macro_rules! a {
        ($p:expr) => {
            A::new(name, $p, args, kwargs)
        };
    }
    let c = cx.cancelled;
    match name {
        // ---- nodes / outliner ----
        "rename" => {
            let a = a!(&["node_id", "name"])?;
            let id = a.str("node_id")?;
            crate::nodes::patch(cx.edit, id, &json!({"name": a.str("name")?}).as_object().cloned().expect("object"))?;
            Ok(Value::Null)
        }
        "set_visible" | "set_locked" | "set_disabled" => {
            let key = &name[4..];
            let a = match key { "visible" => a!(&["ids", "visible"]), "locked" => a!(&["ids", "locked"]), _ => a!(&["ids", "disabled"]) }?;
            let on = a.need(key)?.as_bool().ok_or_else(|| format!("{name}: {key} must be true or false"))?;
            for id in a.ids("ids")? {
                cx.node(&id)?;
                cx.set(&id, key, json!(on))?;
            }
            Ok(Value::Null)
        }
        "set_material" => {
            let a = a!(&["ids", "material_id"])?;
            let m = a.str("material_id")?;
            for id in a.ids("ids")? {
                cx.node(&id)?;
                cx.set(&id, "material", json!(m))?;
            }
            Ok(Value::Null)
        }
        "set_color" => {
            let a = a!(&["ids", "color"])?;
            let color = match a.get("color") {
                None => Value::Null,
                Some(v) => json!(v3(v).filter(|c| c.iter().all(|x| (0. ..=1.).contains(x))).ok_or("set_color: color must be [r, g, b] in 0–1, or null")?),
            };
            for id in a.ids("ids")? {
                cx.node(&id)?;
                cx.set(&id, "color", color.clone())?;
            }
            Ok(Value::Null)
        }
        "set_pivot" => {
            let a = a!(&["node_id", "pivot"])?;
            let id = a.str("node_id")?;
            cx.node(id)?;
            let p = a.opt_v3("pivot")?;
            cx.set(id, "pivot", json!(p))?;
            Ok(Value::Null)
        }
        "group" => {
            let a = a!(&["ids", "name"])?;
            let ids = selection_roots(cx, &a.ids("ids")?)?;
            if ids.iter().any(|i| cx.edit.node(i).is_some_and(|n| !n["component_member"].is_null())) {
                return Err("Group whole component occurrences or detach them first".into());
            }
            let parent = ids.first().and_then(|i| cx.edit.node(i)).and_then(|n| n["parent"].as_str().map(str::to_string));
            let g = cx.add_node("group", a.opt_str("name").unwrap_or("Group"), parent.as_deref(), Map::new())?;
            for i in &ids {
                crate::nodes::move_node(cx.edit, i, Some(&g), None)?;
            }
            Ok(json!(g))
        }
        "move_nodes" | "move_node" => {
            let a = if name == "move_nodes" { a!(&["ids", "new_parent", "index"])? } else { a!(&["node_id", "new_parent", "index"])? };
            let ids = selection_roots(cx, &a.ids(if name == "move_nodes" { "ids" } else { "node_id" })?)?;
            let parent = a.opt_str("new_parent");
            let index = a.opt_num("index")?.map(|i| i as usize);
            for (k, id) in ids.iter().enumerate() {
                crate::nodes::move_node(cx.edit, id, parent, index.map(|i| i + k))?;
            }
            Ok(Value::Null)
        }
        "set_active_group" => {
            let a = a!(&["group_id"])?;
            let g = a.opt_str("group_id");
            if let Some(g) = g
                && cx.node(g)?["kind"] != "group"
            {
                return Err(format!("{} is not a group", cx.name(g)));
            }
            cx.edit.manifest["active_group"] = json!(g);
            Ok(Value::Null)
        }
        "isolate" => {
            let a = a!(&["ids"])?;
            let ids = a.ids("ids")?;
            let mut keep: HashSet<String> = HashSet::new();
            for i in &ids {
                keep.extend(descendants(cx, i));
                let mut p = cx.edit.node(i).and_then(|n| n["parent"].as_str().map(str::to_string));
                while let Some(id) = p {
                    keep.insert(id.clone());
                    p = cx.edit.node(&id).and_then(|n| n["parent"].as_str().map(str::to_string));
                }
            }
            for n in cx.edit.manifest["nodes"].as_array_mut().into_iter().flatten() {
                let id = n["id"].as_str().unwrap_or("").to_string();
                n["visible"] = json!(keep.contains(&id));
            }
            Ok(Value::Null)
        }
        "show_all" => {
            for n in cx.edit.manifest["nodes"].as_array_mut().into_iter().flatten() {
                n["visible"] = json!(true);
            }
            Ok(Value::Null)
        }
        "delete" => {
            let a = a!(&["ids"])?;
            let ids = a.ids("ids")?;
            crate::nodes::delete(cx.edit, &ids)?;
            Ok(Value::Null)
        }
        // ---- primitives ----
        "box" | "box_center" => {
            let a = if name == "box" { a!(&["corner", "size", "name"]) } else { a!(&["center", "size", "name"]) }?;
            let size = a.v3("size")?;
            let corner = if name == "box" { a.v3("corner")? } else { sub(a.v3("center")?, scale(size, 0.5)) };
            if size.iter().any(|s| *s <= 0.) {
                return Err("a box needs three positive sizes".into());
            }
            let b = kernel::build(&Shape::Box { corner, size }, c)?;
            Ok(json!(cx.add_built(Built { kind: Kind::Solid, brep: b }, a.opt_str("name").unwrap_or("Box"), None, None)?))
        }
        "box_three_point" => {
            let a = a!(&["a", "b", "c", "height", "name"])?;
            let (pa, pb, pc) = (a.v3("a")?, a.v3("b")?, a.v3("c")?);
            let x = unit(sub(pb, pa));
            let w = sub(sub(pc, pa), scale(x, dot(sub(pc, pa), x)));
            let y = unit(w);
            let z = cross(x, y);
            let (len, wid) = (dist(pa, pb), dot(sub(pc, pa), y));
            let h = a.num("height")?;
            let pts = vec![pa, add(pa, scale(x, len)), add(add(pa, scale(x, len)), scale(y, wid)), add(pa, scale(y, wid))];
            let b = kernel::build(&Shape::Extrude { loops: vec![pts], direction: scale(z, h) }, c)?;
            Ok(json!(cx.add_built(Built { kind: Kind::Solid, brep: b }, a.opt_str("name").unwrap_or("Box"), None, None)?))
        }
        "cylinder" => {
            let a = a!(&["base", "axis", "radius", "height", "name"])?;
            let b = kernel::build(&Shape::Cylinder { base: a.v3("base")?, axis: a.v3("axis")?, radius: a.num("radius")?, height: a.num("height")? }, c)?;
            Ok(json!(cx.add_built(Built { kind: Kind::Solid, brep: b }, a.opt_str("name").unwrap_or("Cylinder"), None, None)?))
        }
        "sphere" => {
            let a = a!(&["center", "radius", "name"])?;
            let b = kernel::build(&Shape::Sphere { center: a.v3("center")?, radius: a.num("radius")? }, c)?;
            Ok(json!(cx.add_built(Built { kind: Kind::Solid, brep: b }, a.opt_str("name").unwrap_or("Sphere"), None, None)?))
        }
        "new_sketch" => {
            let a = a!(&["plane", "name"])?;
            let plane = cx.plane(a.need("plane")?)?;
            let nm = cx.unique_name(a.opt_str("name").unwrap_or("Sketch"));
            let mut extra = Map::new();
            extra.insert("sketch".into(), Sketch::new(plane, &nm).json());
            Ok(json!(cx.add_node("sketch", &nm, None, extra)?))
        }
        // ---- solids from sketches ----
        "extrude" => {
            let a = a!(&["source", "distance", "direction", "taper_deg", "symmetric", "op", "target", "up_to", "name"])?;
            let (prof, plane) = cx.profile(a.need("source")?)?;
            let d = match a.opt_v3("direction")? {
                Some(d) => d,
                None => plane.map_or([0., 0., 1.], |p| p.normal),
            };
            let body = match a.opt_str("up_to") {
                Some(t) => kernel::op1(Op::ExtrudeUpTo, &[&prof, &cx.body(t)?], &d, &[], c)?,
                None => {
                    let dist = a.num("distance")?;
                    if dist == 0. {
                        return Err("extrude distance is zero".into());
                    }
                    kernel::op1(Op::Extrude, &[&prof], &[&d[..], &[dist, a.num_or("taper_deg", 0.)?, f64::from(u8::from(a.flag("symmetric", false)))]].concat(), &[], c)?
                }
            };
            apply_boolean(cx, body, a.opt_str("op").unwrap_or("new"), a.opt_str("target"), a.opt_str("name").unwrap_or("Extrude"))
        }
        "revolve" => {
            let a = a!(&["source", "axis_point", "axis_dir", "angle_deg", "op", "target", "name"])?;
            let (prof, _) = cx.profile(a.need("source")?)?;
            let body = kernel::op1(Op::Revolve, &[&prof], &[&a.v3("axis_point")?[..], &a.v3("axis_dir")?, &[a.num_or("angle_deg", 360.)?]].concat(), &[], c)?;
            apply_boolean(cx, body, a.opt_str("op").unwrap_or("new"), a.opt_str("target"), a.opt_str("name").unwrap_or("Revolve"))
        }
        "sweep" => {
            let a = a!(&["profile", "path", "options", "name"])?;
            let (prof, _) = cx.profile(a.need("profile")?)?;
            let (path, _) = cx.profile(a.need("path")?)?;
            let o = a.get("options").cloned().unwrap_or(json!({}));
            let nums = [o["scale_end"].as_f64().unwrap_or(1.), f64::from(u8::from(o["frenet"].as_bool().unwrap_or(false))), f64::from(u8::from(o["corner"].as_str() == Some("round")))];
            let body = kernel::op1(Op::Sweep, &[&prof, &path], &nums, &[], c)?;
            Ok(json!(cx.add_built(body, a.opt_str("name").unwrap_or("Sweep"), None, None)?))
        }
        "pipe" => {
            let a = a!(&["path", "diameter", "name"])?;
            let (path, _) = cx.profile(a.need("path")?)?;
            let body = kernel::op1(Op::Pipe, &[&path], &[a.num("diameter")?], &[], c)?;
            Ok(json!(cx.add_built(body, a.opt_str("name").unwrap_or("Pipe"), None, None)?))
        }
        "loft" => {
            let a = a!(&["profiles", "guides", "solid", "ruled", "name"])?;
            let profiles: Vec<Vec<u8>> = a.need("profiles")?.as_array().ok_or("loft: profiles must be a list")?.iter().map(|p| cx.profile(p).map(|x| x.0)).collect::<Result<_, _>>()?;
            let refs: Vec<&[u8]> = profiles.iter().map(Vec::as_slice).collect();
            let body = kernel::op1(Op::Loft, &refs, &[f64::from(u8::from(a.flag("solid", true))), f64::from(u8::from(a.flag("ruled", false)))], &[], c)?;
            Ok(json!(cx.add_built(body, a.opt_str("name").unwrap_or("Loft"), None, None)?))
        }
        "fill" => {
            let a = a!(&["edges", "name"])?;
            let (e, _) = cx.profile(a.need("edges")?)?;
            Ok(json!(cx.add_built(kernel::op1(Op::Fill, &[&e], &[], &[], c)?, a.opt_str("name").unwrap_or("Patch"), None, None)?))
        }
        "bridge" => {
            let a = a!(&["a", "b", "name"])?;
            let (x, _) = cx.profile(a.need("a")?)?;
            let (y, _) = cx.profile(a.need("b")?)?;
            Ok(json!(cx.add_built(kernel::op1(Op::Bridge, &[&x, &y], &[], &[], c)?, a.opt_str("name").unwrap_or("Bridge"), None, None)?))
        }
        // ---- direct editing ----
        "push_pull" => {
            let a = a!(&["node_id", "face", "distance"])?;
            let id = a.str("node_id")?;
            let f = cx.face(a.need("face")?, id)?;
            let d = a.num("distance")?;
            cx.edit_body(id, |_, b| kernel::op1(Op::PushPull, &[&b], &[d], &[f], c))
        }
        "offset_faces" | "clearance" => {
            let a = if name == "offset_faces" { a!(&["node_id", "faces", "distance"])? } else { a!(&["node_id", "faces", "amount"])? };
            let id = a.str("node_id")?;
            let faces = cx.faces(a.need("faces")?, id)?;
            if name == "offset_faces" {
                let d = a.num("distance")?;
                return cx.edit_body(id, |_, b| kernel::op1(Op::OffsetFaces, &[&b], &[d], &faces, c));
            }
            // Holes grow, bosses shrink, planar faces offset inward (`clearance`).
            let amount = a.num_or("amount", 0.2)?;
            cx.edit_body(id, |_, b| {
                let mut out = b;
                for f in &faces {
                    let r = kernel::face_ref(&out, *f)?;
                    out = if r.cylinder {
                        kernel::op1(Op::SetRadius, &[&out], &[r.radius + if r.hole { amount } else { -amount }], &[*f], c)?.brep
                    } else {
                        kernel::op1(Op::PushPull, &[&out], &[-amount], &[*f], c)?.brep
                    };
                }
                Ok(Built { kind: Kind::Solid, brep: out })
            })
        }
        "offset_face_to" => {
            let a = a!(&["node_id", "face", "target", "clearance"])?;
            let id = a.str("node_id")?;
            let f = cx.face(a.need("face")?, id)?;
            let t = cx.body(a.str("target")?)?;
            let cl = a.num_or("clearance", 0.)?;
            cx.edit_body(id, |_, b| kernel::op1(Op::OffsetFaceTo, &[&b, &t], &[cl], &[f], c))
        }
        "move_faces" => {
            let a = a!(&["node_id", "faces", "translation"])?;
            let id = a.str("node_id")?;
            let faces = cx.faces(a.need("faces")?, id)?;
            let t = a.v3("translation")?;
            cx.edit_body(id, |_, b| kernel::op1(Op::MoveFaces, &[&b], &t, &faces, c))
        }
        "rotate_faces" => {
            let a = a!(&["node_id", "faces", "axis_point", "axis_dir", "angle_deg"])?;
            let id = a.str("node_id")?;
            let faces = cx.faces(a.need("faces")?, id)?;
            let nums = [&a.v3("axis_point")?[..], &a.v3("axis_dir")?, &[a.num("angle_deg")?]].concat();
            cx.edit_body(id, |_, b| kernel::op1(Op::RotateFaces, &[&b], &nums, &faces, c))
        }
        "set_radius" | "set_diameter" => {
            let a = if name == "set_radius" { a!(&["node_id", "face", "radius"]) } else { a!(&["node_id", "face", "diameter"]) }?;
            let id = a.str("node_id")?;
            let f = cx.face(a.need("face")?, id)?;
            let r = if name == "set_radius" { a.num("radius")? } else { a.num("diameter")? / 2. };
            cx.edit_body(id, |_, b| kernel::op1(Op::SetRadius, &[&b], &[r], &[f], c))
        }
        "set_distance" => {
            let a = a!(&["node_id", "face_a", "face_b", "distance", "move"])?;
            let id = a.str("node_id")?;
            let (fa, fb) = (cx.face(a.need("face_a")?, id)?, cx.face(a.need("face_b")?, id)?);
            let want = a.num("distance")?;
            let mv = a.opt_str("move").unwrap_or("b");
            cx.edit_body(id, |_, b| {
                let (ra, rb) = (kernel::face_ref(&b, fa)?, kernel::face_ref(&b, fb)?);
                if !(ra.plane && rb.plane) || dot(unit(ra.normal), unit(rb.normal)).abs() < 1. - 1e-6 {
                    return Err("a distance is set between two parallel planar faces".into());
                }
                let current = dot(sub(rb.centroid, ra.centroid), unit(ra.normal)).abs();
                kernel::op1(Op::PushPull, &[&b], &[want - current], &[if mv == "b" { fb } else { fa }], c)
            })
        }
        "set_angle" => {
            let a = a!(&["node_id", "face_a", "face_b", "angle_deg"])?;
            let id = a.str("node_id")?;
            let (fa, fb) = (cx.face(a.need("face_a")?, id)?, cx.face(a.need("face_b")?, id)?);
            let want = a.num("angle_deg")?;
            cx.edit_body(id, |_, b| {
                let (ra, rb) = (kernel::face_ref(&b, fa)?, kernel::face_ref(&b, fb)?);
                let current = dot(unit(ra.normal), unit(rb.normal)).clamp(-1., 1.).acos().to_degrees();
                let axis = unit(cross(ra.normal, rb.normal));
                // The edge the faces share: the topology's edge whose midpoint lies on both.
                let topo = kernel::topology(&b, c)?;
                let on = |p: [f64; 3], r: &kernel::FaceRef| dot(sub(p, r.centroid), unit(r.normal)).abs() < 1e-6;
                let point = topo.edges.iter().find(|e| on(e.midpoint, &ra) && on(e.midpoint, &rb)).map_or(rb.centroid, |e| e.midpoint);
                kernel::op1(Op::RotateFaces, &[&b], &[&point[..], &axis, &[want - current]].concat(), &[fb], c)
            })
        }
        "draft" => {
            let a = a!(&["node_id", "faces", "pull_dir", "angle_deg", "neutral"])?;
            let id = a.str("node_id")?;
            let faces = cx.faces(a.need("faces")?, id)?;
            let neutral = cx.plane(a.need("neutral")?)?;
            let nums = [&a.v3("pull_dir")?[..], &[a.num("angle_deg")?], &neutral.origin, &neutral.normal].concat();
            cx.edit_body(id, |_, b| kernel::op1(Op::Draft, &[&b], &nums, &faces, c))
        }
        "delete_faces" | "untrim" | "remove_fillets" => {
            let a = a!(&["node_id", "faces"])?;
            let id = a.str("node_id")?;
            let faces = cx.faces(a.need("faces")?, id)?;
            cx.edit_body(id, |_, b| kernel::op1(Op::DeleteFaces, &[&b], &[], &faces, c))
        }
        "imprint" => {
            let a = a!(&["node_id", "tool"])?;
            let id = a.str("node_id")?;
            let (t, _) = cx.profile(a.need("tool")?)?;
            cx.edit_body(id, |_, b| kernel::op1(Op::Imprint, &[&b, &t], &[], &[], c))
        }
        "split_face" => {
            let a = a!(&["node_id", "plane"])?;
            let id = a.str("node_id")?;
            let p = cx.plane(a.need("plane")?)?;
            // A plane sheet as the imprint tool (RoboCAD imprints the ±1e4 face).
            let sheet = plane_sheet(&p, c)?;
            cx.edit_body(id, |_, b| kernel::op1(Op::Imprint, &[&b, &sheet], &[], &[], c))
        }
        "boolean" => {
            let a = a!(&["target", "tools", "op", "keep_tools"])?;
            let target = a.str("target")?.to_string();
            let tools = a.ids("tools")?;
            let code = boolean_code(a.opt_str("op").unwrap_or("union"))?;
            if code < 0 {
                return Err("boolean: op must be union, subtract or intersect".into());
            }
            let mut inputs = vec![cx.body(&target)?];
            for t in &tools {
                inputs.push(cx.body(t)?);
            }
            let refs: Vec<&[u8]> = inputs.iter().map(Vec::as_slice).collect();
            let out = kernel::op1(Op::Boolean, &refs, &[f64::from(code)], &[], c)?;
            cx.replace(&target, out)?;
            if !a.flag("keep_tools", false) {
                let removable: Vec<String> = tools.iter().filter(|t| cx.edit.node(t).is_some_and(|n| n["kind"] != "instance")).cloned().collect();
                crate::nodes::delete(cx.edit, &removable)?;
            }
            Ok(json!(target))
        }
        "region" => {
            let a = a!(&["a", "b", "name"])?;
            let (x, y) = (cx.body(a.str("a")?)?, cx.body(a.str("b")?)?);
            let out = kernel::op1(Op::Boolean, &[&x, &y], &[2.], &[], c)?;
            Ok(json!(cx.add_built(out, a.opt_str("name").unwrap_or("Region"), None, None)?))
        }
        "cut" => {
            let a = a!(&["node_id", "cutter", "extend", "keep"])?;
            let id = a.str("node_id")?.to_string();
            let body = cx.body(&id)?;
            let keep = match a.opt_str("keep").unwrap_or("both") {
                "both" => 0.,
                "positive" => 1.,
                "negative" => 2.,
                other => return Err(format!("cut: keep must be both, positive or negative, not {other}")),
            };
            let cutter = a.need("cutter")?;
            let is_plane = cutter.is_object() || cutter.as_str().is_some_and(|s| matches!(s, "xy" | "xz" | "yz") || cx.edit.node(s).is_some_and(|n| n["kind"] == "plane"));
            let parts = if is_plane {
                let p = cx.plane(cutter)?;
                kernel::op(Op::SplitPlane, &[&body], &[&p.origin[..], &p.normal, &[keep]].concat(), &[], c)?
            } else {
                let (mut tool, plane) = cx.profile(cutter)?;
                let kind = cx.edit.node(cutter.as_str().unwrap_or("")).and_then(|n| n["kind"].as_str()).unwrap_or("");
                if a.flag("extend", true) && let Some(p) = plane.filter(|_| kind == "sketch") {
                    // A curve cuts by extruding it far both ways out of its plane.
                    let wires = Sketch::from_json(&cx.node(cutter.as_str().unwrap_or(""))?["sketch"])?.wires(&[], c)?;
                    tool = kernel::op1(Op::Extrude, &[&wires], &[&p.normal[..], &[1e4, 0., 1.]].concat(), &[], c).map(|b| b.brep).unwrap_or(tool);
                }
                kernel::op(Op::SplitTool, &[&body, &tool], &[], &[], c)?
            };
            let mut parts = parts.into_iter();
            let first = parts.next().ok_or("the cutter does not cross the body")?;
            cx.replace(&id, first)?;
            let (material, parent, base) = { let n = cx.node(&id)?; (n["material"].as_str().map(str::to_string), n["parent"].as_str().map(str::to_string), n["name"].as_str().unwrap_or("Body").to_string()) };
            let mut ids = vec![json!(id)];
            for p in parts {
                ids.push(json!(cx.add_built(p, &base, material.as_deref(), parent.as_deref())?));
            }
            Ok(Value::Array(ids))
        }
        "shell" => {
            let a = a!(&["node_id", "thickness", "open_faces"])?;
            let id = a.str("node_id")?;
            let t = a.num("thickness")?;
            let open = a.get("open_faces").map(|f| cx.faces(f, id)).transpose()?.unwrap_or_default();
            cx.edit_body(id, |_, b| kernel::op1(Op::Shell, &[&b], &[t], &open, c))
        }
        "thicken" => {
            let a = a!(&["node_id", "thickness"])?;
            let id = a.str("node_id")?;
            let t = a.num("thickness")?;
            cx.edit_body(id, |_, b| kernel::op1(Op::Thicken, &[&b], &[t], &[], c))
        }
        "fillet" => {
            let a = a!(&["node_id", "edges", "radius", "radius_end"])?;
            let id = a.str("node_id")?;
            let edges = cx.edges(a.need("edges")?, id)?;
            let (r, r_end) = (a.num("radius")?, a.num_or("radius_end", 0.)?);
            cx.edit_body(id, |_, b| kernel::op1(Op::Fillet, &[&b], &[r, r_end], &edges, c))
        }
        "fillet_chordal" => {
            let a = a!(&["node_id", "edges", "chord"])?;
            let id = a.str("node_id")?;
            let edges = cx.edges(a.need("edges")?, id)?;
            let chord = a.num("chord")?;
            cx.edit_body(id, |_, b| kernel::op1(Op::FilletChordal, &[&b], &[chord], &edges, c))
        }
        "fillet_all" => {
            let a = a!(&["node_id", "radius", "tension"])?;
            let id = a.str("node_id")?;
            let r = a.num("radius")? * a.num_or("tension", 1.)?;
            cx.edit_body(id, |_, b| Ok(Built { kind: Kind::Solid, brep: kernel::build(&Shape::Fillet { body: &b, radius: r, edges: vec![] }, c)? }))
        }
        "full_round" => {
            let a = a!(&["node_id", "edge_a", "edge_b"])?;
            let id = a.str("node_id")?;
            let (ea, eb) = (cx.edge(a.need("edge_a")?, id)?, cx.edge(a.need("edge_b")?, id)?);
            cx.edit_body(id, |_, b| kernel::op1(Op::FullRound, &[&b], &[], &[ea, eb], c))
        }
        "chamfer" => {
            let a = a!(&["node_id", "edges", "spec"])?;
            let id = a.str("node_id")?;
            let edges = cx.edges(a.need("edges")?, id)?;
            let spec = a.need("spec")?;
            let d = spec["distance"].as_f64().or_else(|| spec.as_f64()).ok_or("chamfer: spec needs a distance")?;
            if !spec["distance2"].is_null() || !spec["angle_deg"].is_null() {
                return Err("chamfer: an unequal or angled chamfer (distance2, angle_deg) is not available in process yet; use an equal distance".into());
            }
            cx.edit_body(id, |_, b| Ok(Built { kind: Kind::Solid, brep: kernel::build(&Shape::Chamfer { body: &b, distance: d, edges: edges.clone() }, c)? }))
        }
        // ---- arrange ----
        "transform" => {
            let a = a!(&["ids", "translation", "axis", "angle_deg", "center", "scale"])?;
            let ids = a.ids("ids")?;
            let center = a.opt_v3("center")?;
            let centers: Vec<[f64; 3]> = ids.iter().map(|id| center.or_else(|| cx.edit.node(id).and_then(|n| v3(&n["pivot"]))).map_or_else(|| cx.centroid_of(id), Ok)).collect::<Result<_, _>>()?;
            let (doc, stamps) = (cx.doc, cx.stamps);
            let moved = crate::nodes::transform(cx.edit, doc, stamps, &ids, &centers, a.opt_v3("translation")?.unwrap_or([0.; 3]), a.opt_v3("axis")?, a.num_or("angle_deg", 0.)?, a.num_or("scale", 1.)?)?;
            Ok(json!(moved))
        }
        "mirror" => {
            let a = a!(&["ids", "plane", "live", "keep_original"])?;
            let ids = a.ids("ids")?;
            let plane = cx.plane(a.need("plane")?)?;
            let live = a.flag("live", false);
            let mut out = Vec::new();
            for id in &ids {
                let n = cx.node(id)?.clone();
                if !n["component_instance"].is_null() || !n["component_member"].is_null() {
                    return Err("Mirror requires a detached component; rigid component placement preserves physical frames".into());
                }
                let nm = format!("{} mirror", n["name"].as_str().unwrap_or("Body"));
                let parent = n["parent"].as_str();
                let material = n["material"].as_str();
                if live {
                    let mut extra = Map::new();
                    extra.insert("source".into(), json!(id));
                    extra.insert("mirror_plane".into(), plane.json());
                    extra.insert("material".into(), json!(material));
                    out.push(cx.add_node("instance", &nm, parent, extra)?);
                } else {
                    let b = cx.body(id)?;
                    let m = kernel::op1(Op::Mirror, &[&b], &[plane.origin, plane.normal].concat(), &[], c)?;
                    out.push(cx.add_built(m, &nm, material, parent)?);
                }
            }
            if !a.flag("keep_original", true) {
                crate::nodes::delete(cx.edit, &ids)?;
            }
            Ok(json!(out))
        }
        "instance" => {
            let a = a!(&["source", "transform", "name"])?;
            let src = a.str("source")?;
            let n = cx.node(src)?.clone();
            if !n["component_instance"].is_null() || !n["component_member"].is_null() {
                return Err("Use Place in the component library to create another linked assembly".into());
            }
            let mut extra = Map::new();
            extra.insert("source".into(), json!(src));
            extra.insert("material".into(), n["material"].clone());
            if let Some(t) = a.get("transform") {
                extra.insert("transform".into(), t.clone());
            }
            let nm = a.opt_str("name").map(str::to_string).unwrap_or_else(|| format!("{} instance", n["name"].as_str().unwrap_or("Body")));
            Ok(json!(cx.add_node("instance", &nm, None, extra)?))
        }
        "make_unique" => {
            let a = a!(&["instance_id"])?;
            let id = a.str("instance_id")?;
            let n = cx.node(id)?.clone();
            let b = cx.body(id)?;
            let new = cx.add_built(Built { kind: Kind::Solid, brep: b }, n["name"].as_str().unwrap_or("Body"), n["material"].as_str(), n["parent"].as_str())?;
            crate::nodes::delete(cx.edit, &[id.to_string()])?;
            Ok(json!(new))
        }
        "array_rect" => {
            let a = a!(&["ids", "count", "spacing", "extent", "as_instances", "merge"])?;
            let count = a.need("count")?.as_array().filter(|c| c.len() == 3).and_then(|c| c.iter().map(|v| v.as_u64().map(|n| n.max(1) as usize)).collect::<Option<Vec<_>>>()).ok_or("array_rect: count must be [nx, ny, nz]")?;
            let spacing = match a.opt_v3("spacing")? {
                Some(s) => s,
                None => {
                    let e = a.opt_v3("extent")?.ok_or("give a spacing or a total extent")?;
                    [0, 1, 2].map(|i| e[i] / (count[i].max(2) - 1) as f64)
                }
            };
            let mut placements = Vec::new();
            for i in 0..count[0] {
                for j in 0..count[1] {
                    for k in 0..count[2] {
                        if (i, j, k) != (0, 0, 0) {
                            placements.push(([i as f64 * spacing[0], j as f64 * spacing[1], k as f64 * spacing[2]], None, 0.));
                        }
                    }
                }
            }
            array(cx, &a.ids("ids")?, &placements, a.flag("as_instances", false), a.flag("merge", false))
        }
        "array_radial" => {
            let a = a!(&["ids", "count", "axis_point", "axis_dir", "total_angle", "as_instances", "merge"])?;
            let count = a.num("count")? as usize;
            let total = a.num_or("total_angle", 360.)?;
            let step = if (total - 360.).abs() < 1e-9 { total / count as f64 } else { total / (count.max(2) - 1) as f64 };
            let (p, d) = (a.v3("axis_point")?, a.v3("axis_dir")?);
            let placements: Vec<_> = (1..count).map(|i| ([0.; 3], Some((p, d)), step * i as f64)).collect();
            array(cx, &a.ids("ids")?, &placements, a.flag("as_instances", false), a.flag("merge", false))
        }
        "array_curve" => {
            let a = a!(&["ids", "path", "count", "align", "as_instances", "merge"])?;
            let ids = a.ids("ids")?;
            let (path, _) = cx.profile(a.need("path")?)?;
            let count = a.num("count")? as usize;
            let topo = kernel::topology(&path, c)?;
            if topo.edges.is_empty() {
                return Err("the path has no edges".into());
            }
            // The path's edges chained by their ends (straight between them: a polyline approximation).
            let mut pts: Vec<[f64; 3]> = vec![topo.edges[0].start];
            for e in &topo.edges {
                if dist(*pts.last().expect("points"), e.start) > 1e-6 && dist(*pts.last().expect("points"), e.end) < 1e-6 {
                    pts.push(e.start);
                } else {
                    pts.push(e.midpoint);
                    pts.push(e.end);
                }
            }
            let lens: Vec<f64> = pts.windows(2).map(|w| dist(w[0], w[1])).collect();
            let total = lens.iter().sum::<f64>().max(1e-9);
            let first = cx.centroid_of(&ids[0])?;
            let align = a.flag("align", true);
            let mut placements = Vec::new();
            for n in 1..count {
                let target = total * n as f64 / (count - 1).max(1) as f64;
                let (mut acc, mut pos, mut tangent) = (0., *pts.last().expect("points"), unit(sub(pts[pts.len() - 1], pts[pts.len() - 2])));
                for (i, l) in lens.iter().enumerate() {
                    if acc + l >= target {
                        let t = if *l > 0. { (target - acc) / l } else { 0. };
                        pos = add(pts[i], scale(sub(pts[i + 1], pts[i]), t));
                        tangent = unit(sub(pts[i + 1], pts[i]));
                        break;
                    }
                    acc += l;
                }
                let offset = sub(pos, pts[0]);
                let t0 = unit(sub(pts[1], pts[0]));
                let ang = dot(t0, tangent).clamp(-1., 1.).acos().to_degrees();
                placements.push((offset, (align && ang > 1e-6).then(|| (add(first, offset), cross(t0, tangent))), if align { ang } else { 0. }));
            }
            array(cx, &ids, &placements, a.flag("as_instances", false), a.flag("merge", false))
        }
        "join" => {
            let a = a!(&["ids"])?;
            let ids = a.ids("ids")?;
            if ids.len() < 2 {
                return Err("join needs two or more bodies".into());
            }
            let bodies: Vec<Vec<u8>> = ids.iter().map(|i| cx.body(i)).collect::<Result<_, _>>()?;
            let refs: Vec<&[u8]> = bodies.iter().map(Vec::as_slice).collect();
            let joined = kernel::op1(Op::Join, &refs, &[], &[], c)?;
            cx.replace(&ids[0], joined)?;
            crate::nodes::delete(cx.edit, &ids[1..])?;
            Ok(json!(ids[0]))
        }
        "unjoin" => {
            let a = a!(&["node_id"])?;
            let id = a.str("node_id")?.to_string();
            let parts = kernel::op(Op::Unjoin, &[&cx.body(&id)?], &[], &[], c)?;
            if parts.len() <= 1 {
                return Ok(json!([id]));
            }
            let n = cx.node(&id)?.clone();
            let mut parts = parts.into_iter();
            cx.replace(&id, parts.next().expect("parts"))?;
            let mut out = vec![json!(id)];
            for p in parts {
                out.push(json!(cx.add_built(p, n["name"].as_str().unwrap_or("Body"), n["material"].as_str(), n["parent"].as_str())?));
            }
            Ok(Value::Array(out))
        }
        "dissolve" => {
            let a = a!(&["node_id"])?;
            let id = a.str("node_id")?;
            cx.edit_body(id, |_, b| kernel::op1(Op::Dissolve, &[&b], &[], &[], c))
        }
        "extract_components" => {
            let a = a!(&["node_id", "components", "expected_revision"])?;
            let id = a.str("node_id")?.to_string();
            if let Some(r) = a.get("expected_revision").and_then(Value::as_u64)
                && r != cx.doc.manifest["revision"].as_u64().unwrap_or(0)
            {
                return Err("the document changed since the solids were listed; list them again".into());
            }
            let comps = a.need("components")?.as_object().filter(|m| !m.is_empty()).ok_or("components must map nonempty names to lists of solid indices")?.clone();
            let mut ints = Vec::new();
            for (nm, list) in &comps {
                let l = list.as_array().filter(|_| !nm.trim().is_empty()).ok_or("components must map nonempty names to lists of solid indices")?;
                ints.push(l.len() as i32);
                for i in l {
                    ints.push(i.as_i64().ok_or("Solid index is out of range")? as i32);
                }
            }
            if cx.edit.manifest["nodes"].as_array().into_iter().flatten().any(|n| n["joint"]["parent"] == id.as_str() || n["joint"]["child"] == id.as_str()) {
                return Err("Extract components before assigning joints to the source body".into());
            }
            let mut out = kernel::op(Op::ExtractComponents, &[&cx.body(&id)?], &[], &ints, c)?.into_iter();
            cx.replace(&id, out.next().expect("remainder"))?;
            let n = cx.node(&id)?.clone();
            let mut made = Map::new();
            for ((nm, _), built) in comps.iter().zip(out) {
                let new = cx.add_built(built, nm.trim(), n["material"].as_str(), n["parent"].as_str())?;
                made.insert(nm.clone(), json!(new));
            }
            Ok(json!({"remainder": id, "components": made}))
        }
        "project_curve" => {
            let a = a!(&["sketch_or_curve", "onto", "direction", "name"])?;
            let (w, _) = cx.profile(a.need("sketch_or_curve")?)?;
            let onto = cx.body(a.str("onto")?)?;
            let out = kernel::op1(Op::ProjectCurve, &[&w, &onto], &a.v3("direction")?, &[], c)?;
            Ok(json!(cx.add_built(out, a.opt_str("name").unwrap_or("Projected curve"), None, None)?))
        }
        "silhouette" => {
            let a = a!(&["node_id", "plane", "name"])?;
            let b = cx.body(a.str("node_id")?)?;
            let p = cx.plane(a.need("plane")?)?;
            let out = kernel::op1(Op::Silhouette, &[&b], &[p.origin, p.normal, p.x_axis].concat(), &[], c)?;
            Ok(json!(cx.add_built(out, a.opt_str("name").unwrap_or("Silhouette"), None, None)?))
        }
        "set_control_points" => {
            let a = a!(&["node_id", "face", "points"])?;
            let id = a.str("node_id")?;
            let f = cx.face(a.need("face")?, id)?;
            let rows = a.need("points")?.as_array().ok_or("set_control_points: points must be rows of [x, y, z]")?;
            let nv = rows.first().and_then(Value::as_array).map_or(0, Vec::len);
            let mut nums = Vec::new();
            for r in rows {
                let r = r.as_array().filter(|r| r.len() == nv).ok_or("set_control_points: every row needs the same number of points")?;
                for p in r {
                    nums.extend(v3(p).ok_or("set_control_points: a point must be [x, y, z]")?);
                }
            }
            let ints = [f, rows.len() as i32, nv as i32];
            cx.edit_body(id, |_, b| kernel::op1(Op::SetControlPoints, &[&b], &nums, &ints, c))
        }
        "raise_degree" | "rebuild_face" => {
            let a = if name == "raise_degree" { a!(&["node_id", "face", "du", "dv"])? } else { a!(&["node_id", "face", "su", "sv", "degree"])? };
            let id = a.str("node_id")?;
            let f = cx.face(a.need("face")?, id)?;
            let (op, ints) = if name == "raise_degree" { (Op::RaiseDegree, vec![f, a.num("du")? as i32, a.num("dv")? as i32]) } else { (Op::RebuildFace, vec![f, a.num("su")? as i32, a.num("sv")? as i32, a.num_or("degree", 3.)? as i32]) };
            cx.edit_body(id, |_, b| kernel::op1(op, &[&b], &[], &ints, c))
        }
        // ---- planes, measurements ----
        "plane_from_face" | "plane_midplane" => {
            let a = if name == "plane_from_face" { a!(&["node_id", "face", "name"])? } else { a!(&["node_id", "face_a", "face_b", "name"])? };
            let id = a.str("node_id")?;
            let b = cx.body(id)?;
            let plane = if name == "plane_from_face" {
                let r = kernel::face_ref(&b, cx.face(a.need("face")?, id)?)?;
                plane_from_normal(r.centroid, r.normal)
            } else {
                let (ra, rb) = (kernel::face_ref(&b, cx.face(a.need("face_a")?, id)?)?, kernel::face_ref(&b, cx.face(a.need("face_b")?, id)?)?);
                let (pa, pb) = (plane_from_normal(ra.centroid, ra.normal), plane_from_normal(rb.centroid, rb.normal));
                let same = dot(pa.normal, pb.normal) >= 0.;
                let n = if same { pa.normal } else { scale(pa.normal, -1.) };
                let q = if same { pb.normal } else { scale(pb.normal, -1.) };
                Plane { origin: scale(add(pa.origin, pb.origin), 0.5), normal: unit(add(n, q)), x_axis: pa.x_axis }
            };
            add_plane(cx, plane, a.opt_str("name").unwrap_or(if name == "plane_from_face" { "Plane" } else { "Midplane" }))
        }
        "plane_three_points" => {
            let a = a!(&["a", "b", "c", "name"])?;
            let (pa, pb, pc) = (a.v3("a")?, a.v3("b")?, a.v3("c")?);
            let n = cross(sub(pb, pa), sub(pc, pa));
            if dot(n, n) < 1e-18 {
                return Err("the three points are collinear".into());
            }
            add_plane(cx, Plane { origin: pa, normal: unit(n), x_axis: unit(sub(pb, pa)) }, a.opt_str("name").unwrap_or("Plane"))
        }
        "plane_two_points_camera" => {
            let a = a!(&["a", "b", "view_dir", "name"])?;
            let (pa, pb) = (a.v3("a")?, a.v3("b")?);
            let x = unit(sub(pb, pa));
            let n = unit(cross(unit(cross(x, unit(a.v3("view_dir")?))), x));
            add_plane(cx, Plane { origin: pa, normal: n, x_axis: x }, a.opt_str("name").unwrap_or("Plane"))
        }
        "add_measurement" => {
            let a = a!(&["m", "name"])?;
            let mut extra = Map::new();
            extra.insert("measure".into(), a.need("m")?.clone());
            Ok(json!(cx.add_node("measure", a.opt_str("name").unwrap_or("Measurement"), None, extra)?))
        }
        // ---- print helpers ----
        "fastener_hole" => {
            let a = a!(&["node_id", "face", "point", "spec", "depth"])?;
            let id = a.str("node_id")?.to_string();
            let f = cx.face(a.need("face")?, &id)?;
            let point = a.v3("point")?;
            let spec = a.need("spec")?.clone();
            let depth = a.opt_num("depth")?;
            let b = cx.body(&id)?;
            let n = unit(kernel::face_ref(&b, f)?.normal);
            let tool = crate::robotics::fastener_tool(point, n, &spec, depth.unwrap_or(1e3), c)?;
            let cut = kernel::build(&Shape::Cut(vec![&b, &tool]), c)?;
            cx.replace(&id, Built { kind: Kind::Solid, brep: cut })?;
            let mut meta = cx.robot_meta(&id);
            let mut list = meta.get("fasteners").and_then(Value::as_array).cloned().unwrap_or_default();
            list.push(json!({"size": spec["size"].as_str().unwrap_or("M3"), "kind": spec["kind"].as_str().unwrap_or("clearance"), "point": point, "direction": scale(n, -1.), "depth": depth}));
            meta.insert("fasteners".into(), json!(list));
            cx.set(&id, "robot", Value::Object(meta))?;
            Ok(json!(id))
        }
        // ---- robotics ----
        "add_joint" => {
            let a = a!(&["type", "parent", "child", "pivot", "axis", "lower", "upper", "motor", "gear_ratio", "name"])?;
            let ty = a.str("type")?;
            if !crate::robotics::JOINT_TYPES.contains(&ty) {
                return Err(format!("joint type must be one of {:?}", crate::robotics::JOINT_TYPES));
            }
            let parent = a.opt_str("parent");
            let child = a.str("child")?;
            for b in parent.into_iter().chain([child]) {
                if !cx.is_body(b) {
                    return Err(format!("{b} is not a body"));
                }
            }
            let joint = json!({"type": ty, "parent": parent, "child": child, "pivot": a.v3("pivot")?, "axis": a.opt_v3("axis")?.unwrap_or([0., 0., 1.]), "lower": a.opt_num("lower")?, "upper": a.opt_num("upper")?, "motor": a.opt_str("motor"), "gear_ratio": a.num_or("gear_ratio", 1.)?, "damping": 0.0, "friction": 0.0, "home": 0.0, "stroke": 0.0});
            let nm = a.opt_str("name").map(str::to_string).unwrap_or_else(|| format!("{ty} {}", cx.name(child)));
            let mut extra = Map::new();
            extra.insert("joint".into(), joint);
            Ok(json!(cx.add_node("joint", &nm, None, extra)?))
        }
        "set_joint" => {
            let id = args.first().and_then(Value::as_str).or_else(|| kwargs.get("joint_id").and_then(Value::as_str)).ok_or("set_joint: missing joint_id")?.to_string();
            let n = cx.node(&id)?;
            if n["kind"] != "joint" || !n["joint"].is_object() {
                return Err(format!("{} is not a joint", cx.name(&id)));
            }
            let mut j = n["joint"].clone();
            for (k, v) in kwargs.iter().filter(|(k, _)| k.as_str() != "joint_id") {
                if j.get(k).is_some() {
                    j[k.as_str()] = v.clone();
                }
            }
            cx.set(&id, "joint", j)?;
            Ok(json!(id))
        }
        "connect_fixed" => {
            let a = a!(&["parent", "child", "at", "name"])?;
            let (parent, child) = (a.str("parent")?.to_string(), a.str("child")?.to_string());
            let at = match a.opt_v3("at")? {
                Some(p) => p,
                None => cx.centroid_of(&child)?,
            };
            let mut kw = Map::new();
            kw.insert("name".into(), json!(a.opt_str("name")));
            run(cx, "add_joint", &[json!("fixed"), json!(parent), json!(child), json!(at), json!([0., 0., 1.])], &kw)
        }
        "add_motor" => {
            let a = a!(&["spec_id", "mount_point", "shaft_dir", "rotation_deg", "mount_on", "cut_mount", "name"])?;
            let spec = crate::robotics::motor(a.str("spec_id")?)?;
            let (mp, sd, rot) = (a.v3("mount_point")?, a.v3("shaft_dir")?, a.num_or("rotation_deg", 0.)?);
            let (body, mut meta) = crate::robotics::motor_body(&spec, mp, sd, rot, c)?;
            let mount_on = a.opt_str("mount_on");
            meta.insert("mounted_on".into(), json!(mount_on));
            let mut extra = Map::new();
            extra.insert("body_kind".into(), json!("solid"));
            extra.insert("material".into(), json!(if spec.kind == "servo" { "abs" } else { "steel" }));
            extra.insert("robot".into(), Value::Object(meta));
            extra.insert("color".into(), json!(spec.color));
            let id = cx.add_node("body", a.opt_str("name").unwrap_or(&spec.name), None, extra)?;
            cx.edit.entries.insert(format!("brep/{id}.brep"), Some(body));
            if a.flag("cut_mount", false) && let Some(m) = mount_on.filter(|m| cx.is_body(m)) {
                let tool = crate::robotics::mount_holes_tool(&spec, mp, sd, rot, c)?;
                let b = cx.body(m)?;
                let cut = kernel::build(&Shape::Cut(vec![&b, &tool]), c)?;
                cx.replace(m, Built { kind: Kind::Solid, brep: cut })?;
            }
            Ok(json!(id))
        }
        "mount_motor" => {
            let a = a!(&["motor_id", "body_id"])?;
            let id = a.str("motor_id")?;
            let mut meta = cx.robot_meta(id);
            if meta.get("kind").and_then(Value::as_str) != Some("motor") {
                return Err(format!("{} is not a motor", cx.name(id)));
            }
            meta.insert("mounted_on".into(), json!(a.opt_str("body_id")));
            cx.set(id, "robot", Value::Object(meta))?;
            Ok(json!(id))
        }
        "attach_motor" => {
            let a = a!(&["joint_id", "motor_id", "gear_ratio"])?;
            let jid = a.str("joint_id")?.to_string();
            let j = cx.node(&jid)?.clone();
            if j["kind"] != "joint" {
                return Err("not a joint".into());
            }
            let motor = a.opt_str("motor_id");
            if let Some(m) = motor {
                let mut meta = cx.robot_meta(m);
                if meta.get("kind").and_then(Value::as_str) != Some("motor") {
                    return Err(format!("{} is not a motor", cx.name(m)));
                }
                meta.insert("drives".into(), json!(jid));
                if meta.get("mounted_on").is_none_or(Value::is_null) {
                    meta.insert("mounted_on".into(), j["joint"]["parent"].clone());
                }
                cx.set(m, "robot", Value::Object(meta))?;
            }
            let mut joint = j["joint"].clone();
            joint["motor"] = json!(motor);
            joint["gear_ratio"] = json!(a.num_or("gear_ratio", 1.)?);
            cx.set(&jid, "joint", joint)?;
            Ok(json!(jid))
        }
        "set_ground" => {
            let a = a!(&["body_id", "ground"])?;
            let id = a.str("body_id")?;
            let mut meta = cx.robot_meta(id);
            meta.insert("ground".into(), json!(a.flag("ground", true)));
            cx.set(id, "robot", Value::Object(meta))?;
            Ok(json!(id))
        }
        "infer_joints" => {
            A::new(name, &[], args, kwargs)?;
            let found = crate::robotics::infer_joints(cx)?;
            let mut out = Vec::new();
            for j in found {
                let nm = format!("revolute {}", cx.name(j["child"].as_str().unwrap_or("")));
                let mut extra = Map::new();
                extra.insert("joint".into(), j);
                out.push(cx.add_node("joint", &nm, None, extra)?);
            }
            Ok(json!(out))
        }
        "add_sensor" => {
            let kind = args.first().and_then(Value::as_str).or_else(|| kwargs.get("kind").and_then(Value::as_str)).ok_or("add_sensor: missing kind")?;
            if !matches!(kind, "imu" | "encoder" | "current" | "force") {
                return Err("sensor kind must be imu, encoder, current or force".into());
            }
            let get = |i: usize, k: &str| args.get(i).or_else(|| kwargs.get(k)).filter(|v| !v.is_null());
            let body = get(1, "body").and_then(Value::as_str).ok_or("add_sensor: missing body")?;
            if !cx.is_body(body) {
                return Err(format!("{body} is not a body"));
            }
            let point = get(2, "point").and_then(v3).ok_or("add_sensor: point must be [x, y, z]")?;
            let joint = get(5, "joint").and_then(Value::as_str);
            let mut meta = json!({"kind": kind, "body": body, "point": point, "axes": get(3, "axes"), "joint": joint, "joint_name": joint.and_then(|j| cx.edit.node(j)).map(|n| n["name"].clone())});
            for (k, v) in kwargs.iter().filter(|(k, _)| ["rate_hz", "noise", "bias", "bias_walk", "quantization", "range"].contains(&k.as_str())) {
                meta[k.as_str()] = v.clone();
            }
            let nm = get(4, "name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("{kind} on {}", cx.name(body)));
            let mut extra = Map::new();
            extra.insert("robot".into(), meta);
            Ok(json!(cx.add_node("sensor", &nm, None, extra)?))
        }
        "add_cable" => {
            let a = a!(&["from_body", "from_point", "to_body", "to_point", "length", "mass", "stiffness", "name", "damping", "segments"])?;
            let (fb, tb) = (a.str("from_body")?, a.str("to_body")?);
            for b in [fb, tb] {
                if !cx.is_body(b) {
                    return Err(format!("{b} is not a body"));
                }
            }
            let meta = json!({"kind": "cable", "from_body": fb, "from_point": a.v3("from_point")?, "to_body": tb, "to_point": a.v3("to_point")?, "length": a.opt_num("length")?.map(|l| l * 1e-3), "mass": a.opt_num("mass")?, "stiffness": a.opt_num("stiffness")?, "damping": a.opt_num("damping")?, "segments": a.num_or("segments", 4.)? as i64});
            let nm = a.opt_str("name").map(str::to_string).unwrap_or_else(|| format!("cable {}-{}", cx.name(fb), cx.name(tb)));
            let mut extra = Map::new();
            extra.insert("robot".into(), meta);
            Ok(json!(cx.add_node("cable", &nm, None, extra)?))
        }
        "set_robot_setting" => {
            let a = a!(&["key", "value"])?;
            let key = a.str("key")?;
            set_setting(cx, key, a.get("value").cloned().unwrap_or(Value::Null));
            Ok(cx.edit.manifest["robot_settings"].clone())
        }
        "set_battery" => {
            let a = a!(&["cells", "chemistry", "capacity_ah", "internal_resistance", "initial_soc"])?;
            let cells = a.num_or("cells", 2.)?;
            if !(cells >= 1. && cells.fract() == 0. && cells <= 64.) {
                return Err(format!("set_battery: cells must be a whole number of cells (1–64), not {cells}"));
            }
            let chem = a.opt_str("chemistry").unwrap_or("lipo");
            let pick = |lipo: f64, liion: f64, nimh: f64, alkaline: f64, lifepo4: f64, other: f64| match chem { "lipo" => lipo, "liion" => liion, "nimh" => nimh, "alkaline" => alkaline, "lifepo4" => lifepo4, _ => other };
            let nominal = pick(3.7, 3.6, 1.2, 1.5, 3.2, 3.7);
            let r = match a.opt_num("internal_resistance")? { Some(r) => r, None => pick(0.02, 0.05, 0.03, 0.15, 0.02, 0.03) * cells };
            let cutoff = pick(3.0, 3.0, 0.9, 0.9, 2.5, 3.0) * cells;
            set_setting(cx, "battery", json!({"cells": cells as u32, "chemistry": chem, "nominal_voltage": nominal * cells, "internal_resistance": r, "capacity_ah": a.num_or("capacity_ah", 1.)?, "initial_soc": a.num_or("initial_soc", 1.)?, "cutoff_voltage": cutoff}));
            Ok(cx.edit.manifest["robot_settings"].clone())
        }
        "set_control" => {
            let a = a!(&["period_s", "latency_s", "targets", "mode", "trajectory"])?;
            let (period, latency) = (a.num_or("period_s", 0.02)?, a.num_or("latency_s", 0.004)?);
            let mode = a.opt_str("mode").unwrap_or("hold");
            if !matches!(mode, "hold" | "trajectory") {
                return Err(format!("control mode must be 'hold' or 'trajectory', not {mode:?}"));
            }
            if !(period > 0. && latency >= 0.) {
                return Err("control period must be positive and latency nonnegative".into());
            }
            set_setting(cx, "control", json!({"period_s": period, "latency_s": latency, "targets": a.get("targets").cloned().unwrap_or(json!({})), "mode": mode, "trajectory": a.get("trajectory").cloned().unwrap_or(json!([]))}));
            Ok(cx.edit.manifest["robot_settings"].clone())
        }
        "set_uncertainty" => {
            let mut u = cx.edit.manifest["robot_settings"]["uncertainty"].as_object().cloned().unwrap_or_default();
            for (k, v) in kwargs {
                u.insert(k.clone(), if v.is_object() || k == "seed" { v.clone() } else if k.ends_with("_m") { json!({"sigma": v}) } else { json!({"sigma_fraction": v}) });
            }
            set_setting(cx, "uncertainty", Value::Object(u));
            Ok(cx.edit.manifest["robot_settings"].clone())
        }
        "save_motion" => {
            let a = a!(&["program"])?;
            let p = a.need("program")?;
            let pname = p["name"].as_str().filter(|n| !n.trim().is_empty()).ok_or("a motion pattern needs a name")?;
            let mut programs = cx.edit.manifest["robot_settings"]["motion_programs"].as_object().cloned().unwrap_or_default();
            programs.insert(pname.to_string(), p.clone());
            set_setting(cx, "motion_programs", Value::Object(programs));
            Ok(p.clone())
        }
        "delete_motion" => {
            let a = a!(&["name"])?;
            let mut programs = cx.edit.manifest["robot_settings"]["motion_programs"].as_object().cloned().unwrap_or_default();
            programs.remove(a.str("name")?).ok_or("Motion pattern not found")?;
            set_setting(cx, "motion_programs", Value::Object(programs));
            Ok(Value::Null)
        }
        "set_material_props" => {
            let id = args.first().and_then(Value::as_str).or_else(|| kwargs.get("material_id").and_then(Value::as_str)).ok_or("set_material_props: missing material_id")?.to_string();
            let m = cx.edit.material_mut(&id).ok_or_else(|| format!("unknown material {id}"))?;
            let eng = &mut m["engineering"];
            if !eng.is_object() {
                *eng = json!({});
            }
            for (k, v) in kwargs.iter().filter(|(k, _)| k.as_str() != "material_id") {
                if k == "density" {
                    m["density"] = v.clone();
                } else {
                    m["engineering"][k.as_str()] = v.clone();
                }
            }
            Ok(json!(id))
        }
        "set_joint_physics" => {
            let id = args.first().and_then(Value::as_str).or_else(|| kwargs.get("joint_id").and_then(Value::as_str)).ok_or("set_joint_physics: missing joint_id")?.to_string();
            let n = cx.node(&id)?.clone();
            if n["kind"] != "joint" {
                return Err(format!("{} is not a joint", cx.name(&id)));
            }
            // RoboCAD keeps the overrides in `robot.physics` (commands.py set_joint_physics):
            // nested blocks merge, a scalar `backlash` declares an estimated drive gap.
            let mut meta = cx.robot_meta(&id);
            let mut phys = meta.get("physics").and_then(Value::as_object).cloned().unwrap_or_default();
            // Values an earlier in-process edit stored under `joint_physics` move over.
            phys.extend(n["joint_physics"].as_object().cloned().unwrap_or_default());
            for (k, v) in kwargs.iter().filter(|(k, _)| k.as_str() != "joint_id") {
                match (phys.get_mut(k).and_then(Value::as_object_mut), v.as_object()) {
                    _ if v.is_null() => {
                        phys.remove(k);
                    }
                    (Some(old), Some(new)) => old.extend(new.clone()),
                    _ => {
                        phys.insert(k.clone(), v.clone());
                    }
                }
            }
            if kwargs.contains_key("backlash") && !kwargs.contains_key("drive_backlash")
                && let Some(w) = kwargs["backlash"].as_f64()
            {
                phys.insert("drive_backlash".into(), json!({"width_rad": w, "provenance": "estimated", "reference": "legacy scalar backlash override; interpreted as an estimated drive-connection gap"}));
            }
            if let Some(db) = phys.get("drive_backlash") {
                let width = db["width_rad"].as_f64();
                let provenance = db["provenance"].as_str().unwrap_or("");
                let valid = matches!(provenance, "unmeasured" | "estimated" | "measured" | "derived")
                    && db["reference"].as_str().is_some_and(|r| !r.trim().is_empty())
                    && width.is_none_or(|w| w.is_finite() && w >= 0.0)
                    && (provenance == "unmeasured") == width.is_none();
                if !valid {
                    return Err("drive_backlash needs {width_rad, provenance, reference}: provenance unmeasured | estimated | measured | derived, a nonempty reference, and finite nonnegative radians exactly when not unmeasured".into());
                }
            }
            meta.insert("physics".into(), Value::Object(phys));
            cx.set(&id, "robot", Value::Object(meta))?;
            if !n["joint_physics"].is_null() {
                cx.set(&id, "joint_physics", Value::Null)?;
            }
            Ok(json!(id))
        }
        other => Err(format!("{other}: not an operation of the in-process editor (it implements {})", METHODS.len())),
    }
}

fn set_setting(cx: &mut Ctx, key: &str, value: Value) {
    let s = cx.edit.object_mut("robot_settings");
    if value.is_null() {
        s.remove(key);
    } else {
        s.insert(key.to_string(), value);
    }
}

fn descendants(cx: &Ctx, id: &str) -> Vec<String> {
    let mut out = vec![id.to_string()];
    let mut i = 0;
    while i < out.len() {
        let kids: Vec<String> = cx.edit.node(&out[i]).and_then(|n| n["children"].as_array().cloned()).into_iter().flatten().filter_map(|c| c.as_str().map(str::to_string)).collect();
        out.extend(kids);
        i += 1;
    }
    out
}

/// `_selection_roots`: the ids whose ancestors are not also selected.
fn selection_roots(cx: &Ctx, ids: &[String]) -> Result<Vec<String>, String> {
    let selected: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    for id in ids {
        cx.node(id)?;
        let mut p = cx.edit.node(id).and_then(|n| n["parent"].as_str().map(str::to_string));
        let mut covered = false;
        while let Some(pid) = p {
            if selected.contains(pid.as_str()) {
                covered = true;
                break;
            }
            p = cx.edit.node(&pid).and_then(|n| n["parent"].as_str().map(str::to_string));
        }
        if !covered && !out.contains(id) {
            out.push(id.clone());
        }
    }
    Ok(out)
}

fn apply_boolean(cx: &mut Ctx, body: Built, op: &str, target: Option<&str>, name: &str) -> Result<Value, String> {
    let code = boolean_code(op)?;
    match (code, target) {
        (-1, _) | (_, None) => Ok(json!(cx.add_built(body, name, None, None)?)),
        (code, Some(t)) => {
            let tb = cx.body(t)?;
            let out = kernel::op1(Op::Boolean, &[&tb, &body.brep], &[f64::from(code)], &[], cx.cancelled)?;
            cx.replace(t, out)?;
            Ok(json!(t))
        }
    }
}

fn add_plane(cx: &mut Ctx, plane: Plane, name: &str) -> Result<Value, String> {
    let mut extra = Map::new();
    extra.insert("plane".into(), plane.json());
    Ok(json!(cx.add_node("plane", name, None, extra)?))
}

/// A ±10 m square sheet in `plane` (RoboCAD's `BRepBuilderAPI_MakeFace(pl, -1e4, 1e4, …)`).
fn plane_sheet(p: &Plane, c: &dyn Fn() -> bool) -> Result<Vec<u8>, String> {
    let mut s = Sketch::new(*p, "plane");
    s.call(&json!(["rectangle", [[-1e4, -1e4], [2e4, 2e4]]]))?;
    s.profile(&[], c)
}

type Placement = ([f64; 3], Option<([f64; 3], [f64; 3])>, f64);
/// RoboCAD's `_array`: copies (bodies, or instances) or one merged body per source.
fn array(cx: &mut Ctx, ids: &[String], placements: &[Placement], as_instances: bool, merge: bool) -> Result<Value, String> {
    let mut out = Vec::new();
    for id in ids {
        let n = cx.node(id)?.clone();
        let src = cx.body(id)?;
        let base = format!("{} copy", n["name"].as_str().unwrap_or("Body"));
        let mut merged: Option<Vec<u8>> = None;
        for (t, rot, ang) in placements {
            if as_instances {
                let axis = rot.map_or([0., 0., 1.], |r| r.1);
                let mut extra = Map::new();
                extra.insert("source".into(), json!(id));
                extra.insert("material".into(), n["material"].clone());
                extra.insert("transform".into(), json!({"translation": t, "axis": axis, "angle_deg": ang, "scale": 1.0}));
                out.push(json!(cx.add_node("instance", &base, n["parent"].as_str(), extra)?));
                continue;
            }
            let m = match rot {
                Some((center, axis)) => kernel::placement(*t, Some(*axis), *ang, *center, 1.)?,
                None => kernel::placement(*t, None, 0., [0.; 3], 1.)?,
            };
            let b = kernel::build(&Shape::Transform { body: &src, matrix: m }, cx.cancelled)?;
            if merge && n["kind"] == "body" {
                let acc = merged.take().unwrap_or_else(|| src.clone());
                merged = Some(kernel::build(&Shape::Fuse(vec![&acc, &b]), cx.cancelled)?);
            } else {
                out.push(json!(cx.add_built(Built { kind: Kind::Solid, brep: b }, &base, n["material"].as_str(), n["parent"].as_str())?));
            }
        }
        if let Some(m) = merged {
            cx.replace(id, Built { kind: Kind::Solid, brep: m })?;
            out.insert(0, json!(id));
        }
    }
    Ok(Value::Array(out))
}

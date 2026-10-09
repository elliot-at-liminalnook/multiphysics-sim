//! `cad_model`: direct modelling in process, as typed operations a person's
//! tools and an agent's REST calls share. Each is one undoable edit on a job
//! (`local::start`): OCCT builds the B-rep (`sim_cad::kernel`), the archive
//! takes it (`sim_cad::nodes`), the snapshot is rebuilt reusing every body
//! the operation did not touch. Millimetres, the archive's frame (Z up).
//!
//! - New bodies: `box` (corner, size), `cylinder` (base, axis, radius,
//!   height), `sphere` (center, radius), `cone` (base, axis, radius,
//!   radius2, height), `extrude` (closed planar `loops`: the outer first,
//!   then holes; swept along `direction`). `name`, `parent` (a group),
//!   `material` (default PLA).
//! - `group`: a new empty group (`name`, `parent`).
//! - Booleans on `target` with `tools` (RoboCAD's combine): `fuse`, `cut`,
//!   `common`; the tools are deleted unless `keep_tools`.
//! - `fillet` (radius) and `chamfer` (distance) on `node`'s `edges`
//!   (indices from `topology`; none: every line and circle edge for a
//!   fillet, every edge for a chamfer).
//! - `move`: `ids` by `translation`, `axis` + `angle_deg` and uniform
//!   `scale` about `center` (else each body's centroid); RoboCAD's
//!   `Ops.transform` (bodies baked; attached comment pins follow).
//! - `topology` (read only): `node`'s faces and edges by index, with type,
//!   centre or midpoint, normal, area or length, so a caller can name them.
use super::actions::{CAD, Cx, local_edit_at};
use super::document::EditDone;
use crate::app::actions::{Call, Spec, spec};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sim_api::Outcome;
use sim_cad::kernel::{self, Shape};
use sim_cad::nodes;

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelOp {
    Box,
    Cylinder,
    Sphere,
    Cone,
    Extrude,
    Group,
    Fuse,
    Cut,
    Common,
    Fillet,
    Chamfer,
    Move,
    Topology,
    /// RoboCAD operations in order as one all-or-nothing edit.
    Batch,
}

/// `cad_model`'s arguments (see the module doc for which op takes which).
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModelArgs {
    pub op: ModelOp,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub material: Option<String>,
    #[serde(default)]
    pub corner: Option<[f64; 3]>,
    #[serde(default)]
    pub size: Option<[f64; 3]>,
    #[serde(default)]
    pub base: Option<[f64; 3]>,
    #[serde(default)]
    pub center: Option<[f64; 3]>,
    #[serde(default)]
    pub axis: Option<[f64; 3]>,
    #[serde(default)]
    pub radius: Option<f64>,
    #[serde(default)]
    pub radius2: Option<f64>,
    #[serde(default)]
    pub height: Option<f64>,
    #[serde(default)]
    pub loops: Option<Vec<Vec<[f64; 3]>>>,
    #[serde(default)]
    pub direction: Option<[f64; 3]>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    #[serde(default)]
    pub keep_tools: bool,
    #[serde(default)]
    pub node: Option<String>,
    #[serde(default)]
    pub edges: Option<Vec<i32>>,
    #[serde(default)]
    pub distance: Option<f64>,
    #[serde(default)]
    pub ids: Option<Vec<String>>,
    #[serde(default)]
    pub translation: Option<[f64; 3]>,
    #[serde(default)]
    pub angle_deg: Option<f64>,
    #[serde(default)]
    pub scale: Option<f64>,
    /// batch: `[{op, args, kwargs, as}]`; `{"$ref": alias}` names an earlier result.
    #[serde(default)]
    pub operations: Option<Vec<serde_json::Value>>,
}

fn need<T: Clone>(v: &Option<T>, what: &str, op: ModelOp) -> Result<T, String> {
    v.clone().ok_or_else(|| format!("{op:?} needs {what}").to_lowercase())
}

fn name_of(ws: &super::local::Workspace, id: &str) -> String {
    ws.archive.node(id).and_then(|n| n["name"].as_str()).unwrap_or(id).to_string()
}

/// The new body `brep` as `name` under `parent`: its id.
fn add(ws: &mut super::local::Workspace, args: &ModelArgs, default: &str, brep: Vec<u8>) -> Result<EditDone, String> {
    let name = args.name.clone().unwrap_or_else(|| default.to_string());
    let id = nodes::add_body(&mut ws.edit, &name, args.parent.as_deref(), args.material.as_deref(), brep)?;
    Ok(EditDone { message: format!("Added {name}"), result: json!({"id": id, "name": name}) })
}

fn run(ws: &mut super::local::Workspace, args: &ModelArgs) -> Result<EditDone, String> {
    let op = args.op;
    let cancelled = ws.cancelled;
    match op {
        ModelOp::Box => {
            let brep = kernel::build(&Shape::Box { corner: args.corner.unwrap_or([0.; 3]), size: need(&args.size, "size [dx, dy, dz]", op)? }, cancelled)?;
            add(ws, args, "Box", brep)
        }
        ModelOp::Cylinder => {
            let brep = kernel::build(&Shape::Cylinder { base: args.base.unwrap_or([0.; 3]), axis: args.axis.unwrap_or([0., 0., 1.]), radius: need(&args.radius, "radius", op)?, height: need(&args.height, "height", op)? }, cancelled)?;
            add(ws, args, "Cylinder", brep)
        }
        ModelOp::Sphere => {
            let brep = kernel::build(&Shape::Sphere { center: args.center.unwrap_or([0.; 3]), radius: need(&args.radius, "radius", op)? }, cancelled)?;
            add(ws, args, "Sphere", brep)
        }
        ModelOp::Cone => {
            let brep = kernel::build(&Shape::Cone { base: args.base.unwrap_or([0.; 3]), axis: args.axis.unwrap_or([0., 0., 1.]), radius1: need(&args.radius, "radius", op)?, radius2: args.radius2.unwrap_or(0.), height: need(&args.height, "height", op)? }, cancelled)?;
            add(ws, args, "Cone", brep)
        }
        ModelOp::Extrude => {
            let loops = need(&args.loops, "loops [[[x, y, z], ...], ...]", op)?;
            let brep = kernel::build(&Shape::Extrude { loops, direction: need(&args.direction, "direction [dx, dy, dz]", op)? }, cancelled)?;
            add(ws, args, "Extrusion", brep)
        }
        ModelOp::Group => {
            let name = need(&args.name, "name", op)?;
            let id = nodes::add_group(&mut ws.edit, &name, args.parent.as_deref())?;
            Ok(EditDone { message: format!("Added group {name}"), result: json!({"id": id, "name": name}) })
        }
        ModelOp::Fuse | ModelOp::Cut | ModelOp::Common => {
            let target = need(&args.target, "target", op)?;
            let tools = need(&args.tools, "tools", op)?;
            if tools.is_empty() || tools.contains(&target) {
                return Err("a boolean needs tools other than its target".into());
            }
            let mut inputs = vec![nodes::body_bytes(ws.archive, &target)?];
            for t in &tools {
                inputs.push(nodes::body_bytes(ws.archive, t)?);
            }
            let shape = match op {
                ModelOp::Fuse => Shape::Fuse(inputs),
                ModelOp::Cut => Shape::Cut(inputs),
                _ => Shape::Common(inputs),
            };
            let brep = kernel::build(&shape, cancelled)?;
            nodes::replace_body(&mut ws.edit, ws.stamps, &target, brep)?;
            if !args.keep_tools {
                nodes::delete(&mut ws.edit, &tools)?;
            }
            let verb = match op { ModelOp::Fuse => "Joined", ModelOp::Cut => "Cut", _ => "Intersected" };
            Ok(EditDone { message: format!("{verb} {} with {}", name_of(ws, &target), tools.iter().map(|t| name_of(ws, t)).collect::<Vec<_>>().join(", ")), result: json!({"id": target, "tools_deleted": !args.keep_tools}) })
        }
        ModelOp::Fillet | ModelOp::Chamfer => {
            let node = need(&args.node, "node", op)?;
            let bytes = nodes::body_bytes(ws.archive, &node)?;
            let edges = args.edges.clone().unwrap_or_default();
            let shape = if op == ModelOp::Fillet {
                Shape::Fillet { body: bytes, radius: need(&args.radius, "radius", op)?, edges: edges.clone() }
            } else {
                Shape::Chamfer { body: bytes, distance: need(&args.distance, "distance", op)?, edges: edges.clone() }
            };
            let brep = kernel::build(&shape, cancelled)?;
            nodes::replace_body(&mut ws.edit, ws.stamps, &node, brep)?;
            let what = if edges.is_empty() { "every edge".to_string() } else { format!("{} edge(s)", edges.len()) };
            Ok(EditDone { message: format!("{} {} of {}", if op == ModelOp::Fillet { "Filleted" } else { "Chamfered" }, what, name_of(ws, &node)), result: json!({"id": node}) })
        }
        ModelOp::Move => {
            let ids = need(&args.ids, "ids", op)?;
            let centers: Vec<[f64; 3]> = ids.iter().map(|id| args.center.or_else(|| ws.centroid(id)).unwrap_or([0.; 3])).collect();
            let (archive, stamps) = (ws.archive, ws.stamps);
            let moved = nodes::transform(&mut ws.edit, archive, stamps, &ids, &centers, args.translation.unwrap_or([0.; 3]), args.axis, args.angle_deg.unwrap_or(0.), args.scale.unwrap_or(1.))?;
            Ok(EditDone { message: format!("Moved {}", moved.iter().map(|id| name_of(ws, id)).collect::<Vec<_>>().join(", ")), result: json!({"moved": moved, "skipped_locked": ids.iter().filter(|i| !moved.contains(i)).collect::<Vec<_>>()}) })
        }
        ModelOp::Batch => {
            let operations = need(&args.operations, "operations", op)?;
            let n = operations.len();
            let request = crate::cad::types::candidates::CandidateRequest {
                document_id: ws.archive.manifest["document_id"].as_str().unwrap_or("").to_string(),
                expected_revision: ws.archive.manifest["revision"].as_u64().unwrap_or(0),
                label: "Batch".into(),
                operations,
            };
            let out = ws.batch(&request).map_err(|e| e.message)?;
            Ok(EditDone { message: format!("Applied {n} operation(s) as one edit"), result: out["results"].clone() })
        }
        ModelOp::Topology => {
            let node = need(&args.node, "node", op)?;
            let topo = kernel::topology(nodes::body_bytes(ws.archive, &node)?, cancelled)?;
            Ok(EditDone { message: format!("{}: {} faces, {} edges", name_of(ws, &node), topo.faces.len(), topo.edges.len()), result: json!({"node": node, "faces": topo.faces, "edges": topo.edges}) })
        }
    }
}

/// `cad_model`, from any entry point.
pub(super) fn handle(args: &ModelArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    let label = match args.op {
        ModelOp::Topology => format!("Read topology of {}", args.node.as_deref().map_or("?".to_string(), |n| cx.doc.node_name(n))),
        op => format!("{op:?}"),
    };
    // Reading topology changes nothing (no undo step), and may run during a preview.
    let auxiliary = args.op == ModelOp::Topology;
    let args = args.clone();
    local_edit_at(cx.doc, call, None, label, auxiliary, move |ws| run(ws, &args))
}

pub(super) fn specs() -> Vec<Spec> {
    vec![spec(
        "cad_model",
        CAD,
        json!({"op": "cylinder", "name": "Axle", "base": [0.0, -40.0, 30.0], "axis": [0.0, 1.0, 0.0], "radius": 4.0, "height": 80.0, "material": "steel"}),
        "CAD mode: direct modelling in process (OCCT B-reps in the .rcad; one undo step each; the caller waits; answers {message, result, revision}). mm, Z up. op: box {corner, size} | cylinder {base, axis, radius, height} | sphere {center, radius} | cone {base, axis, radius, radius2, height} | extrude {loops: [[[x,y,z],...] outer, holes...], direction} — each with name, parent (a group id), material (default pla); group {name, parent}; fuse | cut | common {target, tools, keep_tools} (target replaced; tools deleted unless keep_tools); fillet {node, radius, edges?} | chamfer {node, distance, edges?} (edges: indices from topology; none = all); move {ids, translation?, axis?, angle_deg?, scale?, center?} (bodies are baked as RoboCAD bakes them; attached comment pins follow); topology {node} (read only: faces with kind, center, normal, area_mm2 and edges with kind, midpoint, length_mm, start, end, by the indices fillet/chamfer and comment faces take).",
    )]
}

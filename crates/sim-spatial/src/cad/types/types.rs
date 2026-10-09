//! The JSON RoboCAD's REST API answers (`cad/robocad/api.py`), typed
//! tolerantly: every struct defaults missing fields and ignores unknown
//! ones, a field Python may write as `null` is an `Option`, and shapes
//! RoboCAD leaves open (transforms, joints, sketches, views, materials) are
//! `serde_json::Value`. Lengths are millimetres, masses grams, as RoboCAD
//! writes them.
use sim_runtime::hardware::protocol::{lenient, lenient_items};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `GET /`: `Service.health`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Health {
    pub ok: bool,
    /// `"robocad"`.
    pub app: String,
    pub version: String,
    /// The document's file; `None` for a new document.
    pub path: Option<String>,
    pub dirty: bool,
    /// Whether a desktop window serves the API (headless otherwise).
    pub gui: bool,
    pub nodes: u64,
    pub document_id: Option<String>,
    /// Increases with every document change.
    pub revision: u64,
}

/// One node as `node_summary` writes it (`/nodes`, `/doc`).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct NodeSummary {
    pub id: String,
    /// body | sheet | curve | sketch | mesh | image | plane | measure | group | instance | joint | sensor | cable …
    pub kind: String,
    pub name: String,
    pub parent: Option<String>,
    pub children: Vec<String>,
    pub visible: bool,
    pub locked: bool,
    pub disabled: bool,
    /// A material id from `DocState::materials`.
    pub material: Option<String>,
    /// RGB in 0..1, or `None` for the material's colour. A malformed value
    /// reads as `None`.
    #[serde(deserialize_with = "lenient")]
    pub color: Option<Vec<f64>>,
    /// A point in mm. A malformed value reads as `None`.
    #[serde(deserialize_with = "lenient")]
    pub pivot: Option<Vec<f64>>,
    /// An instance's source node.
    pub source: Option<String>,
    /// `Transform.to_json`: `{"translation": [x, y, z], "axis": [x, y, z], "angle_deg", "scale"}`.
    pub transform: Value,
    /// Visible itself and through every ancestor (none disabled).
    pub effective_visible: bool,
    pub component_instance: Option<Value>,
    pub component_member: Option<Value>,
}

/// `node_detail`'s mass properties (mm, mm², mm³, g). A `None` (or a
/// `None` coordinate) means RoboCAD sent `null` or a non-finite value
/// (`NaN`, `Infinity`: a degenerate or open body); it is never filled in.
/// A malformed field reads as `None` (a vector as empty) rather than
/// failing the node.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct MassBlock {
    #[serde(deserialize_with = "lenient")]
    pub volume_mm3: Option<f64>,
    #[serde(deserialize_with = "lenient")]
    pub area_mm2: Option<f64>,
    #[serde(deserialize_with = "lenient")]
    pub mass_g: Option<f64>,
    #[serde(deserialize_with = "lenient")]
    pub centroid: Vec<Option<f64>>,
    #[serde(deserialize_with = "lenient")]
    pub bbox_min: Vec<Option<f64>>,
    #[serde(deserialize_with = "lenient")]
    pub bbox_max: Vec<Option<f64>>,
    #[serde(deserialize_with = "lenient")]
    pub size: Vec<Option<f64>>,
}

/// `GET /nodes/{id}` (and `PATCH`'s answer): the summary plus what the node
/// carries. Absent sections are `None`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct NodeDetail {
    #[serde(flatten)]
    pub summary: NodeSummary,
    /// The kernel body's kind, for nodes with geometry.
    pub body_kind: Option<String>,
    pub mass: Option<MassBlock>,
    pub face_count: Option<u64>,
    pub edge_count: Option<u64>,
    pub sketch: Option<Value>,
    pub plane: Option<Value>,
    pub measure: Option<Value>,
    pub mirror_plane: Option<Value>,
    pub joint: Option<Value>,
    pub robot: Option<Value>,
    /// A mesh node's counts: `{"vertices": n, "triangles": n}`.
    pub mesh: Option<Value>,
    pub image: Option<Value>,
}

/// `GET /history`: the undo and redo stacks' labels, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct History {
    pub undo: Vec<String>,
    pub redo: Vec<String>,
}

/// One selected item: `[node id, kind ("body", "face", "edge", …), index]`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct SelectionItem(pub String, pub String, pub i64);

/// `GET /selection`. A malformed item is dropped, not the selection;
/// `mode` is only written with a desktop window.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Selection {
    #[serde(deserialize_with = "lenient_items")]
    pub items: Vec<SelectionItem>,
    pub mode: Option<String>,
}

/// `GET /doc`: `Service.doc_state`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct DocState {
    pub path: Option<String>,
    pub dirty: bool,
    pub roots: Vec<String>,
    pub active_group: Option<String>,
    /// Every node, in tree order (`Document.walk`). A malformed node is
    /// dropped, not the document (its id may still appear in `roots` or a
    /// parent's `children`).
    #[serde(deserialize_with = "lenient_items")]
    pub nodes: Vec<NodeSummary>,
    /// `Material.to_json`: `{"id", "name", "density" (g/cm³), "color", "roughness", "metallic", "tags", "engineering"}`.
    pub materials: Vec<Value>,
    pub selection: Selection,
    /// The camera and display state (`{}` headless).
    pub view: Value,
    pub history: History,
    pub document_id: Option<String>,
    pub revision: u64,
}

/// `GET /nodes/{id}/mesh?tolerance=T`: a tessellation in mm.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct MeshData {
    pub vertices: Vec<[f64; 3]>,
    /// Vertex index triples.
    pub triangles: Vec<[u32; 3]>,
    /// The face each triangle came from (stable per-face ids for picking).
    pub triangle_face: Vec<i64>,
    pub face_count: u64,
}

/// `POST /ops/{name}`: the op's return value and the history after it; a
/// desktop window runs component ops as a job and answers `{"job": …}`
/// instead.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct OpResult {
    pub result: Value,
    pub history: History,
    pub job: Option<Value>,
}

/// One entry of `GET /commands` (the GUI's command registry).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct CommandInfo {
    pub label: String,
    pub category: String,
    /// A key sequence string or a list of them (keymap.json).
    pub keys: Value,
}

/// `POST /commands/{id}`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Ran {
    pub ran: String,
}







/// `GET /autosave` (desktop only).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Autosave {
    pub running: bool,
    pub revision: Option<u64>,
    pub saved_revision: Option<u64>,
    pub path: Option<String>,
}

/// `POST /export`: `format` is stl | 3mf | step | iges | obj | svg |
/// drawing; `settings` the format's settings object; `ids` the nodes to
/// export (all when `None`). Absent options are not sent.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ExportRequest {
    pub format: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<String>>,
}


/// One face of a node's body (`GET /nodes/{id}/faces`, `face_json`:
/// `FaceRef.to_json` plus `index`). Lengths in mm, area in mm². A field
/// Python wrote as `null` (or malformed) is `None`, never filled in.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct FaceInfo {
    /// The face's index in the body's face list (the `index` of
    /// `[node, "face", index]` and of `triangle_face`).
    pub index: i64,
    /// `SurfaceKind`: plane | cylinder | cone | sphere | torus | bspline | other …
    pub kind: String,
    #[serde(deserialize_with = "lenient")]
    pub centroid: Option<[f64; 3]>,
    #[serde(deserialize_with = "lenient")]
    pub normal: Option<[f64; 3]>,
    #[serde(deserialize_with = "lenient")]
    pub area: Option<f64>,
    /// Cylinders, cones, spheres and tori: the axis point and direction and the radius.
    #[serde(deserialize_with = "lenient")]
    pub axis_point: Option<[f64; 3]>,
    #[serde(deserialize_with = "lenient")]
    pub axis_dir: Option<[f64; 3]>,
    #[serde(deserialize_with = "lenient")]
    pub radius: Option<f64>,
    /// The surface point where `normal` was evaluated.
    #[serde(deserialize_with = "lenient")]
    pub point: Option<[f64; 3]>,
}

/// One edge of a node's body (`GET /nodes/{id}/edges`, `edge_json`), with
/// its sampled polyline when asked for (`?samples=N`: the points RoboCAD's
/// viewport draws and picks, `kernel.sample_edges`). mm.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct EdgeInfo {
    pub index: i64,
    /// `CurveKind`: line | circle | ellipse | bspline | other …
    pub kind: String,
    #[serde(deserialize_with = "lenient")]
    pub midpoint: Option<[f64; 3]>,
    #[serde(deserialize_with = "lenient")]
    pub length: Option<f64>,
    #[serde(deserialize_with = "lenient")]
    pub start: Option<[f64; 3]>,
    #[serde(deserialize_with = "lenient")]
    pub end: Option<[f64; 3]>,
    /// Circles and arcs: the centre and radius.
    #[serde(deserialize_with = "lenient")]
    pub center: Option<[f64; 3]>,
    #[serde(deserialize_with = "lenient")]
    pub radius: Option<f64>,
    /// The sampled polyline (empty unless `samples` was asked for; a
    /// malformed point is dropped).
    #[serde(deserialize_with = "lenient_items", skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<[f64; 3]>,
}

/// One vertex of a node's body (`GET /nodes/{id}/vertices`). mm.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct VertexInfo {
    pub index: i64,
    #[serde(deserialize_with = "lenient")]
    pub point: Option<[f64; 3]>,
}


/// `POST /clipboard/paste`: the pasted nodes' ids (in the clip's order),
/// added as one RoboCAD undo step "Paste", and the revision afterwards.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Pasted {
    pub pasted: Vec<String>,
    pub revision: Option<u64>,
    /// The undo/redo labels afterwards (the last undo is "Paste").
    pub history: History,
}

/// `GET /nodes/{id}/control_points?face=i`: the face's B-spline poles
/// (`kernel.control_points`), one row per u pole. mm.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ControlPoints {
    pub node: String,
    pub face: i64,
    pub rows: Vec<Vec<[f64; 3]>>,
}

/// `GET /nodes/{id}/curvature_comb`: the comb's teeth (`analysis.curvature_comb`:
/// point → point + normal·curvature·scale). Empty for a sketch node (it holds
/// no body; RoboCAD's GUI draws none either). mm.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct CurvatureComb {
    pub node: String,
    pub lines: Vec<[[f64; 3]; 2]>,
}

/// One edge of `GET /nodes/{id}/continuity`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct EdgeContinuity {
    /// The edge's index (as in `/edges`).
    pub index: i64,
    /// `G0` | `G1` | `G2` | `boundary` (`kernel.continuity`).
    pub continuity: String,
    /// `kernel.sample_edge(e, body, 16)`: 16 points along a curve, a line's two ends. mm.
    pub points: Vec<[f64; 3]>,
}

/// `GET /nodes/{id}/continuity`: every edge's continuity between its two
/// faces (`analysis.continuity_report`) and the count per grade (`G0`,
/// `G1`, `G2`, `boundary`; a `BTreeMap` keeps RoboCAD's order, which is
/// also the byte order).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Continuity {
    pub node: String,
    pub edges: Vec<EdgeContinuity>,
    pub counts: std::collections::BTreeMap<String, u64>,
}

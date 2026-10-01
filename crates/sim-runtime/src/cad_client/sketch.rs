//! RoboCAD's sketches and construction planes as the client reads and
//! edits them (cad-sketch epic, native-viewer.md "CAD sketch").
//!
//! - **Planes** ([`PlaneFrame`]): `Plane.to_json` (kernel/base.py:131):
//!   `{"origin", "normal", "x_axis"}`, mm. A plane node's `GET /nodes/{id}`
//!   carries one as `plane` ([`plane_of`]); a sketch carries its own.
//! - **Sketch geometry** ([`SketchGeometry`], [`SketchCurve`]): `GET
//!   /nodes/{id}/sketch` answers `Sketch.to_json` (kernel/sketch.py:584):
//!   `{"name", "plane", "curves": [Curve.to_json, …]}`, curves in plane
//!   coordinates (u, v) mm. Read tolerantly: a missing field takes
//!   `Curve.from_json`'s default, a malformed one is dropped to that default.
//! - **Calls** ([`SketchCall`]): `POST /nodes/{id}/sketch {"calls": [[method,
//!   args, kwargs?], …]}` (api.py `Service.edit_sketch`): every name and
//!   argument shape is kernel/sketch.py's. Curves are named by their index
//!   in the sketch's curve list, which api.py turns into the curve for
//!   `trim`, `extend`, `split_at`, `fillet_corner`, `offset`, `reverse`,
//!   `remove`, `unjoin`, `rebuild`, `insert_vertex`, `remove_vertex` (the
//!   first argument), `trim`'s and `extend`'s second argument and `join`'s
//!   list. [`SketchCall::from_json`] reads a REST caller's call and refuses
//!   a wrong one naming the call and the argument.
//! - **New sketch with its calls**: `POST /nodes {"kind": "sketch",
//!   "plane", "calls"}` (api.py `Service.create`: `Ops.new_sketch`, one
//!   undo step "Sketch", then the calls as `edit_sketch`, one undo step
//!   "Sketch (API)") — [`CadClient::create_sketch`].
use super::{CadClient, CadError, NodeDetail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// A construction plane (`kernel.base.Plane`): origin, unit normal and
/// in-plane x axis, mm in RoboCAD's frame. `y_axis` is `normal × x_axis`.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct PlaneFrame {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
    pub x_axis: [f64; 3],
}

impl PlaneFrame {
    /// `Plane.xy()`: origin 0, normal +Z, x +X.
    pub const XY: PlaneFrame = PlaneFrame { origin: [0.0; 3], normal: [0.0, 0.0, 1.0], x_axis: [1.0, 0.0, 0.0] };
    /// `Plane.xz()`: normal −Y, x +X (kernel/base.py:106).
    pub const XZ: PlaneFrame = PlaneFrame { origin: [0.0; 3], normal: [0.0, -1.0, 0.0], x_axis: [1.0, 0.0, 0.0] };
    /// `Plane.yz()`: normal +X, x +Y.
    pub const YZ: PlaneFrame = PlaneFrame { origin: [0.0; 3], normal: [1.0, 0.0, 0.0], x_axis: [0.0, 1.0, 0.0] };

    /// `Plane.y_axis`: `unit(normal × x_axis)`.
    pub fn y_axis(&self) -> [f64; 3] {
        unimplemented!("P1")
    }
    /// `Plane.to_world(u, v, w)`.
    pub fn to_world(&self, u: f64, v: f64, w: f64) -> [f64; 3] {
        let _ = (u, v, w);
        unimplemented!("P1")
    }
    /// `Plane.to_local(p)`: (u, v, w).
    pub fn to_local(&self, p: [f64; 3]) -> [f64; 3] {
        let _ = p;
        unimplemented!("P1")
    }
    /// `Plane.project(p)`: the point of the plane nearest `p`.
    pub fn project(&self, p: [f64; 3]) -> [f64; 3] {
        let _ = p;
        unimplemented!("P1")
    }
    /// Whether two frames are the same plane as Python's frozen dataclass
    /// `==` compares them (component-wise), within `tol` (1e-9 for floats
    /// that went through JSON).
    pub fn same(&self, other: &PlaneFrame, tol: f64) -> bool {
        let _ = (other, tol);
        unimplemented!("P1")
    }
    /// `Plane.to_json`.
    pub fn json(&self) -> Value {
        json!({"origin": self.origin, "normal": self.normal, "x_axis": self.x_axis})
    }
    /// A plane as `Plane.from_json` reads it; None when a vector is missing,
    /// not three finite numbers, or the normal or x axis is zero.
    pub fn from_value(v: &Value) -> Option<PlaneFrame> {
        let _ = v;
        unimplemented!("P1")
    }
}

/// One sketch curve as `Curve.to_json` writes it (kernel/sketch.py:52):
/// `kind` line | polyline | circle | arc | ellipse | spline | control |
/// slot | text; plane coordinates (u, v) mm, angles in degrees. Defaults
/// are `Curve.from_json`'s.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SketchCurve {
    pub kind: String,
    pub points: Vec<[f64; 2]>,
    pub center: Option<[f64; 2]>,
    pub radius: f64,
    pub radius2: f64,
    pub start_angle: f64,
    pub end_angle: f64,
    pub rotation: f64,
    pub degree: u32,
    pub closed: bool,
    pub text: String,
    pub height: f64,
    pub font: String,
    pub name: String,
}

impl Default for SketchCurve {
    /// `Curve.from_json`'s defaults (end_angle 360, degree 3, height 10).
    fn default() -> Self {
        SketchCurve { kind: String::new(), points: Vec::new(), center: None, radius: 0.0, radius2: 0.0, start_angle: 0.0, end_angle: 360.0, rotation: 0.0, degree: 3, closed: false, text: String::new(), height: 10.0, font: String::new(), name: String::new() }
    }
}

impl SketchCurve {
    /// A curve from `Curve.to_json`, tolerant: a missing or malformed
    /// field takes its default; None without a `kind` string.
    pub fn from_value(v: &Value) -> Option<SketchCurve> {
        let _ = v;
        unimplemented!("P1")
    }
    /// `Curve.start()` (plane coordinates).
    pub fn start(&self) -> Option<[f64; 2]> {
        unimplemented!("P1")
    }
    /// `Curve.end()`.
    pub fn end(&self) -> Option<[f64; 2]> {
        unimplemented!("P1")
    }
    /// The polyline RoboCAD's viewport draws for the curve
    /// (viewport.py:922-939): `Curve.sample(n)`, and a slot's stadium
    /// outline closed (`io.exporters._slot_points`). Plane coordinates.
    pub fn sample(&self, n: usize) -> Vec<[f64; 2]> {
        let _ = n;
        unimplemented!("P1")
    }
}

/// `Sketch.to_json`: name, plane and curves.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SketchGeometry {
    pub name: String,
    /// None when the answer's plane is missing or malformed.
    pub plane: Option<PlaneFrame>,
    pub curves: Vec<SketchCurve>,
}

impl SketchGeometry {
    /// A sketch from `Sketch.to_json`, tolerant (a malformed curve is
    /// dropped and counted in `dropped`). Err when `v` is not an object.
    pub fn from_value(v: &Value) -> Result<(SketchGeometry, usize), String> {
        let _ = v;
        unimplemented!("P1")
    }
}

impl<'de> Deserialize<'de> for SketchGeometry {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(deserializer)?;
        SketchGeometry::from_value(&v).map(|(g, _)| g).map_err(serde::de::Error::custom)
    }
}

/// A point in plane coordinates (u, v) mm.
pub type Uv = [f64; 2];

/// One call of `POST /nodes/{id}/sketch`, named and shaped as the
/// `Sketch` method it calls (kernel/sketch.py). Curve arguments are curve
/// indices in the sketch's list at the moment the call runs (earlier calls
/// of the same list may have added or replaced curves).
#[derive(Clone, Debug, PartialEq)]
pub enum SketchCall {
    // ---- Constructors (kernel/sketch.py:162-283) ----
    Line { a: Uv, b: Uv },
    Polyline { points: Vec<Uv>, closed: bool },
    Spline { points: Vec<Uv>, closed: bool },
    ControlCurve { points: Vec<Uv>, degree: u32, closed: bool },
    Circle { center: Uv, radius: f64 },
    CircleTwoPoint { a: Uv, b: Uv },
    CircleThreePoint { a: Uv, b: Uv, c: Uv },
    /// `circle_tangent(curves, radius=None, near=(0, 0))`: api.py does not
    /// turn its curve indices into curves, so RoboCAD's REST cannot run it;
    /// the builder refuses it by name ([`SketchCall::check`]).
    CircleTangent { curves: Vec<usize>, radius: Option<f64>, near: Uv },
    Ellipse { center: Uv, radius_x: f64, radius_y: f64, rotation: f64 },
    Arc { center: Uv, radius: f64, start_deg: f64, end_deg: f64 },
    ArcThreePoint { a: Uv, b: Uv, c: Uv },
    /// `arc_tangent(prev, end)`: as `CircleTangent`, unreachable by REST.
    ArcTangent { prev: usize, end: Uv },
    Rectangle { corner: Uv, size: Uv },
    RectangleCenter { center: Uv, size: Uv },
    RectangleThreePoint { a: Uv, b: Uv, c: Uv },
    /// `polygon(center, radius, sides=None, rotation=0.0)`: `sides` None
    /// uses RoboCAD's `Sketch.last_polygon_sides` (its process remembers it).
    Polygon { center: Uv, radius: f64, sides: Option<u32>, rotation: f64 },
    Slot { a: Uv, b: Uv, width: f64 },
    Spiral { center: Uv, start_radius: f64, end_radius: f64, turns: f64 },
    Text { origin: Uv, text: String, height: f64 },
    // ---- Edits (kernel/sketch.py:296-480) ----
    Remove { curve: usize },
    Reverse { curve: usize },
    SplitAt { curve: usize, point: Uv },
    Trim { curve: usize, cutters: Vec<usize>, click: Uv },
    Extend { curve: usize, targets: Vec<usize>, both: bool },
    FilletCorner { curve: usize, vertex_index: i64, radius: f64 },
    Offset { curve: usize, distance: f64 },
    Join { curves: Vec<usize> },
    Unjoin { curve: usize },
    InsertVertex { curve: usize, after: i64, point: Uv },
    RemoveVertex { curve: usize, index: i64 },
    Rebuild { curve: usize, degree: u32, spans: u32 },
}

/// Every call name [`SketchCall::from_json`] reads, as kernel/sketch.py names them.
pub const SKETCH_CALLS: &[&str] = &[
    "line", "polyline", "spline", "control_curve", "circle", "circle_two_point", "circle_three_point", "circle_tangent", "ellipse", "arc", "arc_three_point", "arc_tangent", "rectangle", "rectangle_center", "rectangle_three_point", "polygon", "slot", "spiral", "text",
    "remove", "reverse", "split_at", "trim", "extend", "fillet_corner", "offset", "join", "unjoin", "insert_vertex", "remove_vertex", "rebuild",
];

impl SketchCall {
    /// The `Sketch` method's name.
    pub fn name(&self) -> &'static str {
        unimplemented!("P1")
    }
    /// The call as api.py reads it: `[name, [args…]]`, or `[name, [args…],
    /// {kwargs}]` when a keyword is sent. Points as `[u, v]`; optional
    /// trailing parameters left at their Python default are not sent.
    pub fn to_json(&self) -> Value {
        unimplemented!("P1")
    }
    /// A REST caller's call (`[name, [args…], {kwargs}?]`, as api.py takes
    /// it), refused naming the call and the argument: an unknown name, a
    /// wrong count or shape, a non-finite number, a negative curve index.
    pub fn from_json(v: &Value) -> Result<SketchCall, String> {
        let _ = v;
        unimplemented!("P1")
    }
    /// Refuse what RoboCAD's REST cannot run or would fail on before any
    /// geometry: `circle_tangent` and `arc_tangent` (api.py passes their
    /// curve indices through as integers), and a curve index at or past
    /// `curves` (the curve count when the call runs, if known).
    pub fn check(&self, curves: Option<usize>) -> Result<(), String> {
        let _ = curves;
        unimplemented!("P1")
    }
    /// How many curves the sketch has after this call, given `before`
    /// (constructors add one; text adds an unknown number: None; edits as
    /// kernel/sketch.py changes the list). Used to check later indices.
    pub fn curves_after(&self, before: usize) -> Option<usize> {
        let _ = before;
        unimplemented!("P1")
    }
}

/// `{"calls": [...]}` for `POST /nodes/{id}/sketch`.
pub fn calls_body(calls: &[SketchCall]) -> Value {
    json!({"calls": calls.iter().map(SketchCall::to_json).collect::<Vec<_>>()})
}

/// The calls checked in order against a sketch of `curves` curves (None:
/// unknown): the first refusal names the call's position and name.
pub fn check_calls(calls: &[SketchCall], curves: Option<usize>) -> Result<(), String> {
    let _ = (calls, curves);
    unimplemented!("P1")
}

/// A plane node's frame from its `GET /nodes/{id}` (`plane`), if any.
pub fn plane_of(detail: &NodeDetail) -> Option<PlaneFrame> {
    detail.plane.as_ref().and_then(PlaneFrame::from_value)
}

impl CadClient {
    /// `GET /nodes/{id}/sketch`: the sketch's name, plane and curves. A
    /// node that is not a sketch answers `null` (api.py), refused here
    /// naming the node.
    pub fn sketch(&self, id: &str) -> Result<SketchGeometry, CadError> {
        let _ = id;
        unimplemented!("P1")
    }
    /// `POST /nodes/{id}/sketch {"calls": [...]}`: one RoboCAD undo step
    /// "Sketch (API)" (an edit: use `EDIT_TIMEOUT`); the node afterwards.
    pub fn edit_sketch(&self, id: &str, calls: &[SketchCall]) -> Result<NodeDetail, CadError> {
        let _ = (id, calls);
        unimplemented!("P1")
    }
    /// `POST /nodes {"kind": "sketch", "plane": plane, "calls": [...]}`:
    /// a new sketch on `plane` ("xy" | "xz" | "yz", a plane node id, or
    /// `{origin, normal, x_axis}`) carrying the calls (`name` when given);
    /// the new node's detail (api.py answers 201).
    pub fn create_sketch(&self, plane: &Value, calls: &[SketchCall], name: Option<&str>) -> Result<NodeDetail, CadError> {
        let _ = (plane, calls, name);
        unimplemented!("P1")
    }
}

#[allow(dead_code)]
fn _uses(_: Map<String, Value>) {}

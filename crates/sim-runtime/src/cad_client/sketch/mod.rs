//! RoboCAD's sketches and construction planes as the client reads and
//! edits them (cad-sketch epic, native-viewer.md "CAD sketch").
//!
//! - **Planes** ([`PlaneFrame`]): `Plane.to_json` (kernel/base.py:131):
//!   `{"origin", "normal", "x_axis"}`, mm. A plane node's `GET /nodes/{id}`
//!   carries one as `plane` ([`plane_of`]); a sketch carries its own.
//! - **Sketch geometry** ([`SketchGeometry`], [`SketchCurve`]): `GET
//!   /nodes/{id}/sketch` answers `Sketch.to_json` (kernel/sketch.py:597):
//!   `{"name", "plane", "curves": [Curve.to_json, …]}`, curves in plane
//!   coordinates (u, v) mm. Read tolerantly: a missing field takes
//!   `Curve.from_json`'s default, a malformed one is dropped to that default.
//! - **Calls** ([`SketchCall`]): `POST /nodes/{id}/sketch {"calls": [[method,
//!   args, kwargs?], …]}` (api.py `Service.edit_sketch`): every name and
//!   argument shape is kernel/sketch.py's. Curves are named by their index
//!   in the sketch's curve list, which api.py turns into the curve for
//!   `trim`, `extend`, `split_at`, `fillet_corner`, `offset`, `reverse`,
//!   `remove`, `unjoin`, `rebuild`, `insert_vertex`, `remove_vertex`,
//!   `arc_tangent` (the first argument), `trim`'s and `extend`'s second
//!   argument and `join`'s and `circle_tangent`'s list, before it turns
//!   two-number lists into points. [`SketchCall::from_json`] reads a REST
//!   caller's call and refuses a wrong one naming the call and the argument.
//! - **New sketch with its calls**: `POST /nodes {"kind": "sketch",
//!   "plane", "calls"}` (api.py `Service.create`: `Ops.new_sketch`, one
//!   undo step "Sketch", then the calls as `edit_sketch`, one undo step
//!   "Sketch (API)") — [`CadClient::create_sketch`].
mod calls;

pub use calls::{SKETCH_CALLS, SketchCall, Uv, calls_body, check_calls};

use super::{CadClient, CadError, NodeDetail, node_route};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

// ------------------------------------------------------------- vectors

fn v_add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn v_sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn v_scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn v_dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn v_cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
/// kernel/base.py `v_unit`: +Z for a (near) zero vector.
fn v_unit(a: [f64; 3]) -> [f64; 3] {
    let n = v_dot(a, a).sqrt();
    if n < 1e-12 { [0.0, 0.0, 1.0] } else { v_scale(a, 1.0 / n) }
}

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

    /// `Plane.y_axis`: `unit(normal × x_axis)` (+Z when they are parallel,
    /// as `v_unit`).
    pub fn y_axis(&self) -> [f64; 3] {
        v_unit(v_cross(self.normal, self.x_axis))
    }
    /// `Plane.to_world(u, v, w)`: `origin + u·x_axis + v·y_axis + w·normal`
    /// (the stored vectors as they are, unnormalised, as Python).
    pub fn to_world(&self, u: f64, v: f64, w: f64) -> [f64; 3] {
        v_add(v_add(v_add(self.origin, v_scale(self.x_axis, u)), v_scale(self.y_axis(), v)), v_scale(self.normal, w))
    }
    /// `Plane.to_local(p)`: (u, v, w).
    pub fn to_local(&self, p: [f64; 3]) -> [f64; 3] {
        let d = v_sub(p, self.origin);
        [v_dot(d, self.x_axis), v_dot(d, self.y_axis()), v_dot(d, self.normal)]
    }
    /// `Plane.project(p)`: the point of the plane nearest `p`.
    pub fn project(&self, p: [f64; 3]) -> [f64; 3] {
        let [u, v, _] = self.to_local(p);
        self.to_world(u, v, 0.0)
    }
    /// Whether two frames are the same plane as Python's frozen dataclass
    /// `==` compares them (component-wise), within `tol` (1e-9 for floats
    /// that went through JSON).
    pub fn same(&self, other: &PlaneFrame, tol: f64) -> bool {
        let close = |a: &[f64; 3], b: &[f64; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol);
        close(&self.origin, &other.origin) && close(&self.normal, &other.normal) && close(&self.x_axis, &other.x_axis)
    }
    /// `Plane.to_json`.
    pub fn json(&self) -> Value {
        json!({"origin": self.origin, "normal": self.normal, "x_axis": self.x_axis})
    }
    /// A plane as `Plane.from_json` reads it; None when a vector is missing,
    /// not three finite numbers, or the normal or x axis is zero (shorter
    /// than `v_unit`'s 1e-12).
    pub fn from_value(v: &Value) -> Option<PlaneFrame> {
        let obj = v.as_object()?;
        let vec3 = |key: &str| -> Option<[f64; 3]> {
            let a = obj.get(key)?.as_array()?;
            if a.len() != 3 {
                return None;
            }
            let mut out = [0.0; 3];
            for (o, x) in out.iter_mut().zip(a) {
                *o = x.as_f64().filter(|x| x.is_finite())?;
            }
            Some(out)
        };
        let frame = PlaneFrame { origin: vec3("origin")?, normal: vec3("normal")?, x_axis: vec3("x_axis")? };
        let long = |a: [f64; 3]| v_dot(a, a).sqrt() >= 1e-12;
        (long(frame.normal) && long(frame.x_axis)).then_some(frame)
    }
}

/// One sketch curve as `Curve.to_json` writes it (kernel/sketch.py:53):
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

/// `[u, v]`: an array of exactly two finite numbers.
fn uv_of(v: &Value) -> Option<Uv> {
    match v.as_array()?.as_slice() {
        [u, w] => {
            let (u, w) = (u.as_f64()?, w.as_f64()?);
            (u.is_finite() && w.is_finite()).then_some([u, w])
        }
        _ => None,
    }
}

impl SketchCurve {
    /// A curve from `Curve.to_json`, tolerant: a missing or malformed
    /// field takes its default (a `points` list with any malformed point
    /// reads as empty; a `center` that is not `[u, v]` as None); None
    /// without a `kind` string.
    pub fn from_value(v: &Value) -> Option<SketchCurve> {
        let obj = v.as_object()?;
        let kind = obj.get("kind")?.as_str()?.to_string();
        let d = SketchCurve::default();
        let num = |key: &str, default: f64| obj.get(key).and_then(Value::as_f64).filter(|x| x.is_finite()).unwrap_or(default);
        let string = |key: &str| obj.get(key).and_then(Value::as_str).map(str::to_string).unwrap_or_default();
        let points = obj.get("points").and_then(Value::as_array).and_then(|a| a.iter().map(uv_of).collect::<Option<Vec<_>>>()).unwrap_or_default();
        Some(SketchCurve {
            kind,
            points,
            center: obj.get("center").and_then(uv_of),
            radius: num("radius", d.radius),
            radius2: num("radius2", d.radius2),
            start_angle: num("start_angle", d.start_angle),
            end_angle: num("end_angle", d.end_angle),
            rotation: num("rotation", d.rotation),
            degree: obj.get("degree").and_then(Value::as_u64).and_then(|x| u32::try_from(x).ok()).unwrap_or(d.degree),
            closed: obj.get("closed").and_then(Value::as_bool).unwrap_or(d.closed),
            text: string("text"),
            height: num("height", d.height),
            font: string("font"),
            name: string("name"),
        })
    }
    /// The arc's point at `deg` degrees.
    fn arc_point(&self, deg: f64) -> Option<[f64; 2]> {
        let c = self.center?;
        let a = deg.to_radians();
        Some([c[0] + self.radius * a.cos(), c[1] + self.radius * a.sin()])
    }
    /// `Curve.start()` (plane coordinates). None where Python would raise
    /// (no points, no center).
    pub fn start(&self) -> Option<[f64; 2]> {
        match self.kind.as_str() {
            "line" | "polyline" | "spline" | "control" => self.points.first().copied(),
            "arc" => self.arc_point(self.start_angle),
            _ => self.points.first().copied().or(self.center),
        }
    }
    /// `Curve.end()`.
    pub fn end(&self) -> Option<[f64; 2]> {
        match self.kind.as_str() {
            "line" | "polyline" | "spline" | "control" => self.points.last().copied(),
            "arc" => self.arc_point(self.end_angle),
            _ => self.points.last().copied().or(self.center),
        }
    }
    /// The polyline RoboCAD's viewport draws for the curve
    /// (viewport.py:922-939): `Curve.sample(n)`, and a slot's stadium
    /// outline closed (the 34 points of `io.exporters._slot_points` plus
    /// the first again: 35, with the caps outward as the kernel's slot:
    /// see `slot_points`). Plane coordinates.
    /// Where Python would raise (a line with fewer than two points, a
    /// circle without a center, a slot without two points) the curve's
    /// points as stored; `n` 0 samples as 1 for the circle, arc, ellipse
    /// and control curve (Python divides by it; an interpolated spline,
    /// which Python samples per span, differs from Python at `n` 0).
    pub fn sample(&self, n: usize) -> Vec<[f64; 2]> {
        let n = n.max(1);
        match self.kind.as_str() {
            "slot" => match self.points.as_slice() {
                [a, b] => {
                    let mut out = slot_points(*a, *b, self.radius);
                    out.push(out[0]);
                    out
                }
                _ => self.points.clone(),
            },
            "line" => match self.points.as_slice() {
                [a, b, ..] => vec![*a, *b],
                _ => self.points.clone(),
            },
            "polyline" => {
                let mut out = self.points.clone();
                if self.closed
                    && let Some(&first) = self.points.first()
                {
                    out.push(first);
                }
                out
            }
            "circle" | "arc" | "ellipse" => {
                let Some(c) = self.center else { return self.points.clone() };
                let arc = self.kind == "arc";
                let a0 = if arc { self.start_angle } else { 0.0 }.to_radians();
                let a1 = if arc { self.end_angle } else { 360.0 }.to_radians();
                let r2 = if self.kind == "ellipse" { self.radius2 } else { self.radius };
                let rot = self.rotation.to_radians();
                (0..=n)
                    .map(|i| {
                        let a = a0 + (a1 - a0) * i as f64 / n as f64;
                        let (x, y) = (self.radius * a.cos(), r2 * a.sin());
                        [c[0] + x * rot.cos() - y * rot.sin(), c[1] + x * rot.sin() + y * rot.cos()]
                    })
                    .collect()
            }
            "spline" | "control" => sample_spline(&self.points, self.degree as usize, self.closed, n, self.kind == "spline"),
            _ => self.points.clone(),
        }
    }
}

/// A slot's stadium outline as `io.exporters._slot_points` lays it out
/// (17 points round the `b` cap from `b + r·n`, then 17 round the `a` cap
/// from `a − r·n`; not closed), but with the caps bulging outward through
/// `b + r·d` and `a − r·d` as the kernel builds the slot (kernel/sketch.py
/// `_wire`, `GC_MakeArcOfCircle` through those points). `_slot_points`
/// sweeps its angles the other way, so RoboCAD's viewport and SVG export
/// draw both caps turned inward (`(8.5, 0)` for a slot from (0, 0) to
/// (10, 0) of radius 1.5, where the solid has `(11.5, 0)`).
fn slot_points(a: Uv, b: Uv, r: f64) -> Vec<Uv> {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l = d[0].hypot(d[1]);
    let l = if l == 0.0 { 1.0 } else { l };
    let d = [d[0] / l, d[1] / l];
    let n = [-d[1], d[0]];
    let ang = n[1].atan2(n[0]);
    let pi = std::f64::consts::PI;
    let mut out = Vec::with_capacity(35);
    for k in 0..17 {
        let t = ang - pi * k as f64 / 16.0;
        out.push([b[0] + r * t.cos(), b[1] + r * t.sin()]);
    }
    for k in 0..17 {
        let t = ang + pi - pi * k as f64 / 16.0;
        out.push([a[0] + r * t.cos(), a[1] + r * t.sin()]);
    }
    out
}

/// kernel/sketch.py `_sample_spline`, exactly as written: Catmull-Rom
/// display sampling through the points (`interpolate`, with its integer
/// division of `n` by the span count), else de Boor on clamped uniform
/// knots (`closed` ignored there, as in Python). `n` ≥ 1.
fn sample_spline(points: &[Uv], degree: usize, closed: bool, n: usize, interpolate: bool) -> Vec<Uv> {
    if points.len() < 2 {
        return points.to_vec();
    }
    if interpolate {
        let mut pts = points.to_vec();
        if closed {
            pts.push(points[0]);
        }
        let per = n / (pts.len() - 1).max(1);
        let mut out = Vec::with_capacity((pts.len() - 1) * (per + 1) + 1);
        for i in 0..pts.len() - 1 {
            let p0 = if i > 0 { pts[i - 1] } else { pts[i] };
            let (p1, p2) = (pts[i], pts[i + 1]);
            let p3 = if i + 2 < pts.len() { pts[i + 2] } else { pts[i + 1] };
            for k in 0..=per {
                let t = k as f64 / (per + 1) as f64;
                let (t2, t3) = (t * t, t * t * t);
                let c = |j: usize| 0.5 * ((2.0 * p1[j]) + (-p0[j] + p2[j]) * t + (2.0 * p0[j] - 5.0 * p1[j] + 4.0 * p2[j] - p3[j]) * t2 + (-p0[j] + 3.0 * p1[j] - 3.0 * p2[j] + p3[j]) * t3);
                out.push([c(0), c(1)]);
            }
        }
        out.push(pts[pts.len() - 1]);
        return out;
    }
    let pts = points;
    let len = pts.len();
    let k = degree.min(len - 1);
    let mut knots = vec![0.0; k + 1];
    knots.extend((1..len - k).map(|i| i as f64 / (len - k) as f64));
    knots.extend(std::iter::repeat_n(1.0, k + 1));
    let mut out = Vec::with_capacity(n + 1);
    for s in 0..=n {
        let t = s as f64 / n as f64;
        if t >= 1.0 {
            out.push(pts[len - 1]);
            continue;
        }
        let mut span = k;
        while span < len - 1 && t >= knots[span + 1] {
            span += 1;
        }
        let mut d: Vec<Uv> = (0..=k).map(|j| pts[j + span - k]).collect();
        for r in 1..=k {
            for j in (r..=k).rev() {
                let i = j + span - k;
                let denom = knots[i + k - r + 1] - knots[i];
                let alpha = if denom == 0.0 { 0.0 } else { (t - knots[i]) / denom };
                d[j] = [(1.0 - alpha) * d[j - 1][0] + alpha * d[j][0], (1.0 - alpha) * d[j - 1][1] + alpha * d[j][1]];
            }
        }
        out.push(d[k]);
    }
    out
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
    /// dropped and its index in RoboCAD's list kept in `dropped`, so a
    /// refusal can name it; a missing name reads as `Sketch.from_json`'s
    /// "Sketch"; missing or non-list `curves` as none). Err when `v` is not
    /// an object.
    pub fn from_value(v: &Value) -> Result<(SketchGeometry, Vec<usize>), String> {
        let obj = v.as_object().ok_or_else(|| format!("a sketch is an object {{name, plane, curves}} (got {v})"))?;
        let name = obj.get("name").and_then(Value::as_str).unwrap_or("Sketch").to_string();
        let plane = obj.get("plane").and_then(PlaneFrame::from_value);
        let mut curves = Vec::new();
        let mut dropped = Vec::new();
        for (i, c) in obj.get("curves").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default().iter().enumerate() {
            match SketchCurve::from_value(c) {
                Some(c) => curves.push(c),
                None => dropped.push(i),
            }
        }
        Ok((SketchGeometry { name, plane, curves }, dropped))
    }
}

impl<'de> Deserialize<'de> for SketchGeometry {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(deserializer)?;
        SketchGeometry::from_value(&v).map(|(g, _)| g).map_err(serde::de::Error::custom)
    }
}

/// A plane node's frame from its `GET /nodes/{id}` (`plane`), if any.
pub fn plane_of(detail: &NodeDetail) -> Option<PlaneFrame> {
    detail.plane.as_ref().and_then(PlaneFrame::from_value)
}

/// `POST /nodes`'s body for a new sketch, in this field order.
#[derive(Serialize)]
struct NewSketchBody<'a> {
    kind: &'static str,
    plane: &'a Value,
    calls: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
}

impl CadClient {
    /// `GET /nodes/{id}/sketch`: the sketch's name, plane and curves. A
    /// node that is not a sketch answers `null` (api.py), refused here
    /// naming the node; an answer that is not an object is refused as
    /// unexpected.
    pub fn sketch(&self, id: &str) -> Result<SketchGeometry, CadError> {
        let route = format!("{}/sketch", node_route(id));
        let value: Value = self.get(&route)?;
        if value.is_null() {
            return Err(CadError { method: "GET", route, status: None, message: format!("{id} is not a sketch") });
        }
        SketchGeometry::from_value(&value).map(|(g, _)| g).map_err(|m| CadError { method: "GET", route, status: None, message: format!("unexpected answer: {m}") })
    }
    /// `POST /nodes/{id}/sketch {"calls": [...]}`: one RoboCAD undo step
    /// "Sketch (API)" (an edit: use `EDIT_TIMEOUT`); the node afterwards.
    pub fn edit_sketch(&self, id: &str, calls: &[SketchCall]) -> Result<NodeDetail, CadError> {
        self.send("POST", &format!("{}/sketch", node_route(id)), Some(&calls_body(calls)))
    }
    /// `POST /nodes {"kind": "sketch", "plane": plane, "calls": [...]}`:
    /// a new sketch on `plane` ("xy" | "xz" | "yz", a plane node id, or
    /// `{origin, normal, x_axis}`) carrying the calls (`name` when given);
    /// the new node's detail (api.py answers 201). An edit: use
    /// `EDIT_TIMEOUT`.
    pub fn create_sketch(&self, plane: &Value, calls: &[SketchCall], name: Option<&str>) -> Result<NodeDetail, CadError> {
        let body = NewSketchBody { kind: "sketch", plane, calls: calls.iter().map(SketchCall::to_json).collect(), name };
        self.send("POST", "/nodes", Some(&body))
    }
}

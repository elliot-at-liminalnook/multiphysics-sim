//! The typed sketch calls of `POST /nodes/{id}/sketch` ([`SketchCall`]):
//! names and argument shapes as kernel/sketch.py's methods and api.py's
//! `Service.edit_sketch` read them, a REST caller's call read and refused by
//! name ([`SketchCall::from_json`]), and the curve-index checks
//! ([`check_calls`]). Split from `sketch` to keep it under 750 lines.
use super::uv_of;
use serde_json::{Map, Value, json};

/// A point in plane coordinates (u, v) mm.
pub type Uv = [f64; 2];

/// One call of `POST /nodes/{id}/sketch`, named and shaped as the
/// `Sketch` method it calls (kernel/sketch.py). Curve arguments are curve
/// indices in the sketch's list at the moment the call runs (earlier calls
/// of the same list may have added or replaced curves).
#[derive(Clone, Debug, PartialEq)]
pub enum SketchCall {
    // ---- Constructors (kernel/sketch.py:159-294) ----
    Line { a: Uv, b: Uv },
    Polyline { points: Vec<Uv>, closed: bool },
    Spline { points: Vec<Uv>, closed: bool },
    ControlCurve { points: Vec<Uv>, degree: u32, closed: bool },
    Circle { center: Uv, radius: f64 },
    CircleTwoPoint { a: Uv, b: Uv },
    CircleThreePoint { a: Uv, b: Uv, c: Uv },
    /// `circle_tangent(curves, radius=None, near=(0, 0))`: a circle tangent
    /// to the curves (three without a radius), solved from `near`. api.py
    /// maps its curve indices to curves (since the cad-sketch fix of
    /// `Service.edit_sketch`).
    CircleTangent { curves: Vec<usize>, radius: Option<f64>, near: Uv },
    Ellipse { center: Uv, radius_x: f64, radius_y: f64, rotation: f64 },
    Arc { center: Uv, radius: f64, start_deg: f64, end_deg: f64 },
    ArcThreePoint { a: Uv, b: Uv, c: Uv },
    /// `arc_tangent(prev, end)`: an arc from the end of curve `prev`,
    /// tangent to it (a line when `end` is straight ahead).
    ArcTangent { prev: usize, end: Uv },
    Rectangle { corner: Uv, size: Uv },
    RectangleCenter { center: Uv, size: Uv },
    RectangleThreePoint { a: Uv, b: Uv, c: Uv },
    /// `polygon(center, radius, sides=None, rotation=0.0)`: `sides` None
    /// uses RoboCAD's `Sketch.last_polygon_sides` (its process remembers it).
    Polygon { center: Uv, radius: f64, sides: Option<u32>, rotation: f64 },
    Slot { a: Uv, b: Uv, width: f64 },
    /// `spiral(…, points_per_turn=36)`: always RoboCAD's default 36.
    Spiral { center: Uv, start_radius: f64, end_radius: f64, turns: f64 },
    /// `text(origin, text, height, font="")`: RoboCAD's default font.
    Text { origin: Uv, text: String, height: f64 },
    // ---- Edits (kernel/sketch.py:300-478) ----
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

/// A call's Python parameters (kernel/sketch.py) and how many are required
/// (the rest have defaults). Only the parameters [`SketchCall`] carries:
/// `spiral`'s `points_per_turn` and `text`'s `font` are not read.
fn signature(name: &str) -> Option<(&'static [&'static str], usize)> {
    let sig: (&'static [&'static str], usize) = match name {
        "line" | "circle_two_point" => (&["a", "b"], 2),
        "polyline" | "spline" => (&["points", "closed"], 1),
        "control_curve" => (&["points", "degree", "closed"], 1),
        "circle" => (&["center", "radius"], 2),
        "circle_three_point" | "arc_three_point" | "rectangle_three_point" => (&["a", "b", "c"], 3),
        "circle_tangent" => (&["curves", "radius", "near"], 1),
        "ellipse" => (&["center", "radius_x", "radius_y", "rotation"], 3),
        "arc" => (&["center", "radius", "start_deg", "end_deg"], 4),
        "arc_tangent" => (&["prev", "end"], 2),
        "rectangle" => (&["corner", "size"], 2),
        "rectangle_center" => (&["center", "size"], 2),
        "polygon" => (&["center", "radius", "sides", "rotation"], 2),
        "slot" => (&["a", "b", "width"], 3),
        "spiral" => (&["center", "start_radius", "end_radius", "turns"], 4),
        "text" => (&["origin", "text", "height"], 2),
        "remove" | "reverse" | "unjoin" => (&["curve"], 1),
        "split_at" => (&["curve", "point"], 2),
        "trim" => (&["curve", "cutters", "click"], 3),
        "extend" => (&["curve", "targets", "both"], 2),
        "fillet_corner" => (&["curve", "vertex_index", "radius"], 3),
        "offset" => (&["curve", "distance"], 2),
        "join" => (&["curves"], 1),
        "insert_vertex" => (&["curve", "after", "point"], 3),
        "remove_vertex" => (&["curve", "index"], 2),
        "rebuild" => (&["curve", "degree", "spans"], 3),
        _ => return None,
    };
    Some(sig)
}

/// A call's arguments bound to its parameters as Python binds them, with
/// typed readers whose refusals name the call and the parameter.
struct Bound<'a> {
    call: &'static str,
    params: &'static [&'static str],
    values: Vec<Option<&'a Value>>,
}

impl<'a> Bound<'a> {
    fn bind(call: &'static str, args: &'a [Value], kwargs: Option<&'a Map<String, Value>>) -> Result<Bound<'a>, String> {
        let (params, required) = signature(call).ok_or_else(|| format!("no sketch method {call}"))?;
        let list = params.join(", ");
        if args.len() > params.len() {
            return Err(format!("{call}: takes at most {} arguments ({list}), got {}", params.len(), args.len()));
        }
        let mut values: Vec<Option<&Value>> = vec![None; params.len()];
        for (slot, a) in values.iter_mut().zip(args) {
            *slot = Some(a);
        }
        for (key, value) in kwargs.into_iter().flatten() {
            let Some(i) = params.iter().position(|p| *p == key.as_str()) else {
                return Err(format!("{call}: no argument {key} (takes {list})"));
            };
            if values[i].is_some() {
                return Err(format!("{call}: argument {key} given twice (by position and by name)"));
            }
            values[i] = Some(value);
        }
        if let Some(i) = (0..required).find(|&i| values[i].is_none()) {
            return Err(format!("{call}: missing argument {} (takes {list}; {required} required)", params[i]));
        }
        Ok(Bound { call, params, values })
    }
    fn refuse(&self, i: usize, what: &str, got: &Value) -> String {
        format!("{}: argument {} must be {what} (got {got})", self.call, self.params[i])
    }
    /// The value at `i` (bound; required ones are checked in `bind`).
    fn value(&self, i: usize) -> Result<&'a Value, String> {
        self.values[i].ok_or_else(|| format!("{}: missing argument {}", self.call, self.params[i]))
    }
    fn given(&self, i: usize) -> bool {
        self.values[i].is_some()
    }
    /// Given and not `null` (an optional parameter whose Python default is `None`).
    fn given_not_null(&self, i: usize) -> bool {
        self.values[i].is_some_and(|v| !v.is_null())
    }
    fn point(&self, i: usize) -> Result<Uv, String> {
        let v = self.value(i)?;
        uv_of(v).ok_or_else(|| self.refuse(i, "a point [u, v] of two finite numbers", v))
    }
    fn points(&self, i: usize) -> Result<Vec<Uv>, String> {
        let v = self.value(i)?;
        let list = v.as_array().ok_or_else(|| self.refuse(i, "a list of points [[u, v], …]", v))?;
        list.iter().enumerate().map(|(k, p)| uv_of(p).ok_or_else(|| format!("{}: argument {}[{k}] must be a point [u, v] of two finite numbers (got {p})", self.call, self.params[i]))).collect()
    }
    fn number(&self, i: usize) -> Result<f64, String> {
        let v = self.value(i)?;
        v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| self.refuse(i, "a finite number", v))
    }
    fn index(&self, i: usize) -> Result<usize, String> {
        let v = self.value(i)?;
        index_of(v).ok_or_else(|| self.refuse(i, "a curve index (a non-negative integer)", v))
    }
    fn indices(&self, i: usize) -> Result<Vec<usize>, String> {
        let v = self.value(i)?;
        let list = v.as_array().ok_or_else(|| self.refuse(i, "a list of curve indices", v))?;
        list.iter().enumerate().map(|(k, x)| index_of(x).ok_or_else(|| format!("{}: argument {}[{k}] must be a curve index (a non-negative integer) (got {x})", self.call, self.params[i]))).collect()
    }
    fn integer(&self, i: usize) -> Result<i64, String> {
        let v = self.value(i)?;
        v.as_i64().ok_or_else(|| self.refuse(i, "an integer", v))
    }
    fn count(&self, i: usize) -> Result<u32, String> {
        let v = self.value(i)?;
        v.as_u64().and_then(|x| u32::try_from(x).ok()).ok_or_else(|| self.refuse(i, "a non-negative integer", v))
    }
    fn boolean(&self, i: usize) -> Result<bool, String> {
        let v = self.value(i)?;
        v.as_bool().ok_or_else(|| self.refuse(i, "true or false", v))
    }
    fn string(&self, i: usize) -> Result<String, String> {
        let v = self.value(i)?;
        v.as_str().map(str::to_string).ok_or_else(|| self.refuse(i, "a string", v))
    }
}

/// A number argument of a [`SketchCall`], for the finiteness check.
enum Num<'a> {
    Number(f64),
    Point(Uv),
    Points(&'a [Uv]),
}

/// Twice the signed area of the triangle a, b, c: kernel/sketch.py's
/// `circumcircle` denominator `d`, which it refuses ("the three points are
/// collinear") when `abs(d) < 1e-12`.
fn circumcircle_denominator(a: Uv, b: Uv, c: Uv) -> f64 {
    2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]))
}

/// A curve index: a non-negative JSON integer (not `1.0`).
fn index_of(v: &Value) -> Option<usize> {
    v.as_u64().and_then(|x| usize::try_from(x).ok())
}

fn pt(p: Uv) -> Value {
    json!([p[0], p[1]])
}
fn pts(points: &[Uv]) -> Value {
    Value::Array(points.iter().copied().map(pt).collect())
}

impl SketchCall {
    /// The `Sketch` method's name.
    pub fn name(&self) -> &'static str {
        use SketchCall::*;
        match self {
            Line { .. } => "line",
            Polyline { .. } => "polyline",
            Spline { .. } => "spline",
            ControlCurve { .. } => "control_curve",
            Circle { .. } => "circle",
            CircleTwoPoint { .. } => "circle_two_point",
            CircleThreePoint { .. } => "circle_three_point",
            CircleTangent { .. } => "circle_tangent",
            Ellipse { .. } => "ellipse",
            Arc { .. } => "arc",
            ArcThreePoint { .. } => "arc_three_point",
            ArcTangent { .. } => "arc_tangent",
            Rectangle { .. } => "rectangle",
            RectangleCenter { .. } => "rectangle_center",
            RectangleThreePoint { .. } => "rectangle_three_point",
            Polygon { .. } => "polygon",
            Slot { .. } => "slot",
            Spiral { .. } => "spiral",
            Text { .. } => "text",
            Remove { .. } => "remove",
            Reverse { .. } => "reverse",
            SplitAt { .. } => "split_at",
            Trim { .. } => "trim",
            Extend { .. } => "extend",
            FilletCorner { .. } => "fillet_corner",
            Offset { .. } => "offset",
            Join { .. } => "join",
            Unjoin { .. } => "unjoin",
            InsertVertex { .. } => "insert_vertex",
            RemoveVertex { .. } => "remove_vertex",
            Rebuild { .. } => "rebuild",
        }
    }
    /// The call as api.py reads it: `[name, [args…]]`, or `[name, [args…],
    /// {kwargs}]` when a keyword is sent. Points as `[u, v]`; optional
    /// trailing parameters left at their Python default are not sent.
    /// Only `polygon` with `sides` None and a rotation sends a keyword
    /// (`{"rotation": deg}`; api.py does not convert keyword values, a
    /// number needs none); `circle_tangent` with `near` but no radius
    /// sends `null` for the radius positionally so `near` still becomes a
    /// tuple.
    pub fn to_json(&self) -> Value {
        use SketchCall::*;
        let mut kwargs = Map::new();
        let args: Vec<Value> = match self {
            Line { a, b } | CircleTwoPoint { a, b } => vec![pt(*a), pt(*b)],
            Polyline { points, closed } | Spline { points, closed } => {
                let mut v = vec![pts(points)];
                if *closed {
                    v.push(json!(true));
                }
                v
            }
            ControlCurve { points, degree, closed } => {
                let mut v = vec![pts(points)];
                if *closed || *degree != 3 {
                    v.push(json!(degree));
                }
                if *closed {
                    v.push(json!(true));
                }
                v
            }
            Circle { center, radius } => vec![pt(*center), json!(radius)],
            CircleThreePoint { a, b, c } | ArcThreePoint { a, b, c } | RectangleThreePoint { a, b, c } => vec![pt(*a), pt(*b), pt(*c)],
            CircleTangent { curves, radius, near } => {
                let mut v = vec![json!(curves)];
                if *near != [0.0, 0.0] {
                    v.push(json!(radius));
                    v.push(pt(*near));
                } else if let Some(r) = radius {
                    v.push(json!(r));
                }
                v
            }
            Ellipse { center, radius_x, radius_y, rotation } => {
                let mut v = vec![pt(*center), json!(radius_x), json!(radius_y)];
                if *rotation != 0.0 {
                    v.push(json!(rotation));
                }
                v
            }
            Arc { center, radius, start_deg, end_deg } => vec![pt(*center), json!(radius), json!(start_deg), json!(end_deg)],
            ArcTangent { prev, end } => vec![json!(prev), pt(*end)],
            Rectangle { corner, size } => vec![pt(*corner), pt(*size)],
            RectangleCenter { center, size } => vec![pt(*center), pt(*size)],
            Polygon { center, radius, sides, rotation } => {
                let mut v = vec![pt(*center), json!(radius)];
                match sides {
                    Some(s) => {
                        v.push(json!(s));
                        if *rotation != 0.0 {
                            v.push(json!(rotation));
                        }
                    }
                    None if *rotation != 0.0 => {
                        kwargs.insert("rotation".to_string(), json!(rotation));
                    }
                    None => {}
                }
                v
            }
            Slot { a, b, width } => vec![pt(*a), pt(*b), json!(width)],
            Spiral { center, start_radius, end_radius, turns } => vec![pt(*center), json!(start_radius), json!(end_radius), json!(turns)],
            Text { origin, text, height } => vec![pt(*origin), json!(text), json!(height)],
            Remove { curve } | Reverse { curve } | Unjoin { curve } => vec![json!(curve)],
            SplitAt { curve, point } => vec![json!(curve), pt(*point)],
            Trim { curve, cutters, click } => vec![json!(curve), json!(cutters), pt(*click)],
            Extend { curve, targets, both } => {
                let mut v = vec![json!(curve), json!(targets)];
                if !*both {
                    v.push(json!(false));
                }
                v
            }
            FilletCorner { curve, vertex_index, radius } => vec![json!(curve), json!(vertex_index), json!(radius)],
            Offset { curve, distance } => vec![json!(curve), json!(distance)],
            Join { curves } => vec![json!(curves)],
            InsertVertex { curve, after, point } => vec![json!(curve), json!(after), pt(*point)],
            RemoveVertex { curve, index } => vec![json!(curve), json!(index)],
            Rebuild { curve, degree, spans } => vec![json!(curve), json!(degree), json!(spans)],
        };
        if kwargs.is_empty() { json!([self.name(), args]) } else { json!([self.name(), args, Value::Object(kwargs)]) }
    }
    /// A REST caller's call (`[name, [args…], {kwargs}?]`, as api.py takes
    /// it; keywords by kernel/sketch.py's parameter names), refused naming
    /// the call and the argument: an unknown name, a wrong count or shape,
    /// an unknown or repeated keyword, a non-finite number, a negative or
    /// non-integer curve index, and what [`SketchCall::check`] refuses
    /// without a curve count.
    pub fn from_json(v: &Value) -> Result<SketchCall, String> {
        let list = v.as_array().filter(|a| (1..=3).contains(&a.len())).ok_or_else(|| format!("a sketch call is [name, [args…], {{kwargs}}?] (got {v})"))?;
        let given = list[0].as_str().ok_or_else(|| format!("a sketch call starts with its name, a string (got {})", list[0]))?;
        let name: &'static str = SKETCH_CALLS.iter().copied().find(|n| *n == given).ok_or_else(|| format!("no sketch method {given} (RoboCAD's sketch calls: {})", SKETCH_CALLS.join(", ")))?;
        let args: &[Value] = match list.get(1) {
            None => &[],
            Some(Value::Array(a)) => a,
            Some(other) => return Err(format!("{name}: arguments must be a list [args…] (got {other})")),
        };
        let kwargs = match list.get(2) {
            None => None,
            Some(Value::Object(m)) => Some(m),
            Some(other) => return Err(format!("{name}: keyword arguments must be an object {{name: value}} (got {other})")),
        };
        let b = Bound::bind(name, args, kwargs)?;
        use SketchCall::*;
        let call = match name {
            "line" => Line { a: b.point(0)?, b: b.point(1)? },
            "polyline" => Polyline { points: b.points(0)?, closed: if b.given(1) { b.boolean(1)? } else { false } },
            "spline" => Spline { points: b.points(0)?, closed: if b.given(1) { b.boolean(1)? } else { false } },
            "control_curve" => ControlCurve { points: b.points(0)?, degree: if b.given(1) { b.count(1)? } else { 3 }, closed: if b.given(2) { b.boolean(2)? } else { false } },
            "circle" => Circle { center: b.point(0)?, radius: b.number(1)? },
            "circle_two_point" => CircleTwoPoint { a: b.point(0)?, b: b.point(1)? },
            "circle_three_point" => CircleThreePoint { a: b.point(0)?, b: b.point(1)?, c: b.point(2)? },
            "circle_tangent" => CircleTangent { curves: b.indices(0)?, radius: if b.given_not_null(1) { Some(b.number(1)?) } else { None }, near: if b.given(2) { b.point(2)? } else { [0.0, 0.0] } },
            "ellipse" => Ellipse { center: b.point(0)?, radius_x: b.number(1)?, radius_y: b.number(2)?, rotation: if b.given(3) { b.number(3)? } else { 0.0 } },
            "arc" => Arc { center: b.point(0)?, radius: b.number(1)?, start_deg: b.number(2)?, end_deg: b.number(3)? },
            "arc_three_point" => ArcThreePoint { a: b.point(0)?, b: b.point(1)?, c: b.point(2)? },
            "arc_tangent" => ArcTangent { prev: b.index(0)?, end: b.point(1)? },
            "rectangle" => Rectangle { corner: b.point(0)?, size: b.point(1)? },
            "rectangle_center" => RectangleCenter { center: b.point(0)?, size: b.point(1)? },
            "rectangle_three_point" => RectangleThreePoint { a: b.point(0)?, b: b.point(1)?, c: b.point(2)? },
            "polygon" => Polygon { center: b.point(0)?, radius: b.number(1)?, sides: if b.given_not_null(2) { Some(b.count(2)?) } else { None }, rotation: if b.given(3) { b.number(3)? } else { 0.0 } },
            "slot" => Slot { a: b.point(0)?, b: b.point(1)?, width: b.number(2)? },
            "spiral" => Spiral { center: b.point(0)?, start_radius: b.number(1)?, end_radius: b.number(2)?, turns: b.number(3)? },
            "text" => Text { origin: b.point(0)?, text: b.string(1)?, height: if b.given(2) { b.number(2)? } else { 10.0 } },
            "remove" => Remove { curve: b.index(0)? },
            "reverse" => Reverse { curve: b.index(0)? },
            "unjoin" => Unjoin { curve: b.index(0)? },
            "split_at" => SplitAt { curve: b.index(0)?, point: b.point(1)? },
            "trim" => Trim { curve: b.index(0)?, cutters: b.indices(1)?, click: b.point(2)? },
            "extend" => Extend { curve: b.index(0)?, targets: b.indices(1)?, both: if b.given(2) { b.boolean(2)? } else { true } },
            "fillet_corner" => FilletCorner { curve: b.index(0)?, vertex_index: b.integer(1)?, radius: b.number(2)? },
            "offset" => Offset { curve: b.index(0)?, distance: b.number(1)? },
            "join" => Join { curves: b.indices(0)? },
            "insert_vertex" => InsertVertex { curve: b.index(0)?, after: b.integer(1)?, point: b.point(2)? },
            "remove_vertex" => RemoveVertex { curve: b.index(0)?, index: b.integer(1)? },
            "rebuild" => Rebuild { curve: b.index(0)?, degree: b.count(1)?, spans: b.count(2)? },
            // `name` is one of SKETCH_CALLS, each matched above.
            other => return Err(format!("no sketch method {other} (RoboCAD's sketch calls: {})", SKETCH_CALLS.join(", "))),
        };
        call.check(None)?;
        Ok(call)
    }
    /// Each curve-index argument: its name as a refusal says it
    /// (`cutters[1]`) and the index.
    fn curve_args(&self) -> Vec<(String, usize)> {
        use SketchCall::*;
        let list = |name: &str, xs: &[usize]| xs.iter().enumerate().map(|(k, &i)| (format!("{name}[{k}]"), i)).collect::<Vec<_>>();
        match self {
            CircleTangent { curves, .. } | Join { curves } => list("curves", &curves[..]),
            ArcTangent { prev, .. } => vec![("prev".to_string(), *prev)],
            Remove { curve } | Reverse { curve } | Unjoin { curve } | SplitAt { curve, .. } | FilletCorner { curve, .. } | Offset { curve, .. } | InsertVertex { curve, .. } | RemoveVertex { curve, .. } | Rebuild { curve, .. } => vec![("curve".to_string(), *curve)],
            Trim { curve, cutters, .. } => {
                let mut v = vec![("curve".to_string(), *curve)];
                v.extend(list("cutters", &cutters[..]));
                v
            }
            Extend { curve, targets, .. } => {
                let mut v = vec![("curve".to_string(), *curve)];
                v.extend(list("targets", &targets[..]));
                v
            }
            _ => Vec::new(),
        }
    }
    /// Each number and point argument by its parameter name.
    fn number_args(&self) -> Vec<(&'static str, Num<'_>)> {
        use Num::{Number as N, Point as P, Points as Ps};
        use SketchCall::*;
        match self {
            Line { a, b } | CircleTwoPoint { a, b } => vec![("a", P(*a)), ("b", P(*b))],
            Polyline { points, .. } | Spline { points, .. } | ControlCurve { points, .. } => vec![("points", Ps(&points[..]))],
            Circle { center, radius } => vec![("center", P(*center)), ("radius", N(*radius))],
            CircleThreePoint { a, b, c } | ArcThreePoint { a, b, c } | RectangleThreePoint { a, b, c } => vec![("a", P(*a)), ("b", P(*b)), ("c", P(*c))],
            CircleTangent { radius, near, .. } => {
                let mut v = vec![("near", P(*near))];
                if let Some(r) = radius {
                    v.insert(0, ("radius", N(*r)));
                }
                v
            }
            Ellipse { center, radius_x, radius_y, rotation } => vec![("center", P(*center)), ("radius_x", N(*radius_x)), ("radius_y", N(*radius_y)), ("rotation", N(*rotation))],
            Arc { center, radius, start_deg, end_deg } => vec![("center", P(*center)), ("radius", N(*radius)), ("start_deg", N(*start_deg)), ("end_deg", N(*end_deg))],
            ArcTangent { end, .. } => vec![("end", P(*end))],
            Rectangle { corner, size } => vec![("corner", P(*corner)), ("size", P(*size))],
            RectangleCenter { center, size } => vec![("center", P(*center)), ("size", P(*size))],
            Polygon { center, radius, rotation, .. } => vec![("center", P(*center)), ("radius", N(*radius)), ("rotation", N(*rotation))],
            Slot { a, b, width } => vec![("a", P(*a)), ("b", P(*b)), ("width", N(*width))],
            Spiral { center, start_radius, end_radius, turns } => vec![("center", P(*center)), ("start_radius", N(*start_radius)), ("end_radius", N(*end_radius)), ("turns", N(*turns))],
            Text { origin, height, .. } => vec![("origin", P(*origin)), ("height", N(*height))],
            SplitAt { point, .. } | InsertVertex { point, .. } => vec![("point", P(*point))],
            Trim { click, .. } => vec![("click", P(*click))],
            FilletCorner { radius, .. } => vec![("radius", N(*radius))],
            Offset { distance, .. } => vec![("distance", N(*distance))],
            Remove { .. } | Reverse { .. } | Extend { .. } | Join { .. } | Unjoin { .. } | RemoveVertex { .. } | Rebuild { .. } => Vec::new(),
        }
    }
    /// A non-finite number or point coordinate, refused as
    /// [`SketchCall::from_json`] words it.
    fn check_finite(&self) -> Result<(), String> {
        let name = self.name();
        let point = |p: &Uv| p.iter().all(|x| x.is_finite());
        for (arg, value) in self.number_args() {
            match value {
                Num::Number(x) if !x.is_finite() => return Err(format!("{name}: argument {arg} must be a finite number (got {x})")),
                Num::Point(p) if !point(&p) => return Err(format!("{name}: argument {arg} must be a point [u, v] of two finite numbers (got [{}, {}])", p[0], p[1])),
                Num::Points(list) => {
                    if let Some((k, p)) = list.iter().enumerate().find(|(_, p)| !point(p)) {
                        return Err(format!("{name}: argument {arg}[{k}] must be a point [u, v] of two finite numbers (got [{}, {}])", p[0], p[1]));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    /// Refuse what RoboCAD's REST would fail on before any geometry,
    /// naming the call and the argument: a curve index at or past `curves`
    /// (the curve count when the call runs, if known); `join` and
    /// `circle_tangent` without curves, `circle_tangent` without a radius
    /// and fewer than three curves (`KernelError`); a degree or span count
    /// of 0; a non-finite number or point coordinate (as
    /// [`SketchCall::from_json`] refuses it); `circle_three_point` and
    /// `arc_three_point` through three collinear (or coincident) points,
    /// with kernel/sketch.py `circumcircle`'s tolerance. Where RoboCAD has
    /// its own words for a refusal, the text after the call's name is them
    /// verbatim: `circumcircle`'s "the three points are collinear"
    /// (kernel/sketch.py:616, RoboCAD's 422) and api.py `edit_sketch`'s
    /// "curve index {i} out of range ({n} curves)" (its 400, `n` the count
    /// when the call runs), each followed by the argument it names. Stricter than
    /// RoboCAD, also refused: `join` naming a curve
    /// twice (RoboCAD would build a polyline that doubles back) and a
    /// polygon of fewer than three sides (0 would mean "the last sides" to
    /// RoboCAD).
    pub fn check(&self, curves: Option<usize>) -> Result<(), String> {
        use SketchCall::*;
        let name = self.name();
        self.check_finite()?;
        match self {
            CircleThreePoint { a, b, c } | ArcThreePoint { a, b, c } if circumcircle_denominator(*a, *b, *c).abs() < 1e-12 => {
                return Err(format!("{name}: the three points are collinear (arguments a [{}, {}], b [{}, {}] and c [{}, {}])", a[0], a[1], b[0], b[1], c[0], c[1]));
            }
            Join { curves: list } | CircleTangent { curves: list, .. } if list.is_empty() => return Err(format!("{name}: argument curves must name at least one curve")),
            Join { curves: list } => {
                if let Some((k, i)) = list.iter().enumerate().find(|&(k, i)| list[..k].contains(i)) {
                    return Err(format!("{name}: argument curves[{k}] names curve {i} twice"));
                }
            }
            CircleTangent { curves: list, radius: None, .. } if list.len() < 3 => return Err(format!("{name}: argument radius is needed for fewer than three curves (got {} curves)", list.len())),
            Polygon { sides: Some(s), .. } if *s < 3 => return Err(format!("{name}: argument sides must be at least 3 (got {s})")),
            ControlCurve { degree: 0, .. } | Rebuild { degree: 0, .. } => return Err(format!("{name}: argument degree must be at least 1 (got 0)")),
            Rebuild { spans: 0, .. } => return Err(format!("{name}: argument spans must be at least 1 (got 0)")),
            _ => {}
        }
        if let Some(count) = curves {
            for (arg, i) in self.curve_args() {
                if i >= count {
                    // api.py's words exactly ("(1 curves)" included).
                    return Err(format!("{name}: argument {arg}: curve index {i} out of range ({count} curves)"));
                }
            }
        }
        Ok(())
    }
    /// How many curves the sketch has after this call, given `before`
    /// (kernel/sketch.py): constructors and `offset` add one, `split_at`
    /// one more piece; `text` adds one per glyph outline, `trim` leaves 0–2
    /// pieces and `unjoin` one line per segment: None (unknown); `remove`
    /// takes one; `join` replaces its curves with one; the rest edit a
    /// curve in place.
    pub fn curves_after(&self, before: usize) -> Option<usize> {
        use SketchCall::*;
        match self {
            Text { .. } | Trim { .. } | Unjoin { .. } => None,
            Line { .. } | Polyline { .. } | Spline { .. } | ControlCurve { .. } | Circle { .. } | CircleTwoPoint { .. } | CircleThreePoint { .. } | CircleTangent { .. } | Ellipse { .. } | Arc { .. } | ArcThreePoint { .. } | ArcTangent { .. } | Rectangle { .. } | RectangleCenter { .. } | RectangleThreePoint { .. } | Polygon { .. } | Slot { .. } | Spiral { .. } | Offset { .. } | SplitAt { .. } => Some(before + 1),
            Remove { .. } => Some(before.saturating_sub(1)),
            Join { curves } => {
                let mut distinct = curves.clone();
                distinct.sort_unstable();
                distinct.dedup();
                (distinct.len() == curves.len()).then(|| before.saturating_sub(curves.len()) + 1)
            }
            Reverse { .. } | Extend { .. } | FilletCorner { .. } | InsertVertex { .. } | RemoveVertex { .. } | Rebuild { .. } => Some(before),
        }
    }
}


/// The calls checked in order against a sketch of `curves` curves (None:
/// unknown), each against the count the calls before it leave
/// ([`SketchCall::curves_after`]): the first refusal names the call's
/// position (1-based) and name, "call 2 (trim): argument cutters[0]: curve
/// index 4 out of range (3 curves)" (the count the calls before it leave).
pub fn check_calls(calls: &[SketchCall], curves: Option<usize>) -> Result<(), String> {
    let mut count = curves;
    for (k, call) in calls.iter().enumerate() {
        if let Err(e) = call.check(count) {
            let name = call.name();
            let rest = e.strip_prefix(name).and_then(|r| r.strip_prefix(": ")).unwrap_or(&e);
            return Err(format!("call {} ({name}): {rest}", k + 1));
        }
        count = count.and_then(|n| call.curves_after(n));
    }
    Ok(())
}

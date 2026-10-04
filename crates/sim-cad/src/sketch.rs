//! Sketches: curves in a construction plane's (u, v) coordinates, stored in a
//! node's `sketch` as RoboCAD writes it, edited by RoboCAD's sketch calls
//! (`[method, [args…], {kwargs}?]`, curves by index) and turned into kernel
//! wires and profile faces through the plane. Reference: RoboCAD's
//! kernel/sketch.py (`Curve`, `Sketch`, the 2D helpers) and kernel/base.py
//! (`Plane`).
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub type V2 = [f64; 2];
pub type V3 = [f64; 3];

fn sub3(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot3(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross3(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unit3(a: V3) -> V3 {
    let l = dot3(a, a).sqrt();
    if l < 1e-12 { [0., 0., 1.] } else { [a[0] / l, a[1] / l, a[2] / l] }
}

/// A construction plane: origin, normal and an in-plane x axis (mm).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plane {
    pub origin: V3,
    pub normal: V3,
    pub x_axis: V3,
}
impl Plane {
    pub fn xy(z: f64) -> Plane {
        Plane { origin: [0., 0., z], normal: [0., 0., 1.], x_axis: [1., 0., 0.] }
    }
    pub fn xz(y: f64) -> Plane {
        Plane { origin: [0., y, 0.], normal: [0., -1., 0.], x_axis: [1., 0., 0.] }
    }
    pub fn yz(x: f64) -> Plane {
        Plane { origin: [x, 0., 0.], normal: [1., 0., 0.], x_axis: [0., 1., 0.] }
    }
    /// A plane through `origin` with `normal`; the x axis as given, else
    /// world X projected (world Y for a plane facing X).
    pub fn new(origin: V3, normal: V3, x_axis: Option<V3>) -> Result<Plane, String> {
        if dot3(normal, normal) < 1e-20 {
            return Err("a plane needs a nonzero normal".into());
        }
        let n = unit3(normal);
        let x = match x_axis {
            Some(x) => x,
            None if n[0].abs() < 0.9 => [1., 0., 0.],
            None => [0., 1., 0.],
        };
        let x = sub3(x, [n[0] * dot3(x, n), n[1] * dot3(x, n), n[2] * dot3(x, n)]);
        if dot3(x, x) < 1e-20 {
            return Err("a plane's x axis must not be parallel to its normal".into());
        }
        Ok(Plane { origin, normal: n, x_axis: unit3(x) })
    }
    pub fn y_axis(&self) -> V3 {
        unit3(cross3(self.normal, self.x_axis))
    }
    pub fn to_world(&self, p: V2) -> V3 {
        let y = self.y_axis();
        [0, 1, 2].map(|i| self.origin[i] + self.x_axis[i] * p[0] + y[i] * p[1])
    }
    pub fn to_local(&self, p: V3) -> V2 {
        let d = sub3(p, self.origin);
        [dot3(d, self.x_axis), dot3(d, self.y_axis())]
    }
    pub fn json(&self) -> Value {
        json!({"origin": self.origin, "normal": self.normal, "x_axis": self.x_axis})
    }
    /// A plane from RoboCAD's forms: "xy" | "xz" | "yz", `{origin, normal[, x_axis]}`
    /// or `{axis: "x" | "y" | "z", offset}` (`ArgConverter.plane`); a plane
    /// node's id is resolved by the caller.
    pub fn parse(v: &Value) -> Result<Plane, String> {
        let p3 = |v: &Value, what: &str| -> Result<V3, String> {
            let a = v.as_array().filter(|a| a.len() == 3).ok_or_else(|| format!("{what} must be [x, y, z]"))?;
            let mut o = [0.; 3];
            for i in 0..3 {
                o[i] = a[i].as_f64().filter(|x| x.is_finite()).ok_or_else(|| format!("{what} must be finite"))?;
            }
            Ok(o)
        };
        match v {
            Value::String(s) => match s.as_str() {
                "xy" => Ok(Plane::xy(0.)),
                "xz" => Ok(Plane::xz(0.)),
                "yz" => Ok(Plane::yz(0.)),
                other => Err(format!("unknown plane {other:?} (xy/xz/yz or a plane node id)")),
            },
            Value::Object(o) if o.contains_key("axis") => {
                let off = o.get("offset").and_then(Value::as_f64).unwrap_or(0.);
                match o["axis"].as_str() {
                    Some("x") => Ok(Plane::yz(off)),
                    Some("y") => Ok(Plane::xz(off)),
                    Some("z") => Ok(Plane::xy(off)),
                    _ => Err("a plane's axis is x, y or z".into()),
                }
            }
            Value::Object(o) => {
                let origin = p3(o.get("origin").ok_or("a plane needs an origin")?, "plane origin")?;
                let normal = p3(o.get("normal").ok_or("a plane needs a normal")?, "plane normal")?;
                let x = o.get("x_axis").map(|x| p3(x, "plane x_axis")).transpose()?;
                Plane::new(origin, normal, x)
            }
            _ => Err("a plane is 'xy'|'xz'|'yz', a plane node id, {origin, normal[, x_axis]} or {axis, offset}".into()),
        }
    }
}

/// One sketch curve (`Curve`): `kind` line | polyline | circle | arc |
/// ellipse | spline | control | slot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Curve {
    pub kind: String,
    pub points: Vec<V2>,
    pub center: Option<V2>,
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
impl Default for Curve {
    fn default() -> Self {
        Curve { kind: "line".into(), points: vec![], center: None, radius: 0., radius2: 0., start_angle: 0., end_angle: 360., rotation: 0., degree: 3, closed: false, text: String::new(), height: 10., font: String::new(), name: String::new() }
    }
}

fn p2(a: V2, b: V2) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
fn u2(v: V2) -> V2 {
    let n = v[0].hypot(v[1]);
    if n < 1e-12 { [1., 0.] } else { [v[0] / n, v[1] / n] }
}

impl Curve {
    fn of(kind: &str) -> Curve {
        Curve { kind: kind.into(), ..Curve::default() }
    }
    pub fn line(a: V2, b: V2) -> Curve {
        Curve { points: vec![a, b], ..Curve::of("line") }
    }
    pub fn polyline(points: Vec<V2>, closed: bool) -> Curve {
        Curve { points, closed, ..Curve::of("polyline") }
    }
    pub fn arc(center: V2, radius: f64, a0: f64, a1: f64) -> Curve {
        Curve { center: Some(center), radius, start_angle: a0, end_angle: a1, ..Curve::of("arc") }
    }
    pub fn circle(center: V2, radius: f64) -> Curve {
        Curve { center: Some(center), radius, ..Curve::of("circle") }
    }
    fn c(&self) -> V2 {
        self.center.unwrap_or([0., 0.])
    }
    pub fn start(&self) -> V2 {
        match self.kind.as_str() {
            "line" | "polyline" | "spline" | "control" => self.points[0],
            "arc" => {
                let a = self.start_angle.to_radians();
                [self.c()[0] + self.radius * a.cos(), self.c()[1] + self.radius * a.sin()]
            }
            _ => self.points.first().copied().unwrap_or(self.c()),
        }
    }
    pub fn end(&self) -> V2 {
        match self.kind.as_str() {
            "line" | "polyline" | "spline" | "control" => *self.points.last().expect("points"),
            "arc" => {
                let a = self.end_angle.to_radians();
                [self.c()[0] + self.radius * a.cos(), self.c()[1] + self.radius * a.sin()]
            }
            _ => self.points.last().copied().unwrap_or(self.c()),
        }
    }
    /// Points along the curve (plane space), `n` steps for round curves.
    pub fn sample(&self, n: usize) -> Vec<V2> {
        match self.kind.as_str() {
            "line" => vec![self.points[0], self.points[1]],
            "polyline" => {
                let mut v = self.points.clone();
                if self.closed && !v.is_empty() {
                    v.push(v[0]);
                }
                v
            }
            "circle" | "arc" | "ellipse" => {
                let (a0, a1) = if self.kind == "arc" { (self.start_angle.to_radians(), self.end_angle.to_radians()) } else { (0., std::f64::consts::TAU) };
                let r2 = if self.kind == "ellipse" { self.radius2 } else { self.radius };
                let rot = self.rotation.to_radians();
                (0..=n)
                    .map(|i| {
                        let a = a0 + (a1 - a0) * i as f64 / n as f64;
                        let (x, y) = (self.radius * a.cos(), r2 * a.sin());
                        [self.c()[0] + x * rot.cos() - y * rot.sin(), self.c()[1] + x * rot.sin() + y * rot.cos()]
                    })
                    .collect()
            }
            "spline" | "control" => sample_spline(&self.points, self.degree as usize, self.closed, n, self.kind == "spline"),
            "slot" => {
                let (a, b) = (self.points[0], self.points[1]);
                let d = u2([b[0] - a[0], b[1] - a[1]]);
                let ang = d[1].atan2(d[0]);
                let r = self.radius;
                let mut out = Vec::new();
                for k in 0..=n / 2 {
                    let t = ang - std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * k as f64 / (n / 2) as f64;
                    out.push([b[0] + r * t.cos(), b[1] + r * t.sin()]);
                }
                for k in 0..=n / 2 {
                    let t = ang + std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * k as f64 / (n / 2) as f64;
                    out.push([a[0] + r * t.cos(), a[1] + r * t.sin()]);
                }
                out.push(out[0]);
                out
            }
            _ => self.points.clone(),
        }
    }
    pub fn reversed(&self) -> Curve {
        let mut c = self.clone();
        match c.kind.as_str() {
            "line" | "polyline" | "spline" | "control" => c.points.reverse(),
            "arc" => std::mem::swap(&mut c.start_angle, &mut c.end_angle),
            _ => {}
        }
        c
    }
}

fn sample_spline(points: &[V2], degree: usize, closed: bool, n: usize, interpolate: bool) -> Vec<V2> {
    if points.len() < 2 {
        return points.to_vec();
    }
    if interpolate {
        // Catmull-Rom through the points, for display; the kernel builds the exact interpolation.
        let mut pts = points.to_vec();
        if closed {
            pts.push(points[0]);
        }
        let steps = n / (pts.len() - 1).max(1) + 1;
        let mut out = Vec::new();
        for i in 0..pts.len() - 1 {
            let p0 = if i > 0 { pts[i - 1] } else { pts[i] };
            let (p1, p2) = (pts[i], pts[i + 1]);
            let p3 = if i + 2 < pts.len() { pts[i + 2] } else { pts[i + 1] };
            for k in 0..steps {
                let t = k as f64 / steps as f64;
                let (t2, t3) = (t * t, t * t * t);
                let f = |j: usize| 0.5 * ((2. * p1[j]) + (-p0[j] + p2[j]) * t + (2. * p0[j] - 5. * p1[j] + 4. * p2[j] - p3[j]) * t2 + (-p0[j] + 3. * p1[j] - 3. * p2[j] + p3[j]) * t3);
                out.push([f(0), f(1)]);
            }
        }
        out.push(*pts.last().expect("points"));
        return out;
    }
    // de Boor, clamped uniform knots.
    let pts = points.to_vec();
    let k = degree.min(pts.len() - 1);
    let mut knots = vec![0.; k + 1];
    for i in 1..pts.len() - k {
        knots.push(i as f64 / (pts.len() - k) as f64);
    }
    knots.extend(std::iter::repeat_n(1., k + 1));
    (0..=n)
        .map(|s| {
            let t = s as f64 / n as f64;
            if t >= 1. {
                return *pts.last().expect("points");
            }
            let mut span = k;
            while span < pts.len() - 1 && t >= knots[span + 1] {
                span += 1;
            }
            let mut d: Vec<V2> = (0..=k).map(|j| pts[j + span - k]).collect();
            for r in 1..=k {
                for j in (r..=k).rev() {
                    let i = j + span - k;
                    let den = knots[i + k - r + 1] - knots[i];
                    let alpha = if den == 0. { 0. } else { (t - knots[i]) / den };
                    d[j] = [(1. - alpha) * d[j - 1][0] + alpha * d[j][0], (1. - alpha) * d[j - 1][1] + alpha * d[j][1]];
                }
            }
            d[k]
        })
        .collect()
}

/// `circumcircle(a, b, c)`.
pub fn circumcircle(a: V2, b: V2, c: V2) -> Result<(V2, f64), String> {
    let d = 2. * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-12 {
        return Err("the three points are collinear".into());
    }
    let s = |p: V2| p[0] * p[0] + p[1] * p[1];
    let ux = (s(a) * (b[1] - c[1]) + s(b) * (c[1] - a[1]) + s(c) * (a[1] - b[1])) / d;
    let uy = (s(a) * (c[0] - b[0]) + s(b) * (a[0] - c[0]) + s(c) * (b[0] - a[0])) / d;
    Ok(([ux, uy], p2([ux, uy], a)))
}
fn seg_param(a: V2, b: V2, p: V2) -> f64 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let l2 = ab[0] * ab[0] + ab[1] * ab[1];
    if l2 < 1e-18 { 0. } else { (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / l2).clamp(0., 1.) }
}
fn seg_dist(a: V2, b: V2, p: V2) -> f64 {
    let t = seg_param(a, b, p);
    p2([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t], p)
}
fn seg_intersect(a: V2, b: V2, c: V2, d: V2) -> Option<V2> {
    let r = [b[0] - a[0], b[1] - a[1]];
    let s = [d[0] - c[0], d[1] - c[1]];
    let den = r[0] * s[1] - r[1] * s[0];
    if den.abs() < 1e-12 {
        return None;
    }
    let qp = [c[0] - a[0], c[1] - a[1]];
    let t = (qp[0] * s[1] - qp[1] * s[0]) / den;
    let u = (qp[0] * r[1] - qp[1] * r[0]) / den;
    ((-1e-9..=1. + 1e-9).contains(&t) && (-1e-9..=1. + 1e-9).contains(&u)).then(|| [a[0] + r[0] * t, a[1] + r[1] * t])
}
/// Where two curves cross (sampled, as RoboCAD's `intersections`).
pub fn intersections(p: &Curve, q: &Curve) -> Vec<V2> {
    let (a, b) = (p.sample(128), q.sample(128));
    let mut out: Vec<V2> = Vec::new();
    for i in 0..a.len().saturating_sub(1) {
        for j in 0..b.len().saturating_sub(1) {
            if let Some(x) = seg_intersect(a[i], a[i + 1], b[j], b[j + 1])
                && out.iter().all(|y| p2(x, *y) > 1e-6)
            {
                out.push(x);
            }
        }
    }
    out
}
fn lengths(pts: &[V2]) -> (Vec<f64>, f64) {
    let l: Vec<f64> = pts.windows(2).map(|w| p2(w[0], w[1])).collect();
    let t = l.iter().sum::<f64>();
    (l, if t > 0. { t } else { 1. })
}
fn param_of(pts: &[V2], p: V2) -> f64 {
    let (lens, total) = lengths(pts);
    let (mut best, mut bt, mut acc) = (f64::INFINITY, 0., 0.);
    for i in 0..pts.len().saturating_sub(1) {
        let d = seg_dist(pts[i], pts[i + 1], p);
        if d < best {
            best = d;
            bt = (acc + seg_param(pts[i], pts[i + 1], p) * lens[i]) / total;
        }
        acc += lens[i];
    }
    bt
}
fn sub_curve(c: &Curve, t0: f64, t1: f64) -> Curve {
    match c.kind.as_str() {
        "arc" => Curve::arc(c.c(), c.radius, c.start_angle + (c.end_angle - c.start_angle) * t0, c.start_angle + (c.end_angle - c.start_angle) * t1),
        "circle" => Curve::arc(c.c(), c.radius, 360. * t0, 360. * t1),
        _ => {
            let pts = c.sample(128);
            let (lens, total) = lengths(&pts);
            let mut out: Vec<V2> = Vec::new();
            let mut acc = 0.;
            for i in 0..pts.len().saturating_sub(1) {
                let (s0, s1) = (acc / total, (acc + lens[i]) / total);
                if s1 >= t0 && s0 <= t1 {
                    let u0 = if s1 > s0 { ((t0 - s0) / (s1 - s0)).max(0.) } else { 0. };
                    let u1 = if s1 > s0 { ((t1 - s0) / (s1 - s0)).min(1.) } else { 1. };
                    let lerp = |u: f64| [pts[i][0] + (pts[i + 1][0] - pts[i][0]) * u, pts[i][1] + (pts[i + 1][1] - pts[i][1]) * u];
                    if out.is_empty() {
                        out.push(lerp(u0));
                    }
                    out.push(lerp(u1));
                }
                acc += lens[i];
            }
            if c.kind == "line" && out.len() >= 2 { Curve::line(out[0], *out.last().expect("points")) } else { Curve::polyline(out, false) }
        }
    }
}
fn offset_polyline(pts: &[V2], d: f64, closed: bool) -> Vec<V2> {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let p = pts[i];
            let prev = (i > 0 || closed).then(|| pts[(i + n - 1) % n]);
            let next = (i < n - 1 || closed).then(|| pts[(i + 1) % n]);
            let mut normals = Vec::new();
            for (a, b) in [(prev, Some(p)), (Some(p), next)] {
                if let (Some(a), Some(b)) = (a, b) {
                    let t = u2([b[0] - a[0], b[1] - a[1]]);
                    normals.push([-t[1], t[0]]);
                }
            }
            if normals.is_empty() {
                return p;
            }
            let k = normals.len() as f64;
            let nn = u2([normals.iter().map(|v| v[0]).sum::<f64>() / k, normals.iter().map(|v| v[1]).sum::<f64>() / k]);
            let scale = if normals.len() == 2 { 1. / (normals[0][0] * nn[0] + normals[0][1] * nn[1]).max(0.2) } else { 1. };
            [p[0] + nn[0] * d * scale, p[1] + nn[1] * d * scale]
        })
        .collect()
}
/// Nelder–Mead in the plane (`_minimize2`).
fn minimize2(f: impl Fn(V2) -> f64, x0: V2) -> V2 {
    let mut pts = [x0, [x0[0] + 5., x0[1]], [x0[0], x0[1] + 5.]];
    let mut vals = pts.map(&f);
    for _ in 0..200 {
        let mut order = [0, 1, 2];
        order.sort_by(|a, b| vals[*a].total_cmp(&vals[*b]));
        pts = order.map(|i| pts[i]);
        vals = order.map(|i| vals[i]);
        let cx = [(pts[0][0] + pts[1][0]) / 2., (pts[0][1] + pts[1][1]) / 2.];
        let refl = [2. * cx[0] - pts[2][0], 2. * cx[1] - pts[2][1]];
        let fr = f(refl);
        if fr < vals[0] {
            let exp = [cx[0] + 2. * (cx[0] - pts[2][0]), cx[1] + 2. * (cx[1] - pts[2][1])];
            let fe = f(exp);
            (pts[2], vals[2]) = if fe < fr { (exp, fe) } else { (refl, fr) };
        } else if fr < vals[1] {
            (pts[2], vals[2]) = (refl, fr);
        } else {
            let con = [cx[0] + 0.5 * (pts[2][0] - cx[0]), cx[1] + 0.5 * (pts[2][1] - cx[1])];
            let fc = f(con);
            if fc < vals[2] {
                (pts[2], vals[2]) = (con, fc);
            } else {
                for i in 1..3 {
                    pts[i] = [(pts[0][0] + pts[i][0]) / 2., (pts[0][1] + pts[i][1]) / 2.];
                    vals[i] = f(pts[i]);
                }
            }
        }
    }
    let best = (0..3).min_by(|a, b| vals[*a].total_cmp(&vals[*b])).expect("three");
    pts[best]
}

/// A sketch: its plane, curves and name (`Sketch`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sketch {
    pub plane: Plane,
    #[serde(default)]
    pub curves: Vec<Curve>,
    #[serde(default = "sketch_name")]
    pub name: String,
}
fn sketch_name() -> String {
    "Sketch".into()
}

/// A call's arguments bound by position or name, with readers whose
/// refusals name the call and parameter.
struct Call<'a> {
    method: &'a str,
    args: &'a [Value],
    kwargs: Option<&'a Map<String, Value>>,
}
impl<'a> Call<'a> {
    fn get(&self, i: usize, name: &str) -> Option<&'a Value> {
        self.args.get(i).or_else(|| self.kwargs.and_then(|k| k.get(name))).filter(|v| !v.is_null())
    }
    fn need(&self, i: usize, name: &str) -> Result<&'a Value, String> {
        self.get(i, name).ok_or_else(|| format!("{}: missing {name}", self.method))
    }
    fn num(&self, i: usize, name: &str) -> Result<f64, String> {
        self.need(i, name)?.as_f64().filter(|v| v.is_finite()).ok_or_else(|| format!("{}: {name} must be a finite number", self.method))
    }
    fn num_or(&self, i: usize, name: &str, default: f64) -> Result<f64, String> {
        if self.get(i, name).is_some() { self.num(i, name) } else { Ok(default) }
    }
    fn uv(&self, i: usize, name: &str) -> Result<V2, String> {
        uv(self.need(i, name)?).ok_or_else(|| format!("{}: {name} must be [u, v]", self.method))
    }
    fn uvs(&self, i: usize, name: &str) -> Result<Vec<V2>, String> {
        self.need(i, name)?.as_array().ok_or_else(|| format!("{}: {name} must be a list of [u, v]", self.method))?.iter().map(|p| uv(p).ok_or_else(|| format!("{}: {name} must be a list of [u, v]", self.method))).collect()
    }
    fn flag(&self, i: usize, name: &str) -> bool {
        self.get(i, name).and_then(Value::as_bool).unwrap_or(false)
    }
    fn index(&self, i: usize, name: &str, len: usize) -> Result<usize, String> {
        let k = self.need(i, name)?.as_u64().ok_or_else(|| format!("{}: {name} must be a curve index", self.method))? as usize;
        if k >= len {
            return Err(format!("{}: {name} {k} is not a curve (the sketch has {len})", self.method));
        }
        Ok(k)
    }
    fn indices(&self, i: usize, name: &str, len: usize) -> Result<Vec<usize>, String> {
        self.need(i, name)?
            .as_array()
            .ok_or_else(|| format!("{}: {name} must be a list of curve indices", self.method))?
            .iter()
            .map(|v| v.as_u64().map(|k| k as usize).filter(|k| *k < len).ok_or_else(|| format!("{}: {name} names a curve that does not exist", self.method)))
            .collect()
    }
}
fn uv(v: &Value) -> Option<V2> {
    let a = v.as_array().filter(|a| a.len() == 2)?;
    Some([a[0].as_f64()?, a[1].as_f64()?]).filter(|p| p.iter().all(|x| x.is_finite()))
}

impl Sketch {
    pub fn new(plane: Plane, name: &str) -> Sketch {
        Sketch { plane, curves: Vec::new(), name: name.into() }
    }
    pub fn from_json(v: &Value) -> Result<Sketch, String> {
        serde_json::from_value(v.clone()).map_err(|e| format!("sketch: {e}"))
    }
    pub fn json(&self) -> Value {
        serde_json::to_value(self).expect("a sketch serializes")
    }

    /// Apply one call `[method, [args…], {kwargs}?]`: the index of the curve it
    /// made or changed (None for a removal).
    pub fn call(&mut self, call: &Value) -> Result<Option<usize>, String> {
        let list = call.as_array().ok_or("a sketch call is [method, [args…], {kwargs}?]")?;
        let method = list.first().and_then(Value::as_str).ok_or("a sketch call starts with its method name")?;
        let empty = Vec::new();
        let args = list.get(1).map(|a| a.as_array().ok_or_else(|| format!("{method}: arguments must be a list"))).transpose()?.unwrap_or(&empty);
        let kwargs = list.get(2).map(|k| k.as_object().ok_or_else(|| format!("{method}: keyword arguments must be an object"))).transpose()?;
        let c = Call { method, args, kwargs };
        let n = self.curves.len();
        let add = |s: &mut Sketch, curve: Curve| {
            s.curves.push(curve);
            Ok(Some(s.curves.len() - 1))
        };
        match method {
            "line" => add(self, Curve::line(c.uv(0, "a")?, c.uv(1, "b")?)),
            "polyline" => {
                let pts = c.uvs(0, "points")?;
                if pts.len() < 2 {
                    return Err("polyline: needs at least two points".into());
                }
                add(self, Curve::polyline(pts, c.flag(1, "closed")))
            }
            "spline" => {
                let pts = c.uvs(0, "points")?;
                if pts.len() < 2 {
                    return Err("spline: needs at least two points".into());
                }
                add(self, Curve { points: pts, closed: c.flag(1, "closed"), ..Curve::of("spline") })
            }
            "control_curve" => {
                let pts = c.uvs(0, "points")?;
                if pts.len() < 2 {
                    return Err("control_curve: needs at least two points".into());
                }
                add(self, Curve { points: pts, degree: c.num_or(1, "degree", 3.)? as u32, closed: c.flag(2, "closed"), ..Curve::of("control") })
            }
            "circle" => {
                let r = c.num(1, "radius")?;
                if r <= 0. {
                    return Err("circle: radius must be positive".into());
                }
                add(self, Curve::circle(c.uv(0, "center")?, r))
            }
            "circle_two_point" => {
                let (a, b) = (c.uv(0, "a")?, c.uv(1, "b")?);
                add(self, Curve::circle([(a[0] + b[0]) / 2., (a[1] + b[1]) / 2.], p2(a, b) / 2.))
            }
            "circle_three_point" => {
                let (center, r) = circumcircle(c.uv(0, "a")?, c.uv(1, "b")?, c.uv(2, "c")?)?;
                add(self, Curve::circle(center, r))
            }
            "circle_tangent" => {
                let idx = c.indices(0, "curves", n)?;
                let radius = c.get(1, "radius").map(|_| c.num(1, "radius")).transpose()?;
                let near = c.get(2, "near").map(|_| c.uv(2, "near")).transpose()?.unwrap_or([0., 0.]);
                let samples: Vec<Vec<V2>> = idx.iter().map(|i| self.curves[*i].sample(64)).collect();
                let dist = |poly: &Vec<V2>, p: V2| poly.windows(2).map(|w| seg_dist(w[0], w[1], p)).fold(f64::INFINITY, f64::min);
                let (center, r) = match radius {
                    None => {
                        if samples.len() < 3 {
                            return Err("a tangent circle to two curves needs a radius; give three curves for a fully determined circle".into());
                        }
                        let res = |p: V2| {
                            let d: Vec<f64> = samples.iter().map(|s| dist(s, p)).collect();
                            (d[0] - d[1]).powi(2) + (d[1] - d[2]).powi(2)
                        };
                        let center = minimize2(res, near);
                        let r = samples.iter().take(3).map(|s| dist(s, center)).sum::<f64>() / 3.;
                        (center, r)
                    }
                    Some(r) => (minimize2(|p| samples.iter().map(|s| (dist(s, p) - r).powi(2)).sum(), near), r),
                };
                add(self, Curve::circle(center, r))
            }
            "ellipse" => add(self, Curve { center: Some(c.uv(0, "center")?), radius: c.num(1, "radius_x")?, radius2: c.num(2, "radius_y")?, rotation: c.num_or(3, "rotation", 0.)?, ..Curve::of("ellipse") }),
            "arc" => add(self, Curve::arc(c.uv(0, "center")?, c.num(1, "radius")?, c.num(2, "start_deg")?, c.num(3, "end_deg")?)),
            "arc_three_point" => {
                let (a, b, cc) = (c.uv(0, "a")?, c.uv(1, "b")?, c.uv(2, "c")?);
                let (center, r) = circumcircle(a, b, cc)?;
                let ang = |p: V2| (p[1] - center[1]).atan2(p[0] - center[0]).to_degrees();
                let (a0, a1, a2) = (ang(a), ang(b), ang(cc));
                let mut sweep = (a2 - a0).rem_euclid(360.);
                if (a1 - a0).rem_euclid(360.) > sweep {
                    sweep -= 360.;
                }
                add(self, Curve::arc(center, r, a0, a0 + sweep))
            }
            "arc_tangent" => {
                let prev = self.curves[c.index(0, "prev", n)?].clone();
                let end = c.uv(1, "end")?;
                let p0 = prev.end();
                let pts = prev.sample(8);
                let t = u2([pts[pts.len() - 1][0] - pts[pts.len() - 2][0], pts[pts.len() - 1][1] - pts[pts.len() - 2][1]]);
                let chord = [end[0] - p0[0], end[1] - p0[1]];
                let nn = [-t[1], t[0]];
                let d = chord[0] * nn[0] + chord[1] * nn[1];
                if d.abs() < 1e-9 {
                    return add(self, Curve::line(p0, end));
                }
                let r = (chord[0] * chord[0] + chord[1] * chord[1]) / (2. * d);
                let center = [p0[0] + nn[0] * r, p0[1] + nn[1] * r];
                let a0 = (p0[1] - center[1]).atan2(p0[0] - center[0]).to_degrees();
                let a1 = (end[1] - center[1]).atan2(end[0] - center[0]).to_degrees();
                let sweep = if r > 0. { (a1 - a0).rem_euclid(360.) } else { -(a0 - a1).rem_euclid(360.) };
                add(self, Curve::arc(center, r.abs(), a0, a0 + sweep))
            }
            "rectangle" | "rectangle_center" => {
                let size = c.uv(1, "size")?;
                let corner = if method == "rectangle" { c.uv(0, "corner")? } else { let m = c.uv(0, "center")?; [m[0] - size[0] / 2., m[1] - size[1] / 2.] };
                let (x, y, w, h) = (corner[0], corner[1], size[0], size[1]);
                add(self, Curve::polyline(vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]], true))
            }
            "rectangle_three_point" => {
                let (a, b, cc) = (c.uv(0, "a")?, c.uv(1, "b")?, c.uv(2, "c")?);
                let nn = u2([-(b[1] - a[1]), b[0] - a[0]]);
                let h = (cc[0] - b[0]) * nn[0] + (cc[1] - b[1]) * nn[1];
                let d = [nn[0] * h, nn[1] * h];
                add(self, Curve::polyline(vec![a, b, [b[0] + d[0], b[1] + d[1]], [a[0] + d[0], a[1] + d[1]]], true))
            }
            "polygon" => {
                let center = c.uv(0, "center")?;
                let radius = c.num(1, "radius")?;
                let sides = c.get(2, "sides").and_then(Value::as_u64).unwrap_or(6).max(3) as usize;
                let rot = c.num_or(3, "rotation", 0.)?.to_radians();
                let pts = (0..sides).map(|i| {
                    let a = rot + std::f64::consts::TAU * i as f64 / sides as f64;
                    [center[0] + radius * a.cos(), center[1] + radius * a.sin()]
                });
                add(self, Curve::polyline(pts.collect(), true))
            }
            "slot" => {
                let (a, b) = (c.uv(0, "a")?, c.uv(1, "b")?);
                let w = c.num(2, "width")?;
                if w <= 0. || p2(a, b) < 1e-9 {
                    return Err("slot: needs two distinct points and a positive width".into());
                }
                let d = u2([b[0] - a[0], b[1] - a[1]]);
                add(self, Curve { points: vec![a, b], radius: w / 2., closed: true, rotation: d[1].atan2(d[0]).to_degrees(), ..Curve::of("slot") })
            }
            "spiral" => {
                let center = c.uv(0, "center")?;
                let (r0, r1, turns) = (c.num(1, "start_radius")?, c.num(2, "end_radius")?, c.num(3, "turns")?);
                let count = ((turns * 36.) as usize).max(8);
                let pts = (0..=count).map(|i| {
                    let t = i as f64 / count as f64;
                    let a = std::f64::consts::TAU * turns * t;
                    let r = r0 + (r1 - r0) * t;
                    [center[0] + r * a.cos(), center[1] + r * a.sin()]
                });
                add(self, Curve { points: pts.collect(), ..Curve::of("spline") })
            }
            "text" => {
                let origin = c.uv(0, "origin")?;
                let text = c.need(1, "text")?.as_str().ok_or("text: text must be a string")?;
                let height = c.num_or(2, "height", 10.)?;
                let mut last = None;
                for poly in text_outlines(text, height)? {
                    self.curves.push(Curve::polyline(poly.iter().map(|p| [origin[0] + p[0], origin[1] + p[1]]).collect(), true));
                    last = Some(self.curves.len() - 1);
                }
                Ok(last)
            }
            "remove" => {
                self.curves.remove(c.index(0, "curve", n)?);
                Ok(None)
            }
            "reverse" => {
                let i = c.index(0, "curve", n)?;
                self.curves[i] = self.curves[i].reversed();
                Ok(Some(i))
            }
            "split_at" => {
                let i = c.index(0, "curve", n)?;
                let t = param_of(&self.curves[i].sample(128), c.uv(1, "point")?);
                let pieces = [sub_curve(&self.curves[i], 0., t), sub_curve(&self.curves[i], t, 1.)];
                self.curves.splice(i..=i, pieces);
                Ok(Some(i))
            }
            "trim" => {
                let i = c.index(0, "curve", n)?;
                let cutters = c.indices(1, "cutters", n)?;
                let click = c.uv(2, "click")?;
                let curve = self.curves[i].clone();
                let xs: Vec<V2> = cutters.iter().filter(|k| **k != i).flat_map(|k| intersections(&curve, &self.curves[*k])).collect();
                if xs.is_empty() {
                    self.curves.remove(i);
                    return Ok(None);
                }
                let pts = curve.sample(128);
                let mut tx: Vec<f64> = xs.iter().map(|x| param_of(&pts, *x)).collect();
                tx.sort_by(f64::total_cmp);
                let tc = param_of(&pts, click);
                let lo = tx.iter().copied().filter(|t| *t <= tc).fold(0., f64::max);
                let hi = tx.iter().copied().filter(|t| *t >= tc).fold(1., f64::min);
                let kept: Vec<Curve> = [(0., lo), (hi, 1.)].into_iter().filter(|(a, b)| b - a > 1e-6).map(|(a, b)| sub_curve(&curve, a, b)).collect();
                let none = kept.is_empty();
                self.curves.splice(i..=i, kept);
                Ok((!none).then_some(i))
            }
            "extend" => {
                let i = c.index(0, "curve", n)?;
                let targets: Vec<Curve> = c.indices(1, "targets", n)?.into_iter().filter(|k| *k != i).map(|k| self.curves[k].clone()).collect();
                let both = c.get(2, "both").and_then(Value::as_bool).unwrap_or(true);
                let curve = &mut self.curves[i];
                if !matches!(curve.kind.as_str(), "line" | "polyline" | "arc") {
                    return Err("only lines, polylines and arcs can be extended".into());
                }
                for at_start in if both { vec![true, false] } else { vec![false] } {
                    extend_end(curve, &targets, at_start);
                }
                Ok(Some(i))
            }
            "fillet_corner" => {
                let i = c.index(0, "curve", n)?;
                let vi = c.need(1, "vertex_index")?.as_i64().ok_or("fillet_corner: vertex_index must be an integer")?;
                let radius = c.num(2, "radius")?;
                fillet_corner(&mut self.curves[i], vi, radius)?;
                Ok(Some(i))
            }
            "offset" => {
                let src = self.curves[c.index(0, "curve", n)?].clone();
                let d = c.num(1, "distance")?;
                let mut pts = src.sample(if matches!(src.kind.as_str(), "line" | "polyline") { 1 } else { 64 });
                if src.kind == "polyline" && src.closed {
                    pts.pop();
                }
                let closed = src.closed || matches!(src.kind.as_str(), "circle" | "ellipse");
                add(self, Curve::polyline(offset_polyline(&pts, d, closed), closed))
            }
            "join" => {
                let idx = c.indices(0, "curves", n)?;
                if idx.is_empty() {
                    return Err("join: needs curves".into());
                }
                let mut remaining: Vec<Curve> = idx.iter().map(|i| self.curves[*i].clone()).collect();
                let flat = |c: &Curve| if matches!(c.kind.as_str(), "line" | "polyline") { c.points.clone() } else { c.sample(32) };
                let first = remaining.remove(0);
                let mut pts = flat(&first);
                while !remaining.is_empty() {
                    let end = *pts.last().expect("points");
                    let k = (0..remaining.len()).min_by(|a, b| {
                        let d = |c: &Curve| p2(c.start(), end).min(p2(c.end(), end));
                        d(&remaining[*a]).total_cmp(&d(&remaining[*b]))
                    }).expect("remaining");
                    let best = remaining.remove(k);
                    let mut seq = flat(&best);
                    if p2(best.end(), end) < p2(best.start(), end) {
                        seq.reverse();
                    }
                    pts.extend(seq.into_iter().skip(1));
                }
                let closed = p2(pts[0], *pts.last().expect("points")) < 1e-6;
                if closed {
                    pts.pop();
                }
                let mut sorted = idx.clone();
                sorted.sort_unstable();
                sorted.dedup();
                for i in sorted.into_iter().rev() {
                    self.curves.remove(i);
                }
                add(self, Curve::polyline(pts, closed))
            }
            "unjoin" => {
                let i = c.index(0, "curve", n)?;
                let curve = self.curves[i].clone();
                if curve.kind != "polyline" {
                    return Ok(Some(i));
                }
                let mut pts = curve.points.clone();
                if curve.closed {
                    pts.push(pts[0]);
                }
                let lines: Vec<Curve> = pts.windows(2).map(|w| Curve::line(w[0], w[1])).collect();
                self.curves.splice(i..=i, lines);
                Ok(Some(i))
            }
            "insert_vertex" => {
                let i = c.index(0, "curve", n)?;
                let after = c.need(1, "after")?.as_i64().ok_or("insert_vertex: after must be an integer")?;
                let p = c.uv(2, "point")?;
                let pts = &mut self.curves[i].points;
                let at = (after + 1).clamp(0, pts.len() as i64) as usize;
                pts.insert(at, p);
                Ok(Some(i))
            }
            "remove_vertex" => {
                let i = c.index(0, "curve", n)?;
                let k = c.need(1, "index")?.as_i64().ok_or("remove_vertex: index must be an integer")?;
                let pts = &mut self.curves[i].points;
                if pts.len() <= 2 {
                    return Err("a curve needs at least two points".into());
                }
                let k = k.rem_euclid(pts.len() as i64) as usize;
                pts.remove(k);
                Ok(Some(i))
            }
            "rebuild" => {
                let i = c.index(0, "curve", n)?;
                let degree = c.num(1, "degree")? as u32;
                let spans = c.num(2, "spans")? as usize;
                let pts = self.curves[i].sample((spans * 4).max(16));
                let step = (pts.len() / (spans + degree as usize).max(1)).max(1);
                let mut ctrl: Vec<V2> = pts.iter().step_by(step).copied().collect();
                if ctrl.last() != pts.last() {
                    ctrl.push(*pts.last().expect("points"));
                }
                let closed = self.curves[i].closed;
                self.curves[i] = Curve { points: ctrl, degree, closed, ..Curve::of("control") };
                Ok(Some(i))
            }
            other => Err(format!("no sketch method {other}")),
        }
    }

    /// Curve `c` as kernel segments in world space (`native/ops.cpp`'s
    /// encoding: one segment count, then per segment its kind and numbers).
    pub fn encode(&self, c: &Curve, args: &mut Vec<f64>, ints: &mut Vec<i32>) -> Result<(), String> {
        let w = |p: V2| self.plane.to_world(p);
        let mut segments: Vec<(i32, Vec<f64>, Vec<i32>)> = Vec::new();
        let n = self.plane.normal;
        let x = self.plane.x_axis;
        match c.kind.as_str() {
            "line" => segments.push((1, [w(c.points[0]), w(c.points[1])].concat(), vec![])),
            "polyline" => {
                let mut pts = c.points.clone();
                if c.closed {
                    pts.push(pts[0]);
                }
                for s in pts.windows(2).filter(|s| p2(s[0], s[1]) > 1e-9) {
                    segments.push((1, [w(s[0]), w(s[1])].concat(), vec![]));
                }
            }
            "circle" => segments.push((2, [w(c.c()), n, x, [c.radius, 0., 0.]].concat()[..10].to_vec(), vec![])),
            "arc" => segments.push((3, [&w(c.c())[..], &n, &x, &[c.radius, c.start_angle.to_radians(), c.end_angle.to_radians()]].concat(), vec![])),
            "ellipse" => {
                let rot = c.rotation.to_radians();
                let y = self.plane.y_axis();
                let mut xw = [0, 1, 2].map(|i| x[i] * rot.cos() + y[i] * rot.sin());
                let (big, small) = (c.radius.max(c.radius2), c.radius.min(c.radius2));
                if c.radius < c.radius2 {
                    xw = cross3(n, xw);
                }
                segments.push((4, [&w(c.c())[..], &n, &xw, &[big, small]].concat(), vec![]));
            }
            "spline" => segments.push((5, c.points.iter().flat_map(|p| w(*p)).collect(), vec![c.points.len() as i32, i32::from(c.closed)])),
            "control" => {
                let mut pts = c.points.clone();
                if c.closed {
                    pts.push(pts[0]);
                }
                segments.push((6, pts.iter().flat_map(|p| w(*p)).collect(), vec![pts.len() as i32, c.degree as i32]));
            }
            "slot" => {
                let (a, b) = (c.points[0], c.points[1]);
                let d = u2([b[0] - a[0], b[1] - a[1]]);
                let nn = [-d[1], d[0]];
                let r = c.radius;
                let at = |p: V2, k: f64, m: V2| [p[0] + m[0] * k, p[1] + m[1] * k];
                let (pa1, pb1, pb2, pa2) = (at(a, r, nn), at(b, r, nn), at(b, -r, nn), at(a, -r, nn));
                segments.push((1, [w(pa1), w(pb1)].concat(), vec![]));
                segments.push((7, [w(pb1), w(at(b, r, d)), w(pb2)].concat(), vec![]));
                segments.push((1, [w(pb2), w(pa2)].concat(), vec![]));
                segments.push((7, [w(pa2), w(at(a, -r, d)), w(pa1)].concat(), vec![]));
            }
            other => return Err(format!("unknown curve kind {other}")),
        }
        if segments.is_empty() {
            return Err(format!("the {} has no length", c.kind));
        }
        ints.push(segments.len() as i32);
        for (kind, nums, extra) in segments {
            ints.push(kind);
            ints.extend(extra);
            args.extend(nums);
        }
        Ok(())
    }

    /// The closed curves of `curves` (all when empty) as a profile face: the
    /// largest outer, the rest holes (RoboCAD's `to_face`, holes cut out).
    pub fn profile(&self, curves: &[usize], cancelled: &dyn Fn() -> bool) -> Result<Vec<u8>, String> {
        let mut chosen: Vec<&Curve> = if curves.is_empty() { self.curves.iter().collect() } else { curves.iter().filter_map(|i| self.curves.get(*i)).collect() };
        chosen.retain(|c| c.closed || matches!(c.kind.as_str(), "circle" | "ellipse" | "slot") || (c.points.len() > 2 && p2(c.start(), c.end()) < 1e-6));
        if chosen.is_empty() {
            return Err("the sketch has no closed curve to make a profile from".into());
        }
        // The outer curve: the one with the largest sampled area.
        let area = |c: &Curve| {
            let s = c.sample(64);
            0.5 * s.windows(2).map(|w| w[0][0] * w[1][1] - w[1][0] * w[0][1]).sum::<f64>().abs()
        };
        chosen.sort_by(|a, b| area(b).total_cmp(&area(a)));
        let (mut args, mut ints) = (Vec::new(), vec![chosen.len() as i32]);
        for c in &chosen {
            self.encode(c, &mut args, &mut ints)?;
        }
        Ok(crate::kernel::op1(crate::kernel::Op::Face, &[], &args, &ints, cancelled)?.brep)
    }

    /// The curves (all when empty) as wires (a compound for more than one).
    pub fn wires(&self, curves: &[usize], cancelled: &dyn Fn() -> bool) -> Result<Vec<u8>, String> {
        let chosen: Vec<&Curve> = if curves.is_empty() { self.curves.iter().collect() } else { curves.iter().filter_map(|i| self.curves.get(*i)).collect() };
        if chosen.is_empty() {
            return Err("the sketch has no curves".into());
        }
        let (mut args, mut ints) = (Vec::new(), Vec::new());
        for c in &chosen {
            self.encode(c, &mut args, &mut ints)?;
        }
        Ok(crate::kernel::op1(crate::kernel::Op::Wires, &[], &args, &ints, cancelled)?.brep)
    }
}

fn extend_end(curve: &mut Curve, targets: &[Curve], at_start: bool) {
    if curve.kind == "arc" {
        for delta in 1..360 {
            let d = delta as f64;
            let trial = Curve::arc(curve.c(), curve.radius, curve.start_angle - if at_start { d } else { 0. }, curve.end_angle + if at_start { 0. } else { d });
            if targets.iter().any(|t| !intersections(&trial, t).is_empty()) {
                if at_start { curve.start_angle -= d } else { curve.end_angle += d }
                return;
            }
        }
        return;
    }
    let pts = &mut curve.points;
    let (p, q) = if at_start { (pts[0], pts[1]) } else { (pts[pts.len() - 1], pts[pts.len() - 2]) };
    let d = u2([p[0] - q[0], p[1] - q[1]]);
    let ray = Curve::line(p, [p[0] + d[0] * 1e4, p[1] + d[1] * 1e4]);
    let hits: Vec<V2> = targets.iter().flat_map(|t| intersections(&ray, t)).collect();
    if let Some(best) = hits.into_iter().min_by(|a, b| p2(*a, p).total_cmp(&p2(*b, p))) {
        let k = if at_start { 0 } else { pts.len() - 1 };
        pts[k] = best;
    }
}

fn fillet_corner(curve: &mut Curve, vertex_index: i64, radius: f64) -> Result<(), String> {
    if curve.kind != "polyline" {
        return Err("corner fillet works on polylines; join the two lines first".into());
    }
    let mut pts = curve.points.clone();
    let n = pts.len();
    let i = vertex_index.rem_euclid(n as i64) as usize;
    if !curve.closed && (i == 0 || i == n - 1) {
        return Err("cannot fillet an end vertex".into());
    }
    let (p0, p1, p2v) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
    let d0 = u2([p0[0] - p1[0], p0[1] - p1[1]]);
    let d1 = u2([p2v[0] - p1[0], p2v[1] - p1[1]]);
    let ang = (d0[0] * d1[0] + d0[1] * d1[1]).clamp(-1., 1.).acos();
    if ang < 1e-6 || (ang - std::f64::consts::PI).abs() < 1e-6 {
        return Err("corner is straight".into());
    }
    let t = radius / (ang / 2.).tan();
    if t > p2(p0, p1) || t > p2(p1, p2v) {
        return Err(format!("fillet radius {radius} is too large for this corner"));
    }
    let a = [p1[0] + d0[0] * t, p1[1] + d0[1] * t];
    let b = [p1[0] + d1[0] * t, p1[1] + d1[1] * t];
    let bis = u2([d0[0] + d1[0], d0[1] + d1[1]]);
    let dc = radius / (ang / 2.).sin();
    let center = [p1[0] + bis[0] * dc, p1[1] + bis[1] * dc];
    let a0 = (a[1] - center[1]).atan2(a[0] - center[0]);
    let a1 = (b[1] - center[1]).atan2(b[0] - center[0]);
    let sweep = (a1 - a0 + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
    let arc: Vec<V2> = (0..=8).map(|k| [center[0] + radius * (a0 + sweep * k as f64 / 8.).cos(), center[1] + radius * (a0 + sweep * k as f64 / 8.).sin()]).collect();
    pts.splice(i..=i, arc);
    curve.points = pts;
    Ok(())
}

/// Glyph outlines of `text` at `height` mm as closed polylines, from a
/// system TrueType font (RoboCAD's `text_outlines`: the same font search).
pub fn text_outlines(text: &str, height: f64) -> Result<Vec<Vec<V2>>, String> {
    let candidates = ["/System/Library/Fonts/Supplemental/Arial.ttf", "/System/Library/Fonts/Helvetica.ttc", "/Library/Fonts/Arial.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "C:/Windows/Fonts/arial.ttf"];
    let data = candidates.iter().find_map(|p| std::fs::read(p).ok()).ok_or("no TrueType font found for text")?;
    let face = ttf_parser::Face::parse(&data, 0).map_err(|e| format!("font: {e}"))?;
    let scale = height / f64::from(face.units_per_em());
    struct Pen {
        out: Vec<Vec<V2>>,
        cur: Vec<V2>,
        last: V2,
        x: f64,
        scale: f64,
    }
    impl Pen {
        fn p(&self, x: f32, y: f32) -> V2 {
            [self.x + f64::from(x) * self.scale, f64::from(y) * self.scale]
        }
        fn curve(&mut self, ctrl: &[V2]) {
            let mut pts = vec![self.last];
            pts.extend_from_slice(ctrl);
            for k in 1..=8 {
                let t = k as f64 / 8.;
                let mut tmp = pts.clone();
                while tmp.len() > 1 {
                    tmp = tmp.windows(2).map(|w| [(1. - t) * w[0][0] + t * w[1][0], (1. - t) * w[0][1] + t * w[1][1]]).collect();
                }
                self.cur.push(tmp[0]);
            }
            self.last = *ctrl.last().expect("control points");
        }
    }
    impl ttf_parser::OutlineBuilder for Pen {
        fn move_to(&mut self, x: f32, y: f32) {
            if self.cur.len() > 2 {
                self.out.push(std::mem::take(&mut self.cur));
            }
            self.last = self.p(x, y);
            self.cur = vec![self.last];
        }
        fn line_to(&mut self, x: f32, y: f32) {
            self.last = self.p(x, y);
            self.cur.push(self.last);
        }
        fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
            let c = [self.p(x1, y1), self.p(x, y)];
            self.curve(&c);
        }
        fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
            let c = [self.p(x1, y1), self.p(x2, y2), self.p(x, y)];
            self.curve(&c);
        }
        fn close(&mut self) {
            if self.cur.len() > 2 {
                self.out.push(std::mem::take(&mut self.cur));
            }
        }
    }
    let mut pen = Pen { out: Vec::new(), cur: Vec::new(), last: [0., 0.], x: 0., scale };
    for ch in text.chars() {
        match face.glyph_index(ch) {
            None => pen.x += 0.5 * f64::from(face.units_per_em()) * scale,
            Some(g) => {
                face.outline_glyph(g, &mut pen);
                if pen.cur.len() > 2 {
                    pen.out.push(std::mem::take(&mut pen.cur));
                }
                pen.x += f64::from(face.glyph_hor_advance(g).unwrap_or(0)) * scale;
            }
        }
    }
    Ok(pen.out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_build_and_edit_curves_as_robocads_do() {
        let mut s = Sketch::new(Plane::xy(0.), "S");
        s.call(&json!(["rectangle", [[0, 0], [20, 10]]])).unwrap();
        s.call(&json!(["circle", [[10, 5], 2]])).unwrap();
        assert_eq!(s.curves.len(), 2);
        s.call(&json!(["fillet_corner", [0, 0, 2]])).unwrap();
        assert!(s.curves[0].points.len() > 4);
        s.call(&json!(["line", [[-5, 5], [25, 5]]])).unwrap();
        s.call(&json!(["trim", [2, [0], [10, 5]]])).unwrap();
        assert_eq!(s.curves.len(), 4, "the line is cut where it crosses the rectangle: two pieces remain");
        assert!(s.call(&json!(["circle", [[0, 0], -1]])).unwrap_err().contains("positive"));
        assert!(s.call(&json!(["nonsense", []])).is_err());
        let (c, r) = circumcircle([1., 0.], [0., 1.], [-1., 0.]).unwrap();
        assert!(c[0].abs() < 1e-12 && c[1].abs() < 1e-12 && (r - 1.).abs() < 1e-12);
    }

    #[test]
    fn a_profile_with_a_hole_becomes_a_face_and_extrudes() {
        let mut s = Sketch::new(Plane::xy(5.), "S");
        s.call(&json!(["rectangle", [[0, 0], [20, 10]]])).unwrap();
        s.call(&json!(["circle", [[10, 5], 2]])).unwrap();
        s.call(&json!(["slot", [[2, 2], [6, 2], 2]])).unwrap();
        let face = s.profile(&[], &|| false).unwrap();
        let solid = crate::kernel::op1(crate::kernel::Op::Extrude, &[&face], &[0., 0., 1., 3., 0., 0.], &[], &|| false).unwrap();
        assert_eq!(solid.kind, crate::kernel::Kind::Solid);
        let t = crate::kernel::topology(&solid.brep, &|| false).unwrap();
        assert!(t.faces.iter().filter(|f| f.kind == "cylinder").count() >= 3, "the hole and the slot's two ends");
    }
}

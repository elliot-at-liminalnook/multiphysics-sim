//! Split a CAD body into printable pieces and join them again (RoboCAD's
//! `print_split.py`).
//!
//! [`plan_cuts`] finds cut planes so every piece fits the printer (from the
//! print registry), preferring cuts through a single, solid cross-section
//! away from holes, and optionally away from the load. [`build_split`] cuts
//! the body and adds joints at each seam, built with booleans on the pieces:
//!
//! * dowel pins: steel pins across the seam; a press fit in the minus piece,
//!   a slip fit in the plus piece (clearances from the registry);
//! * heat-set inserts and screws: an insert pocket in the minus piece, a
//!   clearance hole with a counterbore from the plus piece's far side;
//! * a dovetail rail: a tail on the minus piece that slides into a groove in
//!   the plus piece (angle and clearance from the registry).
//!
//! Joint capacities are not guessed here: each seam is written into a print
//! study (`seams`) and checked by `sim-print analyze` with the same formulas
//! as the library parts, against the loads the part really carries.
//!
//! Nothing here edits the document: [`apply_split`] adds the pieces to a
//! staged edit (one undo step) and hides the source.
use super::{K, Registry, V3, add, cross, dot, norm, round, scale, solid, sub, unit};
use crate::ops::Ctx;
use serde_json::{Map, Value, json};

pub const AXES: [V3; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
const TOL: f64 = 1e-6;

/// A requested cut (e.g. to print a piece in its strong orientation).
#[derive(Clone, Debug)]
pub struct ExtraPlane {
    pub point: V3,
    pub normal: V3,
    pub why: String,
}

#[derive(Clone, Debug)]
pub struct SplitOptions {
    pub printer: String,
    /// auto (pins + screws where a screw can reach, else a dovetail) | pins+screws | dovetail | pins.
    pub joint: String,
    pub screw: String,
    pub pin_diameter: f64,
    /// Material left around a joint feature (mm).
    pub wall: f64,
    /// Area of seam per screw (mm²): one screw per 40 × 40 mm by default.
    pub area_per_screw: f64,
    pub max_screws: usize,
    pub max_pieces: usize,
    /// Candidate cut positions tried per cut.
    pub samples: usize,
    pub extra_planes: Vec<ExtraPlane>,
    /// Pieces keep the part's up direction (turning only about z) when fitting the printer.
    pub keep_up: bool,
}

impl Default for SplitOptions {
    fn default() -> Self {
        SplitOptions { printer: "bambu-h2c".into(), joint: "auto".into(), screw: "M3".into(), pin_diameter: 4.0, wall: 2.0, area_per_screw: 1600.0, max_screws: 6, max_pieces: 24, samples: 13, extra_planes: Vec::new(), keep_up: true }
    }
}

fn v3(v: &Value) -> Option<V3> {
    let a = v.as_array().filter(|a| a.len() == 3)?;
    Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?]).filter(|p| p.iter().all(|x| x.is_finite()))
}

/// An extra plane from JSON: `{point, normal, why?}`.
pub fn extra_plane(v: &Value) -> Result<ExtraPlane, String> {
    let point = v3(&v["point"]).ok_or("a plane needs point [x, y, z] (mm)")?;
    let normal = v3(&v["normal"]).filter(|n| norm(*n) > 1e-9).ok_or("a plane needs a nonzero normal [x, y, z]")?;
    Ok(ExtraPlane { point, normal: unit(normal), why: v["why"].as_str().unwrap_or("requested").to_string() })
}

impl SplitOptions {
    /// The options a REST body gives (RoboCAD's `SplitOptions(**body)` over
    /// printer, joint, screw, pin_diameter, wall, area_per_screw,
    /// max_screws, extra_planes).
    pub fn from_body(body: &Value) -> Result<SplitOptions, String> {
        let mut o = SplitOptions::default();
        if let Some(p) = body["printer"].as_str() {
            o.printer = p.into();
        }
        if let Some(j) = body["joint"].as_str() {
            if !matches!(j, "auto" | "pins+screws" | "dovetail" | "pins") {
                return Err(format!("joint must be auto, pins+screws, dovetail or pins, not {j:?}"));
            }
            o.joint = j.into();
        }
        if let Some(s) = body["screw"].as_str() {
            o.screw = s.into();
        }
        let num = |k: &str| -> Result<Option<f64>, String> {
            match body.get(k).filter(|v| !v.is_null()) {
                None => Ok(None),
                Some(v) => v.as_f64().filter(|x| x.is_finite() && *x > 0.0).map(Some).ok_or_else(|| format!("{k} must be a positive number")),
            }
        };
        if let Some(v) = num("pin_diameter")? {
            o.pin_diameter = v;
        }
        if let Some(v) = num("wall")? {
            o.wall = v;
        }
        if let Some(v) = num("area_per_screw")? {
            o.area_per_screw = v;
        }
        if let Some(v) = num("max_screws")? {
            o.max_screws = v as usize;
        }
        if let Some(planes) = body.get("extra_planes").filter(|v| !v.is_null()) {
            o.extra_planes = planes.as_array().ok_or("extra_planes must be a list of {point, normal, why}")?.iter().map(extra_plane).collect::<Result<_, _>>()?;
        }
        Ok(o)
    }
}

// ---- fitting -------------------------------------------------------------

/// Whether a box fits the build volume: with its z kept up (turning only
/// about z), or in any axis-aligned orientation.
pub fn fits(size: V3, usable: V3, keep_up: bool) -> bool {
    if keep_up {
        let ([x, y, z], [a, b, c]) = (size, usable);
        return z <= c + TOL && ((x <= a + TOL && y <= b + TOL) || (x <= b + TOL && y <= a + TOL));
    }
    let (mut s, mut u) = (size, usable);
    s.sort_by(f64::total_cmp);
    u.sort_by(f64::total_cmp);
    s.iter().zip(u).all(|(a, b)| *a <= b + TOL)
}

/// The build limit each axis of a piece must meet (longest side to the longest room).
pub fn limits(size: V3, usable: V3, keep_up: bool) -> V3 {
    if keep_up {
        let (big, small) = (usable[0].max(usable[1]), usable[0].min(usable[1]));
        return if size[0] >= size[1] { [big, small, usable[2]] } else { [small, big, usable[2]] };
    }
    let mut order = [0usize, 1, 2];
    order.sort_by(|a, b| size[*a].total_cmp(&size[*b]));
    let mut rooms = usable;
    rooms.sort_by(f64::total_cmp);
    let mut out = [0.0; 3];
    for (axis, room) in order.into_iter().zip(rooms) {
        out[axis] = room;
    }
    out
}

// ---- sections ------------------------------------------------------------

/// In-plane axes for a normal (RoboCAD's `plane_basis`).
pub fn plane_basis(normal: V3) -> (V3, V3) {
    let n = unit(normal);
    let x = if n[2].abs() < 0.9 { unit(cross(n, [0.0, 0.0, 1.0])) } else { unit(cross(n, [1.0, 0.0, 0.0])) };
    (x, unit(cross(n, x)))
}

/// A plane with an in-plane x axis (RoboCAD's `Plane`); y = normal × x.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub origin: V3,
    pub normal: V3,
    pub x: V3,
}

impl Frame {
    pub fn new(origin: V3, normal: V3, x: V3) -> Frame {
        Frame { origin, normal, x }
    }
    pub fn y(&self) -> V3 {
        unit(cross(self.normal, self.x))
    }
    pub fn to_world(&self, u: f64, v: f64) -> V3 {
        add(add(self.origin, scale(self.x, u)), scale(self.y(), v))
    }
    pub fn to_local(&self, p: V3) -> [f64; 2] {
        let d = sub(p, self.origin);
        [dot(d, self.x), dot(d, self.y())]
    }
}

pub type P2 = [f64; 2];

fn inside_loop(l: &[P2], p: P2) -> bool {
    let mut inside = false;
    for i in 0..l.len() {
        let ([x0, y0], [x1, y1]) = (l[i], l[(i + 1) % l.len()]);
        if (y0 > p[1]) != (y1 > p[1]) {
            let xc = x0 + (p[1] - y0) * (x1 - x0) / (y1 - y0);
            if p[0] < xc {
                inside = !inside;
            }
        }
    }
    inside
}

/// A body's cross-section on a plane.
#[derive(Clone, Debug)]
pub struct Section {
    pub plane: Frame,
    /// 2D loops (u, v) in plane coordinates.
    pub loops: Vec<Vec<P2>>,
    pub area: f64,
    /// Outer loops (separate regions).
    pub islands: usize,
    pub holes: usize,
}

impl Section {
    /// Even-odd point-in-polygon over all loops.
    pub fn inside(&self, p: P2) -> bool {
        self.loops.iter().filter(|l| inside_loop(l, p)).count() % 2 == 1
    }
    /// Distance from a point to the nearest section edge.
    pub fn clearance(&self, p: P2) -> f64 {
        let mut best = f64::INFINITY;
        for l in &self.loops {
            for i in 0..l.len() {
                let (a, b) = (l[i], l[(i + 1) % l.len()]);
                let ab = [b[0] - a[0], b[1] - a[1]];
                let l2 = (ab[0] * ab[0] + ab[1] * ab[1]).max(1e-12);
                let t = (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / l2).clamp(0.0, 1.0);
                let (dx, dy) = (p[0] - (a[0] + t * ab[0]), p[1] - (a[1] + t * ab[1]));
                best = best.min((dx * dx + dy * dy).sqrt());
            }
        }
        best
    }
}

/// Join section edges (polylines, any order and direction) into closed loops.
pub fn chain(edges: Vec<Vec<V3>>, tol: f64) -> Vec<Vec<V3>> {
    let close = |a: V3, b: V3| (a[0] - b[0]).abs() < tol && (a[1] - b[1]).abs() < tol && (a[2] - b[2]).abs() < tol;
    let mut todo: Vec<Vec<V3>> = edges.into_iter().filter(|e| e.len() >= 2).collect();
    let mut loops = Vec::new();
    while let Some(mut l) = todo.pop() {
        let mut grew = true;
        while grew && !close(l[0], l[l.len() - 1]) {
            grew = false;
            for i in 0..todo.len() {
                let e = &todo[i];
                let (first, last) = (l[0], l[l.len() - 1]);
                if close(last, e[0]) {
                    l.extend_from_slice(&e[1..]);
                } else if close(last, e[e.len() - 1]) {
                    l.extend(e.iter().rev().skip(1));
                } else if close(first, e[e.len() - 1]) {
                    let mut head = e[..e.len() - 1].to_vec();
                    head.extend(l);
                    l = head;
                } else if close(first, e[0]) {
                    let mut head: Vec<V3> = e.iter().rev().copied().collect();
                    head.pop();
                    head.extend(l);
                    l = head;
                } else {
                    continue;
                }
                todo.remove(i);
                grew = true;
                break;
            }
        }
        if close(l[0], l[l.len() - 1]) && l.len() > 3 {
            l.pop();
            loops.push(l);
        }
    }
    loops
}

/// The section of `body` by the plane through `point` with `normal`; None when it misses.
pub fn section(k: &K, body: &[u8], point: V3, normal: V3) -> Result<Option<Section>, String> {
    let (x, _) = plane_basis(normal);
    let plane = Frame::new(point, unit(normal), x);
    let raw = chain(k.section(body, point, plane.normal)?, 1e-3);
    let loops: Vec<Vec<P2>> = raw.iter().map(|l| l.iter().map(|p| plane.to_local(*p)).collect::<Vec<P2>>()).filter(|l| l.len() >= 3).collect();
    if loops.is_empty() {
        return Ok(None);
    }
    let signed: Vec<f64> = loops.iter().map(|l| 0.5 * (0..l.len()).map(|i| { let (a, b) = (l[i], l[(i + 1) % l.len()]); a[0] * b[1] - b[0] * a[1] }).sum::<f64>()).collect();
    // A loop inside another loop is a hole (even nesting depth: outer; odd: hole).
    let hole: Vec<bool> = (0..loops.len()).map(|i| (0..loops.len()).filter(|j| *j != i && inside_loop(&loops[*j], loops[i][0])).count() % 2 == 1).collect();
    let area = signed.iter().zip(&hole).map(|(a, h)| if *h { -a.abs() } else { a.abs() }).sum();
    let outer = hole.iter().filter(|h| !**h).count();
    let holes = loops.len() - outer;
    Ok(Some(Section { plane, loops, area, islands: outer, holes }))
}

// ---- cuts ----------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Cut {
    pub point: V3,
    pub normal: V3,
    pub score: f64,
    pub area: f64,
    pub islands: usize,
    pub holes: usize,
    pub why: String,
}

/// A cut's penalty and its note (a load path through the plane).
pub type LoadPenalty<'a> = &'a dyn Fn(V3, V3) -> (f64, String);

fn linspace(a: f64, b: f64, n: usize) -> Vec<f64> {
    if n <= 1 {
        return vec![a];
    }
    (0..n).map(|i| a + (b - a) * i as f64 / (n - 1) as f64).collect()
}

fn cancelled(k: &K) -> Result<(), String> {
    if (k.cancelled)() { Err("cancelled".into()) } else { Ok(()) }
}

/// Best plane normal to `axis` between `window.0` and `window.1`.
#[allow(clippy::too_many_arguments)]
pub fn choose_cut(k: &K, body: &[u8], axis: usize, lo: f64, hi: f64, window: (f64, f64), samples: usize, load_penalty: Option<LoadPenalty>, nominal: Option<f64>) -> Result<Cut, String> {
    let n = AXES[axis];
    let (blo, bhi) = k.bounds(body)?;
    let centre = [0, 1, 2].map(|i| (blo[i] + bhi[i]) / 2.0);
    let (mut a, mut b) = (window.0.max(lo + 1.0), window.1.min(hi - 1.0));
    if b < a {
        a = (window.0 + window.1) / 2.0;
        b = a;
    }
    let mut best: Option<Cut> = None;
    for s in linspace(a, b, samples.max(1)) {
        cancelled(k)?;
        let mut p = centre;
        p[axis] = s;
        let Some(sec) = section(k, body, p, n)? else { continue };
        if sec.area <= 0.0 {
            continue;
        }
        // Prefer one solid region with no holes and a generous area to join across;
        // every extra island or hole is a seam that is harder to align and weaker.
        let mut score = sec.area.ln() - 1.5 * (sec.islands as f64 - 1.0) - 0.75 * sec.holes as f64;
        if let Some(nominal) = nominal {
            // Between equal sections, pieces of equal size.
            score -= 0.3 * (s - nominal).abs() / (hi - lo).max(1e-9);
        }
        let mut why = format!("{:.0} mm², {} region(s), {} hole(s)", sec.area, sec.islands, sec.holes);
        if let Some(pen) = load_penalty {
            let (pen, note) = pen(p, n);
            score -= pen;
            why += &format!(", {note}");
        }
        if best.as_ref().is_none_or(|c| score > c.score) {
            best = Some(Cut { point: p, normal: n, score, area: sec.area, islands: sec.islands, holes: sec.holes, why });
        }
    }
    best.ok_or_else(|| format!("no plane normal to {} between {a:.1} and {b:.1} mm cuts the body", ["x", "y", "z"][axis]))
}

/// Cut planes (axis-aligned, part frame) that make every piece fit.
pub fn plan_cuts(k: &K, body: &[u8], usable: V3, options: &SplitOptions, load_penalty: Option<LoadPenalty>) -> Result<Vec<Cut>, String> {
    let mut cuts: Vec<Cut> = Vec::new();
    let mut pieces = vec![body.to_vec()];
    for _ in 0..12 {
        let sizes: Vec<V3> = pieces.iter().map(|p| k.size(p)).collect::<Result<_, _>>()?;
        let longest = |s: &V3| s.iter().copied().fold(f64::MIN, f64::max);
        let Some(i) = (0..pieces.len()).filter(|i| !fits(sizes[*i], usable, options.keep_up)).max_by(|a, b| longest(&sizes[*a]).total_cmp(&longest(&sizes[*b]))) else {
            return Ok(cuts);
        };
        let piece = &pieces[i];
        let (lo, hi) = k.bounds(piece)?;
        let size = sub(hi, lo);
        let lim = limits(size, usable, options.keep_up);
        // Cut across the axis that overflows its limit the most.
        let axis = (0..3).max_by(|a, b| (size[*a] / lim[*a].max(1e-9)).total_cmp(&(size[*b] / lim[*b].max(1e-9)))).unwrap_or(0);
        let target = lim[axis];
        let count = ((size[axis] / target).ceil() as usize).max(2);
        let step = size[axis] / count as f64;
        for c in 1..count {
            let nominal = lo[axis] + c as f64 * step;
            // Each piece may be up to `target` long: keep the window where both sides still fit.
            let slack = ((target - step) * 0.9).max(0.0);
            cuts.push(choose_cut(k, piece, axis, lo[axis], hi[axis], (nominal - slack, nominal + slack), options.samples, load_penalty, Some(nominal))?);
        }
        pieces = cut_pieces(k, body, &cuts)?;
        if pieces.len() > options.max_pieces {
            return Err(format!("more than {} pieces needed: use a larger printer", options.max_pieces));
        }
    }
    Err("could not find cuts that make every piece fit".into())
}

fn corners(lo: V3, hi: V3) -> Vec<V3> {
    let mut out = Vec::with_capacity(8);
    for x in [lo[0], hi[0]] {
        for y in [lo[1], hi[1]] {
            for z in [lo[2], hi[2]] {
                out.push([x, y, z]);
            }
        }
    }
    out
}

/// The body cut by every plane, slivers dropped.
pub fn cut_pieces(k: &K, body: &[u8], cuts: &[Cut]) -> Result<Vec<Vec<u8>>, String> {
    let mut pieces = vec![body.to_vec()];
    for c in cuts {
        let mut next = Vec::new();
        for p in pieces {
            cancelled(k)?;
            let (lo, hi) = k.bounds(&p)?;
            let d: Vec<f64> = corners(lo, hi).into_iter().map(|q| dot(sub(q, c.point), c.normal)).collect();
            if d.iter().copied().fold(f64::INFINITY, f64::min) < -TOL && d.iter().copied().fold(f64::NEG_INFINITY, f64::max) > TOL {
                for b in k.cut_with_plane(&p, c.point, c.normal)? {
                    if k.volume_centroid(&b)?.0 > 1e-3 {
                        next.push(b);
                    }
                }
            } else {
                next.push(p);
            }
        }
        pieces = next;
    }
    Ok(pieces)
}

// ---- joints --------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct JointPlan {
    /// dowel | insert_screw | dovetail.
    pub kind: String,
    /// On the seam plane.
    pub at: V3,
    /// What sim-print checks (`Joint` in `sim_print::joints`).
    pub spec: Value,
    /// `[{item, size, count}]`.
    pub hardware: Vec<Value>,
    pub notes: Vec<String>,
    /// How CAD cuts it (screw access, …).
    pub geometry: Value,
}

#[derive(Clone, Debug)]
pub struct Seam {
    pub name: String,
    pub point: V3,
    /// From the minus piece into the plus piece.
    pub normal: V3,
    pub minus: usize,
    pub plus: usize,
    pub area: f64,
    pub joints: Vec<JointPlan>,
    pub notes: Vec<String>,
}

impl Seam {
    /// The seam in a print study's form (`seams`).
    pub fn study(&self) -> Value {
        json!({"name": self.name, "point": self.point, "normal": self.normal, "joints": self.joints.iter().map(|j| j.spec.clone()).collect::<Vec<_>>()})
    }
}

/// Material along `direction` from a point on the body's face (mm).
fn thickness(k: &K, body: &[u8], point: V3, direction: V3) -> Result<f64, String> {
    let start = add(point, scale(direction, 0.02));
    Ok(k.ray_hits(body, start, direction)?.first().map_or(0.0, |h| h.0 + 0.02))
}

/// mm: deeper than this a hex key does not reach comfortably.
pub const MAX_TUNNEL: f64 = 30.0;
pub const MAX_POCKET_REACH: f64 = 30.0;

/// The shortest way out sideways (in the seam plane's directions) from the
/// screw head's seat: (direction, distance to the outside).
fn side_access(k: &K, body: &[u8], p: V3, n: V3, clamp: f64) -> Result<Option<(V3, f64)>, String> {
    let seat = add(p, scale(n, clamp + 1.5));
    let (x, y) = plane_basis(n);
    let mut best: Option<(V3, f64)> = None;
    for a in 0..16 {
        let t = 2.0 * std::f64::consts::PI * a as f64 / 16.0;
        let d = unit(add(scale(x, t.cos()), scale(y, t.sin())));
        if let Some(h) = k.ray_hits(body, seat, d)?.first()
            && h.0 <= MAX_POCKET_REACH
            && best.is_none_or(|b| h.0 < b.1)
        {
            best = Some((d, h.0));
        }
    }
    Ok(best)
}

fn dist2(a: P2, b: P2) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// Farthest-point picks: spread out, away from `avoid`.
pub fn pick_spread(candidates: &[P2], count: usize, avoid: &[P2], spacing: f64) -> Vec<usize> {
    if candidates.is_empty() {
        return Vec::new();
    }
    let mut chosen = Vec::new();
    let mut d: Vec<f64> = candidates.iter().map(|c| avoid.iter().map(|a| dist2(*c, *a)).fold(f64::INFINITY, f64::min)).collect();
    if avoid.is_empty() {
        // Start at the point farthest from the centroid.
        let n = candidates.len() as f64;
        let c = [candidates.iter().map(|p| p[0]).sum::<f64>() / n, candidates.iter().map(|p| p[1]).sum::<f64>() / n];
        let first = argmax(candidates.iter().map(|p| dist2(*p, c)));
        chosen.push(first);
        for (i, p) in candidates.iter().enumerate() {
            d[i] = d[i].min(dist2(*p, candidates[first]));
        }
    }
    while chosen.len() < count {
        let i = argmax(d.iter().copied());
        if !d[i].is_finite() || d[i] < spacing {
            break;
        }
        chosen.push(i);
        for (j, p) in candidates.iter().enumerate() {
            d[j] = d[j].min(dist2(*p, candidates[i]));
        }
    }
    chosen
}

/// numpy's argmax: the first largest.
fn argmax(v: impl Iterator<Item = f64>) -> usize {
    let mut best = (0, f64::NEG_INFINITY);
    for (i, x) in v.enumerate() {
        if x > best.1 || (i == 0 && x.is_nan()) {
            best = (i, x);
        }
    }
    best.0
}

fn arange(lo: f64, hi: f64, step: f64) -> Vec<f64> {
    let n = ((hi - lo) / step).ceil().max(0.0) as usize;
    (0..n).map(|i| lo + i as f64 * step).collect()
}

fn ptp(v: impl Iterator<Item = f64> + Clone) -> f64 {
    v.clone().fold(f64::NEG_INFINITY, f64::max) - v.fold(f64::INFINITY, f64::min)
}

/// A screw size's number (`M2.5` → 2.5).
fn screw_number(size: &str) -> f64 {
    size.trim_start_matches(['M', 'm']).parse().unwrap_or(0.0)
}

/// Choose joint positions on a seam (fills `seam.joints`).
pub fn plan_joints(k: &K, reg: &Registry, seam: &mut Seam, sec: &Section, minus: &[u8], plus: &[u8], options: &SplitOptions) -> Result<(), String> {
    let n = seam.normal;
    let plane = sec.plane;
    // Candidate points on a grid over the section, with their clearance to the edge.
    let all = sec.loops.iter().flatten();
    let (umin, umax) = (all.clone().map(|p| p[0]).fold(f64::INFINITY, f64::min), all.clone().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max));
    let (vmin, vmax) = (all.clone().map(|p| p[1]).fold(f64::INFINITY, f64::min), all.map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max));
    let step = ((umax - umin).min(vmax - vmin) / 60.0).max(0.75);
    let mut pts: Vec<P2> = Vec::new();
    for v in arange(vmin, vmax, step) {
        for u in arange(umin, umax, step) {
            if sec.inside([u, v]) {
                pts.push([u, v]);
            }
        }
    }
    if pts.is_empty() {
        seam.notes.push("the section is too small for any joint".into());
        return Ok(());
    }
    cancelled(k)?;
    let clear: Vec<f64> = pts.iter().map(|p| sec.clearance(*p)).collect();
    let world = |uv: P2| plane.to_world(uv[0], uv[1]);
    let per_d = reg.number("dowel_pin.engagement_per_diameter")?;
    let pin_lengths = reg.numbers("dowel_pin.lengths_mm")?;
    let mut pin_diameters = reg.numbers("dowel_pin.diameters_mm")?;
    let mut want = options.joint.clone();
    let mut screw_size = options.screw.clone();

    let dowel_at = |uv: P2, d_pin: f64| -> Result<Option<JointPlan>, String> {
        let p = world(uv);
        let depth = d_pin * per_d;
        let (tm, tp) = (thickness(k, minus, p, scale(n, -1.0))?, thickness(k, plus, p, n)?);
        let (dm, dp) = (depth.min(tm - 1.2), depth.min(tp - 1.2));
        if dm.min(dp) < d_pin {
            return Ok(None);
        }
        let Some(l) = pin_lengths.iter().copied().filter(|l| *l <= dm + dp - 0.5).reduce(f64::max) else { return Ok(None) };
        Ok(Some(JointPlan {
            kind: "dowel".into(),
            at: p,
            spec: json!({"kind": "dowel", "at": p, "diameter_mm": d_pin, "depth_minus_mm": round(dm, 2), "depth_plus_mm": round(dp, 2)}),
            hardware: vec![json!({"item": "steel dowel pin", "size": format!("Ø{d_pin} × {l} mm"), "count": 1})],
            notes: vec![format!("holes {dm:.1} mm into the minus piece (press fit) and {dp:.1} mm into the plus piece (slip fit); pin {l} mm")],
            geometry: json!({}),
        }))
    };

    let screw_at = |uv: P2, size: &str| -> Result<Option<JointPlan>, String> {
        let (ins, scr) = (reg.insert(size)?, reg.screw(size)?);
        let p = world(uv);
        let (tm, tp) = (thickness(k, minus, p, scale(n, -1.0))?, thickness(k, plus, p, n)?);
        if tm < ins.depth_mm + 1.5 {
            return Ok(None);
        }
        let (cb_d, _) = scr.counterbore_mm;
        // Clamp at most 8 mm of the plus piece. Deeper: a counterbore tunnel from the
        // far face (up to MAX_TUNNEL), else a pocket from the nearest side face.
        let clamp = (tp - 0.5).min(8.0);
        if clamp < 2.5 {
            return Ok(None);
        }
        let grip = ins.length_mm * 0.9;
        let Some(l) = scr.lengths_mm.iter().copied().filter(|l| clamp + 1.0 <= *l && *l <= clamp + grip).reduce(f64::max) else { return Ok(None) };
        let tunnel = tp - clamp;
        let mut geometry = json!({"access": "tunnel", "tunnel_mm": round(tunnel.max(0.0), 2)});
        let mut how = if tunnel > 0.5 { format!(" (counterbore tunnel {tunnel:.1} mm deep, Ø{cb_d})") } else { String::new() };
        if tunnel > MAX_TUNNEL {
            let Some((direction, reach)) = side_access(k, plus, p, n, clamp)? else { return Ok(None) };
            let height = l + scr.head_mm * 0.6 + 1.5;
            if clamp + height > tp - options.wall {
                return Ok(None);
            }
            geometry = json!({"access": "pocket", "direction": direction, "reach_mm": round(reach, 2), "height_mm": round(height, 2), "width_mm": round(cb_d + 1.0, 2)});
            how = format!(" (screw pocket {:.1} mm wide, {height:.1} mm tall, opening {reach:.1} mm away through the side)", cb_d + 1.0);
        }
        Ok(Some(JointPlan {
            kind: "insert_screw".into(),
            at: p,
            spec: json!({"kind": "insert_screw", "at": p, "size": size, "screw_length_mm": l, "clamp_mm": round(clamp, 2)}),
            hardware: vec![json!({"item": "heat-set insert", "size": size, "count": 1}), json!({"item": "socket head screw", "size": format!("{size} × {l} mm"), "count": 1})],
            notes: vec![format!("insert {} × {} mm pocket in the minus piece; {size} × {l} screw through {clamp:.1} mm of the plus piece{how}", ins.hole_mm, ins.depth_mm)],
            geometry,
        }))
    };

    let mut placed: Vec<JointPlan> = Vec::new();
    // Two pins, far apart, where there is room for a pin and its wall; smaller pins if none fit.
    pin_diameters.retain(|d| *d <= options.pin_diameter);
    pin_diameters.sort_by(|a, b| b.total_cmp(a));
    for d_pin in pin_diameters {
        let need = d_pin / 2.0 + options.wall;
        let ok: Vec<P2> = pts.iter().zip(&clear).filter(|(_, c)| **c >= need).map(|(p, _)| *p).collect();
        for i in pick_spread(&ok, 8, &[], 0.0) {
            if placed.len() == 2 {
                break;
            }
            if let Some(j) = dowel_at(ok[i], d_pin)?
                && placed.iter().all(|q| norm(sub(j.at, q.at)) > 3.0 * d_pin)
            {
                placed.push(j);
            }
        }
        if !placed.is_empty() {
            break;
        }
    }
    if want == "auto" || want == "pins+screws" {
        let count = ((sec.area / options.area_per_screw).round() as usize).clamp(2, options.max_screws.max(2));
        // A small section takes smaller screws: try the chosen size, then the next ones down.
        let mut sizes = vec![options.screw.clone()];
        sizes.extend(["M2.5", "M2"].iter().filter(|z| **z != options.screw && screw_number(z) < screw_number(&options.screw)).map(|z| z.to_string()));
        let need_of = |size: &str| -> Result<f64, String> { Ok((reg.insert(size)?.knurl_mm / 2.0).max(reg.screw(size)?.counterbore_mm.0 / 2.0) + options.wall) };
        for size in &sizes {
            screw_size = size.clone();
            let need = need_of(size)?;
            let ok: Vec<P2> = pts.iter().zip(&clear).filter(|(_, c)| **c >= need).map(|(p, _)| *p).collect();
            // Room for two screws a spacing apart?
            if ok.len() >= 2 && ptp(ok.iter().map(|p| p[0])).max(ptp(ok.iter().map(|p| p[1]))) >= 2.2 * need {
                break;
            }
        }
        let need = need_of(&screw_size)?;
        let ok: Vec<P2> = pts.iter().zip(&clear).filter(|(_, c)| **c >= need).map(|(p, _)| *p).collect();
        let avoid: Vec<P2> = placed.iter().map(|j| plane.to_local(j.at)).collect();
        let mut screws: Vec<JointPlan> = Vec::new();
        for i in pick_spread(&ok, count * 3, &avoid, need * 2.0) {
            if screws.len() == count {
                break;
            }
            cancelled(k)?;
            if let Some(j) = screw_at(ok[i], &screw_size)?
                && placed.iter().chain(&screws).all(|q| norm(sub(j.at, q.at)) > 2.2 * need)
            {
                screws.push(j);
            }
        }
        let none = screws.is_empty();
        placed.extend(screws);
        if none {
            seam.notes.push("no screw fits here (too thin for an insert or its clamp)".into());
            if want == "auto" {
                want = "dovetail".into();
            }
        }
    }
    if want == "dovetail" {
        let avoid: Vec<V3> = placed.iter().map(|j| j.at).collect();
        let dv = dovetail_joints(k, reg, seam, sec, minus, plus, options, &pts, &avoid, step)?;
        if dv.is_empty() {
            seam.notes.push("no room for a dovetail either".into());
        } else {
            placed.extend(dv);
        }
    }
    if !placed.iter().any(|j| j.kind == "insert_screw" || j.kind == "dovetail") {
        seam.notes.push("nothing holds this seam closed: glue it, or add a clamp".into());
    }
    seam.joints = placed;
    Ok(())
}

/// The principal (largest-variance) direction of 2D points about their mean.
fn principal(grid: &[P2], c: P2) -> P2 {
    let n = grid.len() as f64 - 1.0;
    let (mut a, mut b, mut d) = (0.0, 0.0, 0.0);
    for p in grid {
        let (x, y) = (p[0] - c[0], p[1] - c[1]);
        a += x * x;
        b += x * y;
        d += y * y;
    }
    let (a, b, d) = (a / n, b / n, d / n);
    let l1 = (a + d) / 2.0 + (((a - d) / 2.0).powi(2) + b * b).sqrt();
    let v = if b.abs() > 1e-12 { [l1 - d, b] } else if a >= d { [1.0, 0.0] } else { [0.0, 1.0] };
    let l = (v[0] * v[0] + v[1] * v[1]).sqrt();
    [v[0] / l, v[1] / l]
}

/// Dovetails across a seam. A thin section (a plate) gets jigsaw tabs
/// through its thickness, spaced along the seam; a thick one gets one
/// sliding rail along its long direction. `grid` is interior sample points (u, v).
#[allow(clippy::too_many_arguments)]
pub fn dovetail_joints(k: &K, reg: &Registry, seam: &Seam, sec: &Section, minus: &[u8], plus: &[u8], options: &SplitOptions, grid: &[P2], avoid: &[V3], step: f64) -> Result<Vec<JointPlan>, String> {
    if grid.len() < 3 {
        return Ok(Vec::new());
    }
    let m = grid.len() as f64;
    let c = [grid.iter().map(|p| p[0]).sum::<f64>() / m, grid.iter().map(|p| p[1]).sum::<f64>() / m];
    let plane = sec.plane;
    let to3 = |d2: P2| unit(sub(plane.to_world(d2[0], d2[1]), plane.origin));
    // CAD parts are mostly square to their axes: snap the seam's directions to a
    // part axis in the seam plane when within 15° (a hub or rim skews the fit).
    let mut t3 = to3(principal(grid, c));
    for ax in AXES.iter().copied().chain(AXES.iter().map(|a| scale(*a, -1.0))) {
        if dot(ax, seam.normal).abs() < 1e-6 && dot(ax, t3) > 15f64.to_radians().cos() {
            t3 = ax;
        }
    }
    let s3 = unit(cross(seam.normal, t3));
    let to2 = |d3: V3| plane.to_local(add(plane.origin, d3));
    let (t2, s2) = (to2(t3), to2(s3));
    let (along_t, along_s) = (t3, s3);
    let ut: Vec<f64> = grid.iter().map(|p| (p[0] - c[0]) * t2[0] + (p[1] - c[1]) * t2[1]).collect();
    let us: Vec<f64> = grid.iter().map(|p| (p[0] - c[0]) * s2[0] + (p[1] - c[1]) * s2[1]).collect();
    let thick = ptp(us.iter().copied());
    let angle = reg.number("dovetail.angle_deg")?.to_radians();
    let r2 = |v: V3| v.map(|x| round(x, 2));
    let mut plans = Vec::new();
    // Stations along the seam: at each, the longest continuous run of material
    // across it. Where that run is plate-like (2.5–25 mm), a tab goes through it.
    let bins: Vec<i64> = ut.iter().map(|u| (u / step.max(1e-6)).round() as i64).collect();
    let mut unique: Vec<i64> = bins.clone();
    unique.sort_unstable();
    unique.dedup();
    let mut stations: Vec<(f64, f64, f64)> = Vec::new();
    for b in unique {
        let mut vals: Vec<f64> = us.iter().zip(&bins).filter(|(_, x)| **x == b).map(|(u, _)| *u).collect();
        vals.sort_by(f64::total_cmp);
        let mut runs: Vec<&[f64]> = Vec::new();
        let mut start = 0;
        for i in 1..vals.len() {
            if vals[i] - vals[i - 1] > 1.5 * step {
                runs.push(&vals[start..i]);
                start = i;
            }
        }
        runs.push(&vals[start..]);
        // The first longest run.
        let run = runs.iter().fold(runs[0], |best, r| if r[r.len() - 1] - r[0] > best[best.len() - 1] - best[0] { r } else { best });
        let local = run[run.len() - 1] - run[0] + step;
        if (2.5..=25.0).contains(&local) {
            stations.push((b as f64 * step, (run[0] + run[run.len() - 1]) / 2.0, local));
        }
    }
    if !stations.is_empty() {
        let span = stations[stations.len() - 1].0 - stations[0].0;
        let count = ((span / 50.0).round() as usize).clamp(1, 6);
        let spacing = span / count as f64;
        let neck = (0.35 * spacing.min(40.0)).clamp(6.0, 14.0);
        let depth = 0.9 * neck;
        for t in 0..count {
            let target = stations[0].0 + spacing * (t as f64 + 0.5);
            let &(u, mid, local) = stations.iter().min_by(|a, b| (a.0 - target).abs().total_cmp(&(b.0 - target).abs())).expect("stations");
            if avoid.iter().any(|q| {
                let l = plane.to_local(*q);
                (u - ((l[0] - c[0]) * t2[0] + (l[1] - c[1]) * t2[1])).abs() < neck
            }) {
                continue;
            }
            let centre2 = [c[0] + t2[0] * u + s2[0] * mid, c[1] + t2[1] * u + s2[1] * mid];
            let centre = plane.to_world(centre2[0], centre2[1]);
            // The tab reaches `depth` into the plus piece; both sides need material there.
            if thickness(k, plus, centre, seam.normal)? < depth + options.wall || thickness(k, minus, centre, scale(seam.normal, -1.0))? < options.wall {
                continue;
            }
            plans.push(JointPlan {
                kind: "dovetail".into(),
                at: centre,
                spec: json!({"kind": "dovetail", "at": centre, "along": along_s, "rail_length_mm": round(local, 2), "neck_mm": round(neck, 2), "depth_mm": round(depth, 2)}),
                hardware: Vec::new(),
                notes: vec![format!("jigsaw tab {neck:.1} mm neck, {depth:.1} mm deep, through {local:.1} mm of plate; drops in along {:?}", r2(along_s))],
                geometry: json!({"extrude_mm": local + 20.0}),
            });
        }
        return Ok(plans);
    }
    let length = ptp(ut.iter().copied());
    let neck = (0.3 * thick).clamp(5.0, 20.0);
    let depth = (0.6 * neck).clamp(3.0, 10.0);
    // The interior point nearest the centroid (the centroid may be in a hole).
    let centre2 = grid[(0..grid.len()).min_by(|a, b| dist2(grid[*a], c).total_cmp(&dist2(grid[*b], c))).unwrap_or(0)];
    let centre = plane.to_world(centre2[0], centre2[1]);
    for f in linspace(-0.4, 0.4, 5) {
        let p = add(centre, scale(along_t, f * length));
        if thickness(k, plus, p, seam.normal)? < depth + options.wall || thickness(k, minus, p, scale(seam.normal, -1.0))? < options.wall {
            return Ok(Vec::new());
        }
    }
    Ok(vec![JointPlan {
        kind: "dovetail".into(),
        at: centre,
        spec: json!({"kind": "dovetail", "at": centre, "along": along_t, "rail_length_mm": round(length, 2), "neck_mm": round(neck, 2), "depth_mm": round(depth, 2)}),
        hardware: Vec::new(),
        notes: vec![format!("rail {length:.0} mm long, neck {neck:.1} mm, {depth:.1} mm deep at {:.0}°; slides in along {:?}", angle.to_degrees(), r2(along_t))],
        geometry: json!({"extrude_mm": length + 20.0}),
    }])
}

// ---- geometry ------------------------------------------------------------

/// The dovetail tail: neck on the seam plane, widening into the plus side.
#[allow(clippy::too_many_arguments)]
fn trapezoid_prism(k: &K, centre: V3, along: V3, normal: V3, neck: f64, depth: f64, angle: f64, length: f64, grow: f64) -> Result<Vec<u8>, String> {
    let across = unit(cross(normal, along));
    let half0 = neck / 2.0 + grow;
    let half1 = neck / 2.0 + depth * angle.tan() + grow;
    let start = add(centre, scale(along, -length / 2.0));
    // The sketch plane: normal `along`, x `across`; y = along × across (±normal).
    let y = unit(cross(along, across));
    let sign = if dot(y, normal) > 0.0 { 1.0 } else { -1.0 };
    // A little into the minus side so the groove's floor clears.
    let lo = -grow;
    let at = |u: f64, v: f64| add(add(start, scale(across, u)), scale(y, v));
    let pts = vec![at(-half0, sign * lo), at(half0, sign * lo), at(half1, sign * (depth + grow)), at(-half1, sign * (depth + grow))];
    k.extrude(pts, scale(along, length))
}

/// A box with edges along three orthonormal axes from one corner.
fn oriented_box(k: &K, corner: V3, axes: (V3, V3, V3), size: V3) -> Result<Vec<u8>, String> {
    let (a, b, c) = axes;
    let pts = vec![corner, add(corner, scale(a, size[0])), add(add(corner, scale(a, size[0])), scale(b, size[1])), add(corner, scale(b, size[1]))];
    k.extrude(pts, scale(c, size[2]))
}

fn interior_samples(sec: &Section, count: usize) -> Vec<P2> {
    let all = sec.loops.iter().flatten();
    let lo = [all.clone().map(|p| p[0]).fold(f64::INFINITY, f64::min), all.clone().map(|p| p[1]).fold(f64::INFINITY, f64::min)];
    let hi = [all.clone().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max), all.map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max)];
    let n = (count as f64).sqrt() as usize + 1;
    let mut out = Vec::new();
    for v in linspace(lo[1], hi[1], n) {
        for u in linspace(lo[0], hi[0], n) {
            if sec.inside([u, v]) {
                out.push([u, v]);
            }
        }
    }
    out
}

/// Where two pieces meet on a cut plane: sections of both, intersected on a grid.
fn shared_section(k: &K, a: &[u8], b: &[u8], c: &Cut) -> Result<Option<Section>, String> {
    let (Some(mut sa), Some(sb)) = (section(k, a, add(c.point, scale(c.normal, -0.05)), c.normal)?, section(k, b, add(c.point, scale(c.normal, 0.05)), c.normal)?) else {
        return Ok(None);
    };
    // Do the two faces overlap? Sample points inside a's section and test them
    // in b's (planes along one normal share their in-plane axes): a real
    // shared face, not pieces touching along a line.
    let samples = interior_samples(&sa, 900);
    let hits = samples.iter().filter(|p| sb.inside(**p)).count();
    if samples.is_empty() || (hits as f64) < (4f64).max(0.05 * samples.len() as f64) {
        return Ok(None);
    }
    // Onto the cut plane itself (the in-plane coordinates do not change).
    sa.plane = Frame::new(c.point, c.normal, sa.plane.x);
    Ok(Some(sa))
}

/// Cut the body and add every seam's joints to its pieces.
pub fn build_split(k: &K, reg: &Registry, body: &[u8], cuts: &[Cut], options: &SplitOptions) -> Result<(Vec<Vec<u8>>, Vec<Seam>), String> {
    let mut pieces = cut_pieces(k, body, cuts)?;
    let centroids: Vec<V3> = pieces.iter().map(|p| k.volume_centroid(p).map(|v| v.1)).collect::<Result<_, _>>()?;
    let mut seams: Vec<Seam> = Vec::new();
    for c in cuts {
        // Pairs of pieces that meet on this plane.
        let sides: Vec<f64> = centroids.iter().map(|cp| dot(sub(*cp, c.point), c.normal)).collect();
        for i in 0..pieces.len() {
            if sides[i] >= 0.0 {
                continue;
            }
            for j in 0..pieces.len() {
                if sides[j] <= 0.0 {
                    continue;
                }
                cancelled(k)?;
                let Some(shared) = shared_section(k, &pieces[i], &pieces[j], c)? else { continue };
                let mut seam = Seam { name: format!("seam {}", seams.len() + 1), point: c.point, normal: c.normal, minus: i, plus: j, area: shared.area, joints: Vec::new(), notes: Vec::new() };
                plan_joints(k, reg, &mut seam, &shared, &pieces[i], &pieces[j], options)?;
                seams.push(seam);
            }
        }
    }
    for s in &mut seams {
        let (m, p) = cut_joints(k, reg, &pieces[s.minus], &pieces[s.plus], s)?;
        pieces[s.minus] = m;
        pieces[s.plus] = p;
    }
    Ok((pieces, seams))
}

/// Cut a seam's joints into its two pieces; a joint the kernel cannot cut
/// is left out with a note.
pub fn cut_joints(k: &K, reg: &Registry, minus: &[u8], plus: &[u8], seam: &mut Seam) -> Result<(Vec<u8>, Vec<u8>), String> {
    let n = seam.normal;
    let slip = reg.number("dowel_pin.slip_clearance_mm")?;
    let press = reg.number("dowel_pin.press_clearance_mm")?;
    let (mut minus, mut plus) = (minus.to_vec(), plus.to_vec());
    let mut kept = Vec::new();
    for j in std::mem::take(&mut seam.joints) {
        cancelled(k)?;
        match cut_joint(k, reg, &minus, &plus, n, &j, slip, press) {
            Ok((m, p)) => {
                minus = m;
                plus = p;
                kept.push(j);
            }
            Err(e) => seam.notes.push(format!("{} at {:?} left out: {e}", j.kind, j.at.map(|x| round(x, 1)))),
        }
    }
    seam.joints = kept;
    Ok((minus, plus))
}

#[allow(clippy::too_many_arguments)]
fn cut_joint(k: &K, reg: &Registry, minus: &[u8], plus: &[u8], n: V3, j: &JointPlan, slip: f64, press: f64) -> Result<(Vec<u8>, Vec<u8>), String> {
    let s = &j.spec;
    let p = j.at;
    let f = |key: &str| s[key].as_f64().ok_or_else(|| format!("{} joint: {key} is missing", j.kind));
    let neg = scale(n, -1.0);
    match j.kind.as_str() {
        "dowel" => {
            let d = f("diameter_mm")?;
            let minus = k.subtract(minus, &k.cylinder(add(p, scale(n, 0.1)), neg, (d + press) / 2.0, f("depth_minus_mm")? + 0.1)?)?;
            let plus = k.subtract(plus, &k.cylinder(add(p, scale(n, -0.1)), n, (d + slip) / 2.0, f("depth_plus_mm")? + 0.1)?)?;
            Ok((minus, plus))
        }
        "insert_screw" => {
            let size = s["size"].as_str().unwrap_or("M3");
            let (ins, scr) = (reg.insert(size)?, reg.screw(size)?);
            let mut minus = k.subtract(minus, &k.cylinder(add(p, scale(n, 0.1)), neg, ins.hole_mm / 2.0, ins.depth_mm + 0.6)?)?;
            // Pilot below the insert for the screw tip.
            minus = k.subtract(&minus, &k.cylinder(p, neg, scr.clearance_mm / 2.0 - 0.3, ins.depth_mm + 3.0)?)?;
            let tp = thickness(k, plus, p, n)?;
            let clamp = f("clamp_mm")?;
            let g = &j.geometry;
            let pocket = g["access"] == "pocket";
            let reach = if pocket { clamp + 0.6 } else { tp + 0.2 };
            let mut plus = k.subtract(plus, &k.cylinder(add(p, scale(n, -0.1)), n, scr.clearance_mm / 2.0, reach)?)?;
            let (cb_d, _) = scr.counterbore_mm;
            if pocket {
                // A window from the side: head seat at `clamp`, tall enough to put the screw in.
                let d = v3(&g["direction"]).ok_or("pocket joint: direction is missing")?;
                let w = g["width_mm"].as_f64().unwrap_or(cb_d + 1.0);
                let side = unit(cross(n, d));
                let seat = add(p, scale(n, clamp));
                let reach = g["reach_mm"].as_f64().unwrap_or(0.0) + 1.0;
                let corner = add(add(seat, scale(side, -w / 2.0)), scale(d, -w / 2.0));
                let bx = oriented_box(k, corner, (side, d, n), [w, reach + w / 2.0, g["height_mm"].as_f64().unwrap_or(0.0)])?;
                // The clearance hole only needs to reach the pocket.
                plus = k.subtract(&plus, &bx)?;
            } else {
                plus = k.subtract(&plus, &k.cylinder(add(p, scale(n, clamp)), n, cb_d / 2.0, tp - clamp + 0.2)?)?;
            }
            Ok((minus, plus))
        }
        "dovetail" => {
            let angle = reg.number("dovetail.angle_deg")?.to_radians();
            let clearance = reg.number("dovetail.sliding_clearance_mm")?;
            let along = v3(&s["along"]).ok_or("dovetail joint: along is missing")?;
            let length = j.geometry["extrude_mm"].as_f64().unwrap_or(f("rail_length_mm")? + 20.0);
            let (neck, depth) = (f("neck_mm")?, f("depth_mm")?);
            let tail = trapezoid_prism(k, p, along, n, neck, depth, angle, length, 0.0)?;
            let groove = trapezoid_prism(k, p, along, n, neck, depth, angle, length, clearance)?;
            // The tail is the part of the prism inside the plus piece's volume; it moves to the minus piece.
            let tail_in = k.intersect(&tail, plus)?;
            let minus = k.union(minus, &tail_in)?;
            let plus = k.subtract(plus, &groove)?;
            Ok((minus, plus))
        }
        other => Err(format!("unknown joint kind {other}")),
    }
}

// ---- top level -----------------------------------------------------------

pub struct SplitResult {
    pub source_id: String,
    pub printer: String,
    pub cuts: Vec<Cut>,
    pub pieces: Vec<Vec<u8>>,
    pub seams: Vec<Seam>,
    pub usable_mm: V3,
    pub registry_sha256: String,
}

impl SplitResult {
    /// The hardware every seam needs, totalled by item and size.
    pub fn hardware(&self) -> Vec<Value> {
        let mut total: std::collections::BTreeMap<(String, String), i64> = Default::default();
        for h in self.seams.iter().flat_map(|s| &s.joints).flat_map(|j| &j.hardware) {
            let key = (h["item"].as_str().unwrap_or("").to_string(), h["size"].as_str().unwrap_or("").to_string());
            *total.entry(key).or_default() += h["count"].as_i64().unwrap_or(0);
        }
        total.into_iter().map(|((item, size), count)| json!({"item": item, "size": size, "count": count})).collect()
    }

    /// The split's record (`robot.print_split` on its group).
    pub fn summary(&self, k: &K) -> Result<Value, String> {
        let mut pieces = Vec::new();
        for (i, p) in self.pieces.iter().enumerate() {
            let size = k.size(p)?;
            let upright = fits(size, self.usable_mm, true);
            pieces.push(json!({"index": i, "size_mm": size.map(|x| round(x, 2)), "fits": upright || fits(size, self.usable_mm, false), "fits_upright": upright, "volume_mm3": round(k.volume_centroid(p)?.0, 1)}));
        }
        let seams: Vec<Value> = self.seams.iter().map(|s| json!({
            "name": s.name, "minus": s.minus, "plus": s.plus, "point": s.point.map(|x| round(x, 3)), "normal": s.normal,
            "area_mm2": round(s.area, 1), "notes": s.notes,
            "joints": s.joints.iter().map(|j| json!({"kind": j.kind, "at": j.at.map(|x| round(x, 2)), "spec": j.spec, "hardware": j.hardware, "notes": j.notes})).collect::<Vec<_>>(),
        })).collect();
        Ok(json!({
            "source": self.source_id, "printer": self.printer, "usable_mm": self.usable_mm, "registry_sha256": self.registry_sha256,
            "cuts": self.cuts.iter().map(|c| json!({"point": c.point, "normal": c.normal, "why": c.why})).collect::<Vec<_>>(),
            "pieces": pieces, "seams": seams, "hardware": self.hardware(),
        }))
    }

    /// The seams in a print study's form, to check on the unsplit part.
    pub fn study_seams(&self) -> Vec<Value> {
        self.seams.iter().map(Seam::study).collect()
    }
}

/// Plan and build a split of `body` (the node's resolved B-rep) without
/// touching the document.
pub fn split_for_printing(k: &K, reg: &Registry, source_id: &str, body: &[u8], options: &SplitOptions, load_penalty: Option<LoadPenalty>) -> Result<SplitResult, String> {
    let usable = reg.usable_mm(&options.printer)?;
    // Tabs and rails stick out past a cut into the next piece: plan against a box
    // smaller by that allowance, check the built pieces, and allow more if needed.
    let mut allowance = 16.0;
    for attempt in 0..4 {
        let room = [usable[0] - allowance, usable[1] - allowance, usable[2]];
        let mut cuts = if fits(k.size(body)?, usable, options.keep_up) { Vec::new() } else { plan_cuts(k, body, room, options, load_penalty)? };
        for extra in &options.extra_planes {
            cuts.push(Cut { point: extra.point, normal: unit(extra.normal), score: 0.0, area: 0.0, islands: 0, holes: 0, why: extra.why.clone() });
        }
        let (pieces, seams) = if cuts.is_empty() { (vec![body.to_vec()], Vec::new()) } else { build_split(k, reg, body, &cuts, options)? };
        let mut all_fit = true;
        for p in &pieces {
            all_fit &= fits(k.size(p)?, usable, options.keep_up);
        }
        if all_fit {
            return Ok(SplitResult { source_id: source_id.into(), printer: options.printer.clone(), cuts, pieces, seams, usable_mm: usable, registry_sha256: reg.sha256().into() });
        }
        if attempt == 3 {
            break;
        }
        allowance += 12.0;
    }
    Err(format!("the pieces with their joints still overflow the {} after 4 tries", options.printer))
}

/// Add the pieces under a new group to a staged edit; the source body is
/// kept, hidden. The group's id.
pub fn apply_split(cx: &mut Ctx, k: &K, result: &SplitResult, name: Option<&str>) -> Result<String, String> {
    let src = cx.node(&result.source_id)?.clone();
    let src_name = src["name"].as_str().unwrap_or(&result.source_id).to_string();
    let material = src["material"].as_str().map(str::to_string);
    let parent = src["parent"].as_str().map(str::to_string);
    let summary = result.summary(k)?;
    let mut extra = Map::new();
    extra.insert("robot".into(), json!({"print_split": summary}));
    let group = cx.add_node("group", name.unwrap_or(&format!("{src_name}: print pieces")), parent.as_deref(), extra)?;
    for (i, body) in result.pieces.iter().enumerate() {
        let seams: Vec<&str> = result.seams.iter().filter(|s| s.minus == i || s.plus == i).map(|s| s.name.as_str()).collect();
        let id = cx.add_built(solid(body.clone()), &format!("{src_name} · piece {}", i + 1), material.as_deref(), Some(&group))?;
        cx.edit.node_mut(&id)?["robot"] = json!({"print_piece": {
            "source": result.source_id, "index": i, "seams": seams, "printer": result.printer,
            "registry_sha256": result.registry_sha256, "fits": summary["pieces"][i]["fits"],
        }});
    }
    cx.edit.node_mut(&result.source_id)?["visible"] = json!(false);
    Ok(group)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting_keeps_up_or_turns_freely() {
        assert!(fits([300.0, 100.0, 50.0], [325.0, 320.0, 325.0], true));
        assert!(!fits([100.0, 100.0, 400.0], [325.0, 320.0, 325.0], true));
        assert!(fits([100.0, 100.0, 400.0], [325.0, 420.0, 325.0], false));
        assert_eq!(limits([400.0, 100.0, 50.0], [320.0, 325.0, 300.0], true), [325.0, 320.0, 300.0]);
    }

    #[test]
    fn edges_chain_into_closed_loops_in_any_order() {
        let sq = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0]];
        let edges = vec![vec![sq[2], sq[1]], vec![sq[0], sq[1]], vec![sq[3], sq[0]], vec![sq[2], sq[3]]];
        let loops = chain(edges, 1e-3);
        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0].len(), 4);
    }

    #[test]
    fn a_section_with_a_hole_counts_it() {
        let frame = Frame::new([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        let outer = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let inner = vec![[4.0, 4.0], [6.0, 4.0], [6.0, 6.0], [4.0, 6.0]];
        let sec = Section { plane: frame, loops: vec![outer, inner], area: 96.0, islands: 1, holes: 1 };
        assert!(sec.inside([1.0, 1.0]));
        assert!(!sec.inside([5.0, 5.0]));
        assert!((sec.clearance([1.0, 5.0]) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn spread_picks_start_far_from_the_centre() {
        let pts = [[0.0, 0.0], [1.0, 0.0], [10.0, 0.0], [5.0, 0.0]];
        let picks = pick_spread(&pts, 2, &[], 0.0);
        assert_eq!(picks[0], 2);
        assert_eq!(picks[1], 0);
    }
}

//! The sketch edits (RoboCAD's ui/app.py:752-785) and the `cad_sketch`
//! action, each one `POST /nodes/{id}/sketch` (or `POST /nodes` for a new
//! sketch) through `ops::send_sketch`.
//!
//! - **Which sketch** ([`selected_sketch`], RoboCAD's `_selected_sketch`):
//!   the first selected node that is a sketch, else the first visible
//!   sketch in the shown tree's order; none: "Select a sketch". Its
//!   geometry must be read at the shown revision (the calls name curves
//!   by index).
//! - **Offset** ("Offset curves"): `offset(c, d)` for every curve
//!   (`for c in list(sk.curves)`): indices 0..n, each call appending its
//!   result, so the indices stay valid.
//! - **Fillet corners** ("Fillet corners"): every corner of every closed
//!   polyline from the last vertex down, RoboCAD swallowing each corner's
//!   `KernelError`. Over REST one failing call fails the whole edit, so
//!   [`fillet_plan`] runs kernel/sketch.py's `fillet_corner` (:389-420)
//!   in plane space exactly, its open-end, straight-corner and too-large
//!   checks included, on the points as each fillet changes them (a fillet replaces
//!   its vertex with 9 arc points, so a later corner sees the new
//!   neighbours), and sends only the corners RoboCAD would fillet.
//! - **Join** ("Join curves"): all curves into one when there are two or more.
//! - **Unreadable curves**: a sketch read with curves dropped (no kind,
//!   `CadSketches::dropped`) has a shorter list than RoboCAD's, so its
//!   indices are not RoboCAD's: the three edits, and a `cad_sketch` call
//!   that names curves by index, are refused by name ([`dropped_refusal`]).
//!
//! Deliberately different, recorded: RoboCAD records an undo step for a
//! join of one curve, an offset of an empty sketch or a fillet with no
//! corner to round, and says nothing when no sketch exists for join; here
//! each is refused by name with nothing sent. The offset and fillet
//! forms open before the sketch is checked (`Needs::Nothing`: RoboCAD's
//! `_selected_sketch` falls back to any visible sketch, which no selection
//! need expresses), and OK refuses "Select a sketch" where RoboCAD refuses
//! before its dialog. RoboCAD's history labels the REST edit "Sketch (API)"
//! (api.py `Service.edit_sketch`); the native edit's label is RoboCAD's GUI label.
use super::specs;
use super::{BasePlane, SketchEdit, SketchTarget};
use crate::app::actions::Call;
use crate::cad::actions::Cx;
use crate::cad::document::CadDocument;
use crate::cad::ops::{Built, Env, OpEntry, Resolved};
use serde_json::{Map, Value};
use sim_api::Outcome;
use sim_runtime::cad_client::{PlaneFrame, SketchCall, SketchCurve, SketchGeometry, Uv, check_calls};
use std::f64::consts::{PI, TAU};
use std::sync::Arc;

/// RoboCAD's `_selected_sketch` (ui/app.py:752-756).
pub(crate) fn selected_sketch(doc: &CadDocument) -> Option<String> {
    let state = doc.doc.as_ref()?;
    let is_sketch = |id: &str| state.nodes.iter().any(|n| n.id == id && n.kind == "sketch");
    doc.selected_nodes().into_iter().find(|id| is_sketch(id.as_str())).or_else(|| state.nodes.iter().find(|n| n.kind == "sketch" && n.effective_visible).map(|n| n.id.clone()))
}

/// Why sketch `id`'s curve indices cannot be used: its read dropped curves.
pub(crate) fn dropped_refusal(env: &Env, id: &str, name: &str, what: &str) -> Result<(), String> {
    match env.sketches.map_or(0, |c| c.dropped(id)) {
        0 => Ok(()),
        n => Err(format!(
            "{what} refused: {n} curve(s) of {name} could not be read (no kind), so the viewer's curve indices are not RoboCAD's; Refresh (cad_refresh), or edit {name} in RoboCAD"
        )),
    }
}

/// A call that names curves by index (the edits and the tangent constructors).
fn names_curves(call: &SketchCall) -> bool {
    use SketchCall::*;
    matches!(
        call,
        CircleTangent { .. }
            | ArcTangent { .. }
            | Remove { .. }
            | Reverse { .. }
            | SplitAt { .. }
            | Trim { .. }
            | Extend { .. }
            | FilletCorner { .. }
            | Offset { .. }
            | Join { .. }
            | Unjoin { .. }
            | InsertVertex { .. }
            | RemoveVertex { .. }
            | Rebuild { .. }
    )
}

/// Sketch `id`'s geometry at the shown revision, or why it is not there.
fn read<'a>(env: &Env<'a>, id: &str, name: &str) -> Result<&'a Arc<SketchGeometry>, String> {
    let Some(cache) = env.sketches else { return Err(format!("{name} is still being read from RoboCAD; try again")) };
    if let Some(g) = cache.sketch(id) {
        return Ok(g);
    }
    Err(match cache.error(id) {
        Some(e) => format!("{name} could not be read from RoboCAD ({e}); Refresh (cad_refresh) and try again"),
        None => format!("{name} is still being read from RoboCAD; try again"),
    })
}

fn number(entry: &OpEntry, values: &Map<String, Value>, name: &str) -> Result<f64, String> {
    let v = values.get(name).ok_or_else(|| format!("{}: {name} is required", entry.id))?;
    v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| format!("{}: {name} must be a finite number (got {v})", entry.id))
}

fn unit(v: Uv) -> Uv {
    let n = v[0].hypot(v[1]);
    if n < 1e-12 { [1.0, 0.0] } else { [v[0] / n, v[1] / n] }
}

fn dist(a: Uv, b: Uv) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// kernel/sketch.py `fillet_corner` (:389-420) on a polyline's points
/// (`closed`: the polyline's): the points after rounding vertex `i`, or
/// None where it raises ("cannot fillet an end vertex" of an open one,
/// "corner is straight", "fillet radius … is too large for this corner";
/// no points: the kernel's `% 0` raises too).
pub(crate) fn fillet_corner(pts: &[Uv], closed: bool, i: usize, radius: f64) -> Option<Vec<Uv>> {
    let n = pts.len();
    if n == 0 {
        return None;
    }
    let i = i % n;
    if !closed && (i == 0 || i == n - 1) {
        return None;
    }
    let (p0, p1, p2) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
    let d0 = unit([p0[0] - p1[0], p0[1] - p1[1]]);
    let d1 = unit([p2[0] - p1[0], p2[1] - p1[1]]);
    let cosang = (d0[0] * d1[0] + d0[1] * d1[1]).clamp(-1.0, 1.0);
    let ang = cosang.acos();
    if ang < 1e-6 || (ang - PI).abs() < 1e-6 {
        return None;
    }
    let t = radius / (ang / 2.0).tan();
    if t > dist(p0, p1) || t > dist(p1, p2) {
        return None;
    }
    let a = [p1[0] + d0[0] * t, p1[1] + d0[1] * t];
    let b = [p1[0] + d1[0] * t, p1[1] + d1[1] * t];
    let bis = unit([d0[0] + d1[0], d0[1] + d1[1]]);
    let dist_c = radius / (ang / 2.0).sin();
    let center = [p1[0] + bis[0] * dist_c, p1[1] + bis[1] * dist_c];
    let a0 = (a[1] - center[1]).atan2(a[0] - center[0]);
    let a1 = (b[1] - center[1]).atan2(b[0] - center[0]);
    // Python's `%` takes the divisor's sign: `rem_euclid` for a positive one.
    let sweep = (a1 - a0 + PI).rem_euclid(TAU) - PI;
    let arc = (0..9).map(|k| {
        let ang = a0 + sweep * f64::from(k) / 8.0;
        [center[0] + radius * ang.cos(), center[1] + radius * ang.sin()]
    });
    let mut out = pts.to_vec();
    out.splice(i..i + 1, arc);
    Some(out)
}

/// RoboCAD's "Fillet corners" handler (ui/app.py:766-780) as calls: for
/// each closed polyline, vertices from the last down (over its length
/// before any fillet), a call for each corner `fillet_corner` would round.
pub(crate) fn fillet_plan(curves: &[SketchCurve], radius: f64) -> Vec<SketchCall> {
    let mut calls = Vec::new();
    for (ci, c) in curves.iter().enumerate() {
        if c.kind != "polyline" || !c.closed {
            continue;
        }
        let mut pts = c.points.clone();
        for i in (0..c.points.len()).rev() {
            if let Some(next) = fillet_corner(&pts, c.closed, i, radius) {
                pts = next;
                calls.push(SketchCall::FilletCorner { curve: ci, vertex_index: i as i64, radius });
            }
        }
    }
    calls
}

/// `Shape::SketchEdit`: offset, fillet corners or join on [`selected_sketch`].
pub(crate) fn calls(entry: &OpEntry, edit: SketchEdit, r: &Resolved, values: &Map<String, Value>, doc: &CadDocument, env: &Env) -> Result<Built, String> {
    let _ = r; // RoboCAD's handlers read the selection themselves (`_selected_sketch`).
    let id = selected_sketch(doc).ok_or_else(|| entry.refusal.to_string())?;
    let name = doc.node_name(&id);
    let g = read(env, &id, &name)?;
    let n = g.curves.len();
    let what = match edit {
        SketchEdit::Offset => "Offset curves",
        SketchEdit::FilletCorners => "Fillet corners",
        SketchEdit::Join => "Join curves",
    };
    dropped_refusal(env, &id, &name, what)?;
    let (calls, label) = match edit {
        SketchEdit::Offset => {
            let d = number(entry, values, "distance")?;
            if n == 0 {
                return Err(format!("Nothing to offset: {name} has no curves"));
            }
            ((0..n).map(|curve| SketchCall::Offset { curve, distance: d }).collect(), "Offset curves")
        }
        SketchEdit::FilletCorners => {
            let radius = number(entry, values, "radius")?;
            let calls = fillet_plan(&g.curves, radius);
            if calls.is_empty() {
                return Err(format!(
                    "No corner of {name} takes a {} fillet: RoboCAD rounds the corners of closed polylines only, and skips straight corners and those too short for the radius",
                    crate::cad::transform::fl(radius)
                ));
            }
            (calls, "Fillet corners")
        }
        SketchEdit::Join => {
            if n < 2 {
                return Err(format!("Nothing to join: {name} has {n} curve(s)"));
            }
            (vec![SketchCall::Join { curves: (0..n).collect() }], "Join curves")
        }
    };
    Ok(Built::Sketch { target: SketchTarget::Node(id), calls, label: label.to_string() })
}

/// A plane argument as RoboCAD's `ArgConverter.plane` reads it (api.py:199-207):
/// "xy" | "xz" | "yz" (any case), or a plane node of the shown tree, with its
/// frame (read at the shown revision; refused by name when it was read
/// without a valid frame).
fn named_plane(name: &str, doc: &CadDocument, env: &Env) -> Result<(Value, PlaneFrame), String> {
    let lower = name.to_ascii_lowercase();
    if let Some(b) = [BasePlane::Xy, BasePlane::Xz, BasePlane::Yz].into_iter().find(|b| b.arg() == lower) {
        return Ok((Value::from(b.arg()), b.frame()));
    }
    let node = doc.doc.as_ref().and_then(|d| d.nodes.iter().find(|n| n.id == name));
    match node {
        Some(n) if n.kind == "plane" => match env.sketches.and_then(|c| c.plane_state(name)) {
            Some(Ok(f)) => Ok((Value::from(name), f)),
            Some(Err(why)) => Err(format!("plane {}: {why}", n.name)),
            None => Err(match env.sketches.and_then(|c| c.error(name)) {
                Some(e) => format!("plane {} ({name}) could not be read from RoboCAD ({e}); Refresh (cad_refresh) and try again", n.name),
                None => format!("plane {} ({name}) is still being read from RoboCAD; try again", n.name),
            }),
        },
        Some(n) => Err(format!("unknown plane {name:?}: {} is a {}, not a plane (xy/xz/yz or a plane node id)", n.name, n.kind)),
        None => Err(format!("unknown plane {name:?} (xy/xz/yz or a plane node id in the shown tree)")),
    }
}

/// Where a shape sent without a node goes, as `cad_sketch` decides it:
/// on `plane` (given, else the active plane, else XY; a plane node's frame
/// read at the shown revision), RoboCAD's sketch tools' rule
/// (`specs::target`). The sketch tools ask it at a completing press, so a
/// shape that would be refused is not sent (`interact::press`).
pub(crate) fn shape_target(plane: Option<&str>, doc: &CadDocument, env: &Env) -> Result<SketchTarget, String> {
    let (plane_arg, frame) = match plane {
        Some(p) => named_plane(p, doc, env)?,
        None => match env.plane {
            Some(active) => (active.arg_or(BasePlane::Xy), active.frame_or_xy()?),
            None => (Value::from(BasePlane::Xy.arg()), BasePlane::Xy.frame()),
        },
    };
    specs::target(doc, env, &plane_arg, frame)
}

/// The edit's label: where and which calls ("Sketch Profile: line, circle").
fn label(where_: &str, calls: &[SketchCall]) -> String {
    let names: Vec<&str> = calls.iter().take(4).map(SketchCall::name).collect();
    let more = if calls.len() > 4 { format!(", … ({} calls)", calls.len()) } else { String::new() };
    format!("Sketch {where_}: {}{more}", names.join(", "))
}

/// What `cad_sketch` sends: the target, the calls read and checked, the label.
fn prepare(node: Option<&str>, plane: Option<&str>, calls: &[Value], doc: &CadDocument, env: &Env) -> Result<(SketchTarget, Vec<SketchCall>, String), String> {
    if calls.is_empty() {
        return Err("cad_sketch: calls is empty: give at least one [method, [args…], {kwargs}?]".into());
    }
    let parsed = calls.iter().enumerate().map(|(i, v)| SketchCall::from_json(v).map_err(|e| format!("cad_sketch call {} of {}: {e}", i + 1, calls.len()))).collect::<Result<Vec<SketchCall>, String>>()?;
    let (target, curves) = match node {
        Some(id) => {
            let n = doc.doc.as_ref().and_then(|d| d.nodes.iter().find(|n| n.id == id)).ok_or_else(|| format!("cad_sketch: no node {id} in the shown tree"))?;
            if n.kind != "sketch" {
                return Err(format!("cad_sketch: {} ({id}) is a {}, not a sketch", n.name, n.kind));
            }
            (SketchTarget::Node(id.to_string()), env.sketches.and_then(|c| c.sketch(id)).map(|g| g.curves.len()))
        }
        None => {
            let target = shape_target(plane, doc, env)?;
            let curves = match &target {
                SketchTarget::Node(id) => env.sketches.and_then(|c| c.sketch(id)).map(|g| g.curves.len()),
                SketchTarget::New { .. } => Some(0),
            };
            (target, curves)
        }
    };
    if let SketchTarget::Node(id) = &target
        && let Some((i, c)) = parsed.iter().enumerate().find(|(_, c)| names_curves(c))
    {
        dropped_refusal(env, id, &doc.node_name(id), &format!("cad_sketch call {} ({})", i + 1, c.name()))?;
    }
    check_calls(&parsed, curves).map_err(|e| format!("cad_sketch: {e}"))?;
    let where_ = match &target {
        SketchTarget::Node(id) => doc.node_name(id),
        SketchTarget::New { plane } => format!("new on {}", plane.as_str().unwrap_or("the plane")),
    };
    let label = label(&where_, &parsed);
    Ok((target, parsed, label))
}

/// `CadAction::CadSketch` (`cad_sketch`, and every sketch tool's finished
/// shape): refused by name with nothing sent while an edit is in flight,
/// the document is stale or changed since `revision`; each call read by
/// `SketchCall::from_json` (a refusal names the call and the argument) and
/// checked against the sketch's curve count when it is read (`check_calls`).
/// With `node`: that sketch of the shown tree. Without: RoboCAD's sketch
/// tools' rule on `plane` (given, else the active plane, else XY;
/// `specs::target`), or one new sketch on it carrying the calls.
pub(in crate::cad) fn sketch_action(node: Option<&str>, plane: Option<&str>, calls: &[Value], revision: Option<u64>, call: &mut Call, cx: &mut Cx) -> Outcome {
    if let Some(why) = cx.doc.commit_refusal(revision) {
        return Outcome::Done(Err(why));
    }
    let (doc, env) = cx.split();
    let (target, parsed, label) = match prepare(node, plane, calls, doc, &env) {
        Ok(p) => p,
        Err(e) => return Outcome::Done(Err(e)),
    };
    // `send_sketch` records a polygon's sides (RoboCAD's `Sketch.last_polygon_sides`).
    crate::cad::ops::send_sketch(doc, call, target, parsed, label)
}

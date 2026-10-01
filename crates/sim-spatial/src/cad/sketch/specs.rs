//! The 13 sketch tools as data ([`spec`]) and the one builder: RoboCAD's
//! `SketchTool` (cad/robocad/ui/tools.py:638-817) with nothing per tool
//! but its row.
//!
//! - [`from_points`] is `_build` (tools.py:728-760): the shape's calls
//!   from clicked points in plane coordinates, for the finished shape and,
//!   with the cursor appended, the preview (partial lists as `_build`
//!   handles them).
//! - [`from_values`] is `commit` (tools.py:784-815): the Tab values at the
//!   anchor (the first clicked point, else the plane origin).
//! - [`target`] is `_ensure_sketch` (tools.py:675-686): which sketch a
//!   shape goes to, or a new one on the plane.
//! - [`calls`] is `Shape::Sketch` (`cad_run sketch.<shape>`, the form's OK).
//!
//! Deliberately different from RoboCAD, each recorded:
//! - RoboCAD creates an empty sketch when a tool is activated
//!   (`_ensure_sketch` in `activate` → `ops.new_sketch`, an undo step). The
//!   native tool creates it with the first finished shape (one `POST
//!   /nodes {"kind": "sketch", "plane", "calls"}`), so activating a tool
//!   never edits the document, and the sketch is picked when the shape is
//!   sent (the selection and visibility at that moment).
//! - The tools without Tab values (circle_2pt, circle_3pt, arc_3pt,
//!   spline) are refused by name on OK; RoboCAD's `commit` builds nothing
//!   and records an empty undo step.
//! - Three collinear points (circle_3pt, arc_3pt) are refused here with
//!   the kernel's own message (`circumcircle`, kernel/sketch.py:613),
//!   before anything is sent; RoboCAD's `_finish` shows the same error
//!   after its kernel call.
//! - The text tool with empty text is refused ("type the text"); RoboCAD
//!   sketches no outlines and records an empty undo step.
//! - Plane coordinates are rounded to 1e-6 mm as the other tools send.
use super::{BasePlane, Finish, Readout, SketchShape, SketchSpec, SketchTarget};
use crate::cad::document::CadDocument;
use crate::cad::ops::{Built, Env, OpEntry};
use crate::cad::transform::round6;
use serde_json::{Map, Value};
use sim_runtime::cad_client::{PlaneFrame, SketchCall, Uv};
use std::collections::HashMap;

const fn row(shape: SketchShape, finish: Finish, chains: bool, readout: Readout, text_form: bool) -> SketchSpec {
    SketchSpec { shape, finish, chains, readout, text_form }
}

/// One row per tool, in `SketchShape::ALL` order: `needed` (tools.py:695),
/// the spline's Enter/double-click (:771-781), the line's chaining (:766),
/// the readout (:720-726) and the text tool's "Text to sketch:" (app.py:744-749).
static SPECS: [SketchSpec; 13] = [
    row(SketchShape::Line, Finish::Points(2), true, Readout::LengthAngle, false),
    row(SketchShape::Rectangle, Finish::Points(2), false, Readout::Size, false),
    row(SketchShape::RectangleCenter, Finish::Points(2), false, Readout::Size, false),
    row(SketchShape::Circle, Finish::Points(2), false, Readout::Radius, false),
    row(SketchShape::Circle2pt, Finish::Points(2), false, Readout::Size, false),
    row(SketchShape::Circle3pt, Finish::Points(3), false, Readout::Size, false),
    row(SketchShape::Arc3pt, Finish::Points(3), false, Readout::Size, false),
    row(SketchShape::Polygon, Finish::Points(2), false, Readout::Radius, false),
    row(SketchShape::Slot, Finish::Points(3), false, Readout::Size, false),
    row(SketchShape::Spline, Finish::EnterOrDouble, false, Readout::LengthAngle, false),
    row(SketchShape::Ellipse, Finish::Points(3), false, Readout::Size, false),
    row(SketchShape::Spiral, Finish::Points(2), false, Readout::Size, false),
    row(SketchShape::Text, Finish::Points(1), false, Readout::Size, true),
];

/// The tool's data.
pub(crate) fn spec(shape: SketchShape) -> &'static SketchSpec {
    let i = SketchShape::ALL.iter().position(|s| *s == shape).unwrap_or(0);
    &SPECS[i]
}

/// RoboCAD's text height for a clicked text (tools.py:760: `text_height`
/// is never set, so `getattr(self, "text_height", 10.0)`).
pub(crate) const CLICKED_TEXT_HEIGHT: f64 = 10.0;

/// A point (mm, RoboCAD's frame) in the plane's coordinates (RoboCAD's
/// `_local`: `to_local`, the normal component dropped), rounded to 1e-6.
pub(crate) fn local(frame: &PlaneFrame, p: [f64; 3]) -> Uv {
    let l = frame.to_local(p);
    [round6(l[0]), round6(l[1])]
}

fn dist(a: Uv, b: Uv) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

/// kernel/sketch.py `circumcircle`'s refusal: three collinear points.
fn collinear(a: Uv, b: Uv, c: Uv) -> Result<(), String> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-12 { Err("the three points are collinear".into()) } else { Ok(()) }
}

/// RoboCAD's `_build` (tools.py:728-760) on plane points: the calls of
/// the shape these points make (none when `_build` adds nothing yet: a
/// three-point shape with fewer than three, a spline with fewer than two).
/// The polygon's sides are left to RoboCAD's `Sketch.last_polygon_sides`
/// (`_build` passes none). Err: three collinear points.
pub(crate) fn from_points(shape: SketchShape, pts: &[Uv], text: &str) -> Result<Vec<SketchCall>, String> {
    let Some(&a) = pts.first() else { return Ok(Vec::new()) };
    let b = if pts.len() > 1 { pts[pts.len() - 1] } else { a };
    let angle = |p: Uv, q: Uv| (q[1] - p[1]).atan2(q[0] - p[0]).to_degrees();
    let call = match shape {
        SketchShape::Line => Some(SketchCall::Line { a, b }),
        SketchShape::Rectangle => Some(SketchCall::Rectangle { corner: [a[0].min(b[0]), a[1].min(b[1])], size: [(b[0] - a[0]).abs(), (b[1] - a[1]).abs()] }),
        SketchShape::RectangleCenter => Some(SketchCall::RectangleCenter { center: a, size: [2.0 * (b[0] - a[0]).abs(), 2.0 * (b[1] - a[1]).abs()] }),
        SketchShape::Circle => Some(SketchCall::Circle { center: a, radius: dist(a, b) }),
        SketchShape::Circle2pt => Some(SketchCall::CircleTwoPoint { a, b }),
        SketchShape::Circle3pt | SketchShape::Arc3pt if pts.len() >= 3 => {
            collinear(pts[0], pts[1], pts[2])?;
            Some(if shape == SketchShape::Circle3pt { SketchCall::CircleThreePoint { a: pts[0], b: pts[1], c: pts[2] } } else { SketchCall::ArcThreePoint { a: pts[0], b: pts[1], c: pts[2] } })
        }
        SketchShape::Circle3pt | SketchShape::Arc3pt => None,
        SketchShape::Polygon => Some(SketchCall::Polygon { center: a, radius: dist(a, b), sides: None, rotation: angle(a, b) }),
        SketchShape::Slot => {
            let width = if pts.len() < 3 {
                4.0
            } else {
                let (p0, p1, p2) = (pts[0], pts[1], pts[2]);
                2.0 * ((p2[0] - p1[0]) * -(p1[1] - p0[1]) + (p2[1] - p1[1]) * (p1[0] - p0[0])).abs() / dist(p0, p1).max(1e-6)
            };
            Some(SketchCall::Slot { a: pts[0], b: if pts.len() > 1 { pts[1] } else { b }, width: width.max(0.5) })
        }
        SketchShape::Ellipse => {
            let rx = if pts.len() < 3 { dist(a, b) } else { dist(a, pts[1]) };
            let ry = if pts.len() < 3 { rx / 2.0 } else { dist(a, pts[2]) };
            let rotation = if pts.len() > 1 { angle(a, pts[1]) } else { 0.0 };
            Some(SketchCall::Ellipse { center: a, radius_x: rx, radius_y: ry, rotation })
        }
        SketchShape::Spline => (pts.len() >= 2).then(|| SketchCall::Spline { points: pts.to_vec(), closed: false }),
        SketchShape::Spiral => {
            let r = dist(a, b);
            Some(SketchCall::Spiral { center: a, start_radius: 0.15 * r, end_radius: r, turns: 3.0 })
        }
        SketchShape::Text => Some(SketchCall::Text { origin: a, text: text.to_string(), height: CLICKED_TEXT_HEIGHT }),
    };
    Ok(call.into_iter().collect())
}

/// A number parameter of the Tab values (by the catalogue's name).
fn value(shape: SketchShape, values: &Map<String, Value>, name: &str) -> Result<f64, String> {
    let v = values.get(name).ok_or_else(|| format!("sketch.{}: {name} is required", shape.name()))?;
    v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| format!("sketch.{}: {name} must be a finite number (got {v})", shape.name()))
}

/// RoboCAD's `commit` (tools.py:784-815): the Tab values at `anchor`
/// (plane coordinates). `polygon_sides` stands in for a polygon's missing
/// `sides` (RoboCAD's field opens with `Sketch.last_polygon_sides`; the
/// catalogue's `sides` has no default, so `cad_run sketch.polygon` without
/// it gets the remembered count, as the form's draft does).
pub(crate) fn from_values(shape: SketchShape, anchor: Uv, values: &Map<String, Value>, text: &str, polygon_sides: u32) -> Result<Vec<SketchCall>, String> {
    let a = anchor;
    let v = |name: &str| value(shape, values, name);
    let call = match shape {
        SketchShape::Line => {
            let (l, ang) = (v("length")?, v("angle")?.to_radians());
            SketchCall::Line { a, b: [a[0] + l * ang.cos(), a[1] + l * ang.sin()] }
        }
        SketchShape::Rectangle => SketchCall::Rectangle { corner: a, size: [v("width")?, v("height")?] },
        SketchShape::RectangleCenter => SketchCall::RectangleCenter { center: a, size: [v("width")?, v("height")?] },
        SketchShape::Circle => SketchCall::Circle { center: a, radius: v("diameter")? / 2.0 },
        SketchShape::Polygon => {
            // `int(values[1])`: truncated toward zero.
            let sides = match values.get("sides") {
                Some(_) => v("sides")?.trunc(),
                None => f64::from(polygon_sides),
            };
            if sides < 3.0 || sides > f64::from(u32::MAX) {
                return Err(format!("sketch.polygon: sides must be a whole number of at least 3 (got {sides})"));
            }
            SketchCall::Polygon { center: a, radius: v("radius")?, sides: Some(sides as u32), rotation: 0.0 }
        }
        SketchShape::Slot => SketchCall::Slot { a, b: [a[0] + v("length")?, a[1]], width: v("width")? },
        SketchShape::Ellipse => SketchCall::Ellipse { center: a, radius_x: v("radius_x")?, radius_y: v("radius_y")?, rotation: 0.0 },
        SketchShape::Spiral => SketchCall::Spiral { center: a, start_radius: v("start_radius")?, end_radius: v("end_radius")?, turns: v("turns")? },
        SketchShape::Text => SketchCall::Text { origin: a, text: text.to_string(), height: v("height")? },
        SketchShape::Circle2pt | SketchShape::Circle3pt | SketchShape::Arc3pt | SketchShape::Spline => {
            return Err(format!("sketch.{} has no exact values: click its points on the plane (RoboCAD's Tab builds nothing for it)", shape.name()));
        }
    };
    Ok(vec![call])
}

/// RoboCAD's `_ensure_sketch` (tools.py:675-686) at the moment a shape is
/// sent: the first selected node that is a sketch on `frame` (selection
/// order), else the first visible sketch on it in the shown tree's walk
/// order, else a new sketch on `plane_arg`. Planes compare as RoboCAD's
/// dataclass `==` (`PlaneFrame::same`, 1e-9 after JSON). Refused, rather
/// than guessed, while any sketch's geometry is not read at the shown
/// revision.
pub(crate) fn target(doc: &CadDocument, env: &Env, plane_arg: &Value, frame: PlaneFrame) -> Result<SketchTarget, String> {
    let Some(state) = &doc.doc else { return Err("the model tree is still being read from RoboCAD; try again".into()) };
    let mut planes: HashMap<&str, Option<PlaneFrame>> = HashMap::new();
    for n in state.nodes.iter().filter(|n| n.kind == "sketch") {
        match env.sketches.and_then(|c| c.sketch(&n.id)) {
            Some(g) => {
                planes.insert(n.id.as_str(), g.plane);
            }
            None => {
                if let Some(e) = env.sketches.and_then(|c| c.error(&n.id)) {
                    return Err(format!("sketch {} could not be read from RoboCAD ({e}); Refresh (cad_refresh) and try again", n.name));
                }
                return Err("sketch planes are still being read from RoboCAD; try again".into());
            }
        }
    }
    let on_plane = |id: &str| planes.get(id).copied().flatten().is_some_and(|p| p.same(&frame, 1e-9));
    if let Some(id) = doc.selected_nodes().into_iter().find(|id| on_plane(id.as_str())) {
        return Ok(SketchTarget::Node(id));
    }
    if let Some(n) = state.nodes.iter().find(|n| n.kind == "sketch" && n.effective_visible && on_plane(n.id.as_str())) {
        return Ok(SketchTarget::Node(n.id.clone()));
    }
    Ok(SketchTarget::New { plane: plane_arg.clone() })
}

/// The polygon's side count RoboCAD remembers after these calls
/// (`Sketch.last_polygon_sides`, set by every `polygon` with sides).
pub(crate) fn polygon_sides_after(calls: &[SketchCall]) -> Option<u32> {
    calls.iter().rev().find_map(|c| match c {
        SketchCall::Polygon { sides: Some(n), .. } => Some(*n),
        _ => None,
    })
}

/// Record the side count a sent polygon leaves in RoboCAD
/// (`OpsState::polygon_sides`, the polygon tool's `sides` field default).
pub(crate) fn remember_polygon_sides(doc: &mut CadDocument, calls: &[SketchCall]) {
    if let Some(n) = polygon_sides_after(calls)
        && doc.ops.polygon_sides != Some(n)
    {
        doc.ops.polygon_sides = Some(n);
    }
}

/// `Shape::Sketch` (`cad_run sketch.<shape>`, the tool form's OK): RoboCAD's
/// `commit` on the active plane (else XY) at the `anchor` parameter (the
/// form's submit fills it with the first clicked point; empty: the plane
/// origin), to the sketch [`target`] picks. `ops::prepare` has checked
/// `commit_refusal`.
pub(crate) fn calls(entry: &OpEntry, shape: SketchShape, values: &Map<String, Value>, doc: &CadDocument, env: &Env) -> Result<Built, String> {
    let frame = match env.plane {
        Some(p) => p.frame_or_xy()?,
        None => PlaneFrame::XY,
    };
    let plane_arg = env.plane.map_or_else(|| Value::from(BasePlane::Xy.arg()), |p| p.arg_or(BasePlane::Xy));
    let anchor = match values.get("anchor") {
        None => [0.0, 0.0],
        Some(v) => match v.as_array().map(|a| a.iter().map(Value::as_f64).collect::<Option<Vec<f64>>>()) {
            Some(Some(p)) if p.len() == 3 && p.iter().all(|x| x.is_finite()) => local(&frame, [p[0], p[1], p[2]]),
            _ => return Err(format!("{}: anchor must be three finite numbers x, y, z (got {v})", entry.id)),
        },
    };
    let text = values.get("text").and_then(Value::as_str).unwrap_or("");
    if spec(shape).text_form && text.is_empty() {
        return Err(format!("{}: type the text to sketch first (its \"Text to sketch:\" field)", entry.id));
    }
    let calls = from_values(shape, anchor, values, text, doc.ops.polygon_sides.unwrap_or(6))?;
    let target = target(doc, env, &plane_arg, frame)?;
    Ok(Built::Sketch { target, calls, label: format!("Sketch {}", shape.name()) })
}

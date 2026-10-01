//! The sketch tools' live preview (display only): RoboCAD's `hover`
//! (ui/tools.py:699-727) builds the shape from the clicked points and the
//! cursor into a throw-away `Sketch` and draws each curve's `sample(48)`
//! (a slot's closed outline) through the active plane in its temporary-shape
//! colour (0.4, 0.9, 1.0). Here the same calls ([`super::specs::from_points`])
//! become curves with the kernel's constructors' geometry
//! ([`preview_curves`], kernel/sketch.py:159-294), sampled with
//! `SketchCurve::sample(48)` and drawn as `ToolGizmos` lines (depth test
//! off, as RoboCAD's temporary shapes). Nothing is sent, nothing changes.
//!
//! Deliberately different, recorded: a text call previews as a placeholder
//! box on its baseline, the text height tall and 0.6 × height wide per
//! character (RoboCAD draws the font outlines from `text_outlines`, which
//! needs fontTools; the text tool finishes on its one click, so RoboCAD's
//! hover never shows it either).
use super::specs::{from_points, local};
use crate::cad::document::CadDocument;
use crate::cad::sketch::{BasePlane, CadActivePlane};
use crate::cad::transform::ToolGizmos;
use crate::cad::view::CadView;
use bevy::prelude::*;
use sim_runtime::cad_client::{PlaneFrame, SketchCall, SketchCurve, Uv};
use std::f64::consts::{PI, TAU};

/// RoboCAD's temporary-shape colour for a sketch preview (tools.py:718).
const PREVIEW: Color = Color::srgb(0.4, 0.9, 1.0);
/// Samples per curve (tools.py:713).
const SAMPLES: usize = 48;

fn curve(kind: &str) -> SketchCurve {
    SketchCurve { kind: kind.to_string(), ..Default::default() }
}

fn polyline(points: Vec<Uv>, closed: bool) -> SketchCurve {
    SketchCurve { points, closed, ..curve("polyline") }
}

fn circle(center: Uv, radius: f64) -> SketchCurve {
    SketchCurve { center: Some(center), radius, ..curve("circle") }
}

fn unit(v: Uv) -> Uv {
    let n = v[0].hypot(v[1]);
    if n < 1e-12 { [1.0, 0.0] } else { [v[0] / n, v[1] / n] }
}

/// kernel/sketch.py `circumcircle`; None for collinear points (it raises).
fn circumcircle(a: Uv, b: Uv, c: Uv) -> Option<(Uv, f64)> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-12 {
        return None;
    }
    let (a2, b2, c2) = (a[0] * a[0] + a[1] * a[1], b[0] * b[0] + b[1] * b[1], c[0] * c[0] + c[1] * c[1]);
    let ux = (a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d;
    let uy = (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d;
    Some(([ux, uy], (a[0] - ux).hypot(a[1] - uy)))
}

/// kernel/sketch.py `rectangle`.
fn rectangle(corner: Uv, size: Uv) -> SketchCurve {
    let ([x, y], [w, h]) = (corner, size);
    polyline(vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]], true)
}

/// The curves the kernel's constructors would add for these calls
/// (kernel/sketch.py:159-294), in order; edits and the tangent
/// constructors (unreachable by REST) add none. `last_sides` stands in for
/// a polygon without sides (RoboCAD's `Sketch.last_polygon_sides`).
pub(crate) fn preview_curves(calls: &[SketchCall], last_sides: u32) -> Vec<SketchCurve> {
    let mut out = Vec::new();
    for call in calls {
        let c = match call {
            SketchCall::Line { a, b } => SketchCurve { points: vec![*a, *b], ..curve("line") },
            SketchCall::Polyline { points, closed } => polyline(points.clone(), *closed),
            SketchCall::Spline { points, closed } => SketchCurve { points: points.clone(), closed: *closed, ..curve("spline") },
            SketchCall::ControlCurve { points, degree, closed } => SketchCurve { points: points.clone(), degree: *degree, closed: *closed, ..curve("control") },
            SketchCall::Circle { center, radius } => circle(*center, *radius),
            SketchCall::CircleTwoPoint { a, b } => circle([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0], (b[0] - a[0]).hypot(b[1] - a[1]) / 2.0),
            SketchCall::CircleThreePoint { a, b, c } => match circumcircle(*a, *b, *c) {
                Some((center, r)) => circle(center, r),
                None => continue,
            },
            SketchCall::Ellipse { center, radius_x, radius_y, rotation } => SketchCurve { center: Some(*center), radius: *radius_x, radius2: *radius_y, rotation: *rotation, ..curve("ellipse") },
            SketchCall::Arc { center, radius, start_deg, end_deg } => SketchCurve { center: Some(*center), radius: *radius, start_angle: *start_deg, end_angle: *end_deg, ..curve("arc") },
            SketchCall::ArcThreePoint { a, b, c } => {
                let Some((center, r)) = circumcircle(*a, *b, *c) else { continue };
                let deg = |p: &Uv| (p[1] - center[1]).atan2(p[0] - center[0]).to_degrees();
                let (a0, a1, a2) = (deg(a), deg(b), deg(c));
                // Sweep from a through b to c (Python's `%`: the divisor's sign).
                let mut sweep = (a2 - a0).rem_euclid(360.0);
                let mid = (a1 - a0).rem_euclid(360.0);
                if mid > sweep {
                    sweep -= 360.0;
                }
                SketchCurve { center: Some(center), radius: r, start_angle: a0, end_angle: a0 + sweep, ..curve("arc") }
            }
            SketchCall::Rectangle { corner, size } => rectangle(*corner, *size),
            SketchCall::RectangleCenter { center, size } => rectangle([center[0] - size[0] / 2.0, center[1] - size[1] / 2.0], *size),
            SketchCall::RectangleThreePoint { a, b, c } => {
                let n = unit([-(b[1] - a[1]), b[0] - a[0]]);
                let h = (c[0] - b[0]) * n[0] + (c[1] - b[1]) * n[1];
                let d = [n[0] * h, n[1] * h];
                polyline(vec![*a, *b, [b[0] + d[0], b[1] + d[1]], [a[0] + d[0], a[1] + d[1]]], true)
            }
            SketchCall::Polygon { center, radius, sides, rotation } => {
                let n = sides.filter(|n| *n > 0).unwrap_or(last_sides).max(1);
                let r0 = rotation.to_radians();
                let pts = (0..n).map(|i| r0 + TAU * f64::from(i) / f64::from(n)).map(|t| [center[0] + radius * t.cos(), center[1] + radius * t.sin()]).collect();
                polyline(pts, true)
            }
            SketchCall::Slot { a, b, width } => {
                let d = unit([b[0] - a[0], b[1] - a[1]]);
                SketchCurve { points: vec![*a, *b], radius: width / 2.0, closed: true, rotation: d[1].atan2(d[0]).to_degrees(), ..curve("slot") }
            }
            SketchCall::Spiral { center, start_radius, end_radius, turns } => {
                // `max(int(turns * points_per_turn), 8)`, 36 points per turn.
                let n = ((turns * 36.0) as i64).max(8);
                let pts = (0..=n)
                    .map(|i| {
                        let t = i as f64 / n as f64;
                        let (a, r) = (2.0 * PI * turns * t, start_radius + (end_radius - start_radius) * t);
                        [center[0] + r * a.cos(), center[1] + r * a.sin()]
                    })
                    .collect();
                SketchCurve { points: pts, ..curve("spline") }
            }
            SketchCall::Text { origin, text, height } => {
                let w = 0.6 * height * text.chars().count().max(1) as f64;
                rectangle(*origin, [w, *height])
            }
            _ => continue,
        };
        out.push(c);
    }
    out
}

/// The preview's polylines (mm, RoboCAD's frame): the calls `_build`
/// makes from `points` (plane coordinates) with the cursor appended.
pub(crate) fn preview_lines(shape: super::SketchShape, points: &[Uv], cursor: Uv, text: &str, last_sides: u32, frame: &PlaneFrame) -> Vec<Vec<[f64; 3]>> {
    let mut pts = points.to_vec();
    pts.push(cursor);
    // RoboCAD's `hover` swallows a builder error (collinear points): no preview.
    let Ok(calls) = from_points(shape, &pts, text) else { return Vec::new() };
    preview_curves(&calls, last_sides).iter().map(|c| c.sample(SAMPLES).into_iter().map(|p| frame.to_world(p[0], p[1], 0.0)).collect()).collect()
}

/// What the last preview was drawn from, and its lines (model mm).
#[derive(Default)]
pub(super) struct Drawn {
    key: Option<(super::SketchShape, Vec<Uv>, Uv, String, u32, [f64; 9])>,
    lines: Vec<Vec<Vec3>>,
}

/// Present: the shape in progress, drawn while points are clicked
/// (RoboCAD's `hover` returns without points). Display only.
pub(super) fn draw(doc: Option<Res<CadDocument>>, view: Option<Res<CadView>>, plane: Option<Res<CadActivePlane>>, mut gizmos: Gizmos<ToolGizmos>, mut drawn: Local<Drawn>) {
    let (Some(doc), Some(view)) = (doc, view) else { return };
    let Some(s) = doc.ops.sketch.as_ref().filter(|s| s.in_progress()) else { return };
    let Some(cursor) = s.cursor else { return };
    if !view.valid || !doc.ops.active.is_some_and(|id| id == format!("sketch.{}", s.shape.name())) {
        return;
    }
    let frame = match plane.as_deref().map(CadActivePlane::frame_or_xy) {
        Some(Ok(f)) => f,
        Some(Err(_)) => return,
        None => BasePlane::Xy.frame(),
    };
    let sides = doc.ops.polygon_sides.unwrap_or(6);
    let points: Vec<Uv> = s.points.iter().map(|p| local(&frame, *p)).collect();
    let cursor = local(&frame, cursor);
    let f = [frame.origin, frame.normal, frame.x_axis].concat();
    let fk = [f[0], f[1], f[2], f[3], f[4], f[5], f[6], f[7], f[8]];
    let key = (s.shape, points, cursor, s.text.clone(), sides, fk);
    if drawn.key.as_ref() != Some(&key) {
        let lines = preview_lines(key.0, &key.1, key.2, &key.3, sides, &frame);
        drawn.lines = lines.into_iter().map(|l| l.into_iter().map(|p| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)).collect()).collect();
        drawn.key = Some(key);
    }
    for line in &drawn.lines {
        gizmos.linestrip(line.iter().map(|p| view.world_from_model.transform_point3(*p)), PREVIEW);
    }
}

//! Joint glyphs, motor shaft axes, sensor triads and sagging cable arcs
//! (display only): RoboCAD's `_draw_robotics` (ui/viewport.py:971-1021)
//! with `robotics.joint_glyph` (robotics.py:444-473).
//!
//! RoboCAD draws, over the bodies (depth test off), 2 px lines:
//! - **a joint** (every visible joint node): the axis line through the
//!   pivot, 1.5 sizes each way, plus a ring of one size (revolute,
//!   continuous), two rings (ball), a double arrow (prismatic) or a square
//!   (fixed and the loop joints), and a 9 px dot at the pivot, in the joint
//!   type's colour; a size is 18 px at the view (26 px when selected);
//! - **a motor**: its shaft axis, `mount_point` → `shaft_tip` (1.0, 0.4, 0.2);
//! - **a sensor**: a triad at `point` along its `axes` (world axes when it
//!   has none), red, green, blue, 14 px long (22 px when selected), and an
//!   8 px dot (0.9, 0.9, 0.3);
//! - **a cable**: a 13-point arc from `from_point` to `to_point` sagging
//!   0.15 × the chord at its middle (−Z), orange, brighter when selected.
//!
//! Here: the same shapes from RoboCAD's reads on the document
//! (`data`: `GET /robot`'s joints and motors, `GET /sensors`, `GET /cables`,
//! in RoboCAD's model frame, mm, Z up), as [`RobotGizmos`] lines (2 px, in
//! front of the bodies) mapped through `CadView::world_from_model`, as the
//! sketches are. The shapes are built by [`glyph_lines`] only when the
//! reads change ([`key`]: document generation, read revision); which are
//! shown and selected only when the panels' revision or the selection
//! changes; each frame only scales the screen-sized parts and maps points.
//! Nodes that are not `effective_visible` in the shown tree draw nothing,
//! as RoboCAD's `is_visible`. RoboCAD has no toggle for them (they are
//! hidden only in its pose preview, which CAD mode does not have).
//!
//! Deliberately different: a screen size is measured at each glyph's own
//! depth (`CadView::mm_per_pixel`), not at the orbit target's distance as
//! RoboCAD's `world_per_pixel`, so far glyphs keep their pixel size; a dot
//! is a small three-axis cross of the same size (gizmos draw no points).
use crate::app::{ViewerMode, ViewerSet};
use crate::cad::document::CadDocument;
use crate::cad::selection::{CadItems, CadSelection};
use crate::cad::view::CadView;
use bevy::math::DVec3;
use bevy::prelude::*;
use crate::cad::types::{CableMeta, NodeDetail, RobotJoint, RobotMotor, RobotSummary, SelectionItem, SensorMeta};
use std::collections::HashMap;

/// The glyphs' lines: 2 px (viewport.py `_draw_temp`) and over the bodies,
/// as RoboCAD draws them with the depth test off.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub(crate) struct RobotGizmos;

pub(crate) fn build(app: &mut App) {
    app.insert_gizmo_config(RobotGizmos, GizmoConfig { depth_bias: -1.0, line: GizmoLineConfig { width: 2.0, ..default() }, ..default() })
        .add_systems(Update, draw.in_set(ViewerSet::Present).run_if(in_state(ViewerMode::Cad)));
}

/// What the shapes are built from: (document generation, the revision
/// RoboCAD's robot reads were made at).
pub(crate) type Key = (u64, Option<u64>);

/// The shapes' key for the document as it is now.
pub(crate) fn key(doc: &CadDocument) -> Key {
    (doc.generation, doc.robot.data.read_at())
}

/// What a glyph stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Joint,
    Motor,
    Sensor,
    Cable,
}
impl Kind {
    /// RoboCAD's screen size of the glyph in pixels (viewport.py:981, 995):
    /// one "size" of a joint glyph, a sensor triad's arm. Motors and cables
    /// are model-sized.
    pub(crate) fn size_px(self, selected: bool) -> f64 {
        match (self, selected) {
            (Kind::Joint, false) => 18.0,
            (Kind::Joint, true) => 26.0,
            (Kind::Sensor, false) => 14.0,
            (Kind::Sensor, true) => 22.0,
            _ => 0.0,
        }
    }
}

/// How a line's points are given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scale {
    /// Model points (mm).
    Model,
    /// Offsets from the glyph's anchor in glyph sizes ([`Kind::size_px`]).
    Size,
    /// Offsets from the anchor in screen pixels (the dots).
    Pixels,
}

/// One polyline of a glyph.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Line {
    pub scale: Scale,
    pub points: Vec<DVec3>,
    /// RGB as RoboCAD gives it.
    pub color: [f32; 3],
    /// The colour while the glyph's node is selected (a cable's; others keep theirs).
    pub selected: [f32; 3],
}
impl Line {
    fn new(scale: Scale, points: Vec<DVec3>, color: [f32; 3]) -> Self {
        Line { scale, points, color, selected: color }
    }
}

/// One node's glyph: its lines around `anchor` (mm).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Glyph {
    pub node: String,
    pub kind: Kind,
    pub anchor: DVec3,
    pub lines: Vec<Line>,
}
impl Glyph {
    /// `line`'s model points (mm) with one glyph size `size_mm` and one
    /// pixel `pixel_mm` long.
    pub(crate) fn place(&self, line: &Line, size_mm: f64, pixel_mm: f64) -> Vec<DVec3> {
        let factor = match line.scale {
            Scale::Model => return line.points.clone(),
            Scale::Size => size_mm,
            Scale::Pixels => pixel_mm,
        };
        line.points.iter().map(|o| self.anchor + *o * factor).collect()
    }
}

/// RoboCAD's joint colours (robotics.py:448); any other type is white.
fn joint_color(kind: &str) -> [f32; 3] {
    match kind {
        "revolute" => [1.0, 0.75, 0.2],
        "continuous" => [1.0, 0.55, 0.2],
        "prismatic" => [0.3, 0.9, 1.0],
        "fixed" => [0.7, 0.7, 0.7],
        "ball" => [0.9, 0.4, 0.9],
        _ => [1.0, 1.0, 1.0],
    }
}
const MOTOR: [f32; 3] = [1.0, 0.4, 0.2];
const SENSOR_AXES: [[f32; 3]; 3] = [[1.0, 0.3, 0.3], [0.3, 1.0, 0.3], [0.3, 0.5, 1.0]];
const SENSOR_DOT: [f32; 3] = [0.9, 0.9, 0.3];
const CABLE: [f32; 3] = [0.8, 0.45, 0.15];
const CABLE_SELECTED: [f32; 3] = [0.95, 0.6, 0.2];
/// A joint's dot (9 px) and a sensor's (8 px), as half widths.
const JOINT_DOT_HALF_PX: f64 = 4.5;
const SENSOR_DOT_HALF_PX: f64 = 4.0;
/// A cable arc's points (viewport.py:1010) and its sag, a fraction of the chord.
const CABLE_POINTS: usize = 13;
const CABLE_SAG: f64 = 0.15;

fn v(p: [f64; 3]) -> DVec3 {
    DVec3::from_array(p)
}

/// RoboCAD's `v_unit`: +Z for a (near) zero vector.
fn unit(a: DVec3) -> DVec3 {
    let n = a.length();
    if n < 1e-12 { DVec3::Z } else { a / n }
}

/// A dot as a three-axis cross, `half` pixels each way (Pixels offsets).
fn dot(half: f64, color: [f32; 3]) -> Vec<Line> {
    [DVec3::X, DVec3::Y, DVec3::Z].into_iter().map(|a| Line::new(Scale::Pixels, vec![-a * half, a * half], color)).collect()
}

/// `robotics.joint_glyph` in glyph sizes around the pivot.
pub(crate) fn joint_glyph(j: &RobotJoint) -> Glyph {
    let a = unit(v(j.axis));
    let color = joint_color(&j.kind);
    let helper = if a.z.abs() < 0.9 { DVec3::Z } else { DVec3::X };
    let u = unit(helper.cross(a));
    let w = a.cross(u);
    let mut lines = vec![Line::new(Scale::Size, vec![-a * 1.5, a * 1.5], color)];
    let ring = |x: DVec3, y: DVec3| (0..=32).map(|i| std::f64::consts::TAU * i as f64 / 32.0).map(|t| x * t.cos() + y * t.sin()).collect::<Vec<_>>();
    match j.kind.as_str() {
        "revolute" | "continuous" | "ball" => {
            lines.push(Line::new(Scale::Size, ring(u, w), color));
            if j.kind == "ball" {
                lines.push(Line::new(Scale::Size, ring(a, w), color));
            }
        }
        "prismatic" => {
            for s in [1.0, -1.0] {
                let tip = a * s * 1.5;
                let back = a * s;
                lines.push(Line::new(Scale::Size, vec![back, tip], color));
                lines.push(Line::new(Scale::Size, vec![tip, back + u * 0.3], color));
                lines.push(Line::new(Scale::Size, vec![tip, back - u * 0.3], color));
            }
        }
        _ => {
            for s in [1.0, -1.0] {
                lines.push(Line::new(Scale::Size, vec![u * s * 0.5 + w * 0.5, u * s * 0.5 - w * 0.5], color));
            }
            lines.push(Line::new(Scale::Size, vec![u * 0.5 + w * 0.5, -u * 0.5 + w * 0.5], color));
            lines.push(Line::new(Scale::Size, vec![u * 0.5 - w * 0.5, -u * 0.5 - w * 0.5], color));
        }
    }
    lines.extend(dot(JOINT_DOT_HALF_PX, color));
    Glyph { node: j.id.clone(), kind: Kind::Joint, anchor: v(j.pivot), lines }
}

/// A motor's shaft axis, `mount_point` → `shaft_tip` (none without both).
pub(crate) fn motor_glyph(m: &RobotMotor) -> Option<Glyph> {
    let (a, b) = (v(m.mount_point?), v(m.shaft_tip?));
    Some(Glyph { node: m.id.clone(), kind: Kind::Motor, anchor: a, lines: vec![Line::new(Scale::Model, vec![a, b], MOTOR)] })
}

/// A sensor's triad (its axes, or the world's) and dot.
pub(crate) fn sensor_glyph(node: &NodeDetail) -> Option<Glyph> {
    let meta = SensorMeta::of(node)?;
    let world = vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    // Python's `axes or [...]`: an empty list is the world's too.
    let axes = meta.axes.filter(|a| !a.is_empty()).unwrap_or(world);
    let mut lines: Vec<Line> = axes.iter().zip(SENSOR_AXES).map(|(ax, c)| Line::new(Scale::Size, vec![DVec3::ZERO, v(*ax)], c)).collect();
    lines.extend(dot(SENSOR_DOT_HALF_PX, SENSOR_DOT));
    Some(Glyph { node: node.summary.id.clone(), kind: Kind::Sensor, anchor: v(meta.point), lines })
}

/// RoboCAD's cable arc: a quadratic through the chord's midpoint (a
/// straight chord), lowered by `0.15 × chord × 4t(1 − t)` in Z.
pub(crate) fn cable_arc(a: DVec3, b: DVec3) -> Vec<DVec3> {
    let mid = (a + b) * 0.5;
    let drop = CABLE_SAG * a.distance(b);
    (0..CABLE_POINTS)
        .map(|k| {
            let t = k as f64 / (CABLE_POINTS - 1) as f64;
            let mut q = a * (1.0 - t).powi(2) + mid * (2.0 * (1.0 - t) * t) + b * (t * t);
            q.z -= drop * 4.0 * t * (1.0 - t);
            q
        })
        .collect()
}

/// A cable's sagging arc.
pub(crate) fn cable_glyph(node: &NodeDetail) -> Option<Glyph> {
    let meta: CableMeta = CableMeta::of(node)?;
    let (a, b) = (v(meta.from_point), v(meta.to_point));
    let line = Line { scale: Scale::Model, points: cable_arc(a, b), color: CABLE, selected: CABLE_SELECTED };
    Some(Glyph { node: node.summary.id.clone(), kind: Kind::Cable, anchor: a, lines: vec![line] })
}

/// Every glyph RoboCAD's reads describe: joints, motors, sensors, cables.
pub(crate) fn glyph_lines(summary: Option<&RobotSummary>, sensors: &[NodeDetail], cables: &[NodeDetail]) -> Vec<Glyph> {
    let mut out: Vec<Glyph> = Vec::new();
    if let Some(s) = summary {
        out.extend(s.joints.iter().map(joint_glyph));
        out.extend(s.motors.iter().filter_map(motor_glyph));
    }
    out.extend(sensors.iter().filter_map(sensor_glyph));
    out.extend(cables.iter().filter_map(cable_glyph));
    out
}

/// Each glyph's (shown, selected): shown when its node is
/// `effective_visible` in the shown tree (absent or no tree: hidden).
pub(crate) fn style(doc: &CadDocument, selection: &[SelectionItem], glyphs: &[Glyph]) -> Vec<(bool, bool)> {
    let Some(state) = &doc.doc else { return vec![(false, false); glyphs.len()] };
    let visible: HashMap<&str, bool> = state.nodes.iter().map(|n| (n.id.as_str(), n.effective_visible)).collect();
    let selected = selection.nodes();
    glyphs.iter().map(|g| (visible.get(g.node.as_str()).copied().unwrap_or(false), selected.contains(&g.node))).collect()
}

/// The built glyphs and their styles, with the keys they were made at.
#[derive(Default)]
struct Drawn {
    key: Option<Key>,
    glyphs: Vec<Glyph>,
    /// (shapes' key, panels' revision, selection change count).
    style_key: Option<(Key, u64, u64)>,
    style: Vec<(bool, bool)>,
}

/// Present: the robot glyphs (display only).
fn draw(doc: Option<Res<CadDocument>>, view: Option<Res<CadView>>, selection: CadSelection, mut gizmos: Gizmos<RobotGizmos>, mut drawn: Local<Drawn>) {
    let (Some(doc), Some(view)) = (doc, view) else { return };
    if !view.valid {
        return;
    }
    let drawn = &mut *drawn;
    let now = key(&doc);
    if drawn.key != Some(now) {
        let data = &doc.robot.data;
        drawn.glyphs = glyph_lines(data.summary(), data.sensors(), data.cables());
        drawn.key = Some(now);
    }
    let style_key = (now, doc.revision, selection.changed());
    if drawn.style_key != Some(style_key) {
        drawn.style = style(&doc, &selection.items(), &drawn.glyphs);
        drawn.style_key = Some(style_key);
    }
    let rgb = |c: [f32; 3]| Color::srgb(c[0], c[1], c[2]);
    for (glyph, &(shown, selected)) in drawn.glyphs.iter().zip(&drawn.style) {
        if !shown {
            continue;
        }
        let pixel = view.mm_per_pixel(glyph.anchor.as_vec3()).map_or(0.0, f64::from);
        let size = pixel * glyph.kind.size_px(selected);
        for line in &glyph.lines {
            // A screen-sized part needs the anchor on screen.
            if line.scale != Scale::Model && pixel <= 0.0 {
                continue;
            }
            let color = rgb(if selected { line.selected } else { line.color });
            gizmos.linestrip(glyph.place(line, size, pixel).into_iter().map(|p| view.world_from_model.transform_point3(p.as_vec3())), color);
        }
    }
}

#[cfg(test)]
#[path = "glyphs_tests.rs"]
mod tests;

//! The measure tool (RoboCAD's `MeasureTool`, ui/tools.py:1027-1062, and
//! `measure_between`, ui/app.py:707-735) and its math (cad/robocad/analysis.py:20-48).
//!
//! - **Tool** (M): the pointer snaps as RoboCAD's (`snap::snap_on`: the
//!   drawn bodies' and visible sketches' candidates, projected onto the
//!   active plane while 2D snapping is on; a press is refused while that
//!   plane node's frame is being read); hovering shows
//!   the snap marker and the readout ("vertex  (x, y, z)"; with a first pick
//!   held, "12.5 mm  (vertex)"). A press picks: the item under the cursor in
//!   the selection mode (a face or body by ray cast; an edge or vertex by
//!   the select click's search, `pick::candidates_at`: within 6 px, not
//!   behind a surface, cut by the section, locked or hidden, as RoboCAD's
//!   pick pass) and the point (the snap point, or the surface hit when
//!   the snap is free or on the plane, as `MeasureTool.press`). The second press writes
//!   `CadMeasure {a, b, keep: Shift}`; `transform::handle` computes it with
//!   [`between`], shows it and, when kept, adds it as one
//!   `POST /ops/add_measurement` (RoboCAD's Shift+click).
//! - **Math** ([`between`]): two parallel planar faces → their distance;
//!   other face pairs → the angle between their normals; the same circular
//!   edge twice → its radius; two line edges → the angle between them; two
//!   other edges → their midpoints' distance; anything else → the points'
//!   distance. Labels are RoboCAD's ("12.000 mm", "R 3.000 mm  (Ø 6.000)",
//!   "90.00°").
//!
//! Deliberately different from RoboCAD, recorded:
//! - The value is not copied to the clipboard (the viewer has no clipboard
//!   dependency): it shows in the status line and the tool bar, and REST
//!   `cad_measure` answers it. The hint says so instead of "copied".
//! - The same circular edge picked twice gives its radius. RoboCAD tests
//!   two edges before the same edge (app.py:722-727), so its radius branch
//!   is unreachable and it reports the midpoints' distance (0); the REST
//!   spec of `cad_measure` asks for the radius.
//! - The label is drawn in the tool bar, not as 3D text at the midpoint
//!   (Bevy's stroke-text gizmo has ASCII only: no "°" or "Ø").
use super::actions::{CadAction, MeasurePick};
use super::display::CadDisplay;
use super::document::{CadDocument, CadTool, SelectMode};
use super::mesh::{CadBody, CadMeshes};
use super::sketch::{CadActivePlane, CadSketches};
use super::snap::{self, Candidate, SnapKind};
use super::topology::{CadTopology, NodeTopology};
use super::transform::{HOT, SNAP_COLOUR, ToolGizmos, cursor_in_view, fl, marker, ray_hit};
use super::view::CadView;
use crate::app::actions::Act;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use serde_json::{Value, json};
use sim_runtime::cad_client::{EdgeInfo, FaceInfo, SelectionItem};

/// A measurement as RoboCAD's `Measurement` (document.py:179): kind
/// (distance | radius | angle), points (mm), value (mm or degrees), label.
#[derive(Clone, Debug, PartialEq)]
pub struct Measurement {
    pub kind: &'static str,
    pub points: Vec<[f64; 3]>,
    pub value: f64,
    pub label: String,
}
impl Measurement {
    /// As `cad_measure` answers it and `add_measurement` takes it
    /// (`Measurement.from_json`: kind, points, value, label).
    pub fn json(&self) -> Value {
        json!({"kind": self.kind, "points": self.points, "value": self.value, "label": self.label})
    }
}

/// The measure tool's picks and its last result.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeasureState {
    /// The first pick, held until the second.
    pub first: Option<MeasurePick>,
    /// The last measurement (shown until the tool changes).
    pub last: Option<Measurement>,
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}
/// RoboCAD's `v_unit` (a zero vector stays zero).
fn unit(a: [f64; 3]) -> [f64; 3] {
    let n = norm(a);
    if n > 0.0 { [a[0] / n, a[1] / n, a[2] / n] } else { a }
}

/// `measure_points`: the distance between two points.
pub fn points(a: [f64; 3], b: [f64; 3]) -> Measurement {
    let d = norm(sub(a, b));
    Measurement { kind: "distance", points: vec![a, b], value: d, label: format!("{d:.3} mm") }
}

/// `measure_radius` of an edge: centre (or the origin) to the midpoint (or the centre).
pub fn edge_radius(e: &EdgeInfo) -> Option<Measurement> {
    let r = e.radius?;
    let center = e.center.unwrap_or([0.0, 0.0, 0.0]);
    let at = e.midpoint.unwrap_or(center);
    Some(Measurement { kind: "radius", points: vec![center, at], value: r, label: format!("R {r:.3} mm  (Ø {:.3})", 2.0 * r) })
}

/// `measure_angle_faces`: the angle between two faces' normals.
pub fn face_angle(a: &FaceInfo, b: &FaceInfo) -> Option<Measurement> {
    let c = dot(unit(a.normal?), unit(b.normal?)).clamp(-1.0, 1.0);
    let angle = c.acos().to_degrees();
    Some(Measurement { kind: "angle", points: vec![a.centroid?, b.centroid?], value: angle, label: format!("{angle:.2}°") })
}

/// `measure_angle_edges`: the angle between two line edges (0..90°).
pub fn edge_angle(a: &EdgeInfo, b: &EdgeInfo) -> Option<Measurement> {
    let da = unit(sub(a.end?, a.start?));
    let db = unit(sub(b.end?, b.start?));
    let angle = dot(da, db).abs().clamp(-1.0, 1.0).acos().to_degrees();
    Some(Measurement { kind: "angle", points: vec![a.midpoint?, b.midpoint?], value: angle, label: format!("{angle:.2}°") })
}

/// `face_distance`: two parallel planar faces' distance along the first's normal.
pub fn face_distance(a: &FaceInfo, b: &FaceInfo) -> Option<f64> {
    Some(dot(sub(b.centroid?, a.centroid?), unit(a.normal?)).abs())
}

/// RoboCAD's parallel test (`abs(abs(n_a · n_b) − 1) < 1e-3`, raw normals).
pub fn parallel(a: &FaceInfo, b: &FaceInfo) -> bool {
    match (a.normal, b.normal) {
        (Some(na), Some(nb)) => (dot(na, nb).abs() - 1.0).abs() < 1e-3,
        _ => false,
    }
}

fn not_loaded(name: &str) -> String {
    format!("the faces and edges of {name} are not loaded yet (they are fetched for the shown revision); measure again in a moment")
}

fn face<'t>(topology: Option<&'t NodeTopology>, name: &str, index: i64) -> Result<&'t FaceInfo, String> {
    topology.ok_or_else(|| not_loaded(name))?.faces.iter().find(|f| f.index == index).ok_or_else(|| format!("{name} has no face {index} at the shown revision"))
}

fn edge<'t>(topology: Option<&'t NodeTopology>, name: &str, index: i64) -> Result<&'t EdgeInfo, String> {
    topology.ok_or_else(|| not_loaded(name))?.edges.iter().find(|e| e.index == index).ok_or_else(|| format!("{name} has no edge {index} at the shown revision"))
}

/// RoboCAD's `measure_between` over the shown revision's topology
/// (`topology(node)`), naming nodes with `name(node)`. An error names what
/// is missing; nothing is guessed.
pub fn between<'t>(a: &MeasurePick, b: &MeasurePick, topology: impl Fn(&str) -> Option<&'t NodeTopology>, name: impl Fn(&str) -> String) -> Result<Measurement, String> {
    let incomplete = |what: &str| format!("RoboCAD's answer for the picked {what} has no position; measure between points instead");
    match (&a.item, &b.item) {
        (Some(SelectionItem(na, ka, ia)), Some(SelectionItem(nb, kb, ib))) if ka == "face" && kb == "face" => {
            let fa = face(topology(na.as_str()), &name(na.as_str()), *ia)?;
            let fb = face(topology(nb.as_str()), &name(nb.as_str()), *ib)?;
            if fa.kind == "plane" && fb.kind == "plane" && parallel(fa, fb) {
                let d = face_distance(fa, fb).ok_or_else(|| incomplete("faces"))?;
                let (ca, cb) = (fa.centroid.ok_or_else(|| incomplete("faces"))?, fb.centroid.ok_or_else(|| incomplete("faces"))?);
                return Ok(Measurement { kind: "distance", points: vec![ca, cb], value: d, label: format!("{d:.3} mm") });
            }
            face_angle(fa, fb).ok_or_else(|| incomplete("faces"))
        }
        (Some(SelectionItem(na, ka, ia)), Some(SelectionItem(nb, kb, ib))) if ka == "edge" && kb == "edge" => {
            let ea = edge(topology(na.as_str()), &name(na.as_str()), *ia)?;
            if na == nb && ia == ib && let Some(m) = edge_radius(ea) {
                return Ok(m);
            }
            let eb = edge(topology(nb.as_str()), &name(nb.as_str()), *ib)?;
            if ea.kind == "line" && eb.kind == "line" {
                return edge_angle(ea, eb).ok_or_else(|| incomplete("edges"));
            }
            Ok(points(ea.midpoint.ok_or_else(|| incomplete("edges"))?, eb.midpoint.ok_or_else(|| incomplete("edges"))?))
        }
        _ => Ok(points(a.point, b.point)),
    }
}

/// Distance from `p` to the segment `a`–`b` (pixels).
pub fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 > 0.0 { ((p - a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
    (p - (a + ab * t)).length()
}

/// SimSync: the measure tool's hover (snap marker, readout) and presses.
#[allow(clippy::too_many_arguments)]
pub(super) fn tool(
    doc: Option<ResMut<CadDocument>>,
    view: Option<Res<CadView>>,
    topology: Option<Res<CadTopology>>,
    meshes: Option<Res<CadMeshes>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    hover: Option<Res<HoverMap>>,
    nodes: Query<(), With<Node>>,
    mut cast: MeshRayCast,
    bodies: Query<&CadBody>,
    mut out: MessageWriter<Act<CadAction>>,
    (plane, sketches, display): (Option<Res<CadActivePlane>>, Option<Res<CadSketches>>, Option<Res<CadDisplay>>),
    mut cache: Local<Option<((u64, u64, u64), Vec<Candidate>)>>,
) {
    let (Some(mut doc), Some(view), Some(topology), Some(meshes)) = (doc, view, topology, meshes) else { return };
    if doc.tool != CadTool::Measure {
        return;
    }
    let Some(cursor) = cursor_in_view(windows.single().ok(), &view, hover.as_deref(), &nodes) else { return };
    // The candidates change only with the topology, the drawn bodies or the sketches.
    let key = snap::candidates_key(Some(&*topology), Some(&*meshes), sketches.as_deref());
    if cache.as_ref().is_none_or(|(k, _)| *k != key) {
        *cache = Some((key, snap::drawn_candidates(&doc, Some(&*topology), Some(&*meshes), sketches.as_deref())));
    }
    let candidates: &[Candidate] = match cache.as_ref() {
        Some((_, c)) => c,
        None => &[],
    };
    let held = |pair: [KeyCode; 2]| keys.as_ref().is_some_and(|k| k.any_pressed(pair));
    let alt = held([KeyCode::AltLeft, KeyCode::AltRight]);
    let shift = held([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    // RoboCAD's `MeasureTool` snaps with no plane of its own: the active plane only with 2D snapping on.
    let on = snap::snap_plane(plane.as_deref());
    let Some(s) = snap::snap_on(&view, cursor, candidates, alt, on.as_ref()) else { return };
    let readout = match &doc.tool_state.measure.first {
        Some(first) => format!("{}  ({})", fl(norm(sub(s.exact, first.point))), s.kind.name()),
        None => s.kind.name().to_string(),
    };
    // Written only on change (the bar refreshes on what it shows).
    if doc.tool_state.snap.as_ref() != Some(&s) {
        doc.tool_state.snap = Some(s.clone());
    }
    if doc.tool_state.readout.as_deref() != Some(readout.as_str()) {
        doc.tool_state.readout = Some(readout);
    }
    if !buttons.is_some_and(|b| b.just_pressed(MouseButton::Left)) {
        return;
    }
    // The hover skips projection while the active plane node is being read; a press refuses (no off-plane point).
    if let Err(why) = snap::press_snap_plane(plane.as_deref()) {
        doc.show(Err(why));
        return;
    }
    let hit = ray_hit(&doc, &mut cast, &view, cursor, &bodies);
    let shown = doc.shown_revision();
    if doc.select_mode == SelectMode::Face
        && let Some(h) = &hit
        && meshes.drawn_revision(&h.node).is_some_and(|r| r != shown)
    {
        let name = doc.node_name(&h.node);
        doc.show(Err(format!("{name} is being redrawn for revision {shown}; press again in a moment")));
        return;
    }
    let item = match doc.select_mode {
        SelectMode::Body => hit.as_ref().map(|h| SelectionItem(h.node.clone(), "body".into(), 0)),
        SelectMode::Face => hit.as_ref().and_then(|h| Some(SelectionItem(h.node.clone(), "face".into(), meshes.face_at(&h.node, h.triangle?, shown)?))),
        // RoboCAD's pick pass (`MeasureTool.press` → `request_pick`): the
        // select click's search, so locked and hidden nodes, edges and
        // vertices behind a surface or cut by the section are not taken.
        SelectMode::Edge | SelectMode::Vertex => {
            let clip = display.as_deref().filter(|d| d.section.enabled).and_then(|d| d.section.plane);
            super::pick::candidates_at(&doc, &meshes, Some(&*topology), &view, cursor, &mut cast, &bodies, clip).into_iter().find(|i| i.1 == doc.select_mode.name())
        }
        SelectMode::Point => None,
    };
    // As `MeasureTool.press`: the snap point unless the snap is free or on the plane, then the surface hit.
    // The snap's f64 point (a vertex exactly as the topology gave it), as `add_measurement` keeps it.
    let point = match &hit {
        Some(h) if matches!(s.kind, SnapKind::Free | SnapKind::Plane) => [f64::from(h.point.x), f64::from(h.point.y), f64::from(h.point.z)],
        _ => s.exact,
    };
    let pick = MeasurePick { item, point };
    match doc.tool_state.measure.first.take() {
        None => doc.tool_state.measure.first = Some(pick),
        Some(a) => {
            out.write(Act::ui(CadAction::CadMeasure { a, b: pick, keep: shift }));
        }
    }
}

/// Present: the snap marker, the held first pick and the line to the
/// pointer, and the last measurement's points (display only).
pub(super) fn draw(doc: Option<Res<CadDocument>>, view: Option<Res<CadView>>, mut gizmos: Gizmos<ToolGizmos>) {
    let (Some(doc), Some(view)) = (doc, view) else { return };
    if doc.tool != CadTool::Measure || !view.valid {
        return;
    }
    let w = |p: Vec3| view.world_from_model.transform_point3(p);
    let at = |p: [f64; 3]| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    if let Some(m) = &doc.tool_state.measure.last
        && let (Some(a), Some(b)) = (m.points.first(), m.points.last())
    {
        gizmos.line(w(at(*a)), w(at(*b)), SNAP_COLOUR);
        marker(&mut gizmos, &view, at(*a), 6.0, SNAP_COLOUR);
        marker(&mut gizmos, &view, at(*b), 6.0, SNAP_COLOUR);
    }
    if let Some(s) = &doc.tool_state.snap {
        marker(&mut gizmos, &view, s.point, 8.0, SNAP_COLOUR);
        if let Some(first) = &doc.tool_state.measure.first {
            gizmos.line(w(at(first.point)), w(s.point), HOT);
        }
    }
    if let Some(first) = &doc.tool_state.measure.first {
        marker(&mut gizmos, &view, at(first.point), 9.0, HOT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plane(index: i64, centroid: [f64; 3], normal: [f64; 3]) -> FaceInfo {
        FaceInfo { index, kind: "plane".into(), centroid: Some(centroid), normal: Some(normal), ..Default::default() }
    }

    /// A 20 mm block's bottom (0), top (1) and side (2); a circular edge (0)
    /// of radius 3 and two perpendicular line edges (1, 2).
    fn block() -> NodeTopology {
        NodeTopology {
            revision: 4,
            faces: vec![plane(0, [5.0, 5.0, 0.0], [0.0, 0.0, -1.0]), plane(1, [5.0, 5.0, 20.0], [0.0, 0.0, 1.0]), plane(2, [10.0, 5.0, 10.0], [1.0, 0.0, 0.0])],
            edges: vec![
                EdgeInfo { index: 0, kind: "circle".into(), center: Some([0.0, 0.0, 20.0]), radius: Some(3.0), midpoint: Some([3.0, 0.0, 20.0]), ..Default::default() },
                EdgeInfo { index: 1, kind: "line".into(), start: Some([0.0, 0.0, 0.0]), end: Some([10.0, 0.0, 0.0]), midpoint: Some([5.0, 0.0, 0.0]), ..Default::default() },
                EdgeInfo { index: 2, kind: "line".into(), start: Some([0.0, 0.0, 0.0]), end: Some([0.0, 0.0, 20.0]), midpoint: Some([0.0, 0.0, 10.0]), ..Default::default() },
            ],
            vertices: Vec::new(),
        }
    }

    fn pick(kind: &str, index: i64, point: [f64; 3]) -> MeasurePick {
        MeasurePick { item: Some(SelectionItem("b1".into(), kind.into(), index)), point }
    }

    #[test]
    fn measure_kinds_and_labels_follow_robocad() {
        let t = block();
        let topo = |id: &str| (id == "b1").then_some(&t);
        let name = |id: &str| if id == "b1" { "Bracket".to_string() } else { id.to_string() };
        // Two parallel planar faces: their distance along the normal.
        let m = between(&pick("face", 0, [1.0, 1.0, 0.0]), &pick("face", 1, [2.0, 2.0, 20.0]), topo, name).unwrap();
        assert_eq!((m.kind, m.label.as_str(), m.value), ("distance", "20.000 mm", 20.0));
        assert_eq!(m.points, vec![[5.0, 5.0, 0.0], [5.0, 5.0, 20.0]]);
        // Two faces at an angle.
        let m = between(&pick("face", 1, [0.0; 3]), &pick("face", 2, [0.0; 3]), topo, name).unwrap();
        assert_eq!((m.kind, m.label.as_str()), ("angle", "90.00°"));
        // The same circular edge twice: its radius.
        let m = between(&pick("edge", 0, [0.0; 3]), &pick("edge", 0, [0.0; 3]), topo, name).unwrap();
        assert_eq!((m.kind, m.label.as_str(), m.value), ("radius", "R 3.000 mm  (Ø 6.000)", 3.0));
        assert_eq!(m.points, vec![[0.0, 0.0, 20.0], [3.0, 0.0, 20.0]]);
        // Two line edges: the angle between them.
        let m = between(&pick("edge", 1, [0.0; 3]), &pick("edge", 2, [0.0; 3]), topo, name).unwrap();
        assert_eq!((m.kind, m.label.as_str()), ("angle", "90.00°"));
        // A circle and a line: the midpoints' distance.
        let m = between(&pick("edge", 0, [0.0; 3]), &pick("edge", 1, [0.0; 3]), topo, name).unwrap();
        assert_eq!(m.kind, "distance");
        assert!((m.value - (4.0f64 + 400.0).sqrt()).abs() < 1e-9);
        // Anything else: the points' distance.
        let a = MeasurePick { item: None, point: [0.0, 0.0, 0.0] };
        let b = MeasurePick { item: Some(SelectionItem("b1".into(), "vertex".into(), 3)), point: [12.0, 0.0, 0.0] };
        let m = between(&a, &b, topo, name).unwrap();
        assert_eq!((m.kind, m.label.as_str()), ("distance", "12.000 mm"));
        assert_eq!(m.json()["label"], "12.000 mm");
        // A face whose topology is not loaded is refused by name.
        let missing = MeasurePick { item: Some(SelectionItem("b9".into(), "face".into(), 0)), point: [0.0; 3] };
        let e = between(&missing, &pick("face", 0, [0.0; 3]), topo, name).unwrap_err();
        assert!(e.contains("b9") && e.contains("not loaded"), "{e}");
        let e = between(&pick("face", 7, [0.0; 3]), &pick("face", 0, [0.0; 3]), topo, name).unwrap_err();
        assert!(e.contains("Bracket has no face 7"), "{e}");
    }

    /// The screen distance the gizmo's hit test uses. (The measure tool's
    /// edge and vertex picks are the select click's search, tested in `pick`.)
    #[test]
    fn segment_distance_is_to_the_nearest_point_of_the_segment() {
        assert_eq!(segment_distance(Vec2::new(0.0, 1.0), Vec2::ZERO, Vec2::new(2.0, 0.0)), 1.0);
        assert_eq!(segment_distance(Vec2::new(5.0, 0.0), Vec2::ZERO, Vec2::new(2.0, 0.0)), 3.0);
    }
}

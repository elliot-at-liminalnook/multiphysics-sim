//! Snapping for CAD mode's tools, a port of RoboCAD's `Viewport.snap`
//! (cad/robocad/ui/viewport.py:1369-1435): the best snap under the cursor
//! among the drawn bodies' B-rep vertices, edge "midpoints" (the middle
//! sample of each edge polyline, `seg[len // 2]`) and edge centres, then
//! the visible sketches' endpoints (line, polyline, spline and control
//! points) and curve centres ([`sketch_candidates`]), within
//! [`SNAP_PIXELS`] of the cursor. With a plane ([`snap_on`]: a tool's
//! `want_plane`, or the active plane while 2D snapping is on) the best snap
//! is projected onto it; with none, the cursor ray meets the plane: the
//! [`GRID_STEP`] grid in the plane's own (u, v) within the same radius
//! ("grid"), else the hit ("plane"). Then the grid on the XY ground plane
//! ("grid"); else the cursor ray's hit on the ground plane ("free"). Alt
//! suppresses every snap (`suppress`). Every candidate competes on screen
//! distance alone, as RoboCAD's loop does (a strictly nearer one replaces
//! the best; on a tie the earlier wins: bodies' vertex, midpoint, centre,
//! then sketches' endpoints and centres).
//!
//! Pure functions over [`CadView`], the topology and the sketch cache;
//! display only. The measure tool (`measure`), the cursor snap and the
//! primitives (`ops::interact`), the plane tools (`sketch::plane`) and the
//! sketch tools read it. Measure and the cursor snap pass the active plane
//! only while 2D snapping is on ([`snap_plane`], RoboCAD's
//! `plane_snapping`); a primitive passes the active plane (or XY) always,
//! as RoboCAD's `PrimitiveTool` passes `want_plane`.
//!
//! Deliberately different from RoboCAD, each recorded:
//! - Centre snaps work here. RoboCAD reads `getattr(item, "centers", [])`
//!   (viewport.py:1388), but `RenderItem` has no `centers` (the list built
//!   at viewport.py:1640-1654 is only stored on the function), so its centre
//!   snap never fires; the epic asks for centres, from `EdgeInfo::center`.
//! - Sketch candidates are read at the shown revision only
//!   (`CadSketches::sketch`): while a sketch is refetched after an edit its
//!   endpoints are not offered, rather than snapping to where they were.
//! - RoboCAD's grid steps also need `show_grid`; the viewer has no grid
//!   toggle, so the grid snap is always on.
//! - A ray that misses the ground plane gives a point along the ray at the
//!   distance of the model origin (RoboCAD uses its camera distance, which
//!   the view snapshot does not carry).
use super::document::CadDocument;
use super::mesh::CadMeshes;
use super::sketch::{CadActivePlane, CadSketches};
use super::topology::{CadTopology, NodeTopology};
use super::view::{CadView, ray_plane};
use bevy::prelude::*;
use sim_runtime::cad_client::{PlaneFrame, SketchGeometry};

/// RoboCAD's `snap_pixels` (viewport.py:300).
pub const SNAP_PIXELS: f32 = 12.0;
/// RoboCAD's `grid_step` (viewport.py:284), mm. Ctrl-snapping drags use it too.
pub const GRID_STEP: f32 = 10.0;

/// What a snap landed on (RoboCAD's `SnapResult.kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapKind {
    Vertex,
    Midpoint,
    Center,
    /// A sketch curve's point (RoboCAD's "endpoint").
    Endpoint,
    Grid,
    /// The cursor ray's hit on the snap plane (RoboCAD's "plane").
    Plane,
    Free,
}
impl SnapKind {
    pub fn name(self) -> &'static str {
        match self {
            SnapKind::Vertex => "vertex",
            SnapKind::Midpoint => "midpoint",
            SnapKind::Center => "center",
            SnapKind::Endpoint => "endpoint",
            SnapKind::Grid => "grid",
            SnapKind::Plane => "plane",
            SnapKind::Free => "free",
        }
    }
}

/// A snap: the point (mm, RoboCAD's frame), its kind and the node it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Snap {
    pub point: Vec3,
    pub kind: SnapKind,
    pub node: Option<String>,
}
impl Snap {
    /// RoboCAD's `_snap_marker` readout (app.py:552-555): "vertex  (1.00, 2.00, 3.00)".
    pub fn readout(&self) -> String {
        format!("{}  ({:.2}, {:.2}, {:.2})", self.kind.name(), self.point.x, self.point.y, self.point.z)
    }
}

/// One snap candidate of a drawn body.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub point: Vec3,
    pub kind: SnapKind,
    pub node: String,
}

fn vec(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

fn arr(p: Vec3) -> [f64; 3] {
    [f64::from(p.x), f64::from(p.y), f64::from(p.z)]
}

/// The candidates of these nodes, in RoboCAD's order per node: vertices,
/// then each edge polyline's middle sample, then edge centres. Nodes are
/// taken in id order so ties resolve the same way every frame.
pub fn candidates<'a>(nodes: impl IntoIterator<Item = (&'a str, &'a NodeTopology)>) -> Vec<Candidate> {
    let mut nodes: Vec<(&str, &NodeTopology)> = nodes.into_iter().collect();
    nodes.sort_by(|a, b| a.0.cmp(b.0));
    let mut out = Vec::new();
    for (id, t) in nodes {
        let push = |out: &mut Vec<Candidate>, p: [f64; 3], kind: SnapKind| out.push(Candidate { point: vec(p), kind, node: id.to_string() });
        for v in &t.vertices {
            if let Some(p) = v.point {
                push(&mut out, p, SnapKind::Vertex);
            }
        }
        for e in &t.edges {
            if e.points.len() >= 2 {
                push(&mut out, e.points[e.points.len() / 2], SnapKind::Midpoint);
            }
        }
        for e in &t.edges {
            if let Some(c) = e.center {
                push(&mut out, c, SnapKind::Center);
            }
        }
    }
    out
}

/// The sketch candidates, after the bodies' (viewport.py:1393-1404): each
/// sketch's line, polyline, spline and control curves' points as
/// "endpoint", then each curve's centre as "center", per curve, through
/// the sketch's plane to the model frame. A sketch whose plane is unknown
/// gives none. Sketches in the order given (the shown tree's).
pub fn sketch_candidates<'a>(sketches: impl IntoIterator<Item = (&'a str, &'a SketchGeometry)>) -> Vec<Candidate> {
    let mut out = Vec::new();
    for (id, sketch) in sketches {
        let Some(plane) = sketch.plane else { continue };
        let world = |p: [f64; 2]| vec(plane.to_world(p[0], p[1], 0.0));
        for c in &sketch.curves {
            if matches!(c.kind.as_str(), "line" | "polyline" | "spline" | "control") {
                out.extend(c.points.iter().map(|p| Candidate { point: world(*p), kind: SnapKind::Endpoint, node: id.to_string() }));
            }
            if let Some(centre) = c.center {
                out.push(Candidate { point: world(centre), kind: SnapKind::Center, node: id.to_string() });
            }
        }
    }
    out
}

/// The visible sketch nodes of the shown tree with their geometry at the
/// shown revision, in tree order (RoboCAD's `doc.nodes` order).
pub(in crate::cad) fn shown_sketches<'a>(doc: &'a CadDocument, cache: &'a CadSketches) -> Vec<(&'a str, &'a SketchGeometry)> {
    let Some(state) = &doc.doc else { return Vec::new() };
    state.nodes.iter().filter(|n| n.kind == "sketch" && n.effective_visible).filter_map(|n| cache.sketch(&n.id).map(|g| (n.id.as_str(), &**g))).collect()
}

/// What the drawn candidates depend on: the topology's, meshes' and sketch cache's epochs.
pub(in crate::cad) fn candidates_key(topology: Option<&CadTopology>, meshes: Option<&CadMeshes>, sketches: Option<&CadSketches>) -> (u64, u64, u64) {
    (topology.map_or(0, |t| t.epoch), meshes.map_or(0, |m| m.epoch), sketches.map_or(0, |s| s.epoch))
}

/// Every snap candidate in view: the drawn bodies' ([`candidates`]), then
/// the visible sketches' ([`sketch_candidates`]), RoboCAD's order.
pub(in crate::cad) fn drawn_candidates(doc: &CadDocument, topology: Option<&CadTopology>, meshes: Option<&CadMeshes>, sketches: Option<&CadSketches>) -> Vec<Candidate> {
    let mut out = match (topology, meshes) {
        (Some(t), Some(m)) => candidates(t.ready().filter(|(id, _)| m.shown(id)).map(|(id, t)| (id.as_str(), &**t))),
        _ => Vec::new(),
    };
    if let Some(cache) = sketches {
        out.extend(sketch_candidates(shown_sketches(doc, cache)));
    }
    out
}

/// The plane a hover or pick snaps onto without a tool's own plane:
/// RoboCAD's `active_plane if plane_snapping else None`. A plane node whose
/// frame is still being read gives none.
pub(in crate::cad) fn snap_plane(plane: Option<&CadActivePlane>) -> Option<PlaneFrame> {
    plane.filter(|p| p.snap_2d).and_then(|p| p.frame().ok().flatten())
}

/// The best snap under window pixel `cursor` with no plane (see the module
/// doc). None only when the view is not ready.
pub fn snap(view: &CadView, cursor: Vec2, candidates: &[Candidate], suppress: bool) -> Option<Snap> {
    snap_on(view, cursor, candidates, suppress, None)
}

/// The best snap under window pixel `cursor` (see the module doc), on
/// `plane` when given (RoboCAD's `want_plane`, or the active plane with 2D
/// snapping on). None only when the view is not ready.
pub fn snap_on(view: &CadView, cursor: Vec2, candidates: &[Candidate], suppress: bool, plane: Option<&PlaneFrame>) -> Option<Snap> {
    let (origin, dir) = view.ray(cursor)?;
    if !suppress {
        let mut best: Option<&Candidate> = None;
        let mut best_d = SNAP_PIXELS;
        for c in candidates {
            let Some(sp) = view.project(c.point) else { continue };
            let d = (sp - cursor).length();
            if d < best_d {
                best = Some(c);
                best_d = d;
            }
        }
        if let Some(c) = best {
            // On a plane, the best snap is projected onto it (viewport.py:1405-1406).
            let point = plane.map_or(c.point, |p| vec(p.project(arr(c.point))));
            return Some(Snap { point, kind: c.kind, node: Some(c.node.clone()) });
        }
    }
    if let Some(p) = plane
        && let Some(hit) = ray_plane(origin, dir, vec(p.origin), vec(p.normal))
    {
        if !suppress {
            // The grid in the plane's own coordinates (viewport.py:1413-1418), half to even.
            let [u, v, _] = p.to_local(arr(hit));
            let step = f64::from(GRID_STEP);
            let grid = vec(p.to_world((u / step).round_ties_even() * step, (v / step).round_ties_even() * step, 0.0));
            if view.project(grid).is_some_and(|sp| (sp - cursor).length() < SNAP_PIXELS) {
                return Some(Snap { point: grid, kind: SnapKind::Grid, node: None });
            }
        }
        return Some(Snap { point: hit, kind: SnapKind::Plane, node: None });
    }
    if let Some(hit) = ray_plane(origin, dir, Vec3::ZERO, Vec3::Z) {
        if !suppress {
            // Python's `round` (half to even), as RoboCAD.
            let grid = Vec3::new((hit.x / GRID_STEP).round_ties_even() * GRID_STEP, (hit.y / GRID_STEP).round_ties_even() * GRID_STEP, 0.0);
            if view.project(grid).is_some_and(|sp| (sp - cursor).length() < SNAP_PIXELS) {
                return Some(Snap { point: grid, kind: SnapKind::Grid, node: None });
            }
        }
        return Some(Snap { point: hit, kind: SnapKind::Free, node: None });
    }
    let distance = (-origin).dot(dir).max(1.0);
    Some(Snap { point: origin + dir * distance, kind: SnapKind::Free, node: None })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use bevy::camera::CameraProjection;
    use bevy::math::Affine3A;
    use sim_runtime::cad_client::{EdgeInfo, VertexInfo};

    /// A camera `height_mm` above the model origin looking straight down
    /// (model +Y is up on screen, +X right), over a 200 × 100 px view at
    /// (10, 20): the view centre (110, 70) is the model origin. At 200 mm
    /// one pixel is about 1.657 mm.
    pub(crate) fn top_view(height_mm: f32) -> CadView {
        // CAD mode's camera near plane (scene.rs): 1 mm, so a close camera's rays start above the ground.
        let projection = PerspectiveProjection { aspect_ratio: 2.0, near: 0.001, ..default() };
        let clip_from_view = projection.get_clip_from_view();
        // Bevy's −Z (the camera's forward) turned to −Y (down); its up to −Z (model +Y).
        let world_from_view = Affine3A::from_rotation_translation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2), Vec3::new(0.0, height_mm * 0.001, 0.0));
        let root = super::super::mesh::root_transform();
        let world_from_model = Affine3A::from_scale_rotation_translation(root.scale, root.rotation, root.translation);
        CadView {
            valid: true,
            world_from_model,
            model_from_world: world_from_model.inverse(),
            view_from_world: world_from_view.inverse(),
            world_from_view,
            clip_from_view,
            view_from_clip: clip_from_view.inverse(),
            min: Vec2::new(10.0, 20.0),
            size: Vec2::new(200.0, 100.0),
        }
    }

    fn topology() -> NodeTopology {
        NodeTopology {
            revision: 1,
            faces: Vec::new(),
            // A straight edge whose middle sample is (14, 0, 0), and a circle centred at (0, 30, 0).
            edges: vec![
                EdgeInfo { index: 0, kind: "line".into(), points: vec![[10.0, 0.0, 0.0], [14.0, 0.0, 0.0], [18.0, 0.0, 0.0]], ..Default::default() },
                EdgeInfo { index: 1, kind: "circle".into(), center: Some([0.0, 30.0, 0.0]), radius: Some(3.0), ..Default::default() },
            ],
            vertices: vec![VertexInfo { index: 0, point: Some([2.0, 0.0, 0.0]) }],
        }
    }

    #[test]
    fn the_nearest_candidate_wins_whatever_its_kind_and_alt_suppresses_every_snap() {
        let view = top_view(200.0);
        let t = topology();
        let cands = candidates([("n1", &t)]);
        assert_eq!(cands.iter().map(|c| c.kind).collect::<Vec<_>>(), vec![SnapKind::Vertex, SnapKind::Midpoint, SnapKind::Center]);
        // At the origin: the vertex (≈1.2 px) beats the midpoint (≈8.4 px).
        let centre = view.project(Vec3::ZERO).unwrap();
        let s = snap(&view, centre, &cands, false).unwrap();
        assert_eq!((s.kind, s.node.as_deref()), (SnapKind::Vertex, Some("n1")));
        assert_eq!(s.point, Vec3::new(2.0, 0.0, 0.0));
        // Over (13, 0, 0): the midpoint is nearer than the vertex.
        let near_mid = view.project(Vec3::new(13.0, 0.0, 0.0)).unwrap();
        assert_eq!(snap(&view, near_mid, &cands, false).unwrap().kind, SnapKind::Midpoint);
        // Over the circle's centre.
        let over_centre = view.project(Vec3::new(0.0, 29.0, 0.0)).unwrap();
        assert_eq!(snap(&view, over_centre, &cands, false).unwrap().kind, SnapKind::Center);
        // Alt: no candidate and no grid, the ground-plane hit under the cursor.
        let free = snap(&view, near_mid, &cands, true).unwrap();
        assert_eq!(free.kind, SnapKind::Free);
        assert!((free.point - Vec3::new(13.0, 0.0, 0.0)).length() < 0.05, "{:?}", free.point);
        assert!(free.readout().starts_with("free  (13.0"), "{}", free.readout());
    }

    #[test]
    fn grid_then_free_on_the_ground_plane() {
        // Far from every candidate: the 10 mm grid point within 12 px.
        let view = top_view(200.0);
        let t = topology();
        let cands = candidates([("n1", &t)]);
        let p = view.project(Vec3::new(101.0, -49.0, 0.0)).unwrap();
        let s = snap(&view, p, &cands, false).unwrap();
        assert_eq!(s.kind, SnapKind::Grid);
        assert!((s.point - Vec3::new(100.0, -50.0, 0.0)).length() < 1e-3, "{:?}", s.point);
        // Zoomed in (10 mm above: ≈0.083 mm a pixel) a grid point 5 mm away is ≈60 px off: free.
        let close = top_view(10.0);
        let p = close.project(Vec3::new(5.0, 5.0, 0.0)).unwrap();
        let s = snap(&close, p, &[], false).unwrap();
        assert_eq!(s.kind, SnapKind::Free);
        assert!((s.point - Vec3::new(5.0, 5.0, 0.0)).length() < 0.01, "{:?}", s.point);
    }

    /// Sketch endpoints and centres come after the bodies', through the
    /// sketch's plane; with a plane the best snap is projected onto it.
    #[test]
    fn sketch_endpoints_and_the_plane_step() {
        use sim_runtime::cad_client::{SketchCurve, SketchGeometry};
        let view = top_view(200.0);
        // A sketch on XY offset 5 mm up: a line from (40, 0) to (60, 0), a circle centred at (-40, 0).
        let plane = PlaneFrame { origin: [0.0, 0.0, 5.0], ..PlaneFrame::XY };
        let sketch = SketchGeometry {
            plane: Some(plane),
            curves: vec![
                SketchCurve { kind: "line".into(), points: vec![[40.0, 0.0], [60.0, 0.0]], ..Default::default() },
                SketchCurve { kind: "circle".into(), center: Some([-40.0, 0.0]), radius: 3.0, ..Default::default() },
            ],
            ..Default::default()
        };
        let t = topology();
        let mut cands = candidates([("n1", &t)]);
        cands.extend(sketch_candidates([("k1", &sketch)]));
        let kinds: Vec<SnapKind> = cands.iter().map(|c| c.kind).collect();
        assert_eq!(kinds, vec![SnapKind::Vertex, SnapKind::Midpoint, SnapKind::Center, SnapKind::Endpoint, SnapKind::Endpoint, SnapKind::Center]);
        assert_eq!(cands[3].point, Vec3::new(40.0, 0.0, 5.0));
        let s = snap(&view, view.project(Vec3::new(41.0, 0.0, 5.0)).unwrap(), &cands, false).unwrap();
        assert_eq!((s.kind, s.node.as_deref(), s.point), (SnapKind::Endpoint, Some("k1"), Vec3::new(40.0, 0.0, 5.0)));
        assert_eq!(s.kind.name(), "endpoint");
        // On the XY plane the endpoint is projected onto it.
        let s = snap_on(&view, view.project(Vec3::new(41.0, 0.0, 5.0)).unwrap(), &cands, false, Some(&PlaneFrame::XY)).unwrap();
        assert_eq!((s.kind, s.point), (SnapKind::Endpoint, Vec3::new(40.0, 0.0, 0.0)));
        // Nothing near on a plane 20 mm up: the grid in its coordinates, else the hit ("plane").
        let up = PlaneFrame { origin: [0.0, 0.0, 20.0], ..PlaneFrame::XY };
        let s = snap_on(&view, view.project(Vec3::new(101.0, -49.0, 20.0)).unwrap(), &cands, false, Some(&up)).unwrap();
        assert_eq!(s.kind, SnapKind::Grid);
        assert!((s.point - Vec3::new(100.0, -50.0, 20.0)).length() < 1e-3, "{:?}", s.point);
        let s = snap_on(&view, view.project(Vec3::new(101.0, -49.0, 20.0)).unwrap(), &cands, true, Some(&up)).unwrap();
        assert_eq!((s.kind, s.kind.name()), (SnapKind::Plane, "plane"));
        assert!((s.point - Vec3::new(101.0, -49.0, 20.0)).length() < 0.05, "{:?}", s.point);
        // 2D snapping passes the active plane only while it is on.
        let mut active = CadActivePlane { plane: Some(crate::cad::sketch::ActivePlane::Base(crate::cad::sketch::BasePlane::Xz)), ..Default::default() };
        assert_eq!(snap_plane(Some(&active)), None);
        active.snap_2d = true;
        assert_eq!(snap_plane(Some(&active)), Some(PlaneFrame::XZ));
    }
}

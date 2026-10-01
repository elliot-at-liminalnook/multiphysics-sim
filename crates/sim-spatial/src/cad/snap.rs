//! Snapping for CAD mode's tools, a port of RoboCAD's `Viewport.snap`
//! (cad/robocad/ui/viewport.py:1369-1435): the best snap under the cursor
//! among the drawn bodies' B-rep vertices, edge "midpoints" (the middle
//! sample of each edge polyline, `seg[len // 2]`) and edge centres, within
//! [`SNAP_PIXELS`] of the cursor; then the [`GRID_STEP`] grid on the XY
//! ground plane within the same radius ("grid"); else the cursor ray's hit
//! on the ground plane ("free"). Alt suppresses every snap (`suppress`).
//! Every candidate competes on screen distance alone, as RoboCAD's loop
//! does (a strictly nearer one replaces the best; on a tie the earlier in
//! vertex, midpoint, centre order wins).
//!
//! Pure functions over [`CadView`] and the topology; display only. The
//! measure tool (`measure`) reads it on hover and press, with the marker
//! and the readout RoboCAD's `_snap_marker` shows ("vertex  (x, y, z)").
//!
//! Deliberately different from RoboCAD, each recorded:
//! - Centre snaps work here. RoboCAD reads `getattr(item, "centers", [])`
//!   (viewport.py:1388), but `RenderItem` has no `centers` (the list built
//!   at viewport.py:1640-1654 is only stored on the function), so its centre
//!   snap never fires; the epic asks for centres, from `EdgeInfo::center`.
//! - The active-plane step ("plane", and grid on that plane) is cad-sketch's:
//!   this epic has no active plane, so the ground plane is the only plane.
//! - Sketch endpoints are not candidates (no sketches are drawn yet).
//! - A ray that misses the ground plane gives a point along the ray at the
//!   distance of the model origin (RoboCAD uses its camera distance, which
//!   the view snapshot does not carry).
use super::topology::NodeTopology;
use super::view::{CadView, ray_plane};
use bevy::prelude::*;

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
    Grid,
    Free,
}
impl SnapKind {
    pub fn name(self) -> &'static str {
        match self {
            SnapKind::Vertex => "vertex",
            SnapKind::Midpoint => "midpoint",
            SnapKind::Center => "center",
            SnapKind::Grid => "grid",
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

/// The best snap under window pixel `cursor` (see the module doc). None
/// only when the view is not ready.
pub fn snap(view: &CadView, cursor: Vec2, candidates: &[Candidate], suppress: bool) -> Option<Snap> {
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
            return Some(Snap { point: c.point, kind: c.kind, node: Some(c.node.clone()) });
        }
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
}

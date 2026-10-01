//! Sub-body selection (cad-select-transform, native-viewer.md "CAD selection
//! and transform"): the handler of the selection actions, the selection
//! commands and the push to RoboCAD. RoboCAD is the reference:
//! `ui/tools.py` `SelectTool` (click, Shift extends, Ctrl toggles, the box),
//! `ui/app.py` `set_selection_mode`, `select_all`, `invert_selection`,
//! `select_same_material`, `convert_edges_to_faces`, and `document.py`
//! `same_material`.
//!
//! - **One handler** ([`handle`], called from `actions::handle`) for
//!   `CadSelect`, `CadSelectMode`, `CadHover`, `CadBoxSelect`,
//!   `CadCandidates`, `CadSelectAll`, `CadInvertSelection`,
//!   `CadSelectSameMaterial` and `CadEdgesToFaces`. The 3D clicks
//!   (`pick`), the tree rows, the mode strip and buttons (`overlay`), the
//!   Alt menu, the keys, `system_ui` and REST all write these actions.
//! - **Push** ([`publish`]): after a selection or mode change the panels are
//!   touched and, when connected and changed, the items and the mode go to
//!   RoboCAD's `PUT /selection` (`sync::push_selection`, one at a time).
//!   A hover is display only: it neither touches the panels nor is pushed.
//! - **Status**: as RoboCAD's `selection_changed`, "n selected" (or the
//!   empty status, "Ready") after a selection change; "Selection mode: m"
//!   after a mode switch.
//!
//! Deliberate differences from RoboCAD (recorded):
//! - Edges → faces: RoboCAD asks its kernel (`faces_of_edge`), which has no
//!   REST route. Here the faces come from RoboCAD's drawn tessellation: a
//!   face bounds the edge when one of its triangles has a side whose two
//!   ends and midpoint lie on the edge's sampled polyline within a
//!   tolerance (one sagitta of the polyline plus RoboCAD's tessellation
//!   tolerance, no fraction of the body's size, so a face across a thin
//!   wall is not taken) and that runs along it (parallel to the nearest
//!   polyline segment within 8° plus the polyline's largest turn)
//!   ([`faces_along`]). It needs the node's mesh and topology at
//!   the same revision and refuses by name until both are loaded.
//! - Select Same Material refuses (by name) when nothing is selected or the
//!   first selected node has no material; RoboCAD silently does nothing for
//!   an empty selection and selects every body without a material for one
//!   with none.
//! - `CadCandidates` with no items closes the Alt menu (a click elsewhere
//!   in the 3D view closes it, as a Qt popup does).
//! - Box select runs inline in the handler: one projection of every drawn
//!   body's 8 bounding-box corners (body, face, point modes), or of every
//!   sampled edge point / vertex of the drawn bodies (edge, vertex modes),
//!   once on release. A few hundred thousand projections (a few ms) is
//!   acceptable for one release; a `Pool::Compute` job would need the
//!   handler to answer Pending and a second apply path for a one-shot
//!   gesture.
use super::actions::{CadAction, Cx};
use super::document::{CadDocument, SelectMode};
use super::mesh::CadMeshes;
use super::topology::CadTopology;
use super::view::CadView;
use crate::app::actions::Call;
use bevy::math::DVec3;
use bevy::prelude::*;
use serde_json::{Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::{MESH_TOLERANCE, MeshData, SelectionItem};
use std::collections::HashMap;

/// The kinds a selection item may have: RoboCAD's selection modes, and
/// "curve" (RoboCAD's pick pass names a curve node's sampled segment
/// `[id, "curve", i]`), so REST and an adopted RoboCAD selection agree.
pub const KINDS: [&str; 6] = ["body", "face", "edge", "vertex", "point", "curve"];
/// The node kinds Select All and Invert take (RoboCAD's `select_all`).
pub const SELECTABLE_KINDS: [&str; 5] = ["body", "sheet", "curve", "instance", "mesh"];
/// The node kinds Same Material takes (RoboCAD's `Document.same_material`).
pub const MATERIAL_KINDS: [&str; 2] = ["body", "instance"];
/// Edges → faces: a rounding allowance (mm) on top of the sagitta and
/// RoboCAD's tessellation tolerance.
pub const EDGE_FACE_EPSILON: f64 = 1e-4;
/// Edges → faces: a triangle side runs along the edge when the cosine of its
/// angle to the nearest polyline segment is above this (about 8°), widened
/// by the polyline's largest turn on curves.
pub const EDGE_FACE_PARALLEL: f64 = 0.99;

/// Picking, the overlays, the mode strip and the Alt menu (windowed CAD mode).
pub(super) fn build(app: &mut App) {
    super::pick::build(app);
    super::overlay::build(app);
}

/// The selection actions (`actions::handle` forwards them here).
pub(super) fn handle(action: &CadAction, _call: &mut Call, cx: &mut Cx) -> Outcome {
    let doc = &mut *cx.doc;
    let result = match action {
        CadAction::CadSelect { ids, items, extend, toggle } => select(doc, ids, items, *extend, *toggle),
        CadAction::CadSelectMode { mode } => Ok(set_mode(doc, *mode)),
        CadAction::CadHover { item } => Ok(hover(doc, item.as_ref())),
        CadAction::CadBoxSelect { rect, extend } => match (cx.meshes.as_deref(), cx.view) {
            (Some(meshes), Some(view)) if view.valid => box_select(doc, meshes, cx.topology.as_deref(), view, *rect, *extend),
            _ => Err("box select needs CAD mode's 3D view, which is not shown yet in this window".to_string()),
        },
        CadAction::CadCandidates { items, extend, toggle } => candidates(doc, items, *extend, *toggle),
        CadAction::CadSelectAll => Ok(select_all(doc)),
        CadAction::CadInvertSelection => Ok(invert(doc)),
        CadAction::CadSelectSameMaterial => same_material(doc),
        CadAction::CadEdgesToFaces => edges_to_faces(doc, cx.meshes.as_deref(), cx.topology.as_deref()),
        _ => Err("not a selection action".to_string()),
    };
    Outcome::Done(result)
}

/// After any selection or mode change: the panels refresh and, when
/// connected and RoboCAD's copy differs (or a push is in flight, which then
/// sends the newest once it answers), the items and the mode are pushed.
/// Returns whether a push was started or queued. Part D calls this when a
/// tool switches the mode (push/pull to face mode).
pub(super) fn publish(doc: &mut CadDocument) -> bool {
    doc.touch();
    if !doc.connected() {
        return false;
    }
    if doc.selection_job.is_some() || differs_from_remote(doc) {
        super::sync::push_selection(doc);
        return true;
    }
    false
}

/// Whether RoboCAD's last known selection (or, with a desktop window, its
/// mode) differs from the viewer's.
pub(super) fn differs_from_remote(doc: &CadDocument) -> bool {
    let gui = doc.health.as_ref().is_some_and(|h| h.gui);
    doc.selection != doc.remote_selection || (gui && doc.remote_mode != Some(doc.select_mode))
}

/// The answer every selection change gives.
fn answer(doc: &CadDocument, pushed: bool) -> Value {
    json!({"selection": doc.selection, "mode": doc.select_mode, "pushed": pushed, "connected": doc.connected()})
}

/// RoboCAD's `selection_changed` status line: "n selected", or the empty status.
fn selection_status(doc: &mut CadDocument) {
    let n = doc.selection.len();
    doc.status = if n == 0 { None } else { Some(Ok(format!("{n} selected"))) };
}

/// Check items before they enter the selection: a known kind, a node of the
/// shown tree, a non-negative index. Refusals name the item.
fn validate(doc: &CadDocument, items: &[SelectionItem]) -> Result<(), String> {
    for SelectionItem(node, kind, index) in items {
        if !KINDS.contains(&kind.as_str()) {
            return Err(format!("[{node}, {kind}, {index}]: the kind must be one of body, face, edge, vertex, point, curve"));
        }
        if !doc.has_node(node) {
            return Err(format!("[{node}, {kind}, {index}]: no node {node} in the shown tree"));
        }
        if *index < 0 {
            return Err(format!("[{node}, {kind}, {index}]: the index must not be negative"));
        }
    }
    Ok(())
}

/// Apply `items` to `selection` as RoboCAD's `SelectTool._apply`: Ctrl
/// toggles each (it wins over Shift), Shift appends the ones not selected,
/// otherwise they replace the selection (duplicates dropped).
pub(super) fn combine(selection: &mut Vec<SelectionItem>, items: Vec<SelectionItem>, extend: bool, toggle: bool) {
    if toggle {
        for item in items {
            match selection.iter().position(|s| *s == item) {
                Some(i) => {
                    selection.remove(i);
                }
                None => selection.push(item),
            }
        }
        return;
    }
    if !extend {
        selection.clear();
    }
    for item in items {
        if !selection.contains(&item) {
            selection.push(item);
        }
    }
}

/// `CadSelect`: `ids` as body items, then `items`; replace, extend or toggle.
/// Closes the Alt menu.
pub(super) fn select(doc: &mut CadDocument, ids: &[String], items: &[SelectionItem], extend: bool, toggle: bool) -> Result<Value, String> {
    let mut wanted: Vec<SelectionItem> = ids.iter().map(|id| SelectionItem(id.clone(), "body".into(), 0)).collect();
    wanted.extend(items.iter().cloned());
    validate(doc, &wanted)?;
    let mut selection = std::mem::take(&mut doc.selection);
    combine(&mut selection, wanted, extend, toggle);
    doc.selection = selection;
    doc.candidates = None;
    selection_status(doc);
    let pushed = publish(doc);
    Ok(answer(doc, pushed))
}

/// `CadSelectMode`: RoboCAD's `set_selection_mode` (the selection is cleared).
pub(super) fn set_mode(doc: &mut CadDocument, mode: SelectMode) -> Value {
    doc.select_mode = mode;
    doc.selection.clear();
    doc.hover = None;
    doc.candidates = None;
    doc.status = Some(Ok(format!("Selection mode: {}", mode.name())));
    let pushed = publish(doc);
    answer(doc, pushed)
}

/// `CadHover`: display only, so no `touch` (that would rebuild the panels).
fn hover(doc: &mut CadDocument, item: Option<&SelectionItem>) -> Value {
    if doc.hover.as_ref() != item {
        doc.hover = item.cloned();
    }
    json!({"hover": doc.hover})
}

/// `CadCandidates`: open the Alt menu (no items closes it).
fn candidates(doc: &mut CadDocument, items: &[SelectionItem], extend: bool, toggle: bool) -> Result<Value, String> {
    if items.is_empty() {
        if doc.candidates.take().is_some() {
            doc.touch();
        }
        return Ok(json!({"candidates": 0, "message": "the Alt menu is closed"}));
    }
    validate(doc, items)?;
    doc.candidates = Some(super::document::Candidates { items: items.to_vec(), extend, toggle });
    doc.touch();
    Ok(json!({"candidates": items.len(), "controls": (0..items.len()).map(|n| format!("cad:candidate:{n}")).collect::<Vec<_>>()}))
}

/// The visible selectable nodes (walk order) as body items, optionally
/// leaving out nodes already selected.
fn visible_bodies(doc: &CadDocument, except: &[String]) -> Vec<SelectionItem> {
    let Some(state) = &doc.doc else { return Vec::new() };
    state
        .nodes
        .iter()
        .filter(|n| SELECTABLE_KINDS.contains(&n.kind.as_str()) && n.effective_visible && !except.contains(&n.id))
        .map(|n| SelectionItem(n.id.clone(), "body".into(), 0))
        .collect()
}

/// `CadSelectAll`: RoboCAD's `select_all`.
pub(super) fn select_all(doc: &mut CadDocument) -> Value {
    doc.selection = visible_bodies(doc, &[]);
    doc.candidates = None;
    selection_status(doc);
    let pushed = publish(doc);
    answer(doc, pushed)
}

/// `CadInvertSelection`: RoboCAD's `invert_selection` (by node).
pub(super) fn invert(doc: &mut CadDocument) -> Value {
    let current = doc.selected_nodes();
    doc.selection = visible_bodies(doc, &current);
    doc.candidates = None;
    selection_status(doc);
    let pushed = publish(doc);
    answer(doc, pushed)
}

/// `CadSelectSameMaterial`: RoboCAD's `same_material` on the first selected node.
pub(super) fn same_material(doc: &mut CadDocument) -> Result<Value, String> {
    let Some(first) = doc.selected().map(str::to_string) else {
        return Err("Select Same Material: nothing is selected; select a body first".to_string());
    };
    let Some(state) = &doc.doc else { return Err("Select Same Material: RoboCAD has not sent the document yet".to_string()) };
    let Some(node) = state.nodes.iter().find(|n| n.id == first) else {
        return Err(format!("Select Same Material: no node {first} in the shown tree"));
    };
    let Some(material) = node.material.clone() else {
        return Err(format!("Select Same Material: {} has no material (assign one first)", node.name));
    };
    let same: Vec<SelectionItem> = state
        .nodes
        .iter()
        .filter(|n| MATERIAL_KINDS.contains(&n.kind.as_str()) && n.material.as_deref() == Some(material.as_str()))
        .map(|n| SelectionItem(n.id.clone(), "body".into(), 0))
        .collect();
    doc.selection = same;
    doc.candidates = None;
    selection_status(doc);
    let pushed = publish(doc);
    let mut out = answer(doc, pushed);
    out["material"] = json!(material);
    Ok(out)
}

/// `CadEdgesToFaces`: RoboCAD's `convert_edges_to_faces` from the drawn
/// tessellation (see the module doc); the mode becomes face.
pub(super) fn edges_to_faces(doc: &mut CadDocument, meshes: Option<&CadMeshes>, topology: Option<&CadTopology>) -> Result<Value, String> {
    let edges = doc.selected_of("edge");
    if edges.is_empty() {
        return Err("Edges → faces: no edges are selected (edge mode, E)".to_string());
    }
    let (Some(meshes), Some(topology)) = (meshes, topology) else {
        return Err("Edges → faces needs RoboCAD's tessellation and topology, which this window does not hold (no 3D view)".to_string());
    };
    let mut faces: Vec<SelectionItem> = Vec::new();
    for (node, edge) in &edges {
        let name = doc.node_name(node);
        let Some(topo) = topology.get(node) else {
            return Err(match topology.error(node) {
                Some(e) => format!("Edges → faces: the topology of {name} could not be fetched: {e}"),
                None => format!("Edges → faces: the topology of {name} is still loading; try again in a moment"),
            });
        };
        if meshes.mesh_data(node).is_none() {
            return Err(format!("Edges → faces: the tessellation of {name} is not drawn yet; try again in a moment"));
        }
        match meshes.drawn_revision(node) {
            Some(r) if r == topo.revision => {}
            r => return Err(format!("Edges → faces: the drawn tessellation of {name} is from revision {}, its topology from revision {}; wait for the refetch", r.map_or("none".to_string(), |r| r.to_string()), topo.revision)),
        }
        if !topo.edges.iter().any(|e| e.index == *edge) {
            return Err(format!("Edges → faces: {name} has no edge {edge} at revision {}", topo.revision));
        }
        for f in faces_of_edge(meshes, topology, node, *edge) {
            let item = SelectionItem(node.clone(), "face".into(), f);
            if !faces.contains(&item) {
                faces.push(item);
            }
        }
    }
    let n = faces.len();
    doc.selection = faces;
    doc.select_mode = SelectMode::Face;
    doc.hover = None;
    doc.candidates = None;
    doc.status = Some(Ok(format!("Selection: {} edge{} → {n} face{}", edges.len(), if edges.len() == 1 { "" } else { "s" }, if n == 1 { "" } else { "s" })));
    let pushed = publish(doc);
    let mut out = answer(doc, pushed);
    out["note"] = json!("faces found from RoboCAD's tessellation (triangles with a side along each edge's sampled polyline), not RoboCAD's kernel");
    Ok(out)
}

/// The faces of node `node` that edge `edge` bounds, from the drawn
/// tessellation (see [`faces_along`]); empty when the mesh or topology is
/// not loaded or has no such edge. Part D's live dimensions use it.
pub(super) fn faces_of_edge(meshes: &CadMeshes, topology: &CadTopology, node: &str, edge: i64) -> Vec<i64> {
    let (Some(mesh), Some(topo)) = (meshes.mesh_data(node), topology.get(node)) else { return Vec::new() };
    let Some(info) = topo.edges.iter().find(|e| e.index == edge) else { return Vec::new() };
    faces_along(mesh, &info.points)
}

/// Point-to-segment distance.
fn segment_distance(p: DVec3, a: DVec3, b: DVec3) -> f64 {
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 > 0.0 { ((p - a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
    (p - (a + ab * t)).length()
}

/// Distance from `p` to the polyline `points`.
fn polyline_distance(p: DVec3, points: &[DVec3]) -> f64 {
    match points {
        [] => f64::INFINITY,
        [only] => (p - *only).length(),
        _ => points.windows(2).map(|w| segment_distance(p, w[0], w[1])).fold(f64::INFINITY, f64::min),
    }
}

/// How far the true curve may stray from its sampled polyline (one arc
/// sagitta, L·θ/8 for a segment L long where the curve turns by θ; zero for
/// a straight edge), and the polyline's largest turn between segments (rad).
fn polyline_bend(points: &[DVec3]) -> (f64, f64) {
    let (mut sag, mut most): (f64, f64) = (0.0, 0.0);
    for w in points.windows(3) {
        let (a, b) = (w[1] - w[0], w[2] - w[1]);
        let (la, lb) = (a.length(), b.length());
        if la <= 0.0 || lb <= 0.0 {
            continue;
        }
        let turn = (a.dot(b) / (la * lb)).clamp(-1.0, 1.0).acos();
        sag = sag.max(la.max(lb) * turn / 8.0);
        most = most.max(turn);
    }
    (sag, most)
}

/// The unit direction of the polyline segment nearest `p` (None for a
/// degenerate polyline).
fn nearest_direction(p: DVec3, points: &[DVec3]) -> Option<DVec3> {
    points
        .windows(2)
        .filter_map(|w| (w[1] - w[0]).try_normalize().map(|d| (segment_distance(p, w[0], w[1]), d)))
        .min_by(|x, y| x.0.total_cmp(&y.0))
        .map(|(_, d)| d)
}

/// The B-rep faces (`triangle_face`) with a triangle side along the
/// polyline: both ends and the midpoint within the tolerance (one sagitta
/// of the polyline, RoboCAD's tessellation tolerance and
/// [`EDGE_FACE_EPSILON`]), the side parallel to the nearest segment
/// ([`EDGE_FACE_PARALLEL`], widened by the polyline's largest turn). Sorted by index.
pub(super) fn faces_along(mesh: &MeshData, polyline: &[[f64; 3]]) -> Vec<i64> {
    let points: Vec<DVec3> = polyline.iter().map(|p| DVec3::from_array(*p)).collect();
    if points.len() < 2 {
        return Vec::new();
    }
    let vertices: Vec<DVec3> = mesh.vertices.iter().map(|v| DVec3::from_array(*v)).collect();
    let (sag, turn) = polyline_bend(&points);
    let tolerance = sag + MESH_TOLERANCE + EDGE_FACE_EPSILON;
    let parallel = (EDGE_FACE_PARALLEL.acos() + turn).min(std::f64::consts::FRAC_PI_2).cos();
    let near = |p: DVec3| polyline_distance(p, &points) <= tolerance;
    let on: Vec<bool> = vertices.iter().map(|v| near(*v)).collect();
    let mut faces: Vec<i64> = Vec::new();
    for (t, tri) in mesh.triangles.iter().enumerate() {
        let Some(&face) = mesh.triangle_face.get(t) else { continue };
        if face < 0 || faces.contains(&face) {
            continue;
        }
        for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
            let (a, b) = (a as usize, b as usize);
            if !(on.get(a).copied().unwrap_or(false) && on.get(b).copied().unwrap_or(false)) {
                continue;
            }
            let (pa, pb) = (vertices[a], vertices[b]);
            let mid = (pa + pb) / 2.0;
            let Some(side) = (pb - pa).try_normalize() else { continue };
            if !near(mid) || nearest_direction(mid, &points).is_none_or(|d| d.dot(side).abs() < parallel) {
                continue;
            }
            faces.push(face);
            break;
        }
    }
    faces.sort_unstable();
    faces
}

/// Whether window point `p` lies in the rectangle (inclusive, as RoboCAD's).
fn inside(rect: [f32; 4], p: Vec2) -> bool {
    rect[0] <= p.x && p.x <= rect[2] && rect[1] <= p.y && p.y <= rect[3]
}

/// What box select finds in `rect` (sorted `[x0, y0, x1, y1]`) in the
/// current mode (RoboCAD's `_box_select`), in tree order, and the drawn
/// nodes whose topology is not loaded yet (edge and vertex modes).
pub(super) fn box_items(doc: &CadDocument, meshes: &CadMeshes, topology: Option<&CadTopology>, view: &CadView, rect: [f32; 4]) -> (Vec<SelectionItem>, Vec<String>) {
    let bounds: HashMap<&str, (Vec3, Vec3)> = meshes.body_bounds().collect();
    let mut found = Vec::new();
    let mut pending = Vec::new();
    let Some(state) = &doc.doc else { return (found, pending) };
    let projected_inside = |p: Vec3| view.project(p).is_some_and(|s| inside(rect, s));
    let point = |p: &[f64; 3]| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    for node in state.nodes.iter().filter(|n| n.effective_visible) {
        let Some((lo, hi)) = bounds.get(node.id.as_str()).copied() else { continue };
        match doc.select_mode {
            SelectMode::Vertex | SelectMode::Edge => {
                let Some(topo) = topology.and_then(|t| t.get(&node.id)) else {
                    pending.push(node.id.clone());
                    continue;
                };
                if doc.select_mode == SelectMode::Vertex {
                    for v in &topo.vertices {
                        if v.point.is_some_and(|p| projected_inside(point(&p))) {
                            found.push(SelectionItem(node.id.clone(), "vertex".into(), v.index));
                        }
                    }
                } else {
                    for e in &topo.edges {
                        if !e.points.is_empty() && e.points.iter().all(|p| projected_inside(point(p))) {
                            found.push(SelectionItem(node.id.clone(), "edge".into(), e.index));
                        }
                    }
                }
            }
            SelectMode::Body | SelectMode::Face | SelectMode::Point => {
                let corners = [lo.x, hi.x].into_iter().flat_map(|x| [lo.y, hi.y].into_iter().flat_map(move |y| [lo.z, hi.z].into_iter().map(move |z| Vec3::new(x, y, z))));
                if corners.into_iter().all(projected_inside) {
                    found.push(SelectionItem(node.id.clone(), "body".into(), 0));
                }
            }
        }
    }
    (found, pending)
}

/// `CadBoxSelect`: RoboCAD's `_box_select` (without `extend` the selection
/// is replaced; found items not yet selected are appended).
pub(super) fn box_select(doc: &mut CadDocument, meshes: &CadMeshes, topology: Option<&CadTopology>, view: &CadView, rect: [f32; 4], extend: bool) -> Result<Value, String> {
    if rect.iter().any(|v| !v.is_finite()) {
        return Err("box select: rect must be four finite numbers [x0, y0, x1, y1]".to_string());
    }
    let sorted = [rect[0].min(rect[2]), rect[1].min(rect[3]), rect[0].max(rect[2]), rect[1].max(rect[3])];
    let (found, pending) = box_items(doc, meshes, topology, view, sorted);
    let n = found.len();
    combine(&mut doc.selection, found, extend, false);
    doc.candidates = None;
    selection_status(doc);
    let pushed = publish(doc);
    let mut out = answer(doc, pushed);
    out["found"] = json!(n);
    if !pending.is_empty() {
        let names: Vec<String> = pending.iter().map(|id| doc.node_name(id)).collect();
        out["not_loaded"] = json!(names);
        out["note"] = json!("the topology of these drawn bodies is still loading; they were not tested");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cad::document::{CadTarget, Candidates, Connection};
    use crate::jobs::Job;
    use bevy::camera::CameraProjection;
    use bevy::math::Affine3A;
    use sim_runtime::cad_client::{DocState, EdgeInfo, Health, NodeSummary, Selection, VertexInfo};
    use std::time::{Duration, Instant};
    use super::super::topology::NodeTopology;

    fn node(id: &str, kind: &str, material: Option<&str>, visible: bool) -> NodeSummary {
        NodeSummary { id: id.into(), kind: kind.into(), name: format!("N{id}"), visible, effective_visible: visible, material: material.map(str::to_string), ..Default::default() }
    }

    /// Not connected (nothing is pushed): bodies b1, b2 (PLA), b3 (hidden),
    /// a sheet s1, a sketch k1 and a mesh m1.
    fn document() -> CadDocument {
        let mut doc = CadDocument::new(CadTarget::Service("http://127.0.0.1:8420".into()));
        doc.doc = Some(DocState {
            nodes: vec![node("b1", "body", Some("pla"), true), node("b2", "body", Some("pla"), true), node("b3", "body", Some("pla"), false), node("s1", "sheet", None, true), node("k1", "sketch", None, true), node("m1", "mesh", None, true), node("i1", "instance", Some("al"), true)],
            revision: 3,
            ..Default::default()
        });
        doc.doc_key = Some((None, 3));
        doc
    }

    fn item(n: &str, k: &str, i: i64) -> SelectionItem {
        SelectionItem(n.into(), k.into(), i)
    }

    #[test]
    fn select_replaces_extends_toggles_and_ids_are_body_items() {
        let mut doc = document();
        doc.candidates = Some(Candidates { items: vec![item("b1", "face", 0)], extend: false, toggle: false });
        let r0 = doc.revision;
        let answer = select(&mut doc, &["b1".into()], &[], false, false).unwrap();
        assert_eq!(doc.selection, vec![item("b1", "body", 0)]);
        assert!(doc.candidates.is_none(), "a choice closes the Alt menu");
        assert!(doc.revision > r0);
        assert_eq!(answer["connected"], json!(false));
        assert_eq!(answer["pushed"], json!(false));
        // Shift appends (once), Ctrl toggles.
        select(&mut doc, &[], &[item("b2", "face", 4), item("b1", "body", 0)], true, false).unwrap();
        assert_eq!(doc.selection, vec![item("b1", "body", 0), item("b2", "face", 4)]);
        select(&mut doc, &[], &[item("b1", "body", 0), item("b2", "edge", 1)], false, true).unwrap();
        assert_eq!(doc.selection, vec![item("b2", "face", 4), item("b2", "edge", 1)]);
        assert_eq!(doc.status, Some(Ok("2 selected".to_string())));
        // Replace; empty clears.
        select(&mut doc, &[], &[item("b2", "vertex", 3)], false, false).unwrap();
        assert_eq!(doc.selection, vec![item("b2", "vertex", 3)]);
        select(&mut doc, &[], &[], false, false).unwrap();
        assert!(doc.selection.is_empty() && doc.status.is_none());
        // Refusals name the item.
        let e = select(&mut doc, &[], &[item("b1", "solid", 0)], false, false).unwrap_err();
        assert!(e.contains("solid") && e.contains("b1"), "{e}");
        let e = select(&mut doc, &["nope".into()], &[], false, false).unwrap_err();
        assert!(e.contains("nope"), "{e}");
        let e = select(&mut doc, &[], &[item("b1", "face", -1)], false, false).unwrap_err();
        assert!(e.contains("negative"), "{e}");
    }

    #[test]
    fn a_mode_switch_clears_the_selection_hover_and_menu() {
        let mut doc = document();
        doc.selection = vec![item("b1", "body", 0)];
        doc.hover = Some(item("b2", "body", 0));
        doc.candidates = Some(Candidates::default());
        set_mode(&mut doc, SelectMode::Edge);
        assert_eq!(doc.select_mode, SelectMode::Edge);
        assert!(doc.selection.is_empty() && doc.hover.is_none() && doc.candidates.is_none());
        assert_eq!(doc.status, Some(Ok("Selection mode: edge".to_string())));
        // A hover is display only: no panel refresh.
        let r = doc.revision;
        hover(&mut doc, Some(&item("b1", "edge", 2)));
        assert_eq!(doc.hover, Some(item("b1", "edge", 2)));
        assert_eq!(doc.revision, r);
    }

    #[test]
    fn select_all_and_invert_take_visible_bodies_sheets_curves_instances_and_meshes() {
        let mut doc = document();
        select_all(&mut doc);
        let ids: Vec<&str> = doc.selection.iter().map(|i| i.0.as_str()).collect();
        assert_eq!(ids, ["b1", "b2", "s1", "m1", "i1"]);
        assert!(doc.selection.iter().all(|i| i.1 == "body" && i.2 == 0));
        doc.selection = vec![item("b1", "face", 2), item("s1", "body", 0)];
        invert(&mut doc);
        let ids: Vec<&str> = doc.selection.iter().map(|i| i.0.as_str()).collect();
        assert_eq!(ids, ["b2", "m1", "i1"]);
    }

    #[test]
    fn same_material_takes_the_first_selected_nodes_material() {
        let mut doc = document();
        assert!(same_material(&mut doc).unwrap_err().contains("nothing is selected"));
        doc.selection = vec![item("b2", "face", 1)];
        let answer = same_material(&mut doc).unwrap();
        assert_eq!(answer["material"], json!("pla"));
        // Hidden b3 too (RoboCAD's same_material does not filter visibility).
        let ids: Vec<&str> = doc.selection.iter().map(|i| i.0.as_str()).collect();
        assert_eq!(ids, ["b1", "b2", "b3"]);
        doc.selection = vec![item("s1", "body", 0)];
        let e = same_material(&mut doc).unwrap_err();
        assert!(e.contains("Ns1") && e.contains("no material"), "{e}");
    }

    /// Face 0: a 10 mm square in z = 0; face 1: a square in y = 0 sharing
    /// the edge (0,0,0)–(10,0,0); face 2: a square in x = 10 meeting the
    /// edge only at its end (10,0,0).
    fn two_faces() -> MeshData {
        MeshData {
            vertices: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, -10.0], [10.0, 0.0, -10.0], [10.0, 10.0, -10.0], [10.0, 5.0, 0.0]],
            triangles: vec![[0, 1, 2], [0, 2, 3], [0, 4, 5], [0, 5, 1], [1, 5, 6], [1, 6, 7]],
            triangle_face: vec![0, 0, 1, 1, 2, 2],
            face_count: 3,
        }
    }

    #[test]
    fn edges_become_the_faces_their_triangles_share() {
        let mesh = two_faces();
        assert_eq!(faces_along(&mesh, &[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]), vec![0, 1]);
        // A sampled arc's sag is allowed for: a polyline bowed 0.05 mm still finds them.
        assert_eq!(faces_along(&mesh, &[[0.0, 0.0, 0.0], [5.0, 0.05, 0.0], [10.0, 0.0, 0.0]]), vec![0, 1]);
        // The vertical edge at x = 0 (0,0,0)–(0,0,−10) bounds face 1 only.
        assert_eq!(faces_along(&mesh, &[[0.0, 0.0, 0.0], [0.0, 0.0, -10.0]]), vec![1]);
        assert!(faces_along(&mesh, &[[0.0, 0.0, 0.0]]).is_empty());

        // Through the action: needs the mesh and the topology at the same revision.
        let mut doc = document();
        doc.select_mode = SelectMode::Edge;
        doc.selection = vec![item("b1", "edge", 7)];
        let mut meshes = CadMeshes::default();
        let mut topology = CadTopology::default();
        let e = edges_to_faces(&mut doc, Some(&meshes), Some(&topology)).unwrap_err();
        assert!(e.contains("Nb1") && e.contains("loading"), "{e}");
        topology.insert("b1", NodeTopology { revision: 3, edges: vec![EdgeInfo { index: 7, points: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]], ..Default::default() }], ..Default::default() });
        let e = edges_to_faces(&mut doc, Some(&meshes), Some(&topology)).unwrap_err();
        assert!(e.contains("tessellation of Nb1"), "{e}");
        meshes.insert_drawn("b1", 3, mesh);
        assert_eq!(faces_of_edge(&meshes, &topology, "b1", 7), vec![0, 1]);
        edges_to_faces(&mut doc, Some(&meshes), Some(&topology)).unwrap();
        assert_eq!(doc.select_mode, SelectMode::Face);
        assert_eq!(doc.selection, vec![item("b1", "face", 0), item("b1", "face", 1)]);
        assert!(edges_to_faces(&mut doc, Some(&meshes), Some(&topology)).unwrap_err().contains("no edges"));
    }

    /// A 0.5 mm wall: the top strip (face 0) and the outer side (face 1)
    /// meet at the edge y = 0, z = 0; the inner side (face 2) runs 0.5 mm
    /// away and is not taken; a sliver (face 3) touching the edge across it
    /// (a side 0.05 mm long, within the tolerance but perpendicular) is not
    /// taken either.
    #[test]
    fn a_thin_walls_far_side_is_not_an_edges_face() {
        let mesh = MeshData {
            vertices: vec![
                [0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 0.5, 0.0], [0.0, 0.5, 0.0],
                [0.0, 0.0, -10.0], [10.0, 0.0, -10.0], [10.0, 0.5, -10.0], [0.0, 0.5, -10.0],
                [0.0, 0.05, 0.0],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3], [0, 4, 5], [0, 5, 1], [3, 2, 6], [3, 6, 7], [0, 8, 4]],
            triangle_face: vec![0, 0, 1, 1, 2, 2, 3],
            face_count: 4,
        };
        assert_eq!(faces_along(&mesh, &[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]), vec![0, 1]);
        // The inner side's own top edge takes the strip and the inner side only.
        assert_eq!(faces_along(&mesh, &[[0.0, 0.5, 0.0], [10.0, 0.5, 0.0]]), vec![0, 2]);
    }

    /// As view.rs's test view: a camera 5 m from the model origin over a
    /// 200 × 100 px view at (10, 20); the origin projects to (110, 70) and
    /// 100 mm is about 2.4 px there.
    fn view() -> CadView {
        let projection = PerspectiveProjection { aspect_ratio: 2.0, ..default() };
        let clip_from_view = projection.get_clip_from_view();
        let world_from_view = Affine3A::from_translation(Vec3::new(0.0, 0.0, 5.0));
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

    fn cube(centre: [f64; 3], half: f64) -> MeshData {
        let [x, y, z] = centre;
        MeshData { vertices: vec![[x - half, y - half, z - half], [x + half, y + half, z + half], [x + half, y - half, z - half]], triangles: vec![[0, 1, 2]], triangle_face: vec![0], face_count: 1 }
    }

    #[test]
    fn box_select_takes_bodies_edges_and_vertices_inside_the_rectangle() {
        let mut doc = document();
        let mut meshes = CadMeshes::default();
        // b1 at the origin (inside), b2 2 m to the right (about 48 px: outside).
        meshes.insert_drawn("b1", 3, cube([0.0, 0.0, 0.0], 100.0));
        meshes.insert_drawn("b2", 3, cube([2000.0, 0.0, 0.0], 100.0));
        let v = view();
        let rect = [120.0, 80.0, 100.0, 60.0];
        doc.selection = vec![item("s1", "body", 0)];
        let answer = box_select(&mut doc, &meshes, None, &v, rect, false).unwrap();
        assert_eq!(doc.selection, vec![item("b1", "body", 0)]);
        assert_eq!(answer["found"], json!(1));
        // Extend keeps what was selected.
        doc.selection = vec![item("s1", "body", 0)];
        box_select(&mut doc, &meshes, None, &v, rect, true).unwrap();
        assert_eq!(doc.selection, vec![item("s1", "body", 0), item("b1", "body", 0)]);

        // Edge mode: an edge whose samples all lie inside; vertex mode: vertices inside.
        let mut topology = CadTopology::default();
        topology.insert(
            "b1",
            NodeTopology {
                revision: 3,
                edges: vec![
                    EdgeInfo { index: 0, points: vec![[-100.0, 0.0, 0.0], [100.0, 0.0, 0.0]], ..Default::default() },
                    EdgeInfo { index: 1, points: vec![[0.0, 0.0, 0.0], [2000.0, 0.0, 0.0]], ..Default::default() },
                ],
                vertices: vec![VertexInfo { index: 0, point: Some([0.0, 0.0, 100.0]) }, VertexInfo { index: 1, point: Some([2000.0, 0.0, 0.0]) }],
                ..Default::default()
            },
        );
        doc.select_mode = SelectMode::Edge;
        let answer = box_select(&mut doc, &meshes, Some(&topology), &v, rect, false).unwrap();
        assert_eq!(doc.selection, vec![item("b1", "edge", 0)]);
        // b2's topology is not loaded: named, not tested.
        assert_eq!(answer["not_loaded"], json!(["Nb2"]));
        doc.select_mode = SelectMode::Vertex;
        box_select(&mut doc, &meshes, Some(&topology), &v, rect, false).unwrap();
        assert_eq!(doc.selection, vec![item("b1", "vertex", 0)]);
    }

    #[test]
    fn a_face_selection_is_pushed_with_its_items_and_mode() {
        let mut doc = document();
        doc.select_mode = SelectMode::Face;
        doc.selection = vec![item("b1", "face", 2)];
        let (items, mode) = super::super::sync::selection_body(&doc);
        assert_eq!(items, vec![item("b1", "face", 2)]);
        assert_eq!(mode, Some("face"));
        // As RoboCAD's PUT /selection body.
        assert_eq!(serde_json::to_value(&items).unwrap(), json!([["b1", "face", 2]]));
    }

    #[test]
    fn robocads_moded_selection_is_adopted_but_not_during_a_push() {
        let mut doc = document();
        doc.client = Some(sim_runtime::cad_client::CadClient::new("http://127.0.0.1:8420").unwrap());
        doc.connection = Connection::Connected;
        doc.health = Some(Health { ok: true, gui: true, revision: 3, ..Default::default() });
        let sent = Instant::now();
        let remote = Selection { items: vec![item("b2", "edge", 5)], mode: Some("edge".into()) };
        assert!(super::super::sync::adopt_selection(&mut doc, sent, remote.clone()));
        assert_eq!(doc.select_mode, SelectMode::Edge);
        assert_eq!(doc.selection, vec![item("b2", "edge", 5)]);
        // The same answer again changes nothing.
        assert!(!super::super::sync::adopt_selection(&mut doc, sent, remote));
        // A headless answer (no mode) keeps the viewer's mode.
        let headless = Selection { items: vec![item("b1", "face", 1)], mode: None };
        assert!(super::super::sync::adopt_selection(&mut doc, sent, headless));
        assert_eq!(doc.select_mode, SelectMode::Edge);
        assert_eq!(doc.selection, vec![item("b1", "face", 1)]);
        // An item naming a node the (current) shown tree lacks is left out;
        // RoboCAD's copy keeps it as read. While the tree is behind, it is kept.
        let ghost = Selection { items: vec![item("b1", "face", 1), item("gone", "body", 0)], mode: None };
        assert!(!super::super::sync::adopt_selection(&mut doc, sent, ghost.clone()));
        assert_eq!(doc.selection, vec![item("b1", "face", 1)]);
        assert_eq!(doc.remote_selection, ghost.items);
        doc.remote_selection.clear();
        doc.stale = Some("refetching revision 4".into());
        assert!(super::super::sync::adopt_selection(&mut doc, sent, ghost.clone()));
        assert_eq!(doc.selection, ghost.items);
        doc.stale = None;
        doc.selection = vec![item("b1", "face", 1)];
        // While our push is in flight, or for a read sent before it answered, nothing is adopted.
        doc.selection_job = Some(Job::finished(doc.generation, Ok(Vec::new())));
        let other = Selection { items: vec![item("b1", "body", 0)], mode: Some("body".into()) };
        assert!(!super::super::sync::adopt_selection(&mut doc, Instant::now(), other.clone()));
        assert_eq!((doc.select_mode, doc.selection.clone()), (SelectMode::Edge, vec![item("b1", "face", 1)]));
        doc.selection_job = None;
        doc.selection_pushed_at = Some(Instant::now());
        assert!(!super::super::sync::adopt_selection(&mut doc, sent.checked_sub(Duration::from_millis(1)).unwrap_or(sent), other));
        assert_eq!(doc.select_mode, SelectMode::Edge);
    }
}

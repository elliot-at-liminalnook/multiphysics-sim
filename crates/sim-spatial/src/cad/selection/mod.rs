//! CAD items in the shared selection. Local body picks and tree gestures
//! apply through CadAction; no remote selection reads, writes or echo exists.
mod shared;

pub(crate) use shared::{CadItems, CadSelection, Shared, View, cad_id, ensure_registered, follow_tree, reopen, source};
#[cfg(test)]
pub(crate) use shared::Fixture;

use super::actions::{CadAction, Cx};
use super::document::{CadDocument, SelectMode};
use crate::selection::Op;
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

/// One availability contract for native controls and every typed selection action.
/// Every mode is available: picks use the exact in-process topology.
pub(super) fn mode_available(_doc: &CadDocument, _mode: SelectMode) -> Result<(), String> {
    // Faces, edges and vertices come from the open archive's exact topology (`topology`).
    Ok(())
}

/// The selection actions (`actions::handle` forwards them here).
pub(super) fn handle(action: &CadAction, _call: &mut Call, cx: &mut Cx) -> Outcome {
    let doc = &mut *cx.doc;
    let shared = &mut cx.shared;
    let result = match action {
        CadAction::CadSelect { ids, items, extend, toggle, picked_at } => select(doc, shared, ids, items, *extend, *toggle, *picked_at),
        CadAction::CadSelectMode { mode } => mode_available(doc, *mode).map(|()| set_mode(doc, shared, *mode)),
        CadAction::CadHover { item } => validate(doc, item.as_slice()).map(|()| hover(doc, item.as_ref())),
        CadAction::CadBoxSelect { rect, extend } => match (cx.meshes.as_deref(), cx.view) {
            (Some(meshes), Some(view)) if view.valid => box_select(doc, shared, meshes, cx.topology.as_deref(), view, *rect, *extend),
            _ => Err("box select needs CAD mode's 3D view, which is not shown yet in this window".to_string()),
        },
        CadAction::CadCandidates { items, extend, toggle } => candidates(doc, items, *extend, *toggle),
        CadAction::CadSelectAll => select_all(doc, shared),
        CadAction::CadInvertSelection => invert(doc, shared),
        CadAction::CadSelectSameMaterial => same_material(doc, shared),
        CadAction::CadEdgesToFaces => edges_to_faces(doc, shared, cx.meshes.as_deref(), cx.topology.as_deref()),
        _ => Err("not a selection action".to_string()),
    };
    Outcome::Done(result)
}

/// Mark the shared selection change for presentation. Always false: no
/// network push is queued and no second selection owner exists.
pub(super) fn publish(doc: &mut CadDocument, shared: View) -> bool {
    doc.touch();
    doc.published_selection = shared.changed();
    false // Shared Selection is authoritative; no remote echo or push.
}

/// A change any writer made since CAD last published or adopted (an
/// `Act<SelectionAction>` applied by the shared system, a re-check after a
/// new tree, an op's clear): published once (`sync::receive`, JobResults).
/// An adopted RoboCAD selection is recorded as published when adopted, so
/// it is never pushed back.
pub(super) fn publish_changes(doc: &mut CadDocument, shared: View) {
    if shared.changed() != doc.published_selection {
        publish(doc, shared);
    }
}

/// Whether RoboCAD's last known selection (or, with a desktop window, its
/// mode) differs from `items`, the shared selection's CAD items.
pub(super) fn differs_from_remote(doc: &CadDocument, items: &[SelectionItem]) -> bool {
    let gui = doc.health.as_ref().is_some_and(|h| h.gui);
    items != doc.remote_selection.as_slice() || (gui && doc.remote_mode != Some(doc.select_mode))
}

/// The answer every selection change gives.
fn answer(doc: &CadDocument, shared: &Shared, pushed: bool) -> Value {
    json!({"selection": shared.items(), "mode": doc.select_mode, "pushed": pushed, "connected": doc.connected()})
}

/// RoboCAD's `selection_changed` status line: "n selected", or the empty status.
fn selection_status(doc: &mut CadDocument, shared: &Shared) {
    let n = shared.items().len();
    doc.status = if n == 0 { None } else { Some(Ok(format!("{n} selected"))) };
}

/// Check items before they enter the selection: a known kind, a node of the
/// shown tree, a non-negative index. Refusals name the item.
fn validate(doc: &CadDocument, items: &[SelectionItem]) -> Result<(), String> {
    for SelectionItem(node, kind, index) in items {
        if let Some(mode) = SelectMode::parse(kind) {
            mode_available(doc, mode)?;
        }
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

/// How RoboCAD's `SelectTool._apply` combines items with the selection, as
/// the shared selection's op: Ctrl toggles each (it wins over Shift), Shift
/// appends the ones not selected, otherwise they replace the selection
/// (duplicates dropped). `Selection::apply` does exactly these.
pub(super) fn op(extend: bool, toggle: bool) -> Op {
    if toggle {
        Op::Toggle
    } else if extend {
        Op::Add
    } else {
        Op::Set
    }
}

/// `CadSelect`: `ids` as body items, then `items`; replace, extend or toggle.
/// Closes the Alt menu. `picked_at`: the shown revision a 3D pick was made
/// at; its face, edge, vertex, point and curve items carry it (their
/// indices belong to it), so one picked against a tree that has since
/// advanced is refused by name. Body items name a node, not an index, and
/// are stamped with the current revision. A choice from the open Alt menu
/// (no `picked_at`, one item of the menu, the menu's extend and toggle: what
/// its `cad:candidate:<n>` control writes) carries the revision the menu
/// was gathered at ([`menu_revision`]); refused that way, the stale menu
/// closes.
pub(super) fn select(doc: &mut CadDocument, shared: &mut Shared, ids: &[String], items: &[SelectionItem], extend: bool, toggle: bool, picked_at: Option<u64>) -> Result<Value, String> {
    let mut wanted: Vec<SelectionItem> = ids.iter().map(|id| SelectionItem(id.clone(), "body".into(), 0)).collect();
    wanted.extend(items.iter().cloned());
    validate(doc, &wanted)?;
    let menu = if picked_at.is_none() && ids.is_empty() { menu_revision(doc, items, extend, toggle) } else { None };
    let picked_at = picked_at.or(menu);
    let stamped = wanted.into_iter().map(|i| {
        let revision = if i.1 == "body" { None } else { picked_at };
        (i, revision)
    });
    if let Err(e) = shared.apply(op(extend, toggle), stamped) {
        if menu.is_some() && doc.candidates.take().is_some() {
            doc.touch();
        }
        return Err(e);
    }
    doc.candidates = None;
    selection_status(doc, shared);
    let pushed = publish(doc, shared.view());
    Ok(answer(doc, shared, pushed))
}

/// `CadSelectMode`: RoboCAD's `set_selection_mode` (the selection is cleared).
pub(super) fn set_mode(doc: &mut CadDocument, shared: &mut Shared, mode: SelectMode) -> Value {
    doc.select_mode = mode;
    // Refused only without a CAD entry, which holds no items to clear.
    let _ = shared.clear();
    doc.hover = None;
    doc.candidates = None;
    doc.status = Some(Ok(format!("Selection mode: {}", mode.name())));
    let pushed = publish(doc, shared.view());
    answer(doc, shared, pushed)
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
    // Gathered against the shown tree (the window's Alt click; a REST list names it too).
    let revision = Some(doc.shown_revision());
    doc.candidates = Some(super::document::Candidates { items: items.to_vec(), extend, toggle, revision });
    doc.touch();
    Ok(json!({"candidates": items.len(), "controls": (0..items.len()).map(|n| format!("cad:candidate:{n}")).collect::<Vec<_>>()}))
}

/// The revision the open Alt menu was gathered at, when `items` is one of
/// its entries chosen as its control chooses it (the menu's extend and
/// toggle); None otherwise.
fn menu_revision(doc: &CadDocument, items: &[SelectionItem], extend: bool, toggle: bool) -> Option<u64> {
    let c = doc.candidates.as_ref()?;
    let [item] = items else { return None };
    (c.extend == extend && c.toggle == toggle && c.items.contains(item)).then_some(c.revision).flatten()
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
pub(super) fn select_all(doc: &mut CadDocument, shared: &mut Shared) -> Result<Value, String> {
    shared.set(visible_bodies(doc, &[]))?;
    doc.candidates = None;
    selection_status(doc, shared);
    let pushed = publish(doc, shared.view());
    Ok(answer(doc, shared, pushed))
}

/// `CadInvertSelection`: RoboCAD's `invert_selection` (by node).
pub(super) fn invert(doc: &mut CadDocument, shared: &mut Shared) -> Result<Value, String> {
    let current = shared.items().nodes();
    shared.set(visible_bodies(doc, &current))?;
    doc.candidates = None;
    selection_status(doc, shared);
    let pushed = publish(doc, shared.view());
    Ok(answer(doc, shared, pushed))
}

/// `CadSelectSameMaterial`: RoboCAD's `same_material` on the first selected node.
pub(super) fn same_material(doc: &mut CadDocument, shared: &mut Shared) -> Result<Value, String> {
    let Some(first) = shared.items().first_node().map(str::to_string) else {
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
    shared.set(same)?;
    doc.candidates = None;
    selection_status(doc, shared);
    let pushed = publish(doc, shared.view());
    let mut out = answer(doc, shared, pushed);
    out["material"] = json!(material);
    Ok(out)
}

/// `CadEdgesToFaces`: RoboCAD's `convert_edges_to_faces` from the drawn
/// tessellation (see the module doc); the mode becomes face. The faces
/// carry the topology's revision.
pub(super) fn edges_to_faces(doc: &mut CadDocument, shared: &mut Shared, meshes: Option<&CadMeshes>, topology: Option<&CadTopology>) -> Result<Value, String> {
    mode_available(doc, SelectMode::Edge)?;
    let edges = shared.items().of_kind("edge");
    if edges.is_empty() {
        return Err("Edges → faces: no edges are selected (edge mode, E)".to_string());
    }
    let (Some(meshes), Some(topology)) = (meshes, topology) else {
        return Err("Edges → faces needs RoboCAD's tessellation and topology, which this window does not hold (no 3D view)".to_string());
    };
    // The revision each selected edge was picked at (its index belongs to it).
    let stamps = shared.id().map_or_else(Vec::new, |id| shared.selection.cad_stamped(id));
    let mut faces: Vec<(SelectionItem, Option<u64>)> = Vec::new();
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
        let picked = stamps.iter().find(|(i, _)| i.0 == *node && i.1 == "edge" && i.2 == *edge).map(|(_, r)| *r);
        if let Some(r) = picked.filter(|r| *r != topo.revision) {
            // Picked at the shown tree while the topology still lags it: the refetch fixes it.
            if r == doc.shown_revision() {
                return Err(format!("Edges → faces: the topology of {name} is from revision {}, the tree from revision {r}; wait for the refetch", topo.revision));
            }
            return Err(format!("Edges → faces: edge {edge} of {name} was picked at revision {r}, its topology is from revision {}; pick the edge again", topo.revision));
        }
        if !topo.edges.iter().any(|e| e.index == *edge) {
            return Err(format!("Edges → faces: {name} has no edge {edge} at revision {}", topo.revision));
        }
        for f in faces_of_edge(meshes, topology, node, *edge) {
            // Converted at the topology's revision: refused by name if the tree has moved on.
            let item = (SelectionItem(node.clone(), "face".into(), f), Some(topo.revision));
            if !faces.contains(&item) {
                faces.push(item);
            }
        }
    }
    let n = faces.len();
    shared.apply(Op::Set, faces)?;
    doc.select_mode = SelectMode::Face;
    doc.hover = None;
    doc.candidates = None;
    doc.status = Some(Ok(format!("Selection: {} edge{} → {n} face{}", edges.len(), if edges.len() == 1 { "" } else { "s" }, if n == 1 { "" } else { "s" })));
    let pushed = publish(doc, shared.view());
    let mut out = answer(doc, shared, pushed);
    out["note"] = json!("faces found from RoboCAD's tessellation (triangles with a side along each edge's sampled polyline), not RoboCAD's kernel");
    Ok(out)
}

/// The faces of node `node` that edge `edge` bounds, from the drawn
/// tessellation (see [`faces_along`]); empty when the mesh or topology is
/// not loaded or has no such edge. Part D's live dimensions use it.
pub(super) fn faces_of_edge(meshes: &CadMeshes, topology: &CadTopology, node: &str, edge: i64) -> Vec<i64> {
    let (Some(mesh), Some(topo)) = (meshes.mesh_data(node), topology.get(node)) else { return Vec::new() };
    // A polyline of one revision against another revision's tessellation names the wrong faces.
    if meshes.drawn_revision(node) != Some(topo.revision) {
        return Vec::new();
    }
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
    // The polyline's bounds widened by the tolerance: vertices outside can't be near, so most skip the distance (box selects of many edges stay quick).
    let (lo, hi) = points.iter().fold((DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    let (lo, hi) = (lo - DVec3::splat(tolerance), hi + DVec3::splat(tolerance));
    let on: Vec<bool> = vertices.iter().map(|v| v.cmpge(lo).all() && v.cmple(hi).all() && near(*v)).collect();
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
/// current mode (RoboCAD's `_box_select`), in tree order, each with the
/// revision its index belongs to (an edge or vertex: its topology's; a
/// body names no index: None), and the drawn nodes whose topology is not
/// loaded yet (edge and vertex modes).
pub(super) fn box_items(doc: &CadDocument, meshes: &CadMeshes, topology: Option<&CadTopology>, view: &CadView, rect: [f32; 4]) -> (Vec<(SelectionItem, Option<u64>)>, Vec<String>) {
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
                            found.push((SelectionItem(node.id.clone(), "vertex".into(), v.index), Some(topo.revision)));
                        }
                    }
                } else {
                    for e in &topo.edges {
                        if !e.points.is_empty() && e.points.iter().all(|p| projected_inside(point(p))) {
                            found.push((SelectionItem(node.id.clone(), "edge".into(), e.index), Some(topo.revision)));
                        }
                    }
                }
            }
            SelectMode::Body | SelectMode::Face | SelectMode::Point => {
                let corners = [lo.x, hi.x].into_iter().flat_map(|x| [lo.y, hi.y].into_iter().flat_map(move |y| [lo.z, hi.z].into_iter().map(move |z| Vec3::new(x, y, z))));
                if corners.into_iter().all(projected_inside) {
                    found.push((SelectionItem(node.id.clone(), "body".into(), 0), None));
                }
            }
        }
    }
    (found, pending)
}

/// `CadBoxSelect`: RoboCAD's `_box_select` (without `extend` the selection
/// is replaced; found items not yet selected are appended).
pub(super) fn box_select(doc: &mut CadDocument, shared: &mut Shared, meshes: &CadMeshes, topology: Option<&CadTopology>, view: &CadView, rect: [f32; 4], extend: bool) -> Result<Value, String> {
    if rect.iter().any(|v| !v.is_finite()) {
        return Err("box select: rect must be four finite numbers [x0, y0, x1, y1]".to_string());
    }
    let sorted = [rect[0].min(rect[2]), rect[1].min(rect[3]), rect[0].max(rect[2]), rect[1].max(rect[3])];
    let (found, pending) = box_items(doc, meshes, topology, view, sorted);
    let n = found.len();
    // Edges and vertices carry their topology's revision: a box over a
    // topology the shown tree has moved past is refused by name.
    shared.apply(op(extend, false), found)?;
    doc.candidates = None;
    selection_status(doc, shared);
    let pushed = publish(doc, shared.view());
    let mut out = answer(doc, shared, pushed);
    out["found"] = json!(n);
    // A failed fetch is retried only on the next revision or cad_refresh: name it, not "loading".
    let (failed, loading): (Vec<&String>, Vec<&String>) = pending.iter().partition(|id| topology.and_then(|t| t.error(id)).is_some());
    if !loading.is_empty() {
        let names: Vec<String> = loading.iter().map(|id| doc.node_name(id)).collect();
        out["not_loaded"] = json!(names);
        out["note"] = json!("the topology of these drawn bodies is still loading; they were not tested");
    }
    if !failed.is_empty() {
        let errors: Vec<Value> = failed.iter().map(|id| json!({"node": doc.node_name(id), "error": topology.and_then(|t| t.error(id)).unwrap_or_default()})).collect();
        out["not_fetched"] = json!(errors);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;

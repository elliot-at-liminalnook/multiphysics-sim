//! Geometry the tools share: the bodies a transform moves, the pivot rule,
//! the selection's revision and face target, the camera's back axis, the
//! cursor over the 3D view, the first surface under it and point markers.
use super::{PivotRule, PushTarget, ToolGizmos};
use crate::cad::document::CadDocument;
use crate::cad::selection::{CadItems, CadSelection};
use crate::cad::mesh::{CadBody, CadMeshes};
use crate::cad::topology::CadTopology;
use crate::cad::view::CadView;
use bevy::picking::hover::HoverMap;
use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings, RayCastVisibility};
use bevy::prelude::*;
use sim_runtime::cad_client::SelectionItem;
use std::collections::HashSet;

/// The nodes `ids` and everything under them in the shown tree (the drawn
/// bodies a transform of `ids` moves).
pub(in crate::cad) fn preview_bodies(doc: &CadDocument, ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = ids.to_vec();
    let mut seen: HashSet<String> = ids.iter().cloned().collect();
    if let Some(state) = &doc.doc {
        // Walk order lists parents before children.
        for n in &state.nodes {
            if n.parent.as_ref().is_some_and(|p| seen.contains(p)) && seen.insert(n.id.clone()) {
                out.push(n.id.clone());
            }
        }
    }
    out
}

/// The transform tools' pivot (see the module doc's pivot rule) for
/// `selection` (the shared selection's CAD items).
pub fn pivot(doc: &CadDocument, selection: &[SelectionItem], meshes: Option<&CadMeshes>) -> Option<(Vec3, PivotRule)> {
    let ids = selection.nodes();
    let first = ids.first()?;
    let node = doc.doc.as_ref().and_then(|d| d.nodes.iter().find(|n| &n.id == first));
    if let Some(p) = node.and_then(|n| n.pivot.as_deref())
        && p.len() >= 3
        && p[..3].iter().all(|v| v.is_finite())
    {
        return Some((Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32), PivotRule::NodePivot));
    }
    if ids.len() == 1
        && let Some((id, revision, Ok(detail))) = &doc.detail
        && id == first
        && *revision == doc.shown_revision()
        && let Some(c) = detail.mass.as_ref().map(|m| &m.centroid)
        && let [Some(x), Some(y), Some(z), ..] = c.as_slice()
    {
        return Some((Vec3::new(*x as f32, *y as f32, *z as f32), PivotRule::MassCentroid));
    }
    let bodies: HashSet<String> = preview_bodies(doc, &ids).into_iter().collect();
    let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for (id, (min, max)) in meshes?.body_bounds() {
        if bodies.contains(id) {
            lo = lo.min(min);
            hi = hi.max(max);
        }
    }
    lo.x.is_finite().then(|| ((lo + hi) / 2.0, PivotRule::BoundsCentre))
}

/// The shown revision `selection` (the shared selection's CAD items) was
/// first seen at ([`track_selection`]); the shown revision when not tracked yet.
pub(in crate::cad) fn selection_revision(doc: &CadDocument, selection: &[SelectionItem]) -> u64 {
    match &doc.tool_state.selection_seen {
        Some((items, revision)) if items.as_slice() == selection => *revision,
        _ => doc.shown_revision(),
    }
}

/// The first selected face as a push/pull target, at the revision the
/// selection was seen at (described from the topology when that is the shown one).
pub(in crate::cad) fn face_target(doc: &CadDocument, selection: &[SelectionItem], topology: Option<&CadTopology>) -> Option<PushTarget> {
    let (node, face) = selection.of_kind("face").into_iter().next()?;
    let revision = selection_revision(doc, selection);
    let info = if revision == doc.shown_revision() { topology.and_then(|t| t.get(&node)).and_then(|t| t.faces.iter().find(|f| f.index == face)).cloned() } else { None };
    Some(PushTarget { node, face, revision, info })
}

/// SimSync: remember the shown revision each new selection appeared at, so
/// a face index from it is not used against renumbered faces after an
/// edit. Approximate: a selection RoboCAD remaps during an edit is dated
/// when the viewer adopts it. A derived cache of the shared selection's CAD
/// items (the one selection owns them), compared by value.
pub(in crate::cad) fn track_selection(doc: Option<ResMut<CadDocument>>, selection: CadSelection) {
    let Some(mut doc) = doc else { return };
    let items = selection.items();
    if doc.tool_state.selection_seen.as_ref().is_some_and(|(seen, _)| *seen == items) {
        return;
    }
    let seen = (items, doc.shown_revision());
    doc.tool_state.selection_seen = Some(seen);
}

/// The camera's back axis (towards the viewer) in the model frame: RoboCAD's `camera.basis()[2]`.
pub(in crate::cad) fn view_back(view: &CadView) -> Vec3 {
    view.model_from_world.transform_vector3(view.world_from_view.transform_vector3(Vec3::Z)).try_normalize().unwrap_or(Vec3::Z)
}

/// The cursor (window logical pixels) when it is over the 3D view and not over a panel.
pub(in crate::cad) fn cursor_in_view(window: Option<&Window>, view: &CadView, hover: Option<&HoverMap>, nodes: &Query<(), With<Node>>) -> Option<Vec2> {
    let p = window?.cursor_position()?;
    (view.contains(p) && !crate::cad::scene::over_ui(hover, nodes)).then_some(p)
}

/// The first unlocked drawn body under the cursor.
pub(in crate::cad) struct Hit {
    pub node: String,
    pub triangle: Option<usize>,
    /// The hit point (model mm).
    pub point: Vec3,
}

/// Bevy's `MeshRayCast` along the cursor ray, on `CadBody` entities only;
/// locked nodes are left out as RoboCAD's pick pass leaves them out.
pub(in crate::cad) fn ray_hit(doc: &CadDocument, cast: &mut MeshRayCast, view: &CadView, cursor: Vec2, bodies: &Query<&CadBody>) -> Option<Hit> {
    let (origin, dir) = view.ray(cursor)?;
    let world_origin = view.world_from_model.transform_point3(origin);
    let world_dir = Dir3::new(view.world_from_model.transform_vector3(dir)).ok()?;
    let filter = |e: Entity| bodies.contains(e);
    let settings = MeshRayCastSettings::default().with_filter(&filter).with_visibility(RayCastVisibility::Visible).never_early_exit();
    let locked = |id: &str| doc.doc.as_ref().and_then(|d| d.nodes.iter().find(|n| n.id == id)).is_some_and(|n| n.locked);
    for (entity, hit) in cast.cast_ray(Ray3d::new(world_origin, world_dir), &settings) {
        let Ok(body) = bodies.get(*entity) else { continue };
        if locked(&body.id) {
            continue;
        }
        return Some(Hit { node: body.id.clone(), triangle: hit.triangle_index, point: view.model_from_world.transform_point3(hit.point) });
    }
    None
}

/// A point marker: a square `pixels` across, facing the camera.
pub(in crate::cad) fn marker(gizmos: &mut Gizmos<ToolGizmos>, view: &CadView, p: Vec3, pixels: f32, color: Color) {
    let Some(per_pixel) = view.mm_per_pixel(p) else { return };
    let r = per_pixel * pixels * 0.5;
    let side = |axis: Vec3| view.model_from_world.transform_vector3(view.world_from_view.transform_vector3(axis)).try_normalize().unwrap_or(Vec3::ZERO) * r;
    let (right, up) = (side(Vec3::X), side(Vec3::Y));
    let corners = [p - right - up, p + right - up, p + right + up, p - right + up, p - right - up];
    gizmos.linestrip(corners.map(|c| view.world_from_model.transform_point3(c)), color);
}

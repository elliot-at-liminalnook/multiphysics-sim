//! Plane nodes and the active plane drawn as translucent quads (RoboCAD's
//! `Viewport._draw_planes`, ui/viewport.py:635-657): every visible plane
//! node with its frame read, a ±60 mm square in its plane, filled in
//! (0.3, 0.6, 0.9) at alpha 0.18 when it is the active plane (compared by
//! value, as RoboCAD's `p == self.active_plane`), else 0.08, outlined in
//! (0.4, 0.7, 1.0) at alpha 0.8, else 0.4. Display only: nothing here
//! changes the document.
//!
//! - **Fill** ([`quads`], SimSync after the plane's sync): one mesh per
//!   quad under the CAD model root (model mm, so the root's Z-up transform
//!   places it, and `DespawnOnExit` goes with the root as the bodies'
//!   does), with an unlit, double-sided, alpha-blended material, not
//!   pickable. Rebuilt only when its inputs change: the document generation
//!   and shown revision (visibility is an edit), the sketch cache's epoch
//!   (frames) and the active plane; frames are the cache's last read
//!   (`plane_last`), so a quad does not blink while it is refetched; the
//!   active plane node's too (its `ActivePlane::Node::frame`, which
//!   operations use, is the shown revision's only and None meanwhile).
//! - **Outline** ([`outlines`], Present): the same squares as gizmo lines.
//! - Native addition, recorded: the active plane is drawn even when no
//!   visible plane node is it (a named plane XY/XZ/YZ, or a hidden node),
//!   so the plane the tools work on is always shown; RoboCAD draws plane
//!   nodes only.
use super::{ActivePlane, CadActivePlane, CadSketches};
use crate::app::{ViewerMode, ViewerSet};
use crate::cad::document::CadDocument;
use crate::cad::mesh::CadRoot;
use crate::cad::transform::ToolGizmos;
use crate::cad::view::CadView;
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use crate::cad::types::PlaneFrame;

/// Half the square's side (viewport.py:644), mm.
const HALF: f64 = 60.0;
/// RoboCAD's fill and outline colours, active then not (viewport.py:646, :651).
const FILL: [Color; 2] = [Color::srgba(0.3, 0.6, 0.9, 0.18), Color::srgba(0.3, 0.6, 0.9, 0.08)];
const LINE: [Color; 2] = [Color::srgba(0.4, 0.7, 1.0, 0.8), Color::srgba(0.4, 0.7, 1.0, 0.4)];

/// One quad: the plane and whether it is the active one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::cad) struct Quad {
    pub frame: PlaneFrame,
    pub active: bool,
}

/// The quads drawn now (the outlines read them).
#[derive(Resource, Default)]
pub(in crate::cad) struct PlaneQuads(pub Vec<Quad>);

/// A quad's fill entity.
#[derive(Component)]
struct PlaneQuad;

pub(in crate::cad) fn build(app: &mut App) {
    app.init_resource::<PlaneQuads>().add_systems(
        Update,
        (
            quads.after(crate::cad::CadSet::Plane).in_set(ViewerSet::SimSync),
            outlines.in_set(ViewerSet::Present),
        )
            .run_if(in_state(ViewerMode::Cad)),
    );
}

/// The square's corners in the plane (model mm), counter-clockwise in (u, v).
fn corners(frame: &PlaneFrame) -> [Vec3; 4] {
    [(-HALF, -HALF), (HALF, -HALF), (HALF, HALF), (-HALF, HALF)].map(|(u, v)| {
        let p = frame.to_world(u, v, 0.0);
        Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
    })
}

/// The quads to draw: the visible plane nodes with a frame read, in tree
/// order, then the active plane when none of them is it (see the module doc).
pub(in crate::cad) fn wanted(doc: &CadDocument, plane: &CadActivePlane, sketches: &CadSketches) -> Vec<Quad> {
    // A plane node's frame is None while it is refetched (operations refuse
    // then); the display draws its last read meanwhile.
    let active = match &plane.plane {
        Some(ActivePlane::Node { id, frame }) => frame.or_else(|| sketches.plane_last(id)),
        _ => plane.frame().ok().flatten(),
    };
    let is_active = |f: &PlaneFrame| active.is_some_and(|a| a.same(f, 1e-9));
    let mut out: Vec<Quad> = match &doc.doc {
        Some(state) => state.nodes.iter().filter(|n| n.kind == "plane" && n.effective_visible).filter_map(|n| sketches.plane_last(&n.id)).map(|frame| Quad { frame, active: is_active(&frame) }).collect(),
        None => Vec::new(),
    };
    if let Some(frame) = active
        && !out.iter().any(|q| q.active)
    {
        out.push(Quad { frame, active: true });
    }
    out
}

/// A quad's mesh: two triangles (model mm), the plane's normal on each corner.
fn quad_mesh(frame: &PlaneFrame) -> Mesh {
    let n = Vec3::new(frame.normal[0] as f32, frame.normal[1] as f32, frame.normal[2] as f32).try_normalize().unwrap_or(Vec3::Z);
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, corners(frame).map(|c| c.to_array()).to_vec());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![n.to_array(); 4]);
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}

/// What the fill was last built from.
#[derive(Default)]
struct Built {
    key: Option<(Option<Entity>, u64, u64, u64, Option<ActivePlane>)>,
    entities: Vec<Entity>,
    /// The active and inactive fill materials (made once).
    materials: Option<[Handle<StandardMaterial>; 2]>,
}

/// SimSync: rebuild the fill when its inputs changed (see the module doc).
#[allow(clippy::too_many_arguments)]
fn quads(
    mut commands: Commands,
    doc: Option<Res<CadDocument>>,
    plane: Option<Res<CadActivePlane>>,
    sketches: Option<Res<CadSketches>>,
    root: Option<Single<Entity, With<CadRoot>>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut drawn: ResMut<PlaneQuads>,
    mut built: Local<Built>,
) {
    let (Some(doc), Some(plane), Some(sketches)) = (doc, plane, sketches) else { return };
    let root = root.map(|r| *r);
    let key = (root, doc.generation, doc.shown_revision(), sketches.epoch, plane.plane.clone());
    if built.key.as_ref() == Some(&key) {
        return;
    }
    let same_root = built.key.as_ref().is_some_and(|k| k.0 == root);
    built.key = Some(key);
    let want = wanted(&doc, &plane, &sketches);
    if same_root && want == drawn.0 {
        return;
    }
    // The old fill (under another root it went with that root).
    for e in std::mem::take(&mut built.entities) {
        if same_root {
            commands.entity(e).try_despawn();
        }
    }
    if drawn.0 != want {
        drawn.0 = want.clone();
    }
    let Some(root) = root else { return };
    let [active, inactive] = built
        .materials
        .get_or_insert_with(|| FILL.map(|base_color| materials.add(StandardMaterial { base_color, unlit: true, double_sided: true, cull_mode: None, alpha_mode: AlphaMode::Blend, ..default() })))
        .clone();
    for q in &want {
        let material = if q.active { active.clone() } else { inactive.clone() };
        let e = commands.spawn((Mesh3d(meshes.add(quad_mesh(&q.frame))), MeshMaterial3d(material), Transform::default(), Visibility::default(), PlaneQuad, Pickable::IGNORE)).id();
        commands.entity(root).add_child(e);
        built.entities.push(e);
    }
}

/// Present: the quads' outlines (display only).
fn outlines(drawn: Res<PlaneQuads>, view: Option<Res<CadView>>, mut gizmos: Gizmos<ToolGizmos>) {
    let Some(view) = view else { return };
    if drawn.0.is_empty() || !view.valid {
        return;
    }
    for q in &drawn.0 {
        let c = corners(&q.frame).map(|p| view.world_from_model.transform_point3(p));
        gizmos.linestrip([c[0], c[1], c[2], c[3], c[0]], LINE[usize::from(!q.active)]);
    }
}

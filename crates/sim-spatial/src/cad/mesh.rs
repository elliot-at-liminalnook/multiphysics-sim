//! CAD mode's 3D bodies: RoboCAD's tessellations (`GET /nodes/{id}/mesh`),
//! drawn as they are. Display only: nothing here PATCHes RoboCAD, and the
//! geometry is never changed.
//!
//! - **What is drawn**: every node of a body-like kind ([`BODY_KINDS`]) that
//!   RoboCAD reports `effective_visible`, at the shown (document id,
//!   revision). A node RoboCAD has no mesh for (404 "no mesh") is
//!   remembered and not asked again until the revision changes. A failed
//!   fetch or build is reported and retried on a revision change,
//!   `cad_refresh` or a reconnection (Lost → Connected:
//!   `CadDocument::mesh_retry`).
//! - **Known cost**: RoboCAD's revision covers the whole document, so every
//!   revision refetches every visible body's mesh (even ones the edit did
//!   not touch); at most [`MAX_FETCHES`] at a time, the old mesh shown meanwhile.
//! - **How**: the fetch runs on `Pool::Dedicated` (network: the jobs
//!   module's pool rule, deliberately not `Pool::Io`, which the asset server
//!   shares; at most [`MAX_FETCHES`] at once), the Bevy mesh data is built on
//!   `Pool::Compute` ([`build`]), and the asset and entity are made on the UI
//!   thread. A body keeps its previous mesh on screen until the new
//!   revision's arrives. Results of another generation or revision are dropped.
//! - **Picking** is `pick`'s (one path for bodies, faces, edges, vertices
//!   and points, through Bevy's `MeshRayCast` on these entities and
//!   [`CadMeshes::face_of`]); the bodies carry `RayCastBackfaces` because
//!   they are drawn double-sided (sheets are seen from both sides).
//! - **Highlight** ([`highlight`]): a body selected as a body item is drawn
//!   in the selection material; faces, edges, vertices and points are drawn
//!   by `overlay`, so a face selection does not paint the whole body.
use super::document::CadDocument;
use crate::jobs::{Job, Pool};
use bevy::picking::mesh_picking::ray_cast::RayCastBackfaces;
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use sim_runtime::cad_client::{MESH_TOLERANCE, MeshData};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// RoboCAD node kinds that have a tessellation (`Document.mesh_of`: a body
/// or sheet's own body, an instance's resolved body, a mesh node).
pub const BODY_KINDS: [&str; 4] = ["body", "sheet", "instance", "mesh"];
/// Mesh requests in flight at once.
pub const MAX_FETCHES: usize = 2;

/// A drawn RoboCAD body.
#[derive(Component, Clone, Debug)]
pub struct CadBody {
    pub id: String,
}

/// The Z-up, millimetre model frame (RoboCAD's), shown in Bevy's Y-up metres.
#[derive(Component)]
pub(super) struct CadRoot;

/// Built off the UI thread: the mesh's attributes and its bounds (mm, RoboCAD's frame).
#[derive(Debug)]
pub struct Built {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub min: Vec3,
    pub max: Vec3,
}

enum State {
    Shown,
    NoMesh,
    Failed(String),
}

struct Entry {
    /// The revision this entry's state was reached at; None asks for a
    /// fetch again (a failed entry being retried).
    revision: Option<u64>,
    state: State,
    /// The drawn entity (kept while a newer revision's mesh is fetched).
    entity: Option<Entity>,
    /// Bounds of the drawn mesh (mm, RoboCAD's frame).
    bounds: Option<(Vec3, Vec3)>,
    /// The revision the drawn mesh came from and RoboCAD's tessellation of
    /// it (positions in mm, triangles, `triangle_face`): face picking,
    /// face outlines and edges → faces read it. The drawn Bevy mesh keeps
    /// RoboCAD's triangle order, so Bevy's triangle i is RoboCAD's triangle i.
    drawn: Option<(u64, Arc<MeshData>)>,
}

struct Fetch {
    id: String,
    revision: u64,
    job: Job<Option<MeshData>>,
}
struct Building {
    id: String,
    revision: u64,
    job: Job<(Built, Arc<MeshData>)>,
}

/// What `cad_state.meshes` reports.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshCounts {
    pub shown: usize,
    pub pending: usize,
    pub no_mesh: usize,
    pub failed: Vec<(String, String)>,
}

/// CAD mode's mesh cache (inserted on entering CAD mode, removed on leaving).
#[derive(Resource, Default)]
pub struct CadMeshes {
    generation: u64,
    document_id: Option<String>,
    entries: HashMap<String, Entry>,
    fetching: Vec<Fetch>,
    building: Vec<Building>,
    pub counts: MeshCounts,
    /// The `CadDocument::mesh_retry` last applied (failed entries retried).
    retry: u64,
    /// Bumped when an entity is spawned, replaced or despawned.
    pub(super) epoch: u64,
    /// A framing the scene applies next frame: (centre, half-diagonal), metres in Bevy's frame.
    pub(super) fit: Option<(Vec3, f32)>,
    /// The document has been framed: 1 on its first mesh, 2 once every
    /// first-revision mesh is in (the camera is framed at both).
    fitted: u8,
}

impl CadMeshes {
    /// Bounds of the drawn bodies (all, or those in `ids`) in Bevy's frame
    /// (metres, Y up): RoboCAD's (x, y, z) mm is (x, z, −y) / 1000 here.
    pub fn bounds(&self, ids: Option<&HashSet<String>>) -> Option<(Vec3, Vec3)> {
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for (id, entry) in &self.entries {
            if ids.is_some_and(|ids| !ids.contains(id)) || entry.entity.is_none() {
                continue;
            }
            let Some((min, max)) = entry.bounds else { continue };
            let a = display(min);
            let b = display(max);
            lo = lo.min(a.min(b));
            hi = hi.max(a.max(b));
        }
        lo.x.is_finite().then_some((lo, hi))
    }

    /// Ask the scene to frame these bounds.
    pub fn frame(&mut self, (lo, hi): (Vec3, Vec3)) {
        self.fit = Some(((lo + hi) / 2.0, ((hi - lo).length() / 2.0).max(0.005)));
    }

    /// Whether node `id` is drawn.
    pub fn shown(&self, id: &str) -> bool {
        self.entries.get(id).is_some_and(|e| e.entity.is_some())
    }

    /// The entity drawing node `id`.
    pub fn entity(&self, id: &str) -> Option<Entity> {
        self.entries.get(id).and_then(|e| e.entity)
    }

    /// The RoboCAD revision node `id`'s drawn mesh came from.
    pub fn drawn_revision(&self, id: &str) -> Option<u64> {
        self.entries.get(id).and_then(|e| e.drawn.as_ref()).map(|(r, _)| *r)
    }

    /// RoboCAD's tessellation of node `id` as drawn (mm, RoboCAD's frame).
    pub fn mesh_data(&self, id: &str) -> Option<&Arc<MeshData>> {
        self.entries.get(id).filter(|e| e.entity.is_some()).and_then(|e| e.drawn.as_ref()).map(|(_, m)| m)
    }

    /// The B-rep face of node `id`'s drawn triangle `triangle` (RoboCAD's
    /// `triangle_face`), as Bevy's ray cast reports the triangle.
    pub fn face_of(&self, id: &str, triangle: usize) -> Option<i64> {
        self.mesh_data(id)?.triangle_face.get(triangle).copied().filter(|f| *f >= 0)
    }

    /// Every drawn body's bounds (mm, RoboCAD's frame): box select's test.
    pub fn body_bounds(&self) -> impl Iterator<Item = (&str, (Vec3, Vec3))> {
        self.entries.iter().filter(|(_, e)| e.entity.is_some()).filter_map(|(id, e)| e.bounds.map(|b| (id.as_str(), b)))
    }
}

/// RoboCAD's frame (mm, Z up) as Bevy's (m, Y up): the root's transform.
fn display(p: Vec3) -> Vec3 {
    Vec3::new(p.x, p.z, -p.y) * 0.001
}

/// The root's transform: −90° about X (Z up → Y up) and mm → m.
pub(super) fn root_transform() -> Transform {
    Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)).with_scale(Vec3::splat(0.001))
}

/// The display materials: one per colour, and the selection's.
#[derive(Resource)]
pub(super) struct CadMaterials {
    pub(super) selected: Handle<StandardMaterial>,
    by_colour: HashMap<[u8; 3], Handle<StandardMaterial>>,
}
impl CadMaterials {
    pub(super) fn new(materials: &mut Assets<StandardMaterial>) -> Self {
        let selected = materials.add(StandardMaterial { base_color: Color::srgb(0.98, 0.62, 0.22), emissive: LinearRgba::rgb(0.35, 0.16, 0.02), perceptual_roughness: 0.6, cull_mode: None, ..default() });
        Self { selected, by_colour: HashMap::new() }
    }
    fn colour(&mut self, materials: &mut Assets<StandardMaterial>, rgb: [f32; 3]) -> Handle<StandardMaterial> {
        let key = rgb.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
        self.by_colour
            .entry(key)
            .or_insert_with(|| materials.add(StandardMaterial { base_color: Color::srgb_u8(key[0], key[1], key[2]), perceptual_roughness: 0.7, metallic: 0.05, cull_mode: None, ..default() }))
            .clone()
    }
}

/// RoboCAD's default body colour (`Material.color`'s default).
const DEFAULT_COLOUR: [f32; 3] = [0.7, 0.7, 0.72];

/// A node's display colour: its own, else its material's, else RoboCAD's default.
fn node_colour(doc: &CadDocument, id: &str) -> [f32; 3] {
    let Some(state) = &doc.doc else { return DEFAULT_COLOUR };
    let Some(node) = state.nodes.iter().find(|n| n.id == id) else { return DEFAULT_COLOUR };
    let rgb = |v: &[f64]| (v.len() >= 3).then(|| [v[0] as f32, v[1] as f32, v[2] as f32]);
    if let Some(c) = node.color.as_deref().and_then(rgb) {
        return c;
    }
    let material = node.material.as_deref().and_then(|m| state.materials.iter().find(|x| x["id"].as_str() == Some(m)));
    let colour: Option<Vec<f64>> = material.and_then(|m| m["color"].as_array()).map(|a| a.iter().filter_map(|x| x.as_f64()).collect());
    colour.as_deref().and_then(rgb).unwrap_or(DEFAULT_COLOUR)
}

/// The mesh data for Bevy (Compute): positions as given (mm), normals
/// smoothed within each B-rep face (`triangle_face`) and split across faces,
/// so edges stay sharp and curved faces shade smoothly. A triangle naming a
/// vertex that does not exist is an error naming the node.
pub fn build(id: &str, mesh: &MeshData) -> Result<Built, String> {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<Vec3> = Vec::new();
    let mut indices: Vec<u32> = Vec::with_capacity(mesh.triangles.len() * 3);
    let mut split: HashMap<(u32, i64), u32> = HashMap::new();
    let vertex = |i: u32| -> Result<Vec3, String> {
        let v = mesh.vertices.get(i as usize).ok_or_else(|| format!("node {id}: RoboCAD's mesh names vertex {i} of {}", mesh.vertices.len()))?;
        Ok(Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32))
    };
    for (t, tri) in mesh.triangles.iter().enumerate() {
        let face = mesh.triangle_face.get(t).copied().unwrap_or(-1 - t as i64);
        let p = [vertex(tri[0])?, vertex(tri[1])?, vertex(tri[2])?];
        // Area-weighted (the cross product's length is twice the area).
        let n = (p[1] - p[0]).cross(p[2] - p[0]);
        for (k, &v) in tri.iter().enumerate() {
            let index = *split.entry((v, face)).or_insert_with(|| {
                positions.push(p[k].to_array());
                normals.push(Vec3::ZERO);
                (positions.len() - 1) as u32
            });
            normals[index as usize] += n;
            indices.push(index);
        }
    }
    let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for p in &positions {
        let p = Vec3::from_array(*p);
        min = min.min(p);
        max = max.max(p);
    }
    if positions.is_empty() {
        return Err(format!("node {id}: RoboCAD's mesh has no triangles"));
    }
    let normals = normals.into_iter().map(|n| n.try_normalize().unwrap_or(Vec3::Z).to_array()).collect();
    Ok(Built { positions, normals, indices, min, max })
}

/// Moves the built vectors in: no copy on the UI thread.
fn bevy_mesh(built: Built) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, built.positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, built.normals);
    mesh.insert_indices(Indices::U32(built.indices));
    mesh
}

/// SimSync: bring the drawn bodies to the shown tree's revision.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync(
    mut commands: Commands,
    doc: Option<Res<CadDocument>>,
    meshes: Option<ResMut<CadMeshes>>,
    mut assets: ResMut<Assets<Mesh>>,
    materials: Option<Res<CadMaterials>>,
    root: Option<Single<Entity, With<CadRoot>>>,
    mut redraw: MessageWriter<bevy::window::RequestRedraw>,
) {
    let (Some(doc), Some(mut meshes), Some(materials), Some(root)) = (doc, meshes, materials, root) else { return };
    let meshes = &mut *meshes;
    let document_id = doc.doc_key.as_ref().and_then(|k| k.0.clone());
    if meshes.generation != doc.generation || meshes.document_id != document_id {
        // Another document (or connection): nothing drawn belongs to it.
        for entry in meshes.entries.values() {
            if let Some(e) = entry.entity {
                commands.entity(e).despawn();
            }
        }
        *meshes = CadMeshes { generation: doc.generation, document_id, epoch: meshes.epoch + 1, retry: doc.mesh_retry, ..default() };
    }
    // `cad_refresh` or a reconnection: failed bodies are fetched again.
    if meshes.retry != doc.mesh_retry {
        meshes.retry = doc.mesh_retry;
        for entry in meshes.entries.values_mut() {
            if matches!(entry.state, State::Failed(_)) {
                entry.revision = None;
            }
        }
    }
    let Some(state) = &doc.doc else { return };
    let revision = doc.doc_key.as_ref().map_or(0, |k| k.1);
    let wanted: HashSet<&str> = state.nodes.iter().filter(|n| n.effective_visible && BODY_KINDS.contains(&n.kind.as_str())).map(|n| n.id.as_str()).collect();

    // Hidden or deleted nodes go (and their requests with them).
    let gone: Vec<String> = meshes.entries.keys().filter(|id| !wanted.contains(id.as_str())).cloned().collect();
    for id in gone {
        if let Some(e) = meshes.entries.remove(&id).and_then(|e| e.entity) {
            commands.entity(e).despawn();
            meshes.epoch += 1;
        }
    }
    meshes.fetching.retain(|f| wanted.contains(f.id.as_str()) && f.revision == revision && f.job.generation() == doc.generation);
    meshes.building.retain(|b| wanted.contains(b.id.as_str()) && b.revision == revision && b.job.generation() == doc.generation);

    // Fetched: build on Compute (or remember "no mesh" / the error).
    let mut i = 0;
    while i < meshes.fetching.len() {
        let Some(result) = meshes.fetching[i].job.poll() else {
            i += 1;
            continue;
        };
        let Fetch { id, revision: at, .. } = meshes.fetching.swap_remove(i);
        match result {
            Ok(Some(data)) => {
                let name = id.clone();
                let job = Job::spawn(Pool::Compute, doc.generation, "cad-mesh-build", move |_| build(&name, &data).map(|built| (built, Arc::new(data))));
                meshes.building.push(Building { id, revision: at, job });
            }
            Ok(None) => {
                let entry = meshes.entries.entry(id).or_insert(Entry { revision: Some(at), state: State::NoMesh, entity: None, bounds: None, drawn: None });
                if let Some(e) = entry.entity.take() {
                    commands.entity(e).despawn();
                    meshes.epoch += 1;
                }
                *entry = Entry { revision: Some(at), state: State::NoMesh, entity: None, bounds: None, drawn: None };
            }
            Err(error) => settle_failed(meshes, id, at, error),
        }
    }

    // Built: the asset and the entity (or the new mesh on the drawn one).
    let mut i = 0;
    while i < meshes.building.len() {
        let Some(result) = meshes.building[i].job.poll() else {
            i += 1;
            continue;
        };
        let Building { id, revision: at, .. } = meshes.building.swap_remove(i);
        match result {
            Ok((built, data)) => {
                let bounds = Some((built.min, built.max));
                let handle = assets.add(bevy_mesh(built));
                let existing = meshes.entries.get(&id).and_then(|e| e.entity);
                let entity = match existing {
                    Some(e) => {
                        commands.entity(e).insert(Mesh3d(handle));
                        e
                    }
                    None => {
                        let e = commands.spawn((Mesh3d(handle), MeshMaterial3d(materials.selected.clone()), Transform::default(), Visibility::default(), CadBody { id: id.clone() }, Pickable::default(), RayCastBackfaces)).id();
                        commands.entity(*root).add_child(e);
                        e
                    }
                };
                meshes.entries.insert(id, Entry { revision: Some(at), state: State::Shown, entity: Some(entity), bounds, drawn: Some((at, data)) });
                meshes.epoch += 1;
            }
            Err(error) => settle_failed(meshes, id, at, error),
        }
    }

    // Start fetches for bodies not yet at this revision.
    if let Some(client) = doc.client.clone().filter(|_| doc.connected()) {
        let mut ids: Vec<&str> = wanted.iter().copied().filter(|id| meshes.entries.get(*id).is_none_or(|e| e.revision != Some(revision))).collect();
        ids.sort_unstable();
        for id in ids {
            if meshes.fetching.len() >= MAX_FETCHES {
                break;
            }
            if meshes.fetching.iter().any(|f| f.id == id) || meshes.building.iter().any(|b| b.id == id) {
                continue;
            }
            let (client, node) = (client.clone(), id.to_string());
            let job = Job::spawn(Pool::Dedicated, doc.generation, "cad-mesh-fetch", move |_| client.mesh(&node, MESH_TOLERANCE).map_err(|e| e.to_string()));
            meshes.fetching.push(Fetch { id: id.to_string(), revision, job });
        }
    }

    let mut counts = MeshCounts::default();
    for id in &wanted {
        match meshes.entries.get(*id) {
            Some(e) if e.revision == Some(revision) => match &e.state {
                State::Shown => counts.shown += 1,
                State::NoMesh => counts.no_mesh += 1,
                State::Failed(error) => counts.failed.push((id.to_string(), error.clone())),
            },
            _ => counts.pending += 1,
        }
    }
    counts.failed.sort();
    if meshes.counts != counts {
        meshes.counts = counts;
    }
    let pending = counts_pending(meshes);
    if pending {
        redraw.write(bevy::window::RequestRedraw);
    }
    // Frame the document on its first mesh, and again once its first meshes are all in.
    let stage = match (meshes.counts.shown > 0, pending) {
        (false, _) => 0,
        (true, true) => 1,
        (true, false) => 2,
    };
    if stage > meshes.fitted {
        meshes.fitted = stage;
        if let Some(b) = meshes.bounds(None) {
            meshes.frame(b);
        }
    }
}

fn counts_pending(meshes: &CadMeshes) -> bool {
    meshes.counts.pending > 0 || !meshes.fetching.is_empty() || !meshes.building.is_empty()
}

/// A failed fetch or build: reported, and the previous mesh (if any) stays
/// drawn. Retried on a new revision, `cad_refresh` or a reconnection.
fn settle_failed(meshes: &mut CadMeshes, id: String, revision: u64, error: String) {
    let entry = meshes.entries.entry(id).or_insert(Entry { revision: Some(revision), state: State::NoMesh, entity: None, bounds: None, drawn: None });
    entry.revision = Some(revision);
    entry.state = State::Failed(error);
}

/// SimSync (after `sync`): each body's colour, or the selection's for a
/// body selected as a body item (`[id, "body", 0]`).
pub(super) fn highlight(
    doc: Option<Res<CadDocument>>,
    meshes: Option<Res<CadMeshes>>,
    materials: Option<ResMut<CadMaterials>>,
    mut assets: ResMut<Assets<StandardMaterial>>,
    mut bodies: Query<(&CadBody, &mut MeshMaterial3d<StandardMaterial>)>,
    added: Query<(), Added<CadBody>>,
    mut last: Local<Option<(u64, u64, u64)>>,
) {
    let (Some(doc), Some(meshes), Some(mut materials)) = (doc, meshes, materials) else { return };
    let key = (doc.generation, doc.revision, meshes.epoch);
    if *last == Some(key) && added.is_empty() {
        return;
    }
    *last = Some(key);
    for (body, mut material) in &mut bodies {
        let selected = doc.selection.iter().any(|s| s.0 == body.id && s.1 == "body");
        let want = if selected { materials.selected.clone() } else { materials.colour(&mut assets, node_colour(&doc, &body.id)) };
        if material.0 != want {
            material.0 = want;
        }
    }
}

impl CadMeshes {
    /// Tests: node `id` drawn from `data` at `revision` (no entity is
    /// spawned; the bounds are the data's).
    #[cfg(test)]
    pub(super) fn insert_drawn(&mut self, id: &str, revision: u64, data: MeshData) {
        let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for v in &data.vertices {
            let p = Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32);
            min = min.min(p);
            max = max.max(p);
        }
        let entry = Entry { revision: Some(revision), state: State::Shown, entity: Some(Entity::PLACEHOLDER), bounds: Some((min, max)), drawn: Some((revision, Arc::new(data))) };
        self.entries.insert(id.to_string(), entry);
        self.epoch += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normals_are_smooth_within_a_face_and_split_across_faces() {
        // Two triangles of one face (a square in z = 0) and one of another face sharing an edge.
        let mesh = MeshData {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, -1.0]],
            triangles: vec![[0, 1, 2], [0, 2, 3], [1, 0, 4]],
            triangle_face: vec![0, 0, 1],
            face_count: 2,
            ..Default::default()
        };
        let built = build("n1", &mesh).unwrap();
        // Face 0 shares its four vertices; face 1 gets its own copies of 0 and 1.
        assert_eq!(built.positions.len(), 4 + 3);
        assert_eq!(built.indices.len(), 9);
        for i in &built.indices[..6] {
            assert_eq!(built.normals[*i as usize], [0.0, 0.0, 1.0]);
        }
        // (1,0,0) → (0,0,0) → (1,0,−1): its normal is −Y.
        let n = Vec3::from_array(built.normals[built.indices[6] as usize]);
        assert!((n + Vec3::Y).length() < 1e-6, "{n:?}");
        assert_eq!((built.min, built.max), (Vec3::new(0.0, 0.0, -1.0), Vec3::new(1.0, 1.0, 0.0)));
        let bad = MeshData { triangles: vec![[0, 1, 9]], ..mesh };
        assert!(build("n2", &bad).unwrap_err().contains("n2"));
    }

    #[test]
    fn display_frame_is_z_up_millimetres() {
        // mm × 0.001 is not exact in f32 (3000 → 3.0000002).
        let d = display(Vec3::new(1000.0, 2000.0, 3000.0));
        assert!((d - Vec3::new(1.0, 3.0, -2.0)).length() < 1e-5, "{d:?}");
        let t = root_transform();
        let p = t.transform_point(Vec3::new(1000.0, 2000.0, 3000.0));
        assert!((p - Vec3::new(1.0, 3.0, -2.0)).length() < 1e-5, "{p:?}");
    }
}

//! The section tool's maths and work (display only):
//!
//! - **Plane maths** on [`SectionPlane`]: validation as RoboCAD's saved
//!   views validate a section plane, its `{axis, offset}` form, the Section
//!   tool's Tab offset and R rotation, signed distance.
//! - **The preview** ([`preview`], SimSync): RoboCAD clips everything with
//!   an OpenGL clip plane that keeps `dot(n, p − o) ≤ 0` (`glClipPlane` with
//!   the negated normal) and outlines the cut from the display triangles
//!   (`section_preview.mesh_segments`). Bevy's `StandardMaterial` has no
//!   clip plane, so each drawn body gets a clipped copy of RoboCAD's
//!   tessellation (`CadMeshes::mesh_data`), built on a `Pool::Compute` job
//!   and cached by (the drawn tessellation, plane, overhang shading), and the
//!   body's `Mesh3d` shows the copy while the section is on. The copy keeps
//!   RoboCAD's triangle order (a removed triangle collapses to a point, which
//!   no ray hits; the second half of a cut triangle is appended after the
//!   last) and its own `triangle_face`, recorded with the tessellation it was
//!   cut from on `CadMeshes` (`set_shown_copy`) while the body shows it: a
//!   pick reads the face from the copy (so an appended half names its face
//!   too), never hits the removed part, and names no face while the shown
//!   copy was cut from an older tessellation than the drawn one. While a
//!   newer copy builds, the previous one stays shown (as `mesh` keeps a
//!   body's previous mesh during a refetch).
//!   The same job builds the build plate's overhang overlay (RoboCAD's
//!   `printing.overhangs` at 45°, drawn in its 0.9, 0.35, 0.3).
//! - **The exact section** ([`exact_jobs`], JobResults): `GET
//!   /nodes/{id}/section` on a `jobs::Latest` (`Pool::Dedicated`, network),
//!   keyed by (node, RoboCAD revision, plane); an answer for a superseded
//!   request is dropped, and `ExactSection::drawn` draws only an answer for
//!   the current revision and plane. While the section stays on the
//!   request's plane, a new RoboCAD revision re-reads it.
use super::{CadDisplay, ExactKey, SectionAxis, SectionPlane};
use crate::cad::document::CadDocument;
use crate::cad::mesh::{Built, CadBody, CadMeshes, CadRoot, ShownCopy, build};
use crate::jobs::{Ctx, Job, Latest, Pool};
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use sim_runtime::cad_client::{MeshData, SectionCurves};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// RoboCAD's overhang threshold (`Viewport.overhang_threshold`, degrees).
pub const OVERHANG_DEG: f64 = 45.0;
/// RoboCAD's overhang colour (`_draw_tris_colored`).
const OVERHANG: Color = Color::srgb(0.9, 0.35, 0.3);

type V3 = [f64; 3];
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unit(a: V3) -> Option<V3> {
    let n = dot(a, a).sqrt();
    (n.is_finite() && n >= 1e-10).then(|| scale(a, 1.0 / n))
}

impl SectionPlane {
    /// As RoboCAD's `saved_views.validate_state` checks a section plane:
    /// finite vectors, nonzero axes (normalised here), perpendicular
    /// within 1e-6. An error names what is wrong.
    pub fn validated(self) -> Result<SectionPlane, String> {
        if [self.origin, self.normal, self.x_axis].iter().flatten().any(|v| !v.is_finite()) {
            return Err("the section plane needs finite 3D vectors (origin, normal, x_axis)".into());
        }
        let (Some(normal), Some(x_axis)) = (unit(self.normal), unit(self.x_axis)) else {
            return Err("the section plane's normal and x_axis must be nonzero".into());
        };
        if dot(normal, x_axis).abs() > 1e-6 {
            return Err("the section plane's normal and x_axis must be perpendicular".into());
        }
        Ok(SectionPlane { origin: self.origin, normal, x_axis })
    }
    /// RoboCAD's `{axis, offset}`: x → `Plane.yz(offset)`, y →
    /// `Plane.xz(offset)` (normal −Y), z → `Plane.xy(offset)`.
    pub fn on_axis(axis: SectionAxis, offset: f64) -> SectionPlane {
        match axis {
            SectionAxis::X => SectionPlane { origin: [offset, 0.0, 0.0], normal: [1.0, 0.0, 0.0], x_axis: [0.0, 1.0, 0.0] },
            SectionAxis::Y => SectionPlane { origin: [0.0, offset, 0.0], normal: [0.0, -1.0, 0.0], x_axis: [1.0, 0.0, 0.0] },
            SectionAxis::Z => SectionPlane { origin: [0.0, 0.0, offset], normal: [0.0, 0.0, 1.0], x_axis: [1.0, 0.0, 0.0] },
        }
    }
    /// The unit normal (+Z for a degenerate one).
    pub fn unit_normal(&self) -> V3 {
        unit(self.normal).unwrap_or([0.0, 0.0, 1.0])
    }
    /// RoboCAD's `Plane.y_axis`: unit(normal × x_axis).
    pub fn y_axis(&self) -> V3 {
        unit(cross(self.normal, self.x_axis)).unwrap_or([0.0, 1.0, 0.0])
    }
    /// The Section tool's Tab: the plane moved `d` mm along its normal.
    pub fn moved(&self, d: f64) -> SectionPlane {
        SectionPlane { origin: add(self.origin, scale(self.unit_normal(), d)), ..*self }
    }
    /// The Section tool's R: the normal turned 90° about Z (to X when it is
    /// nearly Z), with `Plane.from_normal`'s x axis.
    pub fn rotated(&self) -> SectionPlane {
        let n = self.unit_normal();
        let normal = if n[2].abs() < 0.9 { unit(cross([0.0, 0.0, 1.0], n)).unwrap_or([1.0, 0.0, 0.0]) } else { [1.0, 0.0, 0.0] };
        let helper = if normal[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
        let x_axis = unit(cross(helper, normal)).unwrap_or([1.0, 0.0, 0.0]);
        SectionPlane { origin: self.origin, normal, x_axis }
    }
    /// Signed distance of `p` (mm): positive on the side the normal points to.
    pub fn distance(&self, p: V3) -> f64 {
        dot(sub(p, self.origin), self.unit_normal())
    }
    /// The same set of points as `other` (either normal sign), within 1e-6 mm.
    pub fn same_set(&self, other: &SectionPlane) -> bool {
        dot(self.unit_normal(), other.unit_normal()).abs() > 1.0 - 1e-9 && self.distance(other.origin).abs() <= 1e-6
    }
}

/// Every drawn body's bounds together (mm, RoboCAD's frame).
pub fn model_bounds(meshes: &CadMeshes) -> Option<(V3, V3)> {
    let mut out: Option<(V3, V3)> = None;
    for (_, (lo, hi)) in meshes.body_bounds() {
        let (lo, hi) = ([lo.x as f64, lo.y as f64, lo.z as f64], [hi.x as f64, hi.y as f64, hi.z as f64]);
        out = Some(match out {
            None => (lo, hi),
            Some((a, b)) => ([a[0].min(lo[0]), a[1].min(lo[1]), a[2].min(lo[2])], [b[0].max(hi[0]), b[1].max(hi[1]), b[2].max(hi[2])]),
        });
    }
    out
}

/// The kept part of `mesh` (`distance ≤ 0`, RoboCAD's clip plane), in
/// RoboCAD's triangle order: a kept triangle as it is, a removed one
/// collapsed to one point, a cut one's first triangle in its slot and its
/// second (a cut leaving four corners) appended after the last slot with the
/// same face. Cut points on a shared side are shared. A triangle naming a
/// missing vertex is kept as it is (`mesh::build` reports it).
pub fn clip(mesh: &MeshData, plane: &SectionPlane) -> MeshData {
    let mut vertices = mesh.vertices.clone();
    let distances: Vec<f64> = mesh.vertices.iter().map(|v| plane.distance(*v)).collect();
    let mut cuts: HashMap<(u32, u32), u32> = HashMap::new();
    let mut triangles = Vec::with_capacity(mesh.triangles.len());
    let mut faces = Vec::with_capacity(mesh.triangles.len());
    let mut extra: Vec<([u32; 3], i64)> = Vec::new();
    for (t, tri) in mesh.triangles.iter().enumerate() {
        let face = mesh.triangle_face.get(t).copied().unwrap_or(-1);
        faces.push(face);
        let d: Option<Vec<f64>> = tri.iter().map(|i| distances.get(*i as usize).copied()).collect();
        let Some(d) = d else {
            triangles.push(*tri);
            continue;
        };
        let inside = [d[0] <= 0.0, d[1] <= 0.0, d[2] <= 0.0];
        match inside.iter().filter(|x| **x).count() {
            3 => triangles.push(*tri),
            0 => triangles.push([tri[0]; 3]),
            _ => {
                let mut poly: Vec<u32> = Vec::with_capacity(4);
                for i in 0..3 {
                    let j = (i + 1) % 3;
                    if inside[i] {
                        poly.push(tri[i]);
                    }
                    if inside[i] != inside[j] {
                        let (lo, hi) = if tri[i] <= tri[j] { (tri[i], tri[j]) } else { (tri[j], tri[i]) };
                        let index = *cuts.entry((lo, hi)).or_insert_with(|| {
                            let (dl, dh) = (distances[lo as usize], distances[hi as usize]);
                            let (pl, ph) = (mesh.vertices[lo as usize], mesh.vertices[hi as usize]);
                            vertices.push(add(pl, scale(sub(ph, pl), dl / (dl - dh))));
                            (vertices.len() - 1) as u32
                        });
                        poly.push(index);
                    }
                }
                triangles.push([poly[0], poly[1], poly[2]]);
                if poly.len() == 4 {
                    extra.push(([poly[0], poly[2], poly[3]], face));
                }
            }
        }
    }
    for (tri, face) in extra {
        triangles.push(tri);
        faces.push(face);
    }
    MeshData { vertices, triangles, triangle_face: faces, face_count: mesh.face_count }
}

/// RoboCAD's `section_preview.mesh_segments`: where triangles cross the
/// plane (mm), coplanar triangles omitted, zero-length pieces dropped.
pub fn segments(mesh: &MeshData, plane: &SectionPlane) -> Vec<[Vec3; 2]> {
    let mut out = Vec::new();
    for tri in &mesh.triangles {
        let p: Option<Vec<V3>> = tri.iter().map(|i| mesh.vertices.get(*i as usize).copied()).collect();
        let Some(p) = p else { continue };
        let d = [plane.distance(p[0]), plane.distance(p[1]), plane.distance(p[2])];
        if !(d.iter().copied().fold(f64::INFINITY, f64::min) < 0.0 && d.iter().copied().fold(f64::NEG_INFINITY, f64::max) >= 0.0) {
            continue;
        }
        let mut ends: Vec<Vec3> = Vec::with_capacity(2);
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            if (d[a] < 0.0) != (d[b] < 0.0) {
                let q = add(p[a], scale(sub(p[b], p[a]), d[a] / (d[a] - d[b])));
                ends.push(Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32));
            }
        }
        if ends.len() == 2 && ends[0].distance_squared(ends[1]) > 1e-16 {
            out.push([ends[0], ends[1]]);
        }
    }
    out
}

/// RoboCAD's `printing.overhangs` (up +Z): the triangles whose normal faces
/// down more than `threshold` degrees from horizontal (and less than 89°
/// from straight down), as a mesh of their own.
pub fn overhangs(mesh: &MeshData, threshold: f64) -> MeshData {
    let mut out = MeshData { vertices: mesh.vertices.clone(), face_count: mesh.face_count, ..Default::default() };
    for (t, tri) in mesh.triangles.iter().enumerate() {
        let p: Option<Vec<V3>> = tri.iter().map(|i| mesh.vertices.get(*i as usize).copied()).collect();
        let Some(n) = p.and_then(|p| unit(cross(sub(p[1], p[0]), sub(p[2], p[0])))) else { continue };
        let cos = -n[2];
        if cos <= 0.0 {
            continue;
        }
        let from_down = cos.clamp(-1.0, 1.0).acos().to_degrees();
        if 90.0 - from_down > threshold && from_down < 89.0 {
            out.triangles.push(*tri);
            out.triangle_face.push(mesh.triangle_face.get(t).copied().unwrap_or(-1));
        }
    }
    out
}

/// `points` (mm) with the parts on the removed side (`distance > 0`) cut
/// away: the kept runs.
pub fn clip_polyline(points: &[Vec3], plane: &SectionPlane) -> Vec<Vec<Vec3>> {
    let d: Vec<f64> = points.iter().map(|p| plane.distance([p.x as f64, p.y as f64, p.z as f64])).collect();
    let mut out: Vec<Vec<Vec3>> = Vec::new();
    let mut run: Vec<Vec3> = Vec::new();
    let cut = |a: usize, b: usize| points[a] + (points[b] - points[a]) * (d[a] / (d[a] - d[b])) as f32;
    for i in 0..points.len() {
        let inside = d[i] <= 0.0;
        if i > 0 && inside != (d[i - 1] <= 0.0) {
            let x = cut(i - 1, i);
            if inside {
                run.push(x);
            } else {
                run.push(x);
                out.push(std::mem::take(&mut run));
            }
        }
        if inside {
            run.push(points[i]);
        }
    }
    if run.len() >= 2 {
        out.push(run);
    }
    out.retain(|r| r.len() >= 2);
    out
}

/// What a preview job builds for one body.
pub struct DerivedData {
    pub clipped: Option<Built>,
    /// The clipped copy's `triangle_face` (its triangle order).
    pub clipped_faces: Option<Vec<i64>>,
    pub overhang: Option<Built>,
    pub segments: Vec<[Vec3; 2]>,
}

/// One body's preview: the clipped copy (with `plane`), the overhang overlay
/// (with `overhang`) and the cut outline, from RoboCAD's tessellation.
pub fn derive_preview(id: &str, data: &MeshData, plane: Option<SectionPlane>, overhang: bool, ctx: &Ctx) -> Result<DerivedData, String> {
    let cut = plane.map(|p| clip(data, &p));
    let source = cut.as_ref().unwrap_or(data);
    let segments = plane.map(|p| segments(data, &p)).unwrap_or_default();
    if ctx.cancelled() {
        return Err(format!("the section preview of {id} was cancelled"));
    }
    let clipped = match &cut {
        Some(c) => Some(build(id, c)?),
        None => None,
    };
    let clipped_faces = cut.as_ref().map(|c| c.triangle_face.clone());
    let overhang = if overhang {
        let sub = overhangs(source, OVERHANG_DEG);
        if sub.triangles.is_empty() { None } else { Some(build(id, &sub)?) }
    } else {
        None
    };
    Ok(DerivedData { clipped, clipped_faces, overhang, segments })
}

fn to_mesh(built: Built) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, built.positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, built.normals);
    mesh.insert_indices(Indices::U32(built.indices));
    mesh
}

/// A preview's inputs: the drawn tessellation (by identity), plane, overhangs.
#[derive(Clone)]
struct Key {
    data: Arc<MeshData>,
    plane: Option<SectionPlane>,
    overhang: bool,
}
impl PartialEq for Key {
    fn eq(&self, other: &Key) -> bool {
        Arc::ptr_eq(&self.data, &other.data) && self.plane == other.plane && self.overhang == other.overhang
    }
}

struct Shown {
    /// The clipped copy and its `triangle_face`.
    clipped: Option<(Handle<Mesh>, Arc<Vec<i64>>)>,
    overhang: Option<Handle<Mesh>>,
    segments: Arc<Vec<[Vec3; 2]>>,
}

#[derive(Default)]
struct Body {
    shown: Option<(Key, Shown)>,
    job: Option<(Key, Job<DerivedData>)>,
    error: Option<(Key, String)>,
    /// The body's own mesh (as `mesh::sync` last set it).
    original: Option<Handle<Mesh>>,
    /// The clipped copy this system put on the body.
    applied: Option<Handle<Mesh>>,
    overlay: Option<(Entity, Handle<Mesh>)>,
}

/// The section preview and overhang overlays of the drawn bodies.
#[derive(Resource, Default)]
pub struct Derived {
    bodies: HashMap<String, Body>,
    /// Bumped when a shown preview changes (the outline lines rebuild on it).
    pub epoch: u64,
    material: Option<Handle<StandardMaterial>>,
}

impl Derived {
    /// The cut outline of each body whose shown preview is on `plane` (mm).
    pub fn outlines(&self, plane: &SectionPlane) -> impl Iterator<Item = &Arc<Vec<[Vec3; 2]>>> {
        self.bodies.values().filter_map(move |b| b.shown.as_ref().filter(|(k, _)| k.plane.as_ref() == Some(plane)).map(|(_, s)| &s.segments))
    }
}

/// Marks a body's overhang overlay.
#[derive(Component)]
struct OverhangOverlay;

/// SimSync (after `mesh::highlight`): start, take and show each body's
/// preview; restore the body's own mesh when the section goes off.
#[allow(clippy::too_many_arguments)]
pub(super) fn preview(
    mut commands: Commands,
    display: Option<Res<CadDisplay>>,
    meshes: Option<ResMut<CadMeshes>>,
    cache: Option<ResMut<Derived>>,
    mut assets: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bodies: Query<(&CadBody, &mut Mesh3d)>,
    root: Option<Single<Entity, With<CadRoot>>>,
    redraw: Option<MessageWriter<bevy::window::RequestRedraw>>,
) {
    let (Some(display), Some(mut meshes), Some(mut cache), Some(root)) = (display, meshes, cache, root) else { return };
    let cache = &mut *cache;
    let plane = if display.section.enabled { display.section.plane } else { None };
    let overhang = display.build_plate;
    let material = cache.material.get_or_insert_with(|| materials.add(StandardMaterial { base_color: OVERHANG, perceptual_roughness: 0.7, cull_mode: None, depth_bias: 10.0, ..default() })).clone();
    let mut seen: HashSet<String> = HashSet::new();
    for (body, mut mesh3d) in &mut bodies {
        seen.insert(body.id.clone());
        let entry = cache.bodies.entry(body.id.clone()).or_default();
        // Anything shown that is not the copy put here is the body's own mesh.
        if entry.applied.as_ref() != Some(&mesh3d.0) {
            entry.original = Some(mesh3d.0.clone());
            entry.applied = None;
        }
        let want = match meshes.mesh_data(&body.id) {
            Some(data) if plane.is_some() || overhang => Some(Key { data: data.clone(), plane, overhang }),
            _ => None,
        };
        if let Some(result) = entry.job.as_ref().and_then(|(_, job)| job.poll()) {
            let (key, _) = entry.job.take().expect("polled");
            match result {
                Ok(d) => {
                    let faces = Arc::new(d.clipped_faces.unwrap_or_default());
                    let shown = Shown { clipped: d.clipped.map(|b| (assets.add(to_mesh(b)), faces)), overhang: d.overhang.map(|b| assets.add(to_mesh(b))), segments: Arc::new(d.segments) };
                    entry.shown = Some((key, shown));
                    entry.error = None;
                    cache.epoch += 1;
                }
                Err(e) => {
                    // Kept with its inputs, so it is not retried until they change.
                    warn!("CAD section preview of {}: {e}", body.id);
                    entry.error = Some((key, e));
                }
            }
        }
        match &want {
            None => {
                entry.job = None;
                entry.error = None;
                if entry.shown.take().is_some() {
                    cache.epoch += 1;
                }
            }
            Some(key) => {
                let has = |k: Option<&Key>| k == Some(key);
                if !has(entry.shown.as_ref().map(|(k, _)| k)) && !has(entry.job.as_ref().map(|(k, _)| k)) && !has(entry.error.as_ref().map(|(k, _)| k)) {
                    let (k, id) = (key.clone(), body.id.clone());
                    let job = Job::spawn(Pool::Compute, 0, "cad-section-preview", move |ctx| derive_preview(&id, &k.data, k.plane, k.overhang, ctx));
                    entry.job = Some((key.clone(), job));
                }
            }
        }
        // A previous preview stays shown while the next builds, but never one
        // of the other kind (a clipped copy once the section is off).
        let like = |k: &Key| want.as_ref().is_some_and(|w| w.plane.is_some() == k.plane.is_some());
        let clipped = entry.shown.as_ref().filter(|(k, _)| like(k)).and_then(|(k, s)| s.clipped.clone().map(|(h, faces)| (h, ShownCopy { source: k.data.clone(), triangle_face: faces })));
        let copy = match clipped {
            Some((h, copy)) => {
                if mesh3d.0 != h {
                    mesh3d.0 = h.clone();
                }
                entry.applied = Some(h);
                Some(copy)
            }
            None => {
                if let (Some(_), Some(own)) = (entry.applied.take(), entry.original.clone()) {
                    if mesh3d.0 != own {
                        mesh3d.0 = own;
                    }
                }
                None
            }
        };
        // Picks read faces from what the body shows (`CadMeshes::face_of`).
        if !meshes.shows_copy(&body.id, copy.as_ref()) {
            meshes.set_shown_copy(&body.id, copy);
        }
        let overlay = entry.shown.as_ref().filter(|(k, _)| like(k) && k.overhang && overhang).and_then(|(_, s)| s.overhang.clone());
        match (overlay, entry.overlay.clone()) {
            (Some(h), Some((e, shown))) => {
                if shown != h {
                    commands.entity(e).try_insert(Mesh3d(h.clone()));
                    entry.overlay = Some((e, h));
                }
            }
            (Some(h), None) => {
                let e = commands.spawn((Mesh3d(h.clone()), MeshMaterial3d(material.clone()), Transform::default(), Visibility::default(), bevy::light::NotShadowCaster, Pickable::IGNORE, OverhangOverlay)).id();
                commands.entity(*root).add_child(e);
                entry.overlay = Some((e, h));
            }
            (None, Some((e, _))) => {
                commands.entity(e).try_despawn();
                entry.overlay = None;
            }
            (None, None) => {}
        }
    }
    // Bodies no longer drawn show no copy.
    let stale: Vec<String> = meshes.copy_ids().filter(|id| !seen.contains(*id)).map(str::to_string).collect();
    for id in stale {
        meshes.set_shown_copy(&id, None);
    }
    // Bodies no longer drawn: their previews and overlays go.
    let gone: Vec<String> = cache.bodies.keys().filter(|id| !seen.contains(*id)).cloned().collect();
    for id in gone {
        if let Some((e, _)) = cache.bodies.remove(&id).and_then(|b| b.overlay) {
            commands.entity(e).try_despawn();
        }
        cache.epoch += 1;
    }
    if let (true, Some(mut redraw)) = (cache.bodies.values().any(|b| b.job.is_some()), redraw) {
        redraw.write(bevy::window::RequestRedraw);
    }
}

/// The exact section's job.
#[derive(Resource, Default)]
pub struct ExactJob {
    latest: Latest<SectionCurves>,
    running: Option<ExactKey>,
    generation: Option<u64>,
}

/// A finished read is kept only when it answers the current request.
pub fn accept(request: Option<&ExactKey>, running: Option<ExactKey>, result: Result<SectionCurves, String>) -> Option<(ExactKey, Result<Arc<SectionCurves>, String>)> {
    let key = running?;
    (request == Some(&key)).then(|| (key, result.map(Arc::new)))
}

/// JobResults: take a finished read; start one for a request without an
/// answer; follow RoboCAD's revision while the section stays on the plane.
pub(super) fn exact_jobs(doc: Option<Res<CadDocument>>, display: Option<ResMut<CadDisplay>>, job: Option<ResMut<ExactJob>>, redraw: Option<MessageWriter<bevy::window::RequestRedraw>>) {
    let (Some(doc), Some(mut display), Some(mut job)) = (doc, display, job) else { return };
    match job.generation {
        Some(g) if g != doc.generation => {
            // Another document or connection: nothing asked of the old one stands.
            job.latest.cancel();
            job.running = None;
            if display.exact != super::ExactSection::default() {
                display.exact = super::ExactSection::default();
            }
        }
        _ => {}
    }
    job.generation = Some(doc.generation);
    if let Some((_, result)) = job.latest.poll() {
        let running = job.running.take();
        if let Some(answer) = accept(display.exact.request.as_ref(), running, result) {
            display.exact.result = Some(answer);
        }
    }
    let revision = doc.doc_key.as_ref().map_or(0, |k| k.1);
    if let Some(request) = display.exact.request.clone() {
        let follows = request.revision != revision && doc.stale.is_none() && display.section.enabled && display.section.plane == Some(request.plane);
        if follows {
            display.exact.request = Some(ExactKey { revision, ..request });
        }
    }
    match display.exact.request.clone() {
        None => {
            if job.running.take().is_some() {
                job.latest.cancel();
            }
        }
        Some(key) => {
            let answered = display.exact.result.as_ref().is_some_and(|(k, _)| *k == key);
            if !answered && job.running.as_ref() != Some(&key) {
                match doc.client.clone().filter(|_| doc.connected()) {
                    Some(client) => {
                        let (node, query) = (key.node.clone(), key.query.clone());
                        job.latest.start(Pool::Dedicated, "cad-section-exact", move |_| client.section(&node, &query).map_err(|e| e.to_string()));
                        job.running = Some(key);
                    }
                    None => display.exact.result = Some((key, Err("not connected to RoboCAD".into()))),
                }
            }
        }
    }
    if let (true, Some(mut redraw)) = (job.running.is_some(), redraw) {
        redraw.write(bevy::window::RequestRedraw);
    }
}

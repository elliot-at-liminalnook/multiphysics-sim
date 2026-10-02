//! Separate display layer; snapshots restore only entities this feature touched.
use super::*;
use crate::cad::mesh::CadBody;
use crate::camera::Orbit;
use bevy::asset::RenderAssetUsages;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use std::collections::HashMap;
#[derive(Component)]
struct CapturedBody(String);
#[derive(Resource, Default)]
struct Display {
    owner: Option<(u64, u64, bool)>,
    entities: Vec<Entity>,
    live: HashMap<Entity, (Visibility, Transform)>,
    camera: Option<(Entity, Orbit)>,
}
pub(crate) fn build(a: &mut App) {
    a.init_resource::<Display>().add_systems(
        Update,
        display
            .in_set(crate::app::ViewerSet::SimSync)
            .after(crate::cad::CadSet::Mesh)
            .after(crate::camera::CameraSet::Navigate)
            .before(crate::camera::CameraSet::Place),
    );
    a.add_systems(OnExit(crate::app::ModeScope::Cad), restore);
}
pub(crate) fn matrix(rows: [[f64; 4]; 4]) -> Transform {
    // Reference matrices are row-major world-mm deltas; Bevy Mat4 is columns.
    let cols = std::array::from_fn(|c| std::array::from_fn(|r| rows[r][c] as f32));
    Transform::from_matrix(Mat4::from_cols_array_2d(&cols))
}
fn display(
    mut commands: Commands,
    doc: Option<Res<CadDocument>>,
    mut review: ResMut<ReviewState>,
    mut motion: ResMut<crate::cad::motion::MotionState>,
    meshes: Option<Res<crate::cad::mesh::CadMeshes>>,
    mut display: ResMut<Display>,
    mut live: Query<(Entity, &CadBody, &mut Visibility, &mut Transform), Without<CapturedBody>>,
    mut captured: Query<(&CapturedBody, &mut Transform), Without<CadBody>>,
    mut camera: Query<(Entity, &mut Orbit)>,
    mut assets: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut gizmos: Gizmos,
) {
    let owner = doc.as_ref().and_then(|d| {
        if review.active {
            Some((d.generation, review.sequence, true))
        } else if motion.active {
            Some((d.generation, motion.sequence, false))
        } else {
            None
        }
    });
    if display.owner != owner {
        for e in display.entities.drain(..) {
            commands.entity(e).despawn();
        }
        for (e, _, mut visibility, mut transform) in &mut live {
            if let Some((v, t)) = display.live.remove(&e) {
                *visibility = v;
                *transform = t;
            }
        }
        display.live.clear();
        if let Some((e, saved)) = display.camera.take() {
            if let Ok((_, mut camera)) = camera.get_mut(e) {
                *camera = saved;
            }
        }
        display.owner = owner;
        if owner.is_some() {
            for (e, mut camera) in &mut camera {
                display.camera = Some((e, camera.clone()));
                camera.spin = 0.;
                break;
            }
        }
    }
    let Some((_, _, is_review)) = owner else {
        return;
    };
    if let Some(export) = motion.export.as_mut() {
        for (_, mut orbit) in &mut camera {
            if let Some(saved) = &export.camera {
                *orbit = saved.clone();
            } else {
                export.camera = Some(orbit.clone());
            }
            break;
        }
    }
    for (e, _, mut v, mut t) in &mut live {
        display.live.entry(e).or_insert((*v, *t));
        if is_review {
            *v = Visibility::Hidden;
        } else {
            *t = display.live[&e].1;
        }
    }
    if is_review {
        if let Some(c) = review.captured.as_mut() {
            if !c.built.is_empty() && display.entities.is_empty() {
                let material = materials.add(StandardMaterial {
                    base_color: Color::srgb(0.55, 0.7, 0.85),
                    cull_mode: None,
                    ..default()
                });
                for (id, b) in &c.built {
                    let mut mesh = Mesh::new(
                        PrimitiveTopology::TriangleList,
                        RenderAssetUsages::default(),
                    );
                    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, b.positions.clone());
                    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, b.normals.clone());
                    mesh.insert_indices(Indices::U32(b.indices.clone()));
                    let e = commands
                        .spawn((
                            Mesh3d(assets.add(mesh)),
                            MeshMaterial3d(material.clone()),
                            crate::cad::mesh::root_transform(),
                            CapturedBody(id.clone()),
                            DespawnOnExit(crate::app::ModeScope::Cad),
                        ))
                        .id();
                    display.entities.push(e);
                }
            }
        }
        if let Some(matrices) = review.sample["matrices"].as_object() {
            for (body, mut t) in &mut captured {
                let rows = matrices
                    .get(&body.0)
                    .and_then(|v| serde_json::from_value::<[[f64; 4]; 4]>(v.clone()).ok());
                *t = crate::cad::mesh::root_transform();
                if let Some(rows) = rows {
                    *t = *t * matrix(rows);
                }
            }
        }
        if let Some(arrows) = review.sample["flex"].as_array() {
            for arrow in arrows {
                if !review.filter.is_empty()
                    && arrow["node_ids"].as_array().is_some_and(|ids| {
                        !ids.iter()
                            .any(|id| id.as_str() == Some(review.filter.as_str()))
                    })
                {
                    continue;
                }
                let point = |key: &str| -> Option<Vec3> {
                    let p: [f64; 3] = serde_json::from_value(arrow[key].clone()).ok()?;
                    Some(Vec3::new(p[0] as f32, p[2] as f32, -p[1] as f32) * 0.001)
                };
                if let (Some(a), Some(b)) = (point("point_mm"), point("tip_mm")) {
                    gizmos.line(a, b, Color::srgb(0.9, 0.7, 0.2));
                }
            }
        }
    } else if let Some(sample) = &motion.sample {
        for (e, body, _, mut t) in &mut live {
            if let Some(rows) = sample.matrices.get(&body.id) {
                *t = matrix(*rows) * display.live[&e].1;
            }
        }
    }
    if motion.markers && !is_review {
        if let Some(metadata) = &motion.metadata {
            for joint in &metadata.joints {
                let mut p = Vec3::from_array(joint.pivot.map(|v| v as f32));
                let mut a = Vec3::from_array(joint.axis.map(|v| v as f32));
                if let Some(rows) = joint.parent.as_ref().and_then(|id| {
                    motion
                        .sample
                        .as_ref()
                        .and_then(|sample| sample.matrices.get(id))
                }) {
                    let delta = matrix(*rows);
                    p = delta.transform_point(p);
                    a = delta.rotation * a;
                }
                let p = p.to_array();
                let a = a.to_array();
                let point = Vec3::new(p[0] as f32, p[2] as f32, -p[1] as f32) * 0.001;
                let axis = Vec3::new(a[0] as f32, a[2] as f32, -a[1] as f32) * 0.015;
                gizmos.line(point - axis, point + axis, Color::srgb(1., 0.5, 0.2));
                let cad_axis = Vec3::from_array(a).normalize_or_zero();
                let cad_point = Vec3::from_array(p);
                let to_display = |p: Vec3| Vec3::new(p.x, p.z, -p.y) * 0.001;
                let current = motion
                    .sample
                    .as_ref()
                    .and_then(|sample| sample.positions.get(&joint.id))
                    .copied()
                    .or_else(|| motion.positions.get(&joint.id).copied())
                    .unwrap_or(joint.home);
                if joint.unit == "mm" {
                    let lo = cad_point + cad_axis * (joint.display_lower - joint.home) as f32;
                    let hi = cad_point + cad_axis * (joint.display_upper - joint.home) as f32;
                    gizmos.line(to_display(lo), to_display(hi), Color::srgb(0.5, 0.65, 0.8));
                    let current = cad_point + cad_axis * (current - joint.home) as f32;
                    let tangent = cad_axis.cross(Vec3::X).normalize_or_zero() * 2.;
                    gizmos.line(
                        to_display(current - tangent),
                        to_display(current + tangent),
                        Color::srgb(1., 0.7, 0.3),
                    );
                } else {
                    let reference = if cad_axis.x.abs() < 0.9 {
                        Vec3::X
                    } else {
                        Vec3::Y
                    };
                    let u = cad_axis.cross(reference).normalize_or_zero() * 15.;
                    let v = cad_axis.cross(u);
                    let at = |angle: f64| {
                        cad_point + u * (angle as f32).cos() + v * (angle as f32).sin()
                    };
                    let mut prev = at(joint.display_lower - joint.home);
                    for i in 1..=32 {
                        let angle = joint.display_lower
                            + (joint.display_upper - joint.display_lower) * i as f64 / 32.
                            - joint.home;
                        let next = at(angle);
                        gizmos.line(
                            to_display(prev),
                            to_display(next),
                            Color::srgb(0.5, 0.65, 0.8),
                        );
                        prev = next;
                    }
                    gizmos.line(
                        to_display(cad_point),
                        to_display(at(current - joint.home)),
                        Color::srgb(1., 0.7, 0.3),
                    );
                }
            }
        }
    }
    if motion.frame && !is_review {
        if let Some((lo, hi)) = meshes.as_ref().and_then(|m| {
            let ids = motion
                .metadata
                .as_ref()
                .and_then(|meta| {
                    motion
                        .joint
                        .as_ref()
                        .and_then(|joint| meta.focus_ids.get(joint))
                })
                .map(|ids| {
                    ids.iter()
                        .cloned()
                        .collect::<std::collections::HashSet<_>>()
                });
            m.bounds(ids.as_ref())
        }) {
            for (_, mut orbit) in &mut camera {
                orbit.focus = (lo + hi) * 0.5;
                orbit.radius = (hi - lo).length().max(0.01) * 2.;
            }
        }
        motion.frame = false;
    }
    if review.frame && is_review {
        if let Some(c) = &review.captured {
            let mut lo = Vec3::splat(f32::INFINITY);
            let mut hi = Vec3::splat(f32::NEG_INFINITY);
            for n in &c.geometry.nodes {
                if let Some(m) = &n.mesh {
                    for p in &m.vertices {
                        let raw = Vec3::from_array(p.map(|v| v as f32));
                        let raw = review.sample["matrices"]
                            .get(&n.id)
                            .and_then(|rows| {
                                serde_json::from_value::<[[f64; 4]; 4]>(rows.clone()).ok()
                            })
                            .map_or(raw, |rows| matrix(rows).transform_point(raw));
                        let p = Vec3::new(raw.x, raw.z, -raw.y) * 0.001;
                        lo = lo.min(p);
                        hi = hi.max(p);
                    }
                }
            }
            if lo.is_finite() && hi.is_finite() {
                for (_, mut orbit) in &mut camera {
                    orbit.focus = (lo + hi) * 0.5;
                    orbit.radius = (hi - lo).length().max(0.01) * 2.;
                }
            }
        }
        review.frame = false;
    }
}

fn restore(
    mut display: ResMut<Display>,
    mut live: Query<(Entity, &mut Visibility, &mut Transform), With<CadBody>>,
    mut camera: Query<(Entity, &mut Orbit)>,
) {
    for (e, mut v, mut t) in &mut live {
        if let Some((saved_v, saved_t)) = display.live.remove(&e) {
            *v = saved_v;
            *t = saved_t;
        }
    }
    if let Some((e, saved)) = display.camera.take() {
        if let Ok((_, mut orbit)) = camera.get_mut(e) {
            *orbit = saved;
        }
    }
    display.live.clear();
    display.entities.clear();
    display.owner = None;
}

//! CAD mode's selection display (display only, `ViewerSet::Present`):
//!
//! - **3D overlays** (Bevy gizmos, model mm → world through
//!   `CadView::world_from_model`): in edge mode every drawn body's sampled
//!   edges, faintly (a retained `Gizmo` under the Z-up root, rebuilt only
//!   when the mode, the topology or the drawn bodies change); in vertex mode
//!   every drawn body's vertices as small marks; the hovered item (a face's
//!   outline, an edge's polyline, a vertex mark) in the accent colour; the
//!   selected faces, edges, vertices and points in the selection colour (a
//!   point item `[id, "point", f]` names its face: its outline is drawn).
//!   A face's outline is the sides of its triangles (RoboCAD's drawn
//!   tessellation) not shared with another triangle of the same face,
//!   cached by (node, face) until the drawn meshes change. Body items keep
//!   the material highlight (`mesh::highlight`); a hovered body is drawn
//!   as its bounding box in the accent colour (the hover never touches the
//!   document or the materials). Lines are drawn slightly in front of the
//!   surfaces (depth bias) and hidden behind other bodies.
//! - **The selection strip** (top left of the 3D view): the mode segments
//!   (Bodies, Faces, Edges, Vertices, Points: the `cad:mode:*` controls) and
//!   Select All, Invert, Same Material, Edges → Faces (their controls), as
//!   kit buttons carrying `panel::CadButton`, so `panel::buttons` writes
//!   their actions and `Enabled` follows the control. Rebuilt only when
//!   `CadDocument.revision` changes.
//! - **The Alt menu**: a kit list at the Alt+click position listing the
//!   `cad:candidate:<n>` controls ("name: kind #i"); a choice writes
//!   `CadSelect`, which closes it, as do Escape (part D's `CadCancel`) and a
//!   press elsewhere in the 3D view (`pick`).
//! - **The rubber band** while box dragging (`pick::PickState::band`).
//!
//! Every UI root carries `DespawnOnExit(ModeScope::Cad)`; the retained
//! gizmo is a child of the 3D root, despawned with it.
//!
//! Known: the window's UI camera (a `Camera2d`) also draws 3D gizmo lines;
//! at the root's metre scale they fall within a pixel of the window centre
//! (as in robot mode).
use super::document::{CadDocument, SelectMode};
use super::mesh::{CadMeshes, CadRoot};
use super::panel::{CadButton, control, controls};
use super::pick::PickState;
use super::topology::CadTopology;
use super::view::CadView;
use crate::app::{ModeScope, ViewerMode, ViewerSet};
use crate::ui_kit::{ACCENT, BAR, BORDER, Kit, LEFT_WIDTH, Look, SURFACE, TOPBAR, UiFonts, wrap};
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::ui::prelude::AccessibleLabel;
use sim_runtime::cad_client::{MeshData, SelectionItem};
use std::collections::HashMap;
use std::sync::Arc;

/// The selection colour (the selected-body material's base colour, `mesh::CadMaterials`).
const SELECTED: Color = Color::srgb(0.98, 0.62, 0.22);
/// Edges and vertex marks shown for picking in edge and vertex modes.
const FAINT_LINES: Color = Color::srgba(0.80, 0.84, 0.90, 0.45);
/// Gizmo lines sit this far in front of the surfaces (`GizmoConfig::depth_bias`).
const DEPTH_BIAS: f32 = -0.01;
/// A vertex mark's half size (px).
const MARK_PX: f32 = 5.0;

/// The hover and selection lines' gizmo group: wider lines in front of the surfaces.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub(super) struct CadHighlightGizmos;

/// The retained gizmo drawing every edge in edge mode.
#[derive(Component)]
struct CadEdgeLines;
/// The selection strip's root.
#[derive(Component)]
struct CadSelectStrip;
/// The Alt menu's root.
#[derive(Component)]
struct CadMenu;
/// The rubber band.
#[derive(Component)]
struct CadBand;

pub(super) fn build(app: &mut App) {
    app.insert_gizmo_config(CadHighlightGizmos, GizmoConfig { depth_bias: DEPTH_BIAS, line: GizmoLineConfig { width: 3.0, ..default() }, ..default() })
        .add_systems(Update, (edge_lines, highlights, strip, menu, band).in_set(ViewerSet::Present).run_if(in_state(ViewerMode::Cad)));
}

fn model(p: &[f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

/// Present: the faint edges of edge mode, as one retained gizmo under the root.
#[allow(clippy::too_many_arguments)]
fn edge_lines(
    mut commands: Commands,
    doc: Option<Res<CadDocument>>,
    meshes: Option<Res<CadMeshes>>,
    topology: Option<Res<CadTopology>>,
    root: Option<Single<Entity, With<CadRoot>>>,
    lines: Query<(Entity, &Gizmo), With<CadEdgeLines>>,
    mut assets: ResMut<Assets<GizmoAsset>>,
    mut last: Local<Option<(Entity, u64, u64, u64)>>,
) {
    let (Some(doc), Some(meshes), Some(topology), Some(root)) = (doc, meshes, topology, root) else { return };
    let existing = lines.iter().next();
    if doc.select_mode != SelectMode::Edge {
        if let Some((entity, _)) = existing {
            commands.entity(entity).despawn();
        }
        *last = None;
        return;
    }
    let key = (*root, doc.generation, topology.epoch, meshes.epoch);
    if existing.is_some() && *last == Some(key) {
        return;
    }
    *last = Some(key);
    let mut asset = GizmoAsset::new();
    for (id, topo) in topology.ready() {
        if !meshes.shown(id) {
            continue;
        }
        for e in topo.edges.iter().filter(|e| e.points.len() >= 2) {
            asset.linestrip(e.points.iter().map(model), FAINT_LINES);
        }
    }
    match existing {
        Some((_, gizmo)) => {
            if let Some(mut a) = assets.get_mut(&gizmo.handle) {
                *a = asset;
            }
        }
        None => {
            let handle = assets.add(asset);
            let entity = commands.spawn((Gizmo { handle, line_config: GizmoLineConfig { width: 1.0, ..default() }, depth_bias: DEPTH_BIAS }, Transform::default(), CadEdgeLines)).id();
            commands.entity(*root).add_child(entity);
        }
    }
}

/// Face outlines (model mm) by (node, face), valid for one meshes epoch.
#[derive(Default)]
struct Outlines {
    epoch: Option<u64>,
    faces: HashMap<(String, i64), Arc<Vec<(Vec3, Vec3)>>>,
}

/// The sides of face `face`'s triangles not shared with another triangle of
/// the same face (by position), model mm.
pub(super) fn face_outline(mesh: &MeshData, face: i64) -> Vec<(Vec3, Vec3)> {
    type Key = [u64; 3];
    let key = |i: u32| mesh.vertices.get(i as usize).map(|v| v.map(f64::to_bits));
    let mut sides: HashMap<(Key, Key), (usize, Vec3, Vec3)> = HashMap::new();
    for (t, tri) in mesh.triangles.iter().enumerate() {
        if mesh.triangle_face.get(t) != Some(&face) {
            continue;
        }
        for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
            let (Some(ka), Some(kb)) = (key(a), key(b)) else { continue };
            if ka == kb {
                continue;
            }
            let k = if ka <= kb { (ka, kb) } else { (kb, ka) };
            let side = sides.entry(k).or_insert((0, model(&mesh.vertices[a as usize]), model(&mesh.vertices[b as usize])));
            side.0 += 1;
        }
    }
    let mut out: Vec<(Vec3, Vec3)> = sides.into_values().filter(|(n, ..)| *n == 1).map(|(_, a, b)| (a, b)).collect();
    // A stable order (the map's is not).
    out.sort_by(|x, y| x.0.to_array().partial_cmp(&y.0.to_array()).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// A vertex mark: a small cross, [`MARK_PX`] on screen.
fn mark(gizmos: &mut Gizmos<CadHighlightGizmos>, view: &CadView, p: Vec3, color: Color) {
    let scale = view.world_from_model.transform_vector3(Vec3::X).length();
    let half = view.mm_per_pixel(p).unwrap_or(0.0) * MARK_PX * scale;
    if half > 0.0 {
        gizmos.cross(Isometry3d::from_translation(view.world_from_model.transform_point3(p)), half, color);
    }
}

/// One item's highlight: a face's (or point's face's) outline, an edge's
/// polyline, a vertex mark, a body's bounding box (drawn only for the
/// hovered body: a selected body is the material's, `mesh::highlight`).
fn draw(gizmos: &mut Gizmos<CadHighlightGizmos>, view: &CadView, meshes: &CadMeshes, topology: Option<&CadTopology>, outlines: &mut Outlines, item: &SelectionItem, color: Color) {
    let SelectionItem(node, kind, index) = item;
    let world = |p: Vec3| view.world_from_model.transform_point3(p);
    match kind.as_str() {
        "face" | "point" => {
            let key = (node.clone(), *index);
            let outline = match outlines.faces.get(&key) {
                Some(o) => o.clone(),
                None => {
                    let Some(mesh) = meshes.mesh_data(node) else { return };
                    let o = Arc::new(face_outline(mesh, *index));
                    outlines.faces.insert(key, o.clone());
                    o
                }
            };
            for (a, b) in outline.iter() {
                gizmos.line(world(*a), world(*b), color);
            }
        }
        "edge" => {
            let Some(edge) = topology.and_then(|t| t.get(node)).and_then(|t| t.edges.iter().find(|e| e.index == *index)) else { return };
            if edge.points.len() >= 2 {
                gizmos.linestrip(edge.points.iter().map(|p| world(model(p))), color);
            }
        }
        "body" => {
            let Some((lo, hi)) = meshes.body_bounds().find(|(id, _)| *id == node.as_str()).map(|(_, b)| b) else { return };
            let corner = |i: usize| world(Vec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z }));
            // The 12 box edges: corner pairs differing in one axis bit.
            for i in 0..8usize {
                for bit in [1usize, 2, 4] {
                    if i & bit == 0 {
                        gizmos.line(corner(i), corner(i | bit), color);
                    }
                }
            }
        }
        "vertex" => {
            let point = topology.and_then(|t| t.get(node)).and_then(|t| t.vertices.iter().find(|v| v.index == *index)).and_then(|v| v.point);
            if let Some(p) = point {
                mark(gizmos, view, model(&p), color);
            }
        }
        _ => {}
    }
}

/// Present: the hovered item, the selected sub-body items, and the vertex marks of vertex mode.
fn highlights(doc: Option<Res<CadDocument>>, meshes: Option<Res<CadMeshes>>, topology: Option<Res<CadTopology>>, view: Option<Res<CadView>>, mut gizmos: Gizmos<CadHighlightGizmos>, mut outlines: Local<Outlines>) {
    let (Some(doc), Some(meshes), Some(view)) = (doc, meshes, view) else { return };
    if !view.valid {
        return;
    }
    if outlines.epoch != Some(meshes.epoch) {
        outlines.epoch = Some(meshes.epoch);
        outlines.faces.clear();
    }
    let topology = topology.as_deref();
    if doc.select_mode == SelectMode::Vertex
        && let Some(topology) = topology
    {
        for (id, topo) in topology.ready() {
            if !meshes.shown(id) {
                continue;
            }
            for v in &topo.vertices {
                if let Some(p) = v.point {
                    mark(&mut gizmos, &view, model(&p), FAINT_LINES);
                }
            }
        }
    }
    for item in doc.selection.iter().filter(|i| i.1 != "body") {
        draw(&mut gizmos, &view, &meshes, topology, &mut outlines, item, SELECTED);
    }
    if let Some(item) = &doc.hover {
        draw(&mut gizmos, &view, &meshes, topology, &mut outlines, item, ACCENT);
    }
}

/// The strip's buttons: (control id, label).
const COMMANDS: [(&str, &str); 4] = [("cad:select_all", "Select All"), ("cad:invert_selection", "Invert"), ("cad:select_same_material", "Same Material"), ("cad:edges_to_faces", "Edges → Faces")];

/// Present: the selection strip, rebuilt when the document's revision changes.
fn strip(mut commands: Commands, doc: Option<Res<CadDocument>>, fonts: Res<UiFonts>, roots: Query<Entity, With<CadSelectStrip>>, mut last: Local<Option<(Entity, u64, u64)>>) {
    let Some(doc) = doc else { return };
    let root = match roots.iter().next() {
        Some(root) => root,
        None => commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(LEFT_WIDTH + 8.0),
                    top: Val::Px(TOPBAR + super::surfaces::COMMAND_BAR + 8.0),
                    max_width: Val::Px(560.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::FlexStart,
                    row_gap: Val::Px(6.0),
                    padding: UiRect::all(Val::Px(6.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    ..default()
                },
                BackgroundColor(BAR),
                BorderColor::all(BORDER),
                FocusPolicy::Block,
                ZIndex(1),
                AccessibleLabel::new("CAD selection modes and commands"),
                CadSelectStrip,
                DespawnOnExit(ModeScope::Cad),
            ))
            .id(),
    };
    let stamp = (root, doc.generation, doc.revision);
    if *last == Some(stamp) {
        return;
    }
    *last = Some(stamp);
    let k = Kit::new(&fonts);
    let all = controls(&doc);
    commands.entity(root).despawn_related::<Children>();
    commands.entity(root).with_children(|p| {
        p.spawn(k.segments()).with_children(|s| {
            for mode in SelectMode::ALL {
                if let Some(c) = control(&all, &format!("cad:mode:{}", mode.name())) {
                    s.spawn(k.segment(mode.label(), CadButton(c.action.clone()), mode == doc.select_mode, c.ready.is_ok()));
                }
            }
        });
        p.spawn(wrap()).with_children(|r| {
            for (id, label) in COMMANDS {
                if let Some(c) = control(&all, id) {
                    r.spawn(k.button(label, CadButton(c.action.clone()), Look::Secondary, c.ready.is_ok()));
                }
            }
        });
    });
}

/// Present: the Alt menu while `CadDocument.candidates` is set.
#[allow(clippy::too_many_arguments)]
fn menu(mut commands: Commands, doc: Option<Res<CadDocument>>, view: Option<Res<CadView>>, state: Res<PickState>, fonts: Res<UiFonts>, menus: Query<Entity, With<CadMenu>>, mut last: Local<Option<(Entity, u64, u64)>>) {
    let open = doc.as_ref().is_some_and(|d| d.candidates.is_some());
    let existing = menus.iter().next();
    let Some(doc) = doc.filter(|_| open) else {
        if let Some(menu) = existing {
            commands.entity(menu).despawn();
        }
        *last = None;
        return;
    };
    let at = state.menu_at.or_else(|| view.as_ref().map(|v| v.min + Vec2::splat(24.0))).unwrap_or(Vec2::new(LEFT_WIDTH + 24.0, TOPBAR + 24.0));
    let menu = match existing {
        Some(menu) => menu,
        None => commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(at.x + 4.0),
                    top: Val::Px(at.y + 4.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Stretch,
                    row_gap: Val::Px(2.0),
                    padding: UiRect::all(Val::Px(4.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    ..default()
                },
                BackgroundColor(SURFACE),
                BorderColor::all(BORDER),
                FocusPolicy::Block,
                ZIndex(2),
                AccessibleLabel::new("Choose what to select"),
                CadMenu,
                DespawnOnExit(ModeScope::Cad),
            ))
            .id(),
    };
    let stamp = (menu, doc.generation, doc.revision);
    if *last == Some(stamp) {
        return;
    }
    *last = Some(stamp);
    let k = Kit::new(&fonts);
    let entries: Vec<_> = controls(&doc).into_iter().filter(|c| c.id.starts_with("cad:candidate:")).collect();
    commands.entity(menu).despawn_related::<Children>();
    commands.entity(menu).with_children(|p| {
        p.spawn(k.note("Select which (Escape closes)"));
        for c in entries {
            p.spawn(k.button(&c.label, CadButton(c.action), Look::Ghost, c.ready.is_ok()));
        }
    });
}

/// Present: the rubber band while box dragging.
fn band(mut commands: Commands, state: Res<PickState>, mut bands: Query<(Entity, &mut Node), With<CadBand>>) {
    let existing = bands.iter_mut().next();
    let Some((a, b)) = state.band else {
        if let Some((entity, _)) = existing {
            commands.entity(entity).despawn();
        }
        return;
    };
    let (min, size) = (a.min(b), (a - b).abs());
    let place = |node: &mut Node| {
        node.left = Val::Px(min.x);
        node.top = Val::Px(min.y);
        node.width = Val::Px(size.x);
        node.height = Val::Px(size.y);
    };
    match existing {
        Some((_, mut node)) => {
            if node.left != Val::Px(min.x) || node.top != Val::Px(min.y) || node.width != Val::Px(size.x) || node.height != Val::Px(size.y) {
                place(&mut node);
            }
        }
        None => {
            let mut node = Node { position_type: PositionType::Absolute, border: UiRect::all(Val::Px(1.0)), ..default() };
            place(&mut node);
            commands.spawn((node, BorderColor::all(ACCENT), BackgroundColor(ACCENT.with_alpha(0.12)), Pickable::IGNORE, FocusPolicy::Pass, ZIndex(3), CadBand, DespawnOnExit(ModeScope::Cad)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two triangles of one square face: the outline is the square's four
    /// sides, not the shared diagonal; another face's triangle is ignored.
    #[test]
    fn a_face_outline_leaves_out_shared_sides() {
        let mesh = MeshData {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, -1.0]],
            triangles: vec![[0, 1, 2], [0, 2, 3], [1, 0, 4]],
            triangle_face: vec![0, 0, 1],
            face_count: 2,
        };
        let outline = face_outline(&mesh, 0);
        assert_eq!(outline.len(), 4);
        let diagonal = |(a, b): &(Vec3, Vec3)| (*a == Vec3::ZERO && *b == Vec3::new(1.0, 1.0, 0.0)) || (*b == Vec3::ZERO && *a == Vec3::new(1.0, 1.0, 0.0));
        assert!(!outline.iter().any(diagonal));
        assert_eq!(face_outline(&mesh, 1).len(), 3);
        assert!(face_outline(&mesh, 7).is_empty());
    }
}

//! Drawing the display state (display only; RoboCAD is never asked to change
//! anything):
//!
//! - **Display modes** ([`materials`], SimSync after `mesh::highlight`):
//!   each body's material (its colour's, or the selection's) is swapped for
//!   a derived copy per mode and swapped back in shaded, shaded_edges and
//!   render. Xray: RoboCAD's 0.35 alpha, blended without depth writes, plus
//!   edges. Wireframe: a fully transparent copy (the body stays pickable,
//!   as RoboCAD picks in wireframe) plus edges in 0.6 × colour + 0.3; a mesh
//!   node, which has no B-rep edges, shows its triangle sides (RoboCAD's
//!   `GL_LINE` polygon mode). Matcap: RoboCAD's procedural clay
//!   (`_make_matcap`: a lit sphere image, 235/225/210 tint, sampled by the
//!   view-space normal and multiplied by the body colour) is approximated
//!   without a custom shader: the colour times the clay tint, fully rough,
//!   no metal and little specular, lit by the view-following headlight, so
//!   the shading follows the view as a matcap does; its rim term is not
//!   reproduced. Render: RoboCAD's three camera-fixed lights and ground
//!   shadow become the headlight casting shadows (bodies shadow each other)
//!   plus RoboCAD's fill (−0.8, 0.3, 0.4; 0.35, 0.4, 0.5) and back (0.2, 0.9,
//!   −0.3; 0.25, 0.22, 0.2) lights; there is no ground shadow (Bevy has no
//!   shadow-catcher material) and the ambient is not lowered (0.22 there).
//! - **Display edges** ([`edges_sync`], [`lines`]): RoboCAD's sampled B-rep
//!   edges (`GET /nodes/{id}/edges?samples=24`), read from `CadTopology`
//!   when it holds the node, else fetched here on `Pool::Dedicated` (at most
//!   [`MAX_FETCHES`] at once) for every drawn body while the mode shows
//!   edges; dropped on a new revision, as the topology cache does. Drawn in
//!   RoboCAD's 0.08, 0.08, 0.1 (black in high contrast) as one retained
//!   gizmo under the Z-up root, cut by the section plane.
//! - **Curve nodes** ([`edges_sync`], [`lines`]): RoboCAD's `_curve_item`
//!   and `_draw_curve_item` draw a visible `curve` node as its sampled edges
//!   (`sample_edges(count=32)`: `GET /nodes/{id}/edges?samples=32`, fetched
//!   here on `Pool::Dedicated` in every display mode and dropped on a new
//!   revision), 2 px wide, in the node's colour or RoboCAD's 0.35, 0.8, 1.0,
//!   and in 1.0, 0.65, 0.2 while any item of the node is selected; cut by
//!   the section plane (RoboCAD draws them under its clip plane). Display
//!   only: curves are not picked here (RoboCAD's 8 px curve pick pass is
//!   not ported).
//! - **Grid, axes, outlines** ([`lines`]): retained gizmos rebuilt only when
//!   their inputs change. **Build plate and section plane** ([`quads`]):
//!   translucent unlit quads (RoboCAD's colours). **Lights and background**
//!   ([`lights`]).
use super::section::{Derived, clip_polyline, model_bounds};
use super::{BUILD_PLATE_MM, CadDisplay, DisplayMode, GRID_STEP_MM, GRID_STEPS, SectionPlane};
use crate::cad::document::CadDocument;
use crate::cad::mesh::{BODY_KINDS, CadBody, CadMeshes, CadRoot};
use crate::cad::topology::{CadTopology, EDGE_SAMPLES};
use crate::jobs::{Job, Pool};
use bevy::asset::AssetId;
use bevy::light::{CascadeShadowConfigBuilder, NotShadowCaster};
use bevy::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Edge requests in flight at once.
pub const MAX_FETCHES: usize = 2;
/// Lines sit this far in front of the surfaces (`Gizmo::depth_bias`).
const DEPTH_BIAS: f32 = -0.01;
/// RoboCAD's edge colour, and high contrast's.
const EDGE: Color = Color::srgb(0.08, 0.08, 0.1);
const EDGE_CONTRAST: Color = Color::srgb(0.0, 0.0, 0.0);
/// RoboCAD's grid colour (major lines; minor ones are 0.7 ×) and high contrast's.
const GRID: [f32; 3] = [0.36, 0.38, 0.42];
const GRID_CONTRAST_MAJOR: Color = Color::srgb(0.55, 0.55, 0.6);
const GRID_CONTRAST_MINOR: Color = Color::srgb(0.8, 0.8, 0.85);
/// RoboCAD's axis colours.
const AXIS_X: Color = Color::srgb(0.8, 0.3, 0.3);
const AXIS_Y: Color = Color::srgb(0.3, 0.75, 0.3);
const AXIS_Z: Color = Color::srgb(0.3, 0.45, 0.9);
/// RoboCAD's section outline and plane colour; the exact section's.
const SECTION: Color = Color::srgb(1.0, 0.4, 0.3);
const EXACT: Color = Color::srgb(1.0, 0.85, 0.3);
/// RoboCAD's curve colour (`_curve_item`'s default), its selected curve
/// colour (`_draw_curve_item`), line width (px) and samples per edge.
const CURVE: Color = Color::srgb(0.35, 0.8, 1.0);
const CURVE_SELECTED: Color = Color::srgb(1.0, 0.65, 0.2);
const CURVE_WIDTH: f32 = 2.0;
pub const CURVE_SAMPLES: u32 = 32;
/// RoboCAD's high-contrast viewport background.
const BACKGROUND_CONTRAST: Color = Color::srgb(0.98, 0.98, 0.99);

/// A display mode's material look (None: the body's own material).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MaterialLook {
    Xray,
    Wireframe,
    Matcap,
}

pub fn look(mode: DisplayMode) -> Option<MaterialLook> {
    match mode {
        DisplayMode::Xray => Some(MaterialLook::Xray),
        DisplayMode::Wireframe => Some(MaterialLook::Wireframe),
        DisplayMode::Matcap => Some(MaterialLook::Matcap),
        DisplayMode::Shaded | DisplayMode::ShadedEdges | DisplayMode::Render => None,
    }
}

/// `base` as a display mode draws it.
pub fn derive_material(mut m: StandardMaterial, look: MaterialLook) -> StandardMaterial {
    match look {
        MaterialLook::Xray => {
            m.base_color = m.base_color.with_alpha(0.35);
            m.alpha_mode = AlphaMode::Blend;
        }
        MaterialLook::Wireframe => {
            m.base_color = m.base_color.with_alpha(0.0);
            m.alpha_mode = AlphaMode::Blend;
            m.unlit = true;
        }
        MaterialLook::Matcap => {
            let c = m.base_color.to_srgba();
            m.base_color = Color::srgb(c.red * 0.922, c.green * 0.882, c.blue * 0.824);
            m.perceptual_roughness = 1.0;
            m.metallic = 0.0;
            m.reflectance = 0.1;
        }
    }
    m
}

/// The derived materials, by (base material, look), and each one's base.
#[derive(Resource, Default)]
pub struct DisplayMaterials {
    base_of: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
    derived: HashMap<(AssetId<StandardMaterial>, MaterialLook), Handle<StandardMaterial>>,
}
impl DisplayMaterials {
    /// The body's own material behind `shown` (itself when not derived).
    pub fn base(&self, shown: &Handle<StandardMaterial>) -> Handle<StandardMaterial> {
        self.base_of.get(&shown.id()).cloned().unwrap_or_else(|| shown.clone())
    }
}

/// SimSync (after `mesh::highlight`, which sets each body's own material):
/// the display mode's material on every body.
pub(super) fn materials(display: Option<Res<CadDisplay>>, cache: Option<ResMut<DisplayMaterials>>, mut assets: ResMut<Assets<StandardMaterial>>, mut bodies: Query<&mut MeshMaterial3d<StandardMaterial>, With<CadBody>>) {
    let (Some(display), Some(mut cache)) = (display, cache) else { return };
    let look = look(display.mode);
    for mut material in &mut bodies {
        let base = cache.base(&material.0);
        let want = match look {
            None => base,
            Some(look) => match cache.derived.get(&(base.id(), look)) {
                Some(h) => h.clone(),
                None => {
                    let Some(source) = assets.get(&base).cloned() else { continue };
                    let h = assets.add(derive_material(source, look));
                    cache.base_of.insert(h.id(), base.clone());
                    cache.derived.insert((base.id(), look), h.clone());
                    h
                }
            },
        };
        if material.0 != want {
            material.0 = want;
        }
    }
}

type Polylines = Arc<Vec<Vec<[f64; 3]>>>;

/// Edge polylines fetched for display (bodies `CadTopology` does not hold).
#[derive(Resource, Default)]
pub struct DisplayEdges {
    /// (generation, document id, RoboCAD revision) the entries belong to.
    key: Option<(u64, Option<String>, u64)>,
    own: HashMap<String, Result<Polylines, String>>,
    fetching: Vec<(String, Job<Vec<Vec<[f64; 3]>>>)>,
    /// Curve nodes' sampled edges ([`CURVE_SAMPLES`] each), fetched in every mode.
    curves: HashMap<String, Result<Polylines, String>>,
    curve_fetching: Vec<(String, Job<Vec<Vec<[f64; 3]>>>)>,
    /// Bumped when an entry arrives or the cache is dropped.
    pub epoch: u64,
}

/// Take the finished fetches of `fetching` at `revision` into `into`; true
/// when one landed.
fn take_fetched(fetching: &mut Vec<(String, Job<Vec<Vec<[f64; 3]>>>)>, into: &mut HashMap<String, Result<Polylines, String>>, revision: u64) -> bool {
    let mut landed = false;
    let mut i = 0;
    while i < fetching.len() {
        let Some(result) = fetching[i].1.poll() else {
            i += 1;
            continue;
        };
        let (id, job) = fetching.swap_remove(i);
        if job.generation() == revision {
            into.insert(id, result.map(Arc::new));
            landed = true;
        }
    }
    landed
}

/// A sampled-edges fetch of `node` on `Pool::Dedicated`; its generation is
/// the revision it reads, checked on arrival.
fn fetch_edges(local: std::sync::Arc<crate::cad::sync::LocalSnapshot>, node: String, samples: u32, revision: u64) -> Job<Vec<Vec<[f64; 3]>>> {
    Job::spawn(Pool::Compute, revision, "cad-display-edges", move |_| {
        // A node without B-rep geometry (a mesh node) has no edges.
        let Ok(bytes) = sim_cad::geometry::resolved_brep(&local.archive, &node) else { return Ok(Vec::new()) };
        let t = sim_cad::kernel::full_topology(&bytes, samples as i32)?;
        Ok(t["edges"].as_array().into_iter().flatten().map(|e| e["points"].as_array().into_iter().flatten().filter_map(|p| serde_json::from_value::<[f64; 3]>(p.clone()).ok()).collect::<Vec<_>>()).filter(|p| p.len() >= 2).collect())
    })
}

/// The visible curve nodes of the shown tree.
fn curve_nodes(doc: &CadDocument) -> impl Iterator<Item = &sim_runtime::cad_client::NodeSummary> {
    doc.doc.iter().flat_map(|d| d.nodes.iter()).filter(|n| n.effective_visible && n.kind == "curve")
}

/// SimSync: fetch the visible curve nodes' edges, and the drawn bodies'
/// edges while the mode shows them.
pub(super) fn edges_sync(
    doc: Option<Res<CadDocument>>,
    display: Option<Res<CadDisplay>>,
    meshes: Option<Res<CadMeshes>>,
    topology: Option<Res<CadTopology>>,
    edges: Option<ResMut<DisplayEdges>>,
    redraw: Option<MessageWriter<bevy::window::RequestRedraw>>,
) {
    let (Some(doc), Some(display), Some(meshes), Some(mut edges)) = (doc, display, meshes, edges) else { return };
    let e = &mut *edges;
    let key = (doc.generation, doc.doc_key.as_ref().and_then(|k| k.0.clone()), doc.doc_key.as_ref().map_or(0, |k| k.1));
    if e.key.as_ref() != Some(&key) {
        e.key = Some(key.clone());
        e.own.clear();
        e.fetching.clear();
        e.curves.clear();
        e.curve_fetching.clear();
        e.epoch += 1;
    }
    if take_fetched(&mut e.curve_fetching, &mut e.curves, key.2) {
        e.epoch += 1;
    }
    let client = doc.local.clone().filter(|_| doc.connected() && doc.stale.is_none());
    if let Some(client) = &client {
        let mut curves: Vec<&str> = curve_nodes(&doc).map(|n| n.id.as_str()).collect();
        curves.sort_unstable();
        for id in curves {
            if e.curve_fetching.len() >= MAX_FETCHES {
                break;
            }
            if e.curves.contains_key(id) || e.curve_fetching.iter().any(|(f, _)| f == id) {
                continue;
            }
            e.curve_fetching.push((id.to_string(), fetch_edges(client.clone(), id.to_string(), CURVE_SAMPLES, key.2)));
        }
    }
    let curves_pending = !e.curve_fetching.is_empty();
    if !display.mode.edges() {
        e.fetching.clear();
    } else {
        if take_fetched(&mut e.fetching, &mut e.own, key.2) {
            e.epoch += 1;
        }
        if let (Some(client), Some(state)) = (client, &doc.doc) {
            start_body_edges(e, &meshes, topology.as_deref(), state, &client, key.2);
        }
    }
    if let (true, Some(mut redraw)) = (curves_pending || !e.fetching.is_empty(), redraw) {
        redraw.write(bevy::window::RequestRedraw);
    }
}

/// Start edge fetches for the drawn bodies `CadTopology` does not hold.
fn start_body_edges(e: &mut DisplayEdges, meshes: &CadMeshes, topology: Option<&CadTopology>, state: &sim_runtime::cad_client::DocState, client: &std::sync::Arc<crate::cad::sync::LocalSnapshot>, revision: u64) {
    let mut ids: Vec<&str> = state.nodes.iter().filter(|n| n.effective_visible && BODY_KINDS.contains(&n.kind.as_str()) && meshes.shown(&n.id)).map(|n| n.id.as_str()).collect();
    ids.sort_unstable();
    for id in ids {
        if e.fetching.len() >= MAX_FETCHES {
            break;
        }
        let held = topology.is_some_and(|t| t.get(id).is_some() || t.pending(id));
        if held || e.own.contains_key(id) || e.fetching.iter().any(|(f, _)| f == id) {
            continue;
        }
        e.fetching.push((id.to_string(), fetch_edges(client.clone(), id.to_string(), EDGE_SAMPLES, revision)));
    }
}

/// One retained gizmo layer under the Z-up root.
#[derive(Component, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) enum Layer {
    Edges,
    Grid,
    Axes,
    Outline,
    Exact,
    Curves,
}

fn v(p: &[f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

fn hash_plane(h: &mut DefaultHasher, plane: Option<&SectionPlane>) {
    plane.map(|p| [p.origin, p.normal, p.x_axis].map(|a| a.map(f64::to_bits))).hash(h);
}

/// `points` as strips, cut by the section plane when it is on.
fn strips(asset: &mut GizmoAsset, points: Vec<Vec3>, cut: Option<&SectionPlane>, color: Color) {
    match cut {
        Some(plane) => {
            for run in clip_polyline(&points, plane) {
                asset.linestrip(run, color);
            }
        }
        None if points.len() >= 2 => asset.linestrip(points, color),
        None => {}
    }
}

/// RoboCAD's `_draw_curve_item` colour: the selected curve colour, else
/// the node's own colour, else RoboCAD's curve default.
pub fn curve_color(color: Option<&[f64]>, selected: bool) -> Color {
    match color {
        _ if selected => CURVE_SELECTED,
        Some(c) if c.len() >= 3 => Color::srgb(c[0] as f32, c[1] as f32, c[2] as f32),
        _ => CURVE,
    }
}

/// The section plane while the section is on.
fn cut_plane(display: &CadDisplay) -> Option<&SectionPlane> {
    display.section.enabled.then_some(display.section.plane.as_ref()).flatten()
}

/// Present: the edges, grid, axes, section outline and exact section, each a
/// retained gizmo rebuilt only when what it draws changed.
#[allow(clippy::too_many_arguments)]
pub(super) fn lines(
    mut commands: Commands,
    doc: Option<Res<CadDocument>>,
    display: Option<Res<CadDisplay>>,
    meshes: Option<Res<CadMeshes>>,
    topology: Option<Res<CadTopology>>,
    (edges, derived, chosen): (Option<Res<DisplayEdges>>, Option<Res<Derived>>, Option<Res<DisplayMaterials>>),
    materials: Res<Assets<StandardMaterial>>,
    bodies: Query<(&CadBody, &MeshMaterial3d<StandardMaterial>)>,
    root: Option<Single<Entity, With<CadRoot>>>,
    existing: Query<(Entity, &Layer, &Gizmo)>,
    mut assets: ResMut<Assets<GizmoAsset>>,
    mut stamps: Local<HashMap<Layer, u64>>,
    selection: crate::cad::selection::CadSelection,
) {
    let (Some(doc), Some(display), Some(meshes), Some(root)) = (doc, display, meshes, root) else { return };
    let cut = cut_plane(&display);
    let revision = doc.doc_key.as_ref().map_or(0, |k| k.1);
    let present: HashMap<Layer, (Entity, Handle<GizmoAsset>)> = existing.iter().map(|(e, l, g)| (*l, (e, g.handle.clone()))).collect();
    let selected = crate::cad::selection::CadItems::nodes(selection.items().as_slice());
    for layer in [Layer::Edges, Layer::Grid, Layer::Axes, Layer::Outline, Layer::Exact, Layer::Curves] {
        let mut h = DefaultHasher::new();
        hash_plane(&mut h, cut);
        let active = match layer {
            Layer::Edges => {
                (display.mode, display.high_contrast, doc.generation, revision, meshes.epoch, topology.as_ref().map(|t| t.epoch), edges.as_ref().map(|e| e.epoch)).hash(&mut h);
                if display.mode == DisplayMode::Wireframe {
                    // Wireframe edges take each body's colour (the selection's while selected).
                    for (body, material) in &bodies {
                        (body.id.as_str(), chosen.as_ref().map_or_else(|| material.0.id(), |c| c.base(&material.0).id())).hash(&mut h);
                    }
                }
                display.mode.edges()
            }
            Layer::Grid | Layer::Axes => {
                display.high_contrast.hash(&mut h);
                display.grid
            }
            Layer::Outline => {
                derived.as_ref().map(|d| d.epoch).hash(&mut h);
                cut.is_some()
            }
            Layer::Exact => {
                let drawn = display.exact.drawn(&display.section, revision);
                drawn.map(|s| Arc::as_ptr(s) as usize).hash(&mut h);
                drawn.is_some()
            }
            Layer::Curves => {
                (doc.generation, revision, edges.as_ref().map(|e| e.epoch)).hash(&mut h);
                let mut any = false;
                for n in curve_nodes(&doc) {
                    any = true;
                    (n.id.as_str(), n.color.as_ref().map(|c| c.iter().map(|x| x.to_bits()).collect::<Vec<_>>()), selected.contains(&n.id)).hash(&mut h);
                }
                any
            }
        };
        let stamp = h.finish();
        let shown = present.get(&layer);
        if !active {
            if let Some((e, _)) = shown {
                commands.entity(*e).try_despawn();
            }
            stamps.remove(&layer);
            continue;
        }
        if shown.is_some() && stamps.get(&layer) == Some(&stamp) {
            continue;
        }
        stamps.insert(layer, stamp);
        let mut asset = GizmoAsset::new();
        let width = match layer {
            Layer::Edges => {
                draw_edges(&mut asset, &doc, &display, &meshes, topology.as_deref(), edges.as_deref(), chosen.as_deref(), &materials, &bodies, cut);
                1.2
            }
            Layer::Grid => {
                draw_grid(&mut asset, display.high_contrast, cut);
                1.0
            }
            Layer::Axes => {
                let n = GRID_STEPS as f32 * GRID_STEP_MM as f32;
                strips(&mut asset, vec![Vec3::ZERO, Vec3::X * n], cut, AXIS_X);
                strips(&mut asset, vec![Vec3::ZERO, Vec3::Y * n], cut, AXIS_Y);
                strips(&mut asset, vec![Vec3::ZERO, Vec3::Z * n * 0.5], cut, AXIS_Z);
                2.0
            }
            Layer::Outline => {
                if let (Some(derived), Some(plane)) = (derived.as_deref(), cut) {
                    for segments in derived.outlines(plane) {
                        for [a, b] in segments.iter() {
                            asset.line(*a, *b, SECTION);
                        }
                    }
                }
                2.5
            }
            Layer::Exact => {
                if let Some(s) = display.exact.drawn(&display.section, revision) {
                    for line in &s.polylines {
                        if line.len() >= 2 {
                            asset.linestrip(line.iter().map(v), EXACT);
                        }
                    }
                }
                2.5
            }
            Layer::Curves => {
                if let Some(edges) = edges.as_deref() {
                    for n in curve_nodes(&doc) {
                        let Some(Ok(lines)) = edges.curves.get(&n.id) else { continue };
                        let color = curve_color(n.color.as_deref(), selected.contains(&n.id));
                        for line in lines.iter() {
                            strips(&mut asset, line.iter().map(v).collect(), cut, color);
                        }
                    }
                }
                CURVE_WIDTH
            }
        };
        let line_config = GizmoLineConfig { width, ..default() };
        match shown {
            Some((_, handle)) => {
                if let Some(mut a) = assets.get_mut(handle) {
                    *a = asset;
                }
            }
            None => {
                let handle = assets.add(asset);
                let e = commands.spawn((Gizmo { handle, line_config, depth_bias: DEPTH_BIAS }, Transform::default(), layer)).id();
                commands.entity(*root).add_child(e);
            }
        }
    }
}

/// RoboCAD's `_draw_grid` (without its axes): ±20 steps of 10 mm on z = 0,
/// every 5th line major.
fn draw_grid(asset: &mut GizmoAsset, contrast: bool, cut: Option<&SectionPlane>) {
    let (step, n) = (GRID_STEP_MM as f32, GRID_STEPS);
    let extent = n as f32 * step;
    for i in -n..=n {
        let major = i % 5 == 0;
        let color = match (contrast, major) {
            (true, true) => GRID_CONTRAST_MAJOR,
            (true, false) => GRID_CONTRAST_MINOR,
            (false, true) => Color::srgb(GRID[0], GRID[1], GRID[2]),
            (false, false) => Color::srgb(GRID[0] * 0.7, GRID[1] * 0.7, GRID[2] * 0.7),
        };
        let at = i as f32 * step;
        strips(asset, vec![Vec3::new(at, -extent, 0.0), Vec3::new(at, extent, 0.0)], cut, color);
        strips(asset, vec![Vec3::new(-extent, at, 0.0), Vec3::new(extent, at, 0.0)], cut, color);
    }
}

/// Every drawn body's display edges (see the module doc).
#[allow(clippy::too_many_arguments)]
fn draw_edges(
    asset: &mut GizmoAsset,
    doc: &CadDocument,
    display: &CadDisplay,
    meshes: &CadMeshes,
    topology: Option<&CadTopology>,
    edges: Option<&DisplayEdges>,
    chosen: Option<&DisplayMaterials>,
    materials: &Assets<StandardMaterial>,
    bodies: &Query<(&CadBody, &MeshMaterial3d<StandardMaterial>)>,
    cut: Option<&SectionPlane>,
) {
    let wireframe = display.mode == DisplayMode::Wireframe;
    let kinds: HashMap<&str, &str> = doc.doc.as_ref().map(|d| d.nodes.iter().map(|n| (n.id.as_str(), n.kind.as_str())).collect()).unwrap_or_default();
    for (body, material) in bodies {
        let id = body.id.as_str();
        if !meshes.shown(id) {
            continue;
        }
        let color = if wireframe {
            let base = chosen.map_or_else(|| material.0.clone(), |c| c.base(&material.0));
            let c = materials.get(&base).map_or(Color::WHITE, |m| m.base_color).to_srgba();
            Color::srgb(0.6 * c.red + 0.3, 0.6 * c.green + 0.3, 0.6 * c.blue + 0.3)
        } else if display.high_contrast {
            EDGE_CONTRAST
        } else {
            EDGE
        };
        let mesh_node = kinds.get(id) == Some(&"mesh");
        let from_topology = topology.and_then(|t| t.get(id)).map(|t| t.edges.iter().map(|e| e.points.as_slice()).collect::<Vec<_>>());
        let own = edges.and_then(|e| e.own.get(id)).and_then(|r| r.as_ref().ok());
        let lines: Vec<&[[f64; 3]]> = from_topology.unwrap_or_else(|| own.map(|o| o.iter().map(Vec::as_slice).collect()).unwrap_or_default());
        if !lines.is_empty() && !mesh_node {
            for line in lines.iter().filter(|l| l.len() >= 2) {
                strips(asset, line.iter().map(v).collect(), cut, color);
            }
        } else if wireframe {
            // RoboCAD's GL_LINE polygon mode: each triangle side once.
            let Some(data) = meshes.mesh_data(id) else { continue };
            let mut sides: HashSet<(u32, u32)> = HashSet::new();
            for tri in &data.triangles {
                for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                    let side = (a.min(b), a.max(b));
                    if a == b || !sides.insert(side) {
                        continue;
                    }
                    if let (Some(p), Some(q)) = (data.vertices.get(a as usize), data.vertices.get(b as usize)) {
                        strips(asset, vec![v(p), v(q)], cut, color);
                    }
                }
            }
        }
    }
}

/// The build plate quad and the section plane quad.
#[derive(Component)]
pub(super) struct PlateQuad;
#[derive(Component)]
pub(super) struct PlaneQuad;

/// The quads' mesh and materials, made once.
pub(super) struct QuadAssets {
    square: Handle<Mesh>,
    plate: Handle<StandardMaterial>,
    plane: Handle<StandardMaterial>,
}

/// The section plane quad's transform under the root (mm): RoboCAD's
/// `_draw_section_outline` quad, ±max(bounds diagonal, 20) × 0.6 in the
/// plane's own axes.
pub fn plane_transform(plane: &SectionPlane, bounds: Option<([f64; 3], [f64; 3])>) -> Transform {
    let diagonal = bounds.map_or(0.0, |(lo, hi)| ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt());
    let half = diagonal.max(20.0) * 0.6;
    let (x, y, n) = (v(&plane.x_axis).normalize_or_zero(), v(&plane.y_axis()), v(&plane.unit_normal()));
    let rotation = Quat::from_mat3(&Mat3::from_cols(x, y, n)).normalize();
    Transform { translation: v(&plane.origin), rotation, scale: Vec3::new(2.0 * half as f32, 2.0 * half as f32, 1.0) }
}

/// Present: the build plate (RoboCAD's 220 × 220 mm at z = −0.05) and the
/// section plane quad while each is shown.
#[allow(clippy::too_many_arguments)]
pub(super) fn quads(
    mut commands: Commands,
    display: Option<Res<CadDisplay>>,
    meshes: Option<Res<CadMeshes>>,
    root: Option<Single<Entity, With<CadRoot>>>,
    plates: Query<Entity, With<PlateQuad>>,
    mut planes: Query<(Entity, &mut Transform), (With<PlaneQuad>, Without<PlateQuad>)>,
    mut mesh_assets: ResMut<Assets<Mesh>>,
    mut material_assets: ResMut<Assets<StandardMaterial>>,
    mut made: Local<Option<QuadAssets>>,
) {
    let (Some(display), Some(meshes), Some(root)) = (display, meshes, root) else { return };
    let quad = made.get_or_insert_with(|| {
        let translucent = |c: Color| StandardMaterial { base_color: c, alpha_mode: AlphaMode::Blend, unlit: true, cull_mode: None, ..default() };
        QuadAssets {
            square: mesh_assets.add(Rectangle::new(1.0, 1.0)),
            plate: material_assets.add(translucent(Color::srgba(0.25, 0.3, 0.4, 0.35))),
            plane: material_assets.add(translucent(SECTION.with_alpha(0.08))),
        }
    });
    let plate = plates.iter().next();
    match (display.build_plate, plate) {
        (true, None) => {
            let [w, d] = BUILD_PLATE_MM;
            let at = Transform::from_xyz(0.0, 0.0, -0.05).with_scale(Vec3::new(w as f32, d as f32, 1.0));
            let e = commands.spawn((Mesh3d(quad.square.clone()), MeshMaterial3d(quad.plate.clone()), at, Visibility::default(), NotShadowCaster, Pickable::IGNORE, PlateQuad)).id();
            commands.entity(*root).add_child(e);
        }
        (false, Some(e)) => commands.entity(e).despawn(),
        _ => {}
    }
    let wanted = cut_plane(&display).map(|p| plane_transform(p, model_bounds(&meshes)));
    match (wanted, planes.iter_mut().next()) {
        (Some(t), None) => {
            let e = commands.spawn((Mesh3d(quad.square.clone()), MeshMaterial3d(quad.plane.clone()), t, Visibility::default(), NotShadowCaster, Pickable::IGNORE, PlaneQuad)).id();
            commands.entity(*root).add_child(e);
        }
        (Some(t), Some((_, mut at))) => {
            if *at != t {
                *at = t;
            }
        }
        (None, Some((e, _))) => commands.entity(e).despawn(),
        (None, None) => {}
    }
}

/// RoboCAD's render-mode fill and back lights (camera-fixed).
#[derive(Component)]
pub(super) struct RenderLight;

/// Light direction `toward` (view space, RoboCAD's GL light position) as a
/// camera child's transform: the light shines the other way.
fn shine_from(toward: Vec3) -> Transform {
    Transform::default().looking_to(-toward, Vec3::Y)
}

/// Present: render mode's lights and the headlight's shadows; high
/// contrast's background.
#[allow(clippy::type_complexity)]
pub(super) fn lights(
    mut commands: Commands,
    display: Option<Res<CadDisplay>>,
    mut cameras: Query<(Entity, &mut Camera), (With<Camera3d>, With<crate::camera::Orbit>)>,
    mut headlights: Query<(Entity, &mut DirectionalLight, &ChildOf), Without<RenderLight>>,
    render_lights: Query<Entity, With<RenderLight>>,
    mut applied: Local<Option<(Entity, bool)>>,
    mut shadowed: Local<HashSet<Entity>>,
) {
    let Some(display) = display else { return };
    let Some((camera, mut cam)) = cameras.iter_mut().next() else { return };
    if *applied != Some((camera, display.high_contrast)) {
        cam.clear_color = if display.high_contrast { ClearColorConfig::Custom(BACKGROUND_CONTRAST) } else { ClearColorConfig::Default };
        *applied = Some((camera, display.high_contrast));
    }
    let render = display.mode == DisplayMode::Render;
    for (light, mut headlight, parent) in &mut headlights {
        if parent.parent() != camera {
            continue;
        }
        if render && shadowed.insert(light) {
            // The model is tenths of a metre: one tight cascade instead of Bevy's 4 over 150 m.
            let cascades = CascadeShadowConfigBuilder { num_cascades: 1, minimum_distance: 0.0, maximum_distance: 3.0, first_cascade_far_bound: 3.0, overlap_proportion: 0.2 }.build();
            commands.entity(light).try_insert(cascades);
        }
        if headlight.shadow_maps_enabled != render {
            headlight.shadow_maps_enabled = render;
        }
    }
    let shown = render_lights.iter().next().is_some();
    if render && !shown {
        commands.entity(camera).with_children(|c| {
            c.spawn((DirectionalLight { color: Color::srgb(0.7, 0.8, 1.0), illuminance: 3000.0, shadow_maps_enabled: false, ..default() }, shine_from(Vec3::new(-0.8, 0.3, 0.4)), RenderLight));
            c.spawn((DirectionalLight { color: Color::srgb(1.0, 0.88, 0.8), illuminance: 1500.0, shadow_maps_enabled: false, ..default() }, shine_from(Vec3::new(0.2, 0.9, -0.3)), RenderLight));
        });
    } else if !render && shown {
        for e in &render_lights {
            commands.entity(e).despawn();
        }
    }
}

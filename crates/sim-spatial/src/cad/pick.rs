//! CAD mode's picking in the 3D view (RoboCAD's `SelectTool` and
//! `Viewport._pick_pass`, `ui/tools.py`, `ui/viewport.py`): the one path a
//! click, a box drag, an Alt+click and the hover take. It writes actions
//! only (`CadSelect`, `CadCandidates`, `CadBoxSelect`, `CadHover`), which
//! `selection::handle` applies.
//!
//! - **When**: CAD mode, the Select tool, the pointer over the 3D view and
//!   not over a UI node (`HoverMap`, `scene::over_ui`), no text
//!   field focused (`CadInputFocus`). While the Alt menu is open a press in
//!   the 3D view only closes it (as a Qt popup) and nothing is hovered.
//! - **Click** (press and release within 6 px, RoboCAD's Manhattan
//!   distance): body mode picks `[id, "body", 0]`; face and point modes
//!   cast Bevy's `MeshRayCast` on the drawn bodies (`CadBody`) and map the
//!   hit triangle to RoboCAD's face (`CadMeshes::face_at` at the shown
//!   revision, read from the mesh the body shows: Bevy's triangle index is
//!   RoboCAD's, or the section's clipped copy's own; nothing while the mesh
//!   or that copy lags) as `[id, "face", f]` / `[id, "point", f]`; a mesh
//!   node is always a body item. Edge and vertex modes take the nearest
//!   sampled edge polyline or vertex within 6 px on screen that is not
//!   behind the first surface along the cursor ray. Shift extends, Ctrl
//!   (Command) toggles; empty space clears unless Shift or Ctrl is held.
//!   Alt with more than one candidate opens the menu (`CadCandidates`):
//!   every distinct item near the cursor, nearest first. In body, face and
//!   point modes, as RoboCAD's 7 × 7 px pick neighbourhood: rays at the
//!   cursor and at 8 neighbours 3 px away ([`RING_PX`]), each keeping only
//!   its nearest (visible) hit, so two faces meeting at the cursor are both
//!   listed and nothing hidden behind a surface is; ordered by the ray's
//!   pixel distance from the cursor. A plain click takes the cursor ray's
//!   own hit first (else the nearest neighbour's).
//! - **Box**: a left drag past 6 px draws the rubber band (`overlay`) and
//!   on release writes `CadBoxSelect` (Shift or Ctrl extends).
//! - **Catalogue interactions** (cad-modify, `ops::interact`): while a
//!   placement (`Flow::Place`), a plane tool (`Flow::PlanePick`), a sketch
//!   tool (`Flow::Sketch`) or extrude (`Flow::Extrude`) is active the left
//!   button belongs to it (cad-sketch): no click, box or hover here. While a pick-then-form tool
//!   (`Flow::PickThenForm`: fillet, chamfer, shell) is active, a click
//!   toggles the item under the cursor when it is of the tool's kind
//!   (`CadSelect {toggle: true}`; RoboCAD's `EdgeTool.press`), also while
//!   the tool's form has the keyboard; empty space, other kinds, a drag
//!   and Alt do nothing more. RoboCAD's `ShellTool.press` reuses
//!   `EdgeTool.press`, whose `hit[0] == "edge"` test means a face click
//!   there toggles nothing; here the face toggles (deliberately
//!   different: the tool's own hint says "click adds").
//! - **Hover**: coalesced to one search per 33 ms (RoboCAD's hover timer).
//!   Body, face and point modes ray-cast inline (Bevy's own picking backend
//!   casts as often). The edge and vertex screen search over every sampled
//!   polyline runs on a `Pool::Compute` job (snapshot: the ready topology
//!   `Arc`s, the bodies' bounds and a `CadView` copy), at most one in
//!   flight; a newer search waits and replaces any older waiting one, and a
//!   result is dropped when a newer one waits (newest wins). `CadHover` is
//!   written (quietly) only when the item changes.
//! - **Cost of a click's search**: inline. Each body's bounds are projected
//!   first and bodies whose screen rectangle (widened by 6 px) misses the
//!   cursor are skipped, so only the polylines near the cursor are
//!   projected. A job would delay the selection by a frame for no gain.
//! - **Locked nodes** are not picked and do not hide what is behind them
//!   (RoboCAD leaves them out of its pick pass); box select takes them.
//! - **Occlusion** (edges and vertices): a point is hidden when its
//!   distance along the cursor ray exceeds the first surface's by more than
//!   1 % of that distance or 8 px worth of millimetres there. RoboCAD
//!   decides it per pixel with its depth buffer; this compares with the
//!   depth under the cursor only (a recorded approximation).
use super::actions::CadAction;
use super::display::{CadDisplay, SectionPlane};
use super::document::{CadDocument, CadInputFocus, CadTool, SelectMode};
use super::mesh::{CadBody, CadMeshes};
use super::ops::Flow;
use super::topology::{CadTopology, NodeTopology};
use super::view::CadView;
use crate::app::actions::Act;
use crate::app::{ModeScope, ViewerMode, ViewerSet};
use crate::jobs::{Job, Pool};
use bevy::picking::hover::HoverMap;
use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings, RayCastVisibility};
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, RequestRedraw};
use sim_runtime::cad_client::SelectionItem;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A press and release closer than this (Manhattan, px) is a click; farther is a box.
pub const CLICK_SLOP: f32 = 6.0;
/// The neighbour rays' distance from the cursor (px): RoboCAD reads a 7 × 7 px block.
pub const RING_PX: f32 = 3.0;
/// How near (px) an edge or vertex must be to the cursor to be picked.
pub const NEAR_PX: f32 = 6.0;
/// The hover is searched at most this often (RoboCAD's 33 ms timer).
pub const HOVER_PERIOD: Duration = Duration::from_millis(33);

/// The pointer's state across frames (reset on entering and leaving CAD mode).
#[derive(Resource, Default)]
pub(super) struct PickState {
    /// Where the left press began (in the 3D view, Select tool), and the last position seen.
    press: Option<Vec2>,
    last: Vec2,
    dragging: bool,
    /// The press held Alt: a drag past the slop is RoboCAD's Alt+left orbit
    /// (`camera::input`, `OrbitRules::robocad_gestures`), never a box
    /// select; an Alt click still opens the candidates menu.
    alt_drag: bool,
    /// The rubber band's corners while box dragging (`overlay` draws it).
    pub(super) band: Option<(Vec2, Vec2)>,
    /// Where the Alt menu opens (the click's position).
    pub(super) menu_at: Option<Vec2>,
    /// The hover search in flight (with the mode it searched) and the newest one waiting.
    hover_job: Option<(SelectMode, Job<Vec<SelectionItem>>)>,
    hover_waiting: Option<Search>,
    hover_seq: u64,
    /// What the last hover search was for, and when it started.
    hover_key: Option<HoverKey>,
    hover_at: Option<Instant>,
    /// Bumped when the view moves (the hover is searched again).
    view_stamp: u64,
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct HoverKey {
    cursor: Vec2,
    mode: SelectMode,
    topology: u64,
    meshes: u64,
    view: u64,
    /// The shown revision (a lock or visibility edit changes what may be hovered).
    revision: u64,
}

/// A drawn, visible, unlocked node for the screen search: id, topology, bounds (mm).
type SearchNode = (String, Arc<NodeTopology>, Option<(Vec3, Vec3)>);

/// What an edge or vertex search needs, owned (it may run on a job).
#[derive(Clone)]
pub(super) struct Search {
    view: CadView,
    nodes: Vec<SearchNode>,
    cursor: Vec2,
    mode: SelectMode,
    /// The cursor ray (model mm, unit direction) and the first unlocked
    /// surface's distance along it.
    ray: (Vec3, Vec3),
    front: Option<f32>,
    /// Items that come first (a mesh node under the cursor: a body item).
    first: Vec<SelectionItem>,
    /// The section tool's plane while it is on: what lies on its removed
    /// side is not drawn, so not picked (RoboCAD picks under its clip plane).
    clip: Option<SectionPlane>,
}

pub(super) fn build(app: &mut App) {
    app.init_resource::<PickState>()
        .add_systems(OnEnter(ModeScope::Cad), |mut commands: Commands| commands.insert_resource(PickState::default()))
        .add_systems(OnExit(ModeScope::Cad), |mut commands: Commands| commands.insert_resource(PickState::default()))
        // After every system that sets `CadInputFocus` this frame (the name
        // field, the inspector's editors, the numeric bar), so a click reads
        // the focus as it stands.
        .add_systems(
            Update,
            pointer
                .after(crate::app::actions::serve)
                .after(super::panel::name_entry)
                .after(super::inspector::editor_entry)
                .after(super::numeric::entry)
                .in_set(ViewerSet::Input)
                .run_if(in_state(ViewerMode::Cad)),
        );
}

/// One ray hit on a drawn body: node id, triangle, distance along the model ray (mm).
struct Hit {
    id: String,
    triangle: Option<usize>,
    distance: f32,
}

/// The drawn bodies under `cursor`, nearest first (one hit per body),
/// leaving out locked nodes.
fn ray_hits(doc: &CadDocument, view: &CadView, cursor: Vec2, ray_cast: &mut MeshRayCast, bodies: &Query<&CadBody>) -> Option<((Vec3, Vec3), Vec<Hit>)> {
    let (origin, dir) = view.ray(cursor)?;
    let world_origin = view.world_from_model.transform_point3(origin);
    let world_dir = Dir3::new(view.world_from_model.transform_vector3(dir)).ok()?;
    let filter = |e: Entity| bodies.contains(e);
    let settings = MeshRayCastSettings::default().with_filter(&filter).with_visibility(RayCastVisibility::Visible).never_early_exit();
    let locked = |id: &str| doc.doc.as_ref().and_then(|d| d.nodes.iter().find(|n| n.id == id)).is_some_and(|n| n.locked);
    let mut hits = Vec::new();
    for (entity, hit) in ray_cast.cast_ray(Ray3d::new(world_origin, world_dir), &settings) {
        let Ok(body) = bodies.get(*entity) else { continue };
        if locked(&body.id) || hits.iter().any(|h: &Hit| h.id == body.id) {
            continue;
        }
        let model = view.model_from_world.transform_point3(hit.point);
        hits.push(Hit { id: body.id.clone(), triangle: hit.triangle_index, distance: (model - origin).dot(dir) });
    }
    Some(((origin, dir), hits))
}

/// Whether node `id` is a mesh node (always picked as a body item).
fn is_mesh(doc: &CadDocument, id: &str) -> bool {
    doc.doc.as_ref().and_then(|d| d.nodes.iter().find(|n| n.id == id)).is_some_and(|n| n.kind == "mesh")
}

/// One ray hit as an item of body, face or point mode (a mesh node is a body item).
fn surface_item(doc: &CadDocument, meshes: &CadMeshes, hit: &Hit) -> Option<SelectionItem> {
    if doc.select_mode == SelectMode::Body || is_mesh(doc, &hit.id) {
        Some(SelectionItem(hit.id.clone(), "body".into(), 0))
    } else {
        // Nothing while the mesh is being redrawn for a newer revision (its face indices are the old revision's).
        hit.triangle.and_then(|t| meshes.face_at(&hit.id, t, doc.shown_revision())).map(|f| SelectionItem(hit.id.clone(), doc.select_mode.name().into(), f))
    }
}

/// The ray offsets (px) around the cursor, in order of distance: the
/// cursor, the 4 axis neighbours, the 4 diagonal ones.
fn ring() -> [Vec2; 9] {
    let r = RING_PX;
    [Vec2::ZERO, Vec2::new(r, 0.0), Vec2::new(-r, 0.0), Vec2::new(0.0, r), Vec2::new(0.0, -r), Vec2::new(r, r), Vec2::new(r, -r), Vec2::new(-r, r), Vec2::new(-r, -r)]
}

/// The candidates of body, face and point modes: the nearest visible hit
/// of each ray of [`ring`], distinct, nearest ray first.
fn surface_items(doc: &CadDocument, meshes: &CadMeshes, view: &CadView, cursor: Vec2, ray_cast: &mut MeshRayCast, bodies: &Query<&CadBody>) -> Vec<SelectionItem> {
    let mut out: Vec<SelectionItem> = Vec::new();
    for offset in ring() {
        let Some((_, hits)) = ray_hits(doc, view, cursor + offset, ray_cast, bodies) else { continue };
        if let Some(item) = hits.first().and_then(|hit| surface_item(doc, meshes, hit)).filter(|i| !out.contains(i)) {
            out.push(item);
        }
    }
    out
}

/// The snapshot for an edge or vertex search under `cursor`.
fn search_for(doc: &CadDocument, meshes: &CadMeshes, topology: &CadTopology, view: &CadView, cursor: Vec2, ray: (Vec3, Vec3), hits: &[Hit], clip: Option<SectionPlane>) -> Search {
    let bounds: HashMap<&str, (Vec3, Vec3)> = meshes.body_bounds().collect();
    let pickable = |id: &str| doc.doc.as_ref().and_then(|d| d.nodes.iter().find(|n| n.id == id)).is_some_and(|n| n.effective_visible && !n.locked);
    let nodes = topology.ready().filter(|(id, _)| meshes.shown(id) && pickable(id)).map(|(id, t)| (id.clone(), t.clone(), bounds.get(id.as_str()).copied())).collect();
    let first = hits.first().filter(|h| is_mesh(doc, &h.id)).map(|h| SelectionItem(h.id.clone(), "body".into(), 0)).into_iter().collect();
    Search { view: view.clone(), nodes, cursor, mode: doc.select_mode, ray, front: hits.first().map(|h| h.distance), first, clip }
}

/// The screen rectangle of a model box (None when a corner is behind the camera).
fn screen_rect(view: &CadView, lo: Vec3, hi: Vec3) -> Option<(Vec2, Vec2)> {
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    for x in [lo.x, hi.x] {
        for y in [lo.y, hi.y] {
            for z in [lo.z, hi.z] {
                let p = view.project(Vec3::new(x, y, z))?;
                min = min.min(p);
                max = max.max(p);
            }
        }
    }
    Some((min, max))
}

/// The edges or vertices within [`NEAR_PX`] of the cursor that are not
/// behind the first surface, nearest on screen first (after `first`).
pub(super) fn search(s: &Search) -> Vec<SelectionItem> {
    let (origin, dir) = s.ray;
    let cut = |p: Vec3| s.clip.as_ref().is_some_and(|c| c.distance([p.x as f64, p.y as f64, p.z as f64]) > 1e-6);
    let hidden = |p: Vec3| {
        cut(p)
            || match s.front {
                Some(front) => {
                    let slack = (front.abs() * 0.01).max(s.view.mm_per_pixel(p).unwrap_or(0.0) * 8.0);
                    (p - origin).dot(dir) > front + slack
                }
                None => false,
            }
    };
    let point = |p: &[f64; 3]| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    let mut found: Vec<(f32, SelectionItem)> = Vec::new();
    for (id, topo, bounds) in &s.nodes {
        if let Some((lo, hi)) = bounds
            && let Some((min, max)) = screen_rect(&s.view, *lo, *hi)
            && (s.cursor.cmplt(min - NEAR_PX).any() || s.cursor.cmpgt(max + NEAR_PX).any())
        {
            continue;
        }
        match s.mode {
            SelectMode::Vertex => {
                for v in &topo.vertices {
                    let Some(p) = v.point.map(|p| point(&p)) else { continue };
                    let Some(screen) = s.view.project(p) else { continue };
                    let d = screen.distance(s.cursor);
                    if d <= NEAR_PX && !hidden(p) {
                        found.push((d, SelectionItem(id.clone(), "vertex".into(), v.index)));
                    }
                }
            }
            SelectMode::Edge => {
                for e in &topo.edges {
                    let points: Vec<Vec3> = e.points.iter().map(point).collect();
                    let screen: Vec<Option<Vec2>> = points.iter().map(|p| s.view.project(*p)).collect();
                    let mut best: Option<(f32, Vec3)> = None;
                    let mut consider = |d: f32, p: Vec3| {
                        if best.is_none_or(|(b, _)| d < b) {
                            best = Some((d, p));
                        }
                    };
                    if points.len() == 1 {
                        if let Some(a) = screen[0] {
                            consider(a.distance(s.cursor), points[0]);
                        }
                    }
                    for i in 1..points.len() {
                        let (Some(a), Some(b)) = (screen[i - 1], screen[i]) else { continue };
                        let ab = b - a;
                        let t = if ab.length_squared() > 0.0 { ((s.cursor - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
                        consider((a + ab * t).distance(s.cursor), points[i - 1].lerp(points[i], t));
                    }
                    if let Some((d, p)) = best
                        && d <= NEAR_PX
                        && !hidden(p)
                    {
                        found.push((d, SelectionItem(id.clone(), "edge".into(), e.index)));
                    }
                }
            }
            SelectMode::Body | SelectMode::Face | SelectMode::Point => {}
        }
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = s.first.clone();
    for (_, item) in found {
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

/// Every candidate under `cursor` in the current mode, nearest first (a click's, inline).
#[allow(clippy::too_many_arguments)]
fn candidates_at(doc: &CadDocument, meshes: &CadMeshes, topology: Option<&CadTopology>, view: &CadView, cursor: Vec2, ray_cast: &mut MeshRayCast, bodies: &Query<&CadBody>, clip: Option<SectionPlane>) -> Vec<SelectionItem> {
    if matches!(doc.select_mode, SelectMode::Body | SelectMode::Face | SelectMode::Point) {
        return surface_items(doc, meshes, view, cursor, ray_cast, bodies);
    }
    let Some((ray, hits)) = ray_hits(doc, view, cursor, ray_cast, bodies) else { return Vec::new() };
    match doc.select_mode {
        SelectMode::Body | SelectMode::Face | SelectMode::Point => Vec::new(),
        SelectMode::Edge | SelectMode::Vertex => match topology {
            Some(topology) => search(&search_for(doc, meshes, topology, view, cursor, ray, &hits, clip)),
            None => Vec::new(),
        },
    }
}

/// Input: clicks, box drags, Alt+clicks and the hover in the 3D view (see the module doc).
#[allow(clippy::too_many_arguments)]
fn pointer(
    windows: Query<&Window, With<PrimaryWindow>>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    hover_map: Option<Res<HoverMap>>,
    nodes: Query<(), With<Node>>,
    (doc, meshes, topology, view, focus, display): (Option<Res<CadDocument>>, Option<Res<CadMeshes>>, Option<Res<CadTopology>>, Option<Res<CadView>>, Option<Res<CadInputFocus>>, Option<Res<CadDisplay>>),
    mut state: ResMut<PickState>,
    mut ray_cast: MeshRayCast,
    bodies: Query<&CadBody>,
    mut out: MessageWriter<Act<CadAction>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let (Some(doc), Some(meshes), Some(view)) = (doc, meshes, view) else { return };
    let state = &mut *state;
    let clip = display.as_deref().filter(|d| d.section.enabled).and_then(|d| d.section.plane);
    // The section's plane changes what can be picked, as the camera does.
    if view.is_changed() || display.as_ref().is_some_and(|d| d.is_changed()) {
        state.view_stamp += 1;
    }
    let cursor = windows.single().ok().and_then(Window::cursor_position);
    if let Some(p) = cursor {
        state.last = p;
    }
    let focused = focus.is_some_and(|f| f.0);
    // A catalogue op's interaction (cad-modify): a placement owns the left
    // drag (`ops::interact`); a pick-then-form tool's click toggles one item
    // of its kind (RoboCAD's `EdgeTool.press`), also while its form has the keyboard.
    let flow = doc.ops.active.and_then(super::ops::entry).map(|e| e.flow);
    // Placing, a plane tool's picks, a sketch tool's clicks and extrude's drag (cad-sketch) own the left button.
    let placing = matches!(flow, Some(Flow::Place(_) | Flow::PlanePick(_) | Flow::Sketch(_) | Flow::Extrude { .. }));
    let pick_kind = match flow {
        Some(Flow::PickThenForm(mode)) => Some(mode),
        _ => None,
    };
    // An open command surface takes the press that closes it (as a Qt popup does).
    let usable = view.valid && doc.tool == CadTool::Select && (!focused || pick_kind.is_some()) && !placing && doc.ops.surface.is_none();
    let in_view = cursor.is_some_and(|p| view.contains(p)) && !super::scene::over_ui(hover_map.as_deref(), &nodes);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]);
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let menu_open = doc.candidates.is_some();

    // A drag that can no longer finish (another tool, a focused field) is dropped.
    if !usable && state.press.is_some() {
        state.press = None;
        state.dragging = false;
        state.band = None;
    }
    if buttons.just_pressed(MouseButton::Left) && usable && in_view {
        if menu_open {
            // A press elsewhere closes the menu (as a Qt popup); nothing is picked.
            out.write(Act::quiet(CadAction::CadCandidates { items: Vec::new(), extend: false, toggle: false }));
        } else {
            state.press = cursor;
            state.dragging = false;
            state.alt_drag = alt;
        }
    }
    if let Some(start) = state.press {
        let at = state.last;
        if !state.dragging && (at - start).abs().element_sum() > CLICK_SLOP {
            state.dragging = true;
        }
        if state.dragging && !state.alt_drag && buttons.pressed(MouseButton::Left) && state.band != Some((start, at)) {
            state.band = Some((start, at));
        }
        if buttons.just_released(MouseButton::Left) || !buttons.pressed(MouseButton::Left) {
            state.press = None;
            state.band = None;
            if std::mem::take(&mut state.dragging) {
                // RoboCAD's pick-then-form tools pick on press only: no box select.
                if pick_kind.is_none() && !state.alt_drag {
                    let rect = [start.x.min(at.x), start.y.min(at.y), start.x.max(at.x), start.y.max(at.y)];
                    out.write(Act::ui(CadAction::CadBoxSelect { rect, extend: shift || ctrl }));
                }
            } else if let Some(mode) = pick_kind {
                // RoboCAD's `EdgeTool.press` (ShellTool's too): the hit toggles when it is
                // of the tool's kind; anything else, or empty space, changes nothing. The
                // face index comes from `surface_item` (`CadMeshes::face_at` at the shown
                // revision), an edge from the topology search at the shown revision.
                state.menu_at = None;
                let items = candidates_at(&doc, &meshes, topology.as_deref(), &view, at, &mut ray_cast, &bodies, clip);
                if let Some(item) = items.into_iter().next().filter(|i| i.1 == mode.name()) {
                    out.write(Act::ui(CadAction::CadSelect { ids: Vec::new(), items: vec![item], extend: false, toggle: true, picked_at: Some(doc.shown_revision()) }));
                }
            } else {
                let items = candidates_at(&doc, &meshes, topology.as_deref(), &view, at, &mut ray_cast, &bodies, clip);
                // Only an Alt+click's menu opens at the pointer (a REST cad_candidates opens at the view's corner).
                state.menu_at = (alt && items.len() > 1).then_some(at);
                if alt && items.len() > 1 {
                    out.write(Act::ui(CadAction::CadCandidates { items, extend: shift, toggle: ctrl }));
                } else if let Some(item) = items.into_iter().next() {
                    out.write(Act::ui(CadAction::CadSelect { ids: Vec::new(), items: vec![item], extend: shift, toggle: ctrl, picked_at: Some(doc.shown_revision()) }));
                } else if !(shift || ctrl) {
                    out.write(Act::ui(CadAction::CadSelect { ids: Vec::new(), items: Vec::new(), extend: false, toggle: false, picked_at: None }));
                }
            }
        }
    }

    // Hover.
    let hovering = usable && in_view && !menu_open && !state.dragging;
    let Some(cursor) = cursor.filter(|_| hovering) else {
        // The pointer's hover ends (a REST `cad_hover` is left alone while
        // the pointer is elsewhere).
        state.hover_job = None;
        state.hover_waiting = None;
        if state.hover_key.take().is_some() {
            set_hover(&doc, &mut out, None);
        }
        return;
    };
    let key = HoverKey { cursor, mode: doc.select_mode, topology: topology.as_ref().map_or(0, |t| t.epoch), meshes: meshes.epoch, view: state.view_stamp, revision: doc.shown_revision() };
    if state.hover_key != Some(key) {
        if state.hover_at.is_some_and(|t| t.elapsed() < HOVER_PERIOD) {
            // Coalesced: searched on a later frame.
            redraw.write(RequestRedraw);
        } else {
            state.hover_key = Some(key);
            state.hover_at = Some(Instant::now());
            match doc.select_mode {
                SelectMode::Body | SelectMode::Face | SelectMode::Point => {
                    // The cursor ray's own nearest hit (no neighbours: a hover is one item).
                    let items: Vec<SelectionItem> = ray_hits(&doc, &view, cursor, &mut ray_cast, &bodies).and_then(|(_, hits)| hits.first().and_then(|h| surface_item(&doc, &meshes, h))).into_iter().collect();
                    state.hover_job = None;
                    state.hover_waiting = None;
                    set_hover(&doc, &mut out, items.into_iter().next());
                }
                SelectMode::Edge | SelectMode::Vertex => match (topology.as_deref(), ray_hits(&doc, &view, cursor, &mut ray_cast, &bodies)) {
                    (Some(topology), Some((ray, hits))) => {
                        let next = search_for(&doc, &meshes, topology, &view, cursor, ray, &hits, clip);
                        if state.hover_job.is_some() {
                            state.hover_waiting = Some(next);
                        } else {
                            start_hover(state, next);
                        }
                    }
                    _ => set_hover(&doc, &mut out, None),
                },
            }
        }
    }
    if let Some((mode, job)) = &state.hover_job {
        match job.poll() {
            Some(result) => {
                let mode = *mode;
                state.hover_job = None;
                match state.hover_waiting.take() {
                    // A newer search waits: it replaces this result.
                    Some(next) => start_hover(state, next),
                    None if mode == doc.select_mode => set_hover(&doc, &mut out, result.ok().and_then(|items| items.into_iter().next())),
                    None => {}
                }
            }
            None => {
                redraw.write(RequestRedraw);
            }
        }
    }
}

/// Write the hover (quietly) when it differs from the document's.
fn set_hover(doc: &CadDocument, out: &mut MessageWriter<Act<CadAction>>, item: Option<SelectionItem>) {
    if doc.hover != item {
        out.write(Act::quiet(CadAction::CadHover { item }));
    }
}

/// Start an edge or vertex hover search on a Compute job.
fn start_hover(state: &mut PickState, search: Search) {
    state.hover_seq += 1;
    let mode = search.mode;
    state.hover_job = Some((mode, Job::spawn(Pool::Compute, state.hover_seq, "cad-hover-search", move |_| Ok(self::search(&search)))));
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::camera::CameraProjection;
    use bevy::math::Affine3A;
    use sim_runtime::cad_client::{EdgeInfo, VertexInfo};

    fn view() -> CadView {
        let projection = PerspectiveProjection { aspect_ratio: 2.0, ..default() };
        let clip_from_view = projection.get_clip_from_view();
        let world_from_view = Affine3A::from_translation(Vec3::new(0.0, 0.0, 5.0));
        let root = super::super::mesh::root_transform();
        let world_from_model = Affine3A::from_scale_rotation_translation(root.scale, root.rotation, root.translation);
        CadView { valid: true, world_from_model, model_from_world: world_from_model.inverse(), view_from_world: world_from_view.inverse(), world_from_view, clip_from_view, view_from_clip: clip_from_view.inverse(), min: Vec2::new(10.0, 20.0), size: Vec2::new(200.0, 100.0) }
    }

    /// The nearest edge within 6 px is first; an edge behind the first
    /// surface is left out; vertices likewise.
    #[test]
    fn edges_and_vertices_near_the_cursor_nearest_first_and_not_hidden() {
        let v = view();
        let centre = v.project(Vec3::ZERO).unwrap();
        let (origin, dir) = v.ray(centre).unwrap();
        let topo = NodeTopology {
            revision: 1,
            edges: vec![
                // Through the centre, 0 px away.
                EdgeInfo { index: 0, points: vec![[-100.0, 0.0, 0.0], [100.0, 0.0, 0.0]], ..Default::default() },
                // 100 mm above: about 2.4 px away.
                EdgeInfo { index: 1, points: vec![[-100.0, 0.0, 100.0], [100.0, 0.0, 100.0]], ..Default::default() },
                // 1 m above: far.
                EdgeInfo { index: 2, points: vec![[-100.0, 0.0, 1000.0], [100.0, 0.0, 1000.0]], ..Default::default() },
                // Behind (model +Y is away from the camera) the surface at the origin.
                EdgeInfo { index: 3, points: vec![[-100.0, 2000.0, 0.0], [100.0, 2000.0, 0.0]], ..Default::default() },
            ],
            vertices: vec![VertexInfo { index: 4, point: Some([0.0, 0.0, 100.0]) }, VertexInfo { index: 5, point: Some([0.0, 0.0, 1000.0]) }],
            ..Default::default()
        };
        let front = (Vec3::ZERO - origin).dot(dir);
        let mut s = Search { view: v.clone(), nodes: vec![("b1".into(), Arc::new(topo), None)], cursor: centre, mode: SelectMode::Edge, ray: (origin, dir), front: Some(front), first: Vec::new(), clip: None };
        let items = search(&s);
        assert_eq!(items, vec![SelectionItem("b1".into(), "edge".into(), 0), SelectionItem("b1".into(), "edge".into(), 1)]);
        // Without a surface in front, the edge behind is a candidate too.
        s.front = None;
        assert!(search(&s).contains(&SelectionItem("b1".into(), "edge".into(), 3)));
        s.mode = SelectMode::Vertex;
        assert_eq!(search(&s), vec![SelectionItem("b1".into(), "vertex".into(), 4)]);
        // A body whose screen rectangle misses the cursor is skipped.
        s.nodes[0].2 = Some((Vec3::new(1900.0, -10.0, -10.0), Vec3::new(2100.0, 10.0, 10.0)));
        assert!(search(&s).is_empty());
    }
}

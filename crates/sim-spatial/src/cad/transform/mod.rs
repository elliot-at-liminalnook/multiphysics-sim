//! CAD mode's direct tools (cad-select-transform part D; RoboCAD's
//! `ui/tools.py`): the move, rotate and uniform-scale gizmo ([`gizmo`]),
//! push/pull and offset of a face ([`push_pull`]), measure
//! (`super::measure`), the numeric bar (`super::numeric`), snapping
//! (`super::snap`) and live dimensions ([`dimensions`]). The tool arms of
//! `CadAction` (`CadTool`, `CadTransform`, `CadPushPull`, `CadOffsetFaces`,
//! `CadSetDimension`, `CadNumeric`, `CadMeasure`, `CadCancel`) come here
//! from `actions::handle` ([`handle`]).
//!
//! - **Previews are display only.** A drag changes only the selected
//!   bodies' `CadBody` display `Transform`s (model mm, under the Z-up root)
//!   and overlay lines; nothing is sent and no geometry changes until the
//!   release. [`previews`] (SimSync, after `mesh::sync`) is declarative:
//!   every frame each drawn body gets the preview's transform or identity,
//!   so no path can leave a body offset.
//! - **Each commit is exactly one Ops call** (`POST /ops/{name}`) through
//!   `actions::edit` (one edit at a time; a REST caller waits for RoboCAD's
//!   answer). A drag release, `cad_transform`/`cad_push_pull`/… from REST,
//!   `system_ui` and the numeric bar's Enter all go through [`commit`].
//!   It is refused, naming why, with nothing sent, while an edit is in
//!   flight, when not connected, when the shown document is behind
//!   RoboCAD's, or when RoboCAD's revision changed since the drag or entry
//!   began (`CadDocument::commit_refusal`); for the numeric bar, the
//!   revision when the entry gained focus (a REST `cad_numeric`: the shown
//!   revision when it arrives).
//! - **After a commit** the preview stays until every moved body's drawn
//!   mesh comes from a revision newer than the one the drag began at (the
//!   new tessellation already holds the move), then the bodies return to
//!   identity in the same frame the new mesh is drawn. It is dropped at
//!   once when the edit fails (the status is an error when the edit job
//!   ends), when the document is replaced, or 20 s after the edit ended if
//!   a mesh never arrives (a failed fetch is reported by the mesh cache).
//!   A revision change from elsewhere during a drag cancels it.
//!
//! ## Decisions (with the alternatives rejected)
//!
//! - **The gizmo is CAD mode's own (RoboCAD's handles, drawn with
//!   `Gizmos` and hit-tested in screen space with `CadView`), not
//!   `bevy::gizmos::transform_gizmo::TransformGizmoPlugin`.** Read in
//!   bevy_gizmos-0.19.1/src/transform_gizmo.rs (all 752 lines) and
//!   bevy_gizmos_render-0.19.1/src/transform_gizmo_render.rs:
//!   1. Its renderer is added by `GizmoRenderPlugin` whenever bevy_pbr is
//!      on (bevy_gizmos_render lib.rs:117-119) and spawns the handle meshes
//!      and an overlay `Camera3d` once, in `Startup` (render.rs:84-90,
//!      151-357). This app's `app::scope_new_entities` (app/mod.rs:248)
//!      scopes every parentless `Transform` root to the mode active at
//!      that moment, so the first mode switch would despawn them for good.
//!   2. That overlay camera (order 1, render.rs:339-349) would render in
//!      every mode with `Camera::default()`'s clear colour (bevy_camera
//!      camera.rs:424), beside each mode's own cameras.
//!   3. Scale is per axis only; the uniform view handle is absent in scale
//!      mode (transform_gizmo.rs:386, 580-622). RoboCAD's scale is uniform.
//!   4. A drag starts on any raw left press over a handle
//!      (transform_gizmo.rs:415), with no notion of UI panels over the view.
//!   5. `confine_cursor` defaults to true (:172) and confines with
//!      `CursorGrabMode::Confined` (:479); its systems run in `PostUpdate`
//!      (:236-243), outside `ViewerSet`'s Input → Actions order.
//!   6. Its handles (35 px hit distance, :62) are not RoboCAD's (centre
//!      handle within 10 px, axes and rings within 14 px, 90 px long), and
//!      its world-axis colours would show RoboCAD's Z as green.
//!   So the plugin is not added (its `TransformGizmoSettings` resource
//!   never exists, so its Startup spawn never runs). *Revisit if* Bevy's
//!   renderer spawns per camera or on demand and scale gains a uniform handle.
//! - **Pivot rule** ([`pivot`]): the first selected node's `pivot`
//!   (`NodeSummary.pivot`), else, with one node selected, its mass centroid
//!   from `GET /nodes/{id}` when that detail is at the shown revision, else
//!   the centre of the selected bodies' drawn-mesh bounds. RoboCAD
//!   (tools.py:256-266) uses `selection_properties`' mass-weighted centroid
//!   of every selected body (mesh-only nodes add nothing to it). Differs for
//!   several nodes (bounds centre instead of the mass-weighted centroid:
//!   the viewer has no per-node mass without a request each) and while the
//!   detail is being refetched (bounds centre until it lands).
//! - **Numbers sent are rounded to 1e-6** (mm, degrees, factor): drag maths
//!   run in f32 on screen data; RoboCAD gets the value the readout shows.
//! - **A typed rotation turns about the last dragged handle's axis**, also
//!   after dragging the X ring (handle 0). Deliberately different: RoboCAD's
//!   `axes[self.axis_index or 2]` (tools.py:379) treats index 0 as falsy, so
//!   after the X ring it turns about Z. The centre handle's view axis is
//!   kept from its drag; with no drag yet, Z, as RoboCAD.
//! - **Ctrl is Control or Command** for snapping, as the other CAD keys
//!   (Qt's ControlModifier is Command on macOS). Python's `round` is half to
//!   even; the snaps use `round_ties_even`.
//! - **Keys** (RoboCAD keymap.json): G move, R rotate, S scale (only
//!   without Ctrl/Command: Ctrl+S saves; without Shift: Shift+R and
//!   "Shift+A, S" are other tools), D push/pull, Shift+D offset face, M
//!   measure, Escape cancel; Tab and the entry keys belong to the numeric
//!   bar. No clash: `keys.rs` has Ctrl+Z/S/A, Ctrl+Shift+I/M, Delete,
//!   Home, B, E, V, P; nothing in `app/` or `ui_kit` reads keys; the other
//!   modes' keys run only in their modes.
mod commit;
mod dimensions;
mod gizmo;
mod preview;
mod push_pull;
#[cfg(test)]
mod tests;

pub use dimensions::Entry as DimensionEntry;
pub(super) use dimensions::keep_entry;
pub use gizmo::Drag;
pub use push_pull::{PushDrag, Target as PushTarget};
pub(super) use preview::{previews, state_json};

use super::actions::{CadAction, Cx, Dimension};
use super::document::{CadDocument, CadInputFocus, CadTool};
use super::measure::MeasureState;
use super::mesh::{CadBody, CadMeshes};
use super::numeric::Numeric;
use super::snap::Snap;
use super::topology::CadTopology;
use super::view::CadView;
use sim_runtime::cad_client::SelectionItem;
use crate::app::actions::{Act, Call};
use crate::app::{ModeScope, ViewerMode, ViewerSet};
use bevy::picking::hover::HoverMap;
use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings, RayCastVisibility};
use bevy::prelude::*;
use bevy::window::{CursorIcon, PrimaryWindow, SystemCursorIcon};
use serde_json::{Map, Value, json};
use sim_api::Outcome;
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// The gizmo's axes in RoboCAD's model frame.
pub const AXES: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];
/// RoboCAD's axis colours (tools.py:265).
pub const AXIS_COLOURS: [Color; 3] = [Color::srgb(0.85, 0.3, 0.3), Color::srgb(0.3, 0.75, 0.3), Color::srgb(0.3, 0.45, 0.95)];
/// RoboCAD's highlight and temporary-shape colour (1.0, 0.9, 0.3).
pub const HOT: Color = Color::srgb(1.0, 0.9, 0.3);
/// The gizmo's centre handle (viewport.py:1092).
pub const CENTRE_COLOUR: Color = Color::srgb(0.95, 0.95, 0.95);
/// The snap marker (tools.py:1057).
pub const SNAP_COLOUR: Color = Color::srgb(0.5, 1.0, 0.6);
/// How long a released drag waits for its commit to be applied.
const RELEASE_WAIT: Duration = Duration::from_secs(2);
/// How long a committed preview waits for the new meshes after the edit ended.
const MESH_WAIT: Duration = Duration::from_secs(20);

/// The tools' overlay lines: drawn over the bodies, as RoboCAD draws its
/// gizmo and temporary shapes with the depth test off.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub(super) struct ToolGizmos;

/// Which rule gave the transform pivot ([`pivot`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PivotRule {
    NodePivot,
    MassCentroid,
    BoundsCentre,
}
impl PivotRule {
    pub fn name(self) -> &'static str {
        match self {
            PivotRule::NodePivot => "node pivot",
            PivotRule::MassCentroid => "mass centroid",
            PivotRule::BoundsCentre => "bounds centre",
        }
    }
}

/// A transform preview's change, in RoboCAD's model frame (mm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Delta {
    Translate(Vec3),
    Rotate { axis: Vec3, angle_deg: f32, center: Vec3 },
    Scale { factor: f32, center: Vec3 },
}
impl Delta {
    /// The display transform that applies it to a body drawn in model mm.
    pub fn transform(&self) -> Transform {
        match *self {
            Delta::Translate(t) => Transform::from_translation(t),
            Delta::Rotate { axis, angle_deg, center } => {
                let q = Quat::from_axis_angle(axis.try_normalize().unwrap_or(Vec3::Z), angle_deg.to_radians());
                Transform { translation: center - q * center, rotation: q, scale: Vec3::ONE }
            }
            Delta::Scale { factor, center } => Transform { translation: center - center * factor, rotation: Quat::IDENTITY, scale: Vec3::splat(factor) },
        }
    }
    /// Whether it changes anything (RoboCAD's 1e-9 thresholds, tools.py:349-355).
    pub fn significant(&self) -> bool {
        match *self {
            Delta::Translate(t) => t.abs().max_element() > 1e-9,
            Delta::Rotate { angle_deg, .. } => angle_deg.abs() > 1e-9,
            Delta::Scale { factor, .. } => (factor - 1.0).abs() > 1e-9,
        }
    }
    pub fn json(&self) -> Value {
        match *self {
            Delta::Translate(t) => json!({"translation": mm(t)}),
            Delta::Rotate { axis, angle_deg, center } => json!({"axis": mm(axis), "angle_deg": r6(angle_deg), "center": mm(center)}),
            Delta::Scale { factor, center } => json!({"scale": r6(factor), "center": mm(center)}),
        }
    }
}

/// Where a body preview is in its life.
#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    /// Following a drag.
    Live,
    /// Released: the commit action was written and waits for the handler.
    Released { action: CadAction, at: Instant },
    /// The commit's edit (sequence `seq`) was started; `ended` once it finished.
    Committed { seq: u64, ended: Option<Instant> },
}

/// The selected bodies' display offset while a transform is previewed.
#[derive(Clone, Debug, PartialEq)]
pub struct Preview {
    pub generation: u64,
    /// The drawn nodes moved (the selected nodes and their descendants).
    pub bodies: Vec<String>,
    pub delta: Delta,
    /// RoboCAD's revision the drag began at.
    pub began: u64,
    pub phase: Phase,
}

/// How a numeric field is read: a length (bare numbers mm), an angle
/// (degrees) or a factor (no unit), as RoboCAD's `NumericField`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Length,
    Angle,
    Factor,
}
impl FieldKind {
    /// RoboCAD's `NumericBar.values`: `evaluate(text, angle, None if angle
    /// or not unit else "mm")`. The error names the token and its position.
    pub fn evaluate(self, text: &str) -> Result<f64, String> {
        let r = match self {
            FieldKind::Length => sim_runtime::units::evaluate(text, false, Some("mm")),
            FieldKind::Angle => sim_runtime::units::evaluate(text, true, None),
            FieldKind::Factor => sim_runtime::units::evaluate(text, false, None),
        };
        r.map_err(|e| e.to_string())
    }
    /// A value as the field shows it (RoboCAD's `set_fields`).
    pub fn show(self, v: f64) -> String {
        match self {
            FieldKind::Length => fl(v),
            FieldKind::Angle => fa(v),
            FieldKind::Factor => g(v),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            FieldKind::Length => "length",
            FieldKind::Angle => "angle",
            FieldKind::Factor => "factor",
        }
    }
}

/// What a numeric field commits.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldCommit {
    Dx,
    Dy,
    Dz,
    Angle,
    Factor,
    Distance,
    /// A live dimension: `CadSetDimension`.
    Dimension { node: String, dimension: Dimension, faces: Vec<i64> },
    /// Shown, not editable: RoboCAD's message.
    ReadOnly(String),
}

/// One numeric field: its name, how it is read, the value it opens with.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: String,
    pub kind: FieldKind,
    pub value: f64,
    pub commit: FieldCommit,
}
impl Field {
    pub fn new(name: impl Into<String>, kind: FieldKind, value: f64, commit: FieldCommit) -> Self {
        Field { name: name.into(), kind, value, commit }
    }
    /// The text the field opens with.
    pub fn text(&self) -> String {
        self.kind.show(self.value)
    }
}

/// The active tool's state (`CadDocument::tool_state`); reset when a tool is activated.
#[derive(Default)]
pub struct ToolState {
    /// The transform tools' pivot (mm) and the rule that gave it.
    pub pivot: Option<(Vec3, PivotRule)>,
    /// The gizmo handle under the pointer: 0, 1, 2 the axes, 3 the centre.
    pub hover: Option<usize>,
    /// The last dragged handle (RoboCAD's `axis_index`; a typed rotation
    /// turns about it, else Z) and, for the centre handle, the view axis.
    pub axis: Option<usize>,
    pub free_axis: Option<Vec3>,
    /// The gizmo drag in progress.
    pub drag: Option<Drag>,
    /// The bodies' display offset.
    pub preview: Option<Preview>,
    /// Push/pull and offset: the face, and the drag in progress.
    pub push: Option<PushTarget>,
    pub push_drag: Option<PushDrag>,
    /// The numeric bar's fields and drafts (`numeric`).
    pub numeric: Numeric,
    /// A double-clicked face's dimension in the bar (RoboCAD's `edit_dimension_at`).
    pub dimension: Option<DimensionEntry>,
    pub measure: MeasureState,
    /// The last snap under the pointer (measure).
    pub snap: Option<Snap>,
    /// The live readout (Δ, angle, scale, push/pull distance, measure distance).
    pub readout: Option<String>,
    /// The selection and the shown revision it was first seen at (its
    /// face indices refer to that revision); kept across tool changes.
    pub selection_seen: Option<(Vec<SelectionItem>, u64)>,
}

pub(super) fn build(app: &mut App) {
    app.insert_gizmo_config(ToolGizmos, GizmoConfig { depth_bias: -1.0, line: GizmoLineConfig { width: 2.5, ..default() }, ..default() })
        .add_systems(OnEnter(ModeScope::Cad), super::numeric::spawn)
        .add_systems(
            Update,
            (
                // After the name field (it may take the keyboard this frame) and before the CAD keys (they honour the focus this sets).
                super::numeric::entry.after(crate::app::actions::serve).after(super::panel::name_entry).before(super::keys::keys),
                keys.after(crate::app::actions::serve).after(super::numeric::entry),
            )
                .in_set(ViewerSet::Input)
                .run_if(in_state(ViewerMode::Cad)),
        )
        .add_systems(
            Update,
            // After the camera snapshot (this frame's view) and the mesh sync (a new mesh and the preview's reset land together).
            (track_selection, gizmo::drag, push_pull::tool, super::measure::tool, dimensions::double_click, super::numeric::sync, previews)
                .chain()
                .after(super::view::update)
                .after(super::mesh::sync)
                .in_set(ViewerSet::SimSync)
                .run_if(in_state(ViewerMode::Cad)),
        )
        .add_systems(Update, (gizmo::draw, push_pull::draw, super::measure::draw, super::numeric::refresh, tool_cursor).in_set(ViewerSet::Present).run_if(in_state(ViewerMode::Cad)))
        .add_systems(OnExit(ModeScope::Cad), restore_cursor);
}

// ---- Formatting ------------------------------------------------------------

/// RoboCAD's `format_length(mm)`: "12.5 mm".
pub(super) fn fl(mm: f64) -> String {
    sim_runtime::units::format_length(mm, "mm", 3)
}
/// RoboCAD's `format_angle(deg)`: "45°".
pub(super) fn fa(deg: f64) -> String {
    sim_runtime::units::format_angle(deg, 2)
}
/// Python's `f"{v:g}"` for the factor field (six significant digits, no trailing zeros).
pub(super) fn g(v: f64) -> String {
    if !v.is_finite() {
        return v.to_string();
    }
    let digits = if v == 0.0 { 0 } else { (5 - v.abs().log10().floor() as i32).clamp(0, 15) as usize };
    let s = format!("{v:.digits$}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    if s == "-0" { "0".into() } else { s }
}
/// A number as sent and labelled: rounded to 1e-6, no "-0".
pub(super) fn r6(v: f32) -> f64 {
    round6(f64::from(v))
}
pub(super) fn round6(v: f64) -> f64 {
    let r = (v * 1e6).round() / 1e6;
    if r == 0.0 { 0.0 } else { r }
}
/// A point or vector as sent: [x, y, z] rounded to 1e-6.
pub(super) fn mm(v: Vec3) -> [f64; 3] {
    [r6(v.x), r6(v.y), r6(v.z)]
}
/// A number in a label: "10", "0.5", "-12.25".
pub(super) fn num(v: f64) -> String {
    format!("{}", round6(v))
}

/// RoboCAD's hint for each tool (`Tool.hint`). Measure's says where the
/// value goes instead of "copied to the clipboard" (`measure` module doc).
pub fn hint(tool: CadTool) -> &'static str {
    match tool {
        CadTool::Select => "Click to select • Shift adds • Ctrl toggles • drag for box select • double-click a face for its dimension",
        CadTool::Move => "Drag an axis (Ctrl snaps to grid) • Tab for exact distance • click the centre handle for screen-space move",
        CadTool::Rotate => "Drag a ring (Ctrl snaps 15°) • Tab for exact angle",
        CadTool::Scale => "Drag an axis handle • Tab for exact factor",
        CadTool::PushPull | CadTool::OffsetFace => "Drag a face along its normal • Tab for exact distance • Shift: offset instead of push",
        CadTool::Measure => "Click two points/faces/edges • the value shows here and in the status line • Shift+click keeps it as an annotation",
    }
}

fn title(name: &str) -> String {
    name.split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or_else(String::new, |f| f.to_uppercase().collect::<String>() + c.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// RoboCAD's mode label (app.py:521): "Push Pull  ·  Face".
pub fn mode_label(doc: &CadDocument) -> String {
    format!("{}  ·  {}", title(doc.tool.name()), title(doc.select_mode.name()))
}

pub(super) fn is_transform(tool: CadTool) -> bool {
    matches!(tool, CadTool::Move | CadTool::Rotate | CadTool::Scale)
}

// ---- Geometry the tools share ------------------------------------------------

/// The nodes `ids` and everything under them in the shown tree (the drawn
/// bodies a transform of `ids` moves).
pub(super) fn preview_bodies(doc: &CadDocument, ids: &[String]) -> Vec<String> {
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

/// The transform tools' pivot (see the module doc's pivot rule).
pub fn pivot(doc: &CadDocument, meshes: Option<&CadMeshes>) -> Option<(Vec3, PivotRule)> {
    let ids = doc.selected_nodes();
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

/// The shown revision the current selection was first seen at
/// ([`track_selection`]); the shown revision when not tracked yet.
pub(super) fn selection_revision(doc: &CadDocument) -> u64 {
    match &doc.tool_state.selection_seen {
        Some((items, revision)) if *items == doc.selection => *revision,
        _ => doc.shown_revision(),
    }
}

/// The first selected face as a push/pull target, at the revision the
/// selection was seen at (described from the topology when that is the shown one).
pub(super) fn face_target(doc: &CadDocument, topology: Option<&CadTopology>) -> Option<PushTarget> {
    let (node, face) = doc.selected_of("face").into_iter().next()?;
    let revision = selection_revision(doc);
    let info = if revision == doc.shown_revision() { topology.and_then(|t| t.get(&node)).and_then(|t| t.faces.iter().find(|f| f.index == face)).cloned() } else { None };
    Some(PushTarget { node, face, revision, info })
}

/// SimSync: remember the shown revision each new selection appeared at, so
/// a face index from it is not used against renumbered faces after an
/// edit. Approximate: a selection RoboCAD remaps during an edit is dated
/// when the viewer adopts it.
pub(super) fn track_selection(doc: Option<ResMut<CadDocument>>) {
    let Some(mut doc) = doc else { return };
    if doc.tool_state.selection_seen.as_ref().is_some_and(|(items, _)| *items == doc.selection) {
        return;
    }
    let seen = (doc.selection.clone(), doc.shown_revision());
    doc.tool_state.selection_seen = Some(seen);
}

/// The camera's back axis (towards the viewer) in the model frame: RoboCAD's `camera.basis()[2]`.
pub(super) fn view_back(view: &CadView) -> Vec3 {
    view.model_from_world.transform_vector3(view.world_from_view.transform_vector3(Vec3::Z)).try_normalize().unwrap_or(Vec3::Z)
}

/// The cursor (window logical pixels) when it is over the 3D view and not over a panel.
pub(super) fn cursor_in_view(window: Option<&Window>, view: &CadView, hover: Option<&HoverMap>, nodes: &Query<(), With<Node>>) -> Option<Vec2> {
    let p = window?.cursor_position()?;
    (view.contains(p) && !super::scene::over_ui(hover, nodes)).then_some(p)
}

/// The first unlocked drawn body under the cursor.
pub(super) struct Hit {
    pub node: String,
    pub triangle: Option<usize>,
    /// The hit point (model mm).
    pub point: Vec3,
}

/// Bevy's `MeshRayCast` along the cursor ray, on `CadBody` entities only;
/// locked nodes are left out as RoboCAD's pick pass leaves them out.
pub(super) fn ray_hit(doc: &CadDocument, cast: &mut MeshRayCast, view: &CadView, cursor: Vec2, bodies: &Query<&CadBody>) -> Option<Hit> {
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
pub(super) fn marker(gizmos: &mut Gizmos<ToolGizmos>, view: &CadView, p: Vec3, pixels: f32, color: Color) {
    let Some(per_pixel) = view.mm_per_pixel(p) else { return };
    let r = per_pixel * pixels * 0.5;
    let side = |axis: Vec3| view.model_from_world.transform_vector3(view.world_from_view.transform_vector3(axis)).try_normalize().unwrap_or(Vec3::ZERO) * r;
    let (right, up) = (side(Vec3::X), side(Vec3::Y));
    let corners = [p - right - up, p + right - up, p + right + up, p - right + up, p - right - up];
    gizmos.linestrip(corners.map(|c| view.world_from_model.transform_point3(c)), color);
}

// ---- Numeric fields ------------------------------------------------------------

/// The active tool's numeric fields (RoboCAD's `ctx.numeric(fields)`):
/// move dx dy dz, rotate angle, scale factor, push/pull or offset distance;
/// in the Select tool a double-clicked face's dimension, else the selected
/// faces' and edges' live dimensions; none for measure.
pub fn fields(doc: &CadDocument, topology: Option<&CadTopology>, meshes: Option<&CadMeshes>) -> Vec<Field> {
    match doc.tool {
        CadTool::Move => vec![Field::new("dx", FieldKind::Length, 0.0, FieldCommit::Dx), Field::new("dy", FieldKind::Length, 0.0, FieldCommit::Dy), Field::new("dz", FieldKind::Length, 0.0, FieldCommit::Dz)],
        CadTool::Rotate => vec![Field::new("angle", FieldKind::Angle, 0.0, FieldCommit::Angle)],
        CadTool::Scale => vec![Field::new("factor", FieldKind::Factor, 1.0, FieldCommit::Factor)],
        CadTool::PushPull | CadTool::OffsetFace => vec![Field::new("distance", FieldKind::Length, 0.0, FieldCommit::Distance)],
        CadTool::Measure => Vec::new(),
        CadTool::Select => match &doc.tool_state.dimension {
            Some(entry) => vec![entry.field.clone()],
            None => dimensions::live(doc, topology, meshes),
        },
    }
}

/// The axis a typed rotation turns about: the last dragged one, else Z
/// (RoboCAD: `axes[self.axis_index or 2]`, but handle 0 counts; see the module doc).
pub(super) fn numeric_axis(state: &ToolState) -> Vec3 {
    match state.axis {
        Some(3) => state.free_axis.unwrap_or(Vec3::Z),
        Some(i) if i < 3 => AXES[i],
        _ => Vec3::Z,
    }
}

// ---- The handler ---------------------------------------------------------------

/// The tool arms of `CadAction` (see the module doc).
pub(super) fn handle(action: &CadAction, call: &mut Call, cx: &mut Cx) -> Outcome {
    match action {
        CadAction::CadTool { tool } => Outcome::Done(Ok(activate(cx, *tool))),
        CadAction::CadCancel => Outcome::Done(Ok(cancel(cx))),
        CadAction::CadTransform { .. } | CadAction::CadPushPull { .. } | CadAction::CadOffsetFaces { .. } | CadAction::CadSetDimension { .. } => commit::commit(cx.doc, cx.topology.as_deref(), call, action),
        CadAction::CadNumeric { values } => commit::numeric(cx, call, values),
        CadAction::CadMeasure { a, b, keep } => commit::measure(cx, call, a, b, *keep),
        _ => Outcome::Done(Err("not a CAD tool action".into())),
    }
}

/// End what follows the pointer: drags, a live preview, the measure tool's first pick.
fn end_live(doc: &mut CadDocument) {
    let s = &mut doc.tool_state;
    s.drag = None;
    s.push_drag = None;
    s.hover = None;
    s.measure.first = None;
    if s.preview.as_ref().is_some_and(|p| p.phase == Phase::Live) {
        s.preview = None;
    }
}

/// `CadTool`: RoboCAD's `set_tool` (the old tool's live work ends, the new
/// one activates). A committed preview is kept until its edit lands.
fn activate(cx: &mut Cx, tool: CadTool) -> Value {
    let doc = &mut *cx.doc;
    end_live(doc);
    let keep = doc.tool_state.preview.take().filter(|p| p.phase != Phase::Live);
    let seen = doc.tool_state.selection_seen.take();
    doc.tool_state = ToolState { preview: keep, selection_seen: seen, ..Default::default() };
    doc.tool = tool;
    let mut status = hint(tool).to_string();
    let mut answer = Map::new();
    answer.insert("tool".into(), json!(tool.name()));
    match tool {
        CadTool::Move | CadTool::Rotate | CadTool::Scale => {
            let p = pivot(doc, cx.meshes.as_deref());
            doc.tool_state.pivot = p;
            if doc.selection.is_empty() {
                status = "Select something to transform".into();
            }
            answer.insert("pivot".into(), p.map_or(Value::Null, |(at, rule)| json!({"point": mm(at), "rule": rule.name()})));
        }
        CadTool::PushPull | CadTool::OffsetFace => {
            // RoboCAD's PushPullTool.activate sets the mode directly (the selection is kept).
            if doc.select_mode != super::document::SelectMode::Face {
                doc.select_mode = super::document::SelectMode::Face;
                super::selection::publish(doc);
            }
            doc.tool_state.push = face_target(doc, cx.topology.as_deref());
            answer.insert("target".into(), doc.tool_state.push.as_ref().map_or(Value::Null, |t| json!({"node": t.node, "face": t.face})));
        }
        CadTool::Select | CadTool::Measure => {}
    }
    answer.insert("hint".into(), json!(hint(tool)));
    doc.show(Ok(status));
    Value::Object(answer)
}

/// `CadCancel` (Escape; RoboCAD app.py:487-497): close the Alt menu if it
/// is open; else end the tool's live work and return to Select; in the
/// Select tool, clear the selection.
fn cancel(cx: &mut Cx) -> Value {
    if cx.doc.candidates.is_some() {
        cx.doc.candidates = None;
        cx.doc.touch();
        return json!({"closed": "the Alt menu"});
    }
    if cx.doc.tool != CadTool::Select {
        let from = cx.doc.tool.name();
        activate(cx, CadTool::Select);
        return json!({"tool": "select", "from": from});
    }
    let doc = &mut *cx.doc;
    end_live(doc);
    doc.tool_state.dimension = None;
    doc.tool_state.numeric.focus = None;
    if doc.selection.is_empty() {
        return json!({"selection": "already empty"});
    }
    doc.selection.clear();
    super::selection::publish(doc);
    json!({"selection": "cleared"})
}

// ---- Systems ---------------------------------------------------------------------

/// RoboCAD's tool cursor (app.py:520-522: arrow for Select, size-all for
/// the transform tools, a cross for the others), over the 3D view only (as
/// RoboCAD sets it on its viewport); the default elsewhere.
pub(super) fn tool_cursor(
    mut commands: Commands,
    doc: Option<Res<CadDocument>>,
    view: Option<Res<CadView>>,
    windows: Query<(Entity, &Window, Option<&CursorIcon>), With<PrimaryWindow>>,
    hover: Option<Res<HoverMap>>,
    nodes: Query<(), With<Node>>,
) {
    let Ok((entity, window, current)) = windows.single() else { return };
    let over_view = view.as_deref().is_some_and(|v| cursor_in_view(Some(window), v, hover.as_deref(), &nodes).is_some());
    let icon = match doc.map(|d| d.tool) {
        Some(CadTool::Move | CadTool::Rotate | CadTool::Scale) if over_view => SystemCursorIcon::Move,
        Some(CadTool::PushPull | CadTool::OffsetFace | CadTool::Measure) if over_view => SystemCursorIcon::Crosshair,
        _ => SystemCursorIcon::Default,
    };
    let want = CursorIcon::System(icon);
    // Inserted only on change (winit applies it when the component changes).
    if current.map_or(icon != SystemCursorIcon::Default, |c| *c != want) {
        commands.entity(entity).try_insert(want);
    }
}

/// OnExit(Cad): the window's default cursor again.
pub(super) fn restore_cursor(mut commands: Commands, windows: Query<(Entity, Option<&CursorIcon>), With<PrimaryWindow>>) {
    for (entity, current) in &windows {
        if current.is_some_and(|c| *c != CursorIcon::default()) {
            commands.entity(entity).try_insert(CursorIcon::default());
        }
    }
}

/// Input: G, R, S, D, Shift+D, M and Escape as the tool controls'
/// actions (`panel::controls`: cad:tool:<tool>, cad:cancel), ignored while
/// a text field has the keyboard (`CadInputFocus`; the numeric bar's own
/// Tab, Enter and Escape are `numeric::entry`'s).
pub(super) fn keys(keys: Option<Res<ButtonInput<KeyCode>>>, focus: Option<Res<CadInputFocus>>, doc: Option<ResMut<CadDocument>>, mut out: MessageWriter<Act<CadAction>>) {
    let Some(keys) = keys else { return };
    if focus.is_some_and(|f| f.0) {
        return;
    }
    let command = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let id = if keys.just_pressed(KeyCode::Escape) {
        "cad:cancel"
    } else if command || alt {
        return;
    } else if !shift && keys.just_pressed(KeyCode::KeyG) {
        "cad:tool:move"
    } else if !shift && keys.just_pressed(KeyCode::KeyR) {
        "cad:tool:rotate"
    } else if !shift && keys.just_pressed(KeyCode::KeyS) {
        "cad:tool:scale"
    } else if keys.just_pressed(KeyCode::KeyD) {
        if shift { "cad:tool:offset_face" } else { "cad:tool:push_pull" }
    } else if !shift && keys.just_pressed(KeyCode::KeyM) {
        "cad:tool:measure"
    } else {
        return;
    };
    let Some(mut doc) = doc else { return };
    let Some(found) = super::panel::controls(&doc).into_iter().find(|c| c.id == id) else { return };
    match found.ready {
        Ok(()) => {
            out.write(Act::ui(found.action));
        }
        Err(why) => doc.show(Err(format!("{}: {why}", found.label))),
    }
}

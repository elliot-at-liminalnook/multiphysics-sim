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
mod numeric_fields;
mod geometry;
mod gizmo;
mod input;
mod preview;
mod push_pull;
#[cfg(test)]
mod tests;

// The paths the rest of CAD mode uses (`super::transform::X`,
// `crate::cad::transform::X`) and the submodules' `super::X`, whichever
// file now holds X. No re-export is wider than its item.
pub(crate) use commit::{OpCall, face_ref};
pub use dimensions::Entry as DimensionEntry;
pub(super) use dimensions::keep_entry;
pub use numeric_fields::{Field, FieldCommit, FieldKind, fields};
pub(super) use numeric_fields::numeric_axis;
pub use geometry::pivot;
pub(super) use geometry::{cursor_in_view, face_target, marker, preview_bodies, ray_hit, selection_revision, track_selection, view_back};
pub use gizmo::Drag;
pub(super) use input::{keys, restore_cursor, tool_cursor};
pub use push_pull::{PushDrag, Target as PushTarget};
pub(super) use preview::{previews, state_json};

use super::actions::{CadAction, Cx};
use super::document::{CadDocument, CadTool};
use super::measure::MeasureState;
use super::numeric::Numeric;
use super::snap::Snap;
use sim_runtime::cad_client::SelectionItem;
use crate::app::actions::Call;
use crate::app::{ModeScope, ViewerMode, ViewerSet};
use bevy::prelude::*;
use serde_json::{Map, Value, json};
use sim_api::Outcome;
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
    /// The inspector's pivot or transform value being typed (`inspector`'s
    /// editors); kept across tool changes. Here so the inspector's part
    /// key, which reads the document only, sees each keystroke.
    pub inspector_edit: Option<super::inspector::EditDraft>,
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
    // The inspector's pivot and transform editors (their typing, Input).
    super::inspector::build(app);
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
    // RoboCAD's `set_tool` replaces a catalogue pick or place tool too: its
    // interaction ends and its form closes (a dialog form stays).
    if let Some(active) = doc.ops.active.take() {
        if doc.ops.form.as_ref().is_some_and(|f| f.op == active) {
            doc.ops.form = None;
        }
        doc.ops.place = None;
    }
    let keep = doc.tool_state.preview.take().filter(|p| p.phase != Phase::Live);
    let seen = doc.tool_state.selection_seen.take();
    let inspector_edit = doc.tool_state.inspector_edit.take();
    doc.tool_state = ToolState { preview: keep, selection_seen: seen, inspector_edit, ..Default::default() };
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
/// is open; else end an active catalogue pick or placement, or an open
/// form, exactly as the form's Cancel does (`ops::form_cancel`); else end
/// the tool's live work and return to Select; in the Select tool, clear
/// the selection.
fn cancel(cx: &mut Cx) -> Value {
    if cx.doc.candidates.is_some() {
        cx.doc.candidates = None;
        cx.doc.touch();
        return json!({"closed": "the Alt menu"});
    }
    if cx.doc.ops.active.is_some() || cx.doc.ops.form.is_some() {
        return super::ops::form_cancel(cx.doc);
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

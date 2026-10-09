//! The one sketch interaction: RoboCAD's `SketchTool` (ui/tools.py:638-817)
//! for all 13 shapes, driven by each shape's row (`specs::spec`); no code
//! per tool.
//!
//! - **Start** ([`begin`], from `ops::invoke` after the tool's form opened
//!   beside the view): the shape's state (`CadDocument::ops.sketch`); the
//!   polygon's `sides` draft is the remembered count (RoboCAD's field opens
//!   with `Sketch.last_polygon_sides`); the text tool's first field, "Text
//!   to sketch:", takes the keyboard (RoboCAD asks with its `getText`
//!   dialog before the tool starts; here that dialog is the tool form's
//!   first field, so the form and the clicks are one interaction).
//! - **Pointer** ([`pointer`], SimSync, after the camera snapshot, the
//!   mesh sync and `CadSet::Plane`, so the sketch cache and the active
//!   plane are the shown revision's): while the active op is `Flow::Sketch(shape)` the cursor is
//!   snapped as RoboCAD's sketch tools snap, always with the active plane
//!   (`snap::snap_on` with its frame, else XY: RoboCAD's `activate` turns
//!   `plane_snapping` on and `press`/`hover` pass `plane=active_plane()`;
//!   Alt suppresses), over the drawn bodies' and the visible sketches'
//!   candidates, and projected onto the plane; it is the preview's last
//!   point (`SketchState::cursor`). A left press (over the 3D view, no
//!   command surface open or just closed by it, no text field keeping the
//!   keyboard) appends the point ([`press`]); the first records the shown
//!   revision (`began`). When the shape has its `needed` points
//!   (`Finish::Points`) its calls (`specs::from_points` on plane
//!   coordinates) go out as ONE `CadSketch { node: None, plane, calls,
//!   revision: began }` (the action picks the sketch, tools.py:675-686,
//!   and refuses by name when the document changed since); the points
//!   reset, a line keeps its last point (lines chain, `SketchState::chained`;
//!   the chained point's segment takes the revision of its next press).
//!   The spline finishes on Enter (no text field focused) or a
//!   double-click with at least two points ([`finish_check`]). Escape is
//!   `CadCancel` (`ops::form_cancel` drops the shape; nothing is sent).
//! - **Refused before it is taken** (recorded decision; RoboCAD's GUI calls
//!   its kernel synchronously, so it has no such case): the action is
//!   handled a frame later, and a finished shape it refused would already
//!   be gone from the tool (fast chained lines were silently lost). So a
//!   press that records `began` (the first point, or the one after a
//!   chained point), a press that completes the shape, and the spline's
//!   finish are checked first, with the action's own checks:
//!   `CadDocument::commit_refusal` (an edit in flight, not connected, the
//!   shown document stale or behind RoboCAD's revision, the revision
//!   changed since `began`) and, when it completes, the plane and the
//!   target sketch (`edits::shape_target`: the active plane node's frame
//!   and every sketch read at the shown revision). Refused, the point is
//!   not added, the shape's points are kept, nothing is sent, and the
//!   status line says "Sketch line not sent: <why>; click again when
//!   RoboCAD has caught up". One case cannot catch up: RoboCAD's revision
//!   moved since the shape's earlier first click (its points were made
//!   against geometry that is gone); then the shape's points are dropped
//!   and the message says to click it again. An edit that starts between
//!   the check and the action's arrival (the next frame's Actions) still
//!   refuses the action by name.
//! - **Double-click**: Qt delivers a double-click instead of the second
//!   press (RoboCAD's `double`), so a second press within [`DOUBLE_CLICK`]
//!   and [`DOUBLE_DISTANCE`] of the first is not a press for any tool; it
//!   only finishes a spline. 400 ms and 5 px are Qt's defaults
//!   (`QStyleHints::mouseDoubleClickInterval`, `mouseDoubleClickDistance`).
//! - **Readout** (`tool_state.readout`, while points are clicked): RoboCAD's
//!   `hover` text (tools.py:720-726): "length L  angle A" (line, spline),
//!   "radius R" (circle, polygon), "W × H" (the rest), from the first point
//!   to the cursor; cleared when the shape or the tool ends.
//! - **Preview** (`preview::draw`, Present): display only.
//!
//! The polygon's side count: a clicked polygon is sent with the viewer's
//! remembered count (`OpsState::polygon_sides`, 6 at first), the one its
//! preview draws (`specs` module doc: RoboCAD's `_build` passes none). A
//! polygon sent with sides (clicked, the form's OK or `cad_sketch`) becomes
//! the remembered count once its edit succeeds (`specs::note_polygon_sides`
//! in `ops::send_sketch`, the one path every sketch call takes;
//! `specs::polygon_edit_done` in `sync::finish_edit`).
//!
//! Deliberately different, recorded: RoboCAD's `SketchTool.deactivate`
//! (tools.py:658-660) turns `plane_snapping` off when a sketch tool ends,
//! whatever it was before the tool. Here the tool never changes the 2D
//! snap toggle (`CadActivePlane::snap_2d`): its own snap always uses the
//! active plane (as RoboCAD's `press`/`hover` pass `plane=`), and the
//! user's toggle stays as the user left it when the tool ends.
use super::specs::{self, local, spec};
use super::{BasePlane, CadActivePlane, CadSketches, Finish, Readout, SketchShape, SketchSpec, SketchState};
use crate::app::actions::Act;
use crate::app::{ViewerMode, ViewerSet};
use crate::cad::actions::CadAction;
use crate::cad::document::CadDocument;
use crate::cad::mesh::CadMeshes;
use crate::cad::ops::{Env, Flow, entry};
use crate::cad::snap::{self, Candidate};
use crate::cad::topology::CadTopology;
use crate::cad::transform::{cursor_in_view, fa, fl};
use crate::cad::view::CadView;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use serde_json::Value;
use crate::cad::types::{PlaneFrame, SketchCall, Uv};
use std::time::{Duration, Instant};

/// Qt's default double-click interval.
pub(crate) const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// Qt's default double-click distance (logical px).
pub(crate) const DOUBLE_DISTANCE: f32 = 5.0;

pub(in crate::cad) fn build(app: &mut App) {
    app.add_systems(
        Update,
        // After the camera snapshot (this frame's view), the mesh sync (the
        // drawn bodies the snap reads) and the sketch cache and active plane
        // (`cache::sync` then `plane::sync`, `CadSet::Plane`): a press stamps
        // this frame's shown revision as `began`, so the plane frame and the
        // sketches it decides with must be that revision's too (as
        // `extrude::pointer` and `plane::picks` are ordered).
        pointer.after(crate::cad::CadSet::View).after(crate::cad::CadSet::Mesh).after(crate::cad::CadSet::Plane).in_set(ViewerSet::SimSync).run_if(in_state(ViewerMode::Cad)),
    )
    .add_systems(Update, super::preview::draw.in_set(ViewerSet::Present).run_if(in_state(ViewerMode::Cad)));
}

/// `Flow::Sketch` starts: RoboCAD's `SketchTool.activate` (and the text
/// tool's dialog), after `ops::invoke` opened the tool's form.
pub(in crate::cad) fn begin(doc: &mut CadDocument, shape: SketchShape) -> Result<(), String> {
    doc.ops.sketch = Some(SketchState::new(shape));
    let sides = doc.ops.polygon_sides.unwrap_or(6);
    if let Some(form) = doc.ops.form.as_mut()
        && let Some(e) = entry(form.op)
        && e.flow == Flow::Sketch(shape)
    {
        let at = |name: &str| e.params.iter().position(|p| p.name == name);
        if shape == SketchShape::Polygon
            && let Some(i) = at("sides")
            && let Some(t) = form.texts.get_mut(i)
        {
            *t = sides.to_string();
        }
        if spec(shape).text_form
            && let Some(i) = at("text")
        {
            form.focus = Some(i);
            form.select_all = true;
        }
    }
    doc.touch();
    Ok(())
}

/// The sketch shape whose tool is active.
fn sketching(doc: &CadDocument) -> Option<SketchShape> {
    match entry(doc.ops.active?)?.flow {
        Flow::Sketch(shape) => Some(shape),
        _ => None,
    }
}

/// RoboCAD's `hover` readout (tools.py:720-725) from `a` (the first point)
/// to `b` (the cursor), plane coordinates.
pub(crate) fn readout(kind: Readout, a: Uv, b: Uv) -> String {
    let (du, dv) = (b[0] - a[0], b[1] - a[1]);
    match kind {
        Readout::LengthAngle => format!("length {}  angle {}", fl(du.hypot(dv)), fa(dv.atan2(du).to_degrees())),
        Readout::Radius => format!("radius {}", fl(du.hypot(dv))),
        Readout::Size => format!("{} × {}", fl(du.abs()), fl(dv.abs())),
    }
}

/// The text tool's text: its form's "Text to sketch:" draft.
fn form_text(doc: &CadDocument) -> String {
    let Some(form) = &doc.ops.form else { return String::new() };
    let Some(e) = entry(form.op) else { return String::new() };
    e.params.iter().position(|p| p.name == "text").and_then(|i| form.texts.get(i)).cloned().unwrap_or_default()
}

/// What happened at a finish: the action to write, or the error to show.
/// `polygon_sides`: the remembered count (`OpsState::polygon_sides`, 6 at
/// first), the one the preview draws.
pub(crate) fn finish_action(shape: SketchShape, s: &SketchState, frame: &PlaneFrame, plane_arg: &Value, polygon_sides: u32) -> Result<Option<CadAction>, String> {
    let uv: Vec<Uv> = s.points.iter().map(|p| local(frame, *p)).collect();
    if spec(shape).text_form && s.text.is_empty() {
        return Err("type the text to sketch first (the form's \"Text to sketch:\" field), then click where it starts".into());
    }
    let calls = specs::from_points(shape, &uv, &s.text, polygon_sides)?;
    if calls.is_empty() {
        return Ok(None);
    }
    Ok(Some(CadAction::CadSketch { node: None, plane: plane_arg.as_str().map(str::to_string), calls: calls.iter().map(SketchCall::to_json).collect(), revision: Some(s.began) }))
}

/// RoboCAD's `_finish` (tools.py:768): the points reset; a line keeps its
/// last point (lines chain: `chained`). True when a point was kept.
pub(crate) fn reset_after_finish(tool: &SketchSpec, s: &mut SketchState) -> bool {
    let last = s.points.last().copied();
    s.points.clear();
    s.chained = match last {
        Some(p) if tool.chains => {
            s.points.push(p);
            true
        }
        _ => false,
    };
    s.chained
}

/// What a press (or the spline's finish) did to the shape.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Step {
    /// The point was added; the shape needs more.
    Added,
    /// The shape is complete and can be sent now: [`finish_action`].
    Finish,
    /// Refused for now (see the module doc): nothing added, the points
    /// kept, nothing sent; the message to show.
    Wait(String),
    /// The document's revision moved since the shape's first click: its points
    /// were dropped; the message to show.
    Dropped(String),
}

/// The checks a press that records `began` or completes the shape (or the
/// spline's finish) makes before it is taken (the module doc): None when
/// it may be taken.
#[allow(clippy::too_many_arguments)]
fn refusal(shape: SketchShape, s: &mut SketchState, began: u64, stamps: bool, completes: bool, doc: &CadDocument, env: &Env, plane_arg: &Value) -> Option<Step> {
    let name = shape.name();
    if let Some(why) = doc.commit_refusal(Some(began)) {
        // `began` stamped now, or a passing refusal (an edit in flight, stale): it catches up.
        if stamps || doc.commit_refusal(None).is_some() {
            let what = if completes { "not sent" } else { "point not taken" };
            return Some(Step::Wait(format!("Sketch {name} {what}: {why}; click again in a moment")));
        }
        s.points.clear();
        s.chained = false;
        let now = doc.shown_revision();
        return Some(Step::Dropped(format!("Sketch {name} not sent: the document changed since its first point was clicked (revision {began}, now {now}); its points are dropped: click the {name} again")));
    }
    if completes && let Err(why) = super::edits::shape_target(plane_arg.as_str(), doc, env) {
        return Some(Step::Wait(format!("Sketch {name} not sent: {why}; click again in a moment")));
    }
    None
}

/// A left press at `p` (snapped onto the plane): the pointer's step,
/// windowless. A press after an empty or chained shape records `began`
/// (the shown revision now); it and a completing press are checked first
/// (the module doc). Taken, the point is added and the chain cleared.
pub(crate) fn press(tool: &SketchSpec, s: &mut SketchState, p: [f64; 3], doc: &CadDocument, env: &Env, plane_arg: &Value) -> Step {
    let stamps = s.points.is_empty() || s.chained;
    let began = if stamps { doc.shown_revision() } else { s.began };
    let completes = matches!(tool.finish, Finish::Points(n) if s.points.len() + 1 >= n);
    if (stamps || completes)
        && let Some(step) = refusal(tool.shape, s, began, stamps, completes, doc, env, plane_arg)
    {
        return step;
    }
    s.began = began;
    s.chained = false;
    s.points.push(p);
    if completes { Step::Finish } else { Step::Added }
}

/// The spline's Enter or double-click with at least two points: checked
/// as a completing press (the module doc).
pub(crate) fn finish_check(tool: &SketchSpec, s: &mut SketchState, doc: &CadDocument, env: &Env, plane_arg: &Value) -> Step {
    let began = s.began;
    refusal(tool.shape, s, began, false, true, doc, env, plane_arg).unwrap_or(Step::Finish)
}

/// The pointer's state across frames.
#[derive(Default)]
pub(super) struct Pointer {
    /// Snap candidates (drawn bodies, visible sketches), keyed by their caches' epochs.
    cache: Option<((u64, u64, u64), Vec<Candidate>)>,
    /// The readout is this module's (cleared when the shape or the tool ends).
    readout: bool,
    /// A command surface was open at the end of the last frame's SimSync,
    /// i.e. when this frame's Input saw the press.
    surface_open: bool,
    /// The last press that counted, for the double-click.
    last_press: Option<(Vec2, Instant)>,
}

/// SimSync: the sketch tool's snapped cursor, presses, finishes and readout
/// (see the module doc).
#[allow(clippy::too_many_arguments)]
pub(super) fn pointer(
    doc: Option<ResMut<CadDocument>>,
    view: Option<Res<CadView>>,
    plane: Option<Res<CadActivePlane>>,
    topology: Option<Res<CadTopology>>,
    meshes: Option<Res<CadMeshes>>,
    sketches: Option<Res<CadSketches>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    keyboard: crate::cad::keys::Held,
    hover: Option<Res<HoverMap>>,
    nodes: Query<(), With<Node>>,
    mut out: MessageWriter<Act<CadAction>>,
    mut state: Local<Pointer>,
    selection: crate::cad::selection::CadSelection,
) {
    let (Some(mut doc), Some(view)) = (doc, view) else { return };
    let state = &mut *state;
    // The surface as this frame's Input saw it: an outside press closed it in Actions, before this system.
    let surface_was_open = std::mem::replace(&mut state.surface_open, doc.ops.surface.is_some());
    let Some(shape) = sketching(&doc) else {
        // The tool ended elsewhere (cancelled, another op): its shape and readout go.
        if doc.ops.sketch.is_some() {
            doc.ops.sketch = None;
        }
        if std::mem::take(&mut state.readout) && doc.tool_state.readout.is_some() {
            doc.tool_state.readout = None;
        }
        state.last_press = None;
        return;
    };
    let tool = spec(shape);
    // RoboCAD's `self.ctx.active_plane()`: the active plane, else XY.
    let frame = plane.as_deref().map_or(Ok(BasePlane::Xy.frame()), CadActivePlane::frame_or_xy);
    let plane_arg = plane.as_deref().map_or_else(|| Value::from(BasePlane::Xy.arg()), |p| p.arg_or(BasePlane::Xy));
    let key = snap::candidates_key(topology.as_deref(), meshes.as_deref(), sketches.as_deref());
    if state.cache.as_ref().is_none_or(|(k, _)| *k != key) {
        state.cache = Some((key, snap::drawn_candidates(&doc, topology.as_deref(), meshes.as_deref(), sketches.as_deref())));
    }
    let candidates: &[Candidate] = match state.cache.as_ref() {
        Some((_, c)) => c,
        None => &[],
    };
    let cursor = if view.valid { cursor_in_view(windows.single().ok(), &view, hover.as_deref(), &nodes) } else { None };
    let alt = keys.as_ref().is_some_and(|k| k.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]));
    let snapped: Option<[f64; 3]> = match (&frame, cursor) {
        // The f64 snap (`Snap::exact`, projected onto the plane): a point snapped onto an existing one coincides with it.
        (Ok(f), Some(c)) => snap::snap_on(&view, c, candidates, alt, Some(f)).map(|s| s.exact),
        _ => None,
    };
    // The form's, the numeric bar's or the inspector editor's field keeping
    // the keyboard after this frame's Input (a press in the view ends their
    // typing first, the kit's or the form's, so that press counts; the
    // sticky name field never blocked a sketch click).
    let typing = keyboard.field().is_some_and(|f| [crate::cad::surfaces::FORM, crate::cad::numeric::NUMERIC, crate::cad::inspector::EDITOR].contains(&f));
    let open = surface_was_open || doc.ops.surface.is_some();
    let pressed = buttons.as_ref().is_some_and(|b| b.just_pressed(MouseButton::Left)) && !open && !typing;
    let press_at = cursor.filter(|_| pressed);
    let enter = keys.as_ref().is_some_and(|k| k.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter])) && !keyboard.get() && !open;
    let now = Instant::now();
    let double = press_at.is_some_and(|c| state.last_press.is_some_and(|(p, t)| now.duration_since(t) <= DOUBLE_CLICK && p.distance(c) <= DOUBLE_DISTANCE));
    if let Some(c) = press_at {
        state.last_press = if double { None } else { Some((c, now)) };
    }

    let before = doc.ops.sketch.clone();
    let mut s = before.clone().filter(|s| s.shape == shape).unwrap_or_else(|| SketchState::new(shape));
    if tool.text_form {
        s.text = form_text(&doc);
    }
    if snapped.is_some() {
        s.cursor = snapped;
    }
    let selection = selection.items();
    let env = Env { defaults: None, selection: &selection, topology: topology.as_deref(), view: Some(&*view), plane: plane.as_deref(), sketches: sketches.as_deref() };
    let mut step = Step::Added;
    let mut error: Option<String> = None;
    if press_at.is_some() && !double {
        match (&frame, snapped) {
            (Ok(_), Some(p)) => step = press(tool, &mut s, p, &doc, &env, &plane_arg),
            (Err(e), _) => error = Some(e.clone()),
            _ => {}
        }
    }
    // RoboCAD's `double` and `key` (Enter): a spline with at least two points.
    if tool.finish == Finish::EnterOrDouble && (double || enter) && s.points.len() >= 2 {
        step = finish_check(tool, &mut s, &doc, &env, &plane_arg);
    }
    let finish = match step {
        Step::Finish => true,
        Step::Added => false,
        Step::Wait(e) | Step::Dropped(e) => {
            error = Some(e);
            false
        }
    };
    if finish && let Ok(f) = &frame {
        match finish_action(shape, &s, f, &plane_arg, doc.ops.polygon_sides.unwrap_or(6)) {
            Ok(Some(action)) => {
                out.write(Act::ui(action));
            }
            Ok(None) => {}
            Err(e) => error = Some(e),
        }
        reset_after_finish(tool, &mut s);
    }
    let text = match (&frame, s.points.first(), s.cursor) {
        (Ok(f), Some(a), Some(b)) => Some(readout(tool.readout, local(f, *a), local(f, b))),
        _ => None,
    };
    if before.as_ref() != Some(&s) {
        doc.ops.sketch = Some(s);
    }
    if let Some(e) = error {
        doc.show(Err(e));
    }
    match text {
        Some(r) => {
            if doc.tool_state.readout.as_deref() != Some(r.as_str()) {
                doc.tool_state.readout = Some(r);
            }
            state.readout = true;
        }
        None => {
            if std::mem::take(&mut state.readout) && doc.tool_state.readout.is_some() {
                doc.tool_state.readout = None;
            }
        }
    }
}

//! RoboCAD's `ExtrudeTool` (ui/tools.py:822-931): extrude and revolve a
//! sketch, curve or sheet, as catalogue entries `tool.extrude` (X) and
//! `tool.revolve` (Shift+R) with `Flow::Extrude` and `Shape::Extrude`.
//!
//! - **Start** ([`begin`], from `ops::invoke` after the entry's form opened
//!   beside the view): the source by RoboCAD's `activate` rule ([`source`]).
//!   Never refused: RoboCAD refuses at apply ("Select a sketch or closed
//!   curve first"); `invoke` shows the hint.
//! - **Source** ([`source`]): the last selected node whose kind is sketch,
//!   curve or sheet; else the first effectively visible sketch with curves
//!   in the shown tree. A sketch's curves are known only once the sketch
//!   cache (`CadSketches`) has read it at the shown revision: an unread
//!   sketch before any known one is [`ExtrudeSource::Reading`] and the run
//!   is refused naming it (never guessed). The source follows the selection
//!   while the tool is active (see "Deliberately different").
//! - **Drag** ([`pointer`], SimSync): a left press over the 3D view (not
//!   over a panel, no command surface open now or when the press came, no
//!   text field focused, the Select tool) starts the drag where the cursor
//!   ray meets the source's plane (the sketch's plane, else the active
//!   plane; RoboCAD's `_plane`) and records the press's boolean in the
//!   form's `boolean` draft (RoboCAD's `_mods`, which a later Tab commit
//!   uses). Moving the pointer sets the height along the plane normal
//!   through a plane facing the camera (`transform::push_distance`; Ctrl or
//!   Command snaps to the 10 mm grid; never exactly 0: 0.001), readout
//!   "extrude <h>". The release with |h| > 1e-6 writes one `CadRun
//!   {tool.extrude, {distance, taper 0, boolean}, revision at the press}`,
//!   the boolean from the release's modifiers (Shift subtract, Ctrl union,
//!   Alt intersect; RoboCAD's `release(pos, mods)`). As RoboCAD, a press
//!   and release without moving applies the current height (10 mm until
//!   the first drag). The drag's taper is always 0, a RoboCAD quirk kept
//!   for parity: its release sends `self.taper`, which stays 0.0 (only its
//!   Tab commit reads the field), so a typed taper applies only to Tab/Enter
//!   (the form's OK); consistent with the preview, which draws no taper.
//! - **Revolve press** ([`pointer`]): RoboCAD's behaviour, kept for parity.
//!   A press on the source's plane and its release (no movement needed:
//!   RoboCAD's `h` starts at 10 and is never 0) writes one `CadRun
//!   {tool.revolve, {angle 360, boolean}, revision at the press}` with the
//!   release's modifiers: RoboCAD's release calls `_apply(h, …, mods)`
//!   with `angle=None`, i.e. `angle or 360.0`, whatever the angle field
//!   says. The readout says so and that Tab/Enter (the form's OK) revolves
//!   by the typed angle. The entry's hint is RoboCAD's one `ExtrudeTool`
//!   hint for both tools.
//! - **Calls** ([`calls`]): `extrude(source, distance, None, taper, False,
//!   op, target)` or `revolve(source, plane.origin, plane.x_axis, angle or
//!   360, op, target)` (commands.py:480, :489), `op` "new" and no target
//!   unless a body is under the selection ([`body_under_selection`]).
//! - **Preview** ([`draw`], Present, display only): a sketch source's
//!   closed curves (all of them when none is closed, as `Ops._profile`) at
//!   the base and moved by h along the plane normal, with a few connecting
//!   lines, in RoboCAD's preview colour (0.4, 0.8, 1.0).
//!
//! Deliberately different from RoboCAD (each with its reason):
//! - The source follows the selection while the tool is active (RoboCAD
//!   fixes it at `activate`): the run reads the selection (as a REST run
//!   reads its items), so the preview and the run always agree.
//! - The preview is outlines only, and taper is not drawn: RoboCAD
//!   tessellates a preview body with its kernel (`_preview`), which has no
//!   REST route. A curve or sheet source's preview is its topology's edges
//!   moved along +Z, the direction `Ops.extrude` uses for a source without
//!   a sketch plane (commands.py:482); RoboCAD previews along the active
//!   plane's normal there, which is not what its apply does.
//! - "The first visible sketch with curves" is in the shown tree's walk
//!   order (RoboCAD iterates `doc.nodes`, its insertion order).
use crate::app::actions::Act;
use crate::app::{ViewerMode, ViewerSet};
use crate::cad::actions::CadAction;
use crate::cad::document::{CadDocument, CadInputFocus, CadTool};
use crate::cad::ops::{Built, Env, Flow, OpEntry, Resolved, entry, history};
use crate::cad::sketch::{CadActivePlane, CadSketches};
use crate::cad::topology::CadTopology;
use crate::cad::transform::{OpCall, ToolGizmos, cursor_in_view, fa, fl, push_distance, round6, view_back};
use crate::cad::view::{CadView, ray_plane};
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use serde_json::{Map, Value, json};
use sim_runtime::cad_client::{PlaneFrame, SelectionItem};

/// RoboCAD's refusal when there is nothing to extrude (`_apply`, ui/tools.py:911-912).
pub const NO_SOURCE: &str = "Select a sketch or closed curve first";
/// The node kinds `activate` takes from the selection (ui/tools.py:839).
const SOURCE_KINDS: [&str; 3] = ["sketch", "curve", "sheet"];
/// RoboCAD's `BooleanOp` values (kernel/base.py:258-262).
const OPS: [&str; 4] = ["new", "union", "subtract", "intersect"];
/// RoboCAD's `ExtrudeTool.h` before any drag (ui/tools.py:829).
const START_HEIGHT: f64 = 10.0;
/// RoboCAD's extrude preview colour (ui/tools.py:904).
const PREVIEW: Color = Color::srgb(0.4, 0.8, 1.0);
/// Points per sampled curve in the preview (RoboCAD's viewport samples 48).
const SAMPLES: usize = 48;
/// Connecting lines per curve between the base and the top outline.
const CONNECTORS: usize = 4;

/// What the tool extrudes (RoboCAD's `ExtrudeTool.source`).
#[derive(Clone, Debug, PartialEq)]
pub enum ExtrudeSource {
    /// A selected sketch, curve or sheet, or the first visible sketch with curves.
    Node(String),
    /// Nothing of those kinds is selected and this visible sketch (the first
    /// in tree order not known to be empty) has not been read yet: whether
    /// it has curves, and so whether it is the source, is not known.
    Reading(String),
    /// Nothing to extrude.
    Nothing,
}

/// A height drag in progress (mm, RoboCAD's frame).
#[derive(Clone, Debug, PartialEq)]
pub struct ExtrudeDrag {
    /// Where the press's ray met the source's plane (RoboCAD's `self.start`).
    pub start: Vec3,
    /// The plane's unit normal.
    pub normal: Vec3,
    /// RoboCAD's revision at the press: the release's run is refused if it changed.
    pub began: u64,
    /// The cursor when the height was last taken (the height changes only when it moves, as RoboCAD's `drag`).
    pub cursor: Vec2,
}

/// The extrude or revolve tool's state (`CadDocument::ops.extrude`).
#[derive(Clone, Debug, PartialEq)]
pub struct ExtrudeState {
    pub revolve: bool,
    pub source: ExtrudeSource,
    /// RoboCAD's `self.h`: the dragged height, kept across drags.
    pub height: f64,
    pub drag: Option<ExtrudeDrag>,
}

pub(in crate::cad) fn build(app: &mut App) {
    app.add_systems(
        Update,
        // After the camera snapshot (this frame's view) and the mesh sync, as the other CAD tools.
        pointer.after(crate::cad::view::update).after(crate::cad::mesh::sync).in_set(ViewerSet::SimSync).run_if(in_state(ViewerMode::Cad)),
    )
    .add_systems(Update, draw.in_set(ViewerSet::Present).run_if(in_state(ViewerMode::Cad)));
}

// ---- The source and the target ---------------------------------------------

/// The node's kind in the shown tree.
fn kind_of<'a>(doc: &'a CadDocument, id: &str) -> Option<&'a str> {
    doc.doc.as_ref().and_then(|d| d.nodes.iter().find(|n| n.id == id)).map(|n| n.kind.as_str())
}

/// The selected nodes, each once, in selection order (RoboCAD's `Selection.nodes()`).
fn selected_nodes(items: &[SelectionItem]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for SelectionItem(node, _, _) in items {
        if !out.contains(node) {
            out.push(node.clone());
        }
    }
    out
}

/// RoboCAD's `ExtrudeTool.activate` source rule (ui/tools.py:837-842) on
/// the selected nodes `nodes` (selection order): the last of kind sketch,
/// curve or sheet; else the first effectively visible sketch with curves
/// (`n.sketch.curves`), its curves read from `sketches` at the shown
/// revision. An unread (or unreadable) sketch met before one with curves is
/// `Reading`: the rule cannot be decided without it.
pub fn source(doc: &CadDocument, nodes: &[String], sketches: Option<&CadSketches>) -> ExtrudeSource {
    if let Some(id) = nodes.iter().rev().find(|n| kind_of(doc, n).is_some_and(|k| SOURCE_KINDS.contains(&k))) {
        return ExtrudeSource::Node(id.clone());
    }
    let Some(state) = &doc.doc else { return ExtrudeSource::Nothing };
    for n in state.nodes.iter().filter(|n| n.kind == "sketch" && n.effective_visible) {
        match sketches.and_then(|c| c.sketch(&n.id)) {
            Some(g) if !g.curves.is_empty() => return ExtrudeSource::Node(n.id.clone()),
            Some(_) => {}
            None => return ExtrudeSource::Reading(n.id.clone()),
        }
    }
    ExtrudeSource::Nothing
}

/// Why sketch `id`'s curves (or plane) are not known: still being read, or the read's error.
fn unread(doc: &CadDocument, id: &str, sketches: Option<&CadSketches>) -> String {
    let name = doc.node_name(id);
    match sketches.and_then(|c| c.error(id)) {
        Some(e) => format!("sketch {name} could not be read from RoboCAD ({e}); select the sketch, curve or sheet to use, or Refresh"),
        None => format!("sketch {name} is still being read from RoboCAD; select the sketch, curve or sheet to use, or try again in a moment"),
    }
}

/// RoboCAD's `App.body_under_selection` (ui/app.py:647-652): the first
/// selected node of kind body, else the only effectively visible body or
/// sheet (`Document.bodies(visible_only=True)`, document.py:430-431) when
/// there is exactly one. Known gap: RoboCAD's `bodies` also requires
/// `n.body is not None`; `NodeSummary` carries no such field, so a body or
/// sheet node without geometry still counts here and can become the target
/// where RoboCAD would pick none (or another body).
pub fn body_under_selection(doc: &CadDocument, nodes: &[String]) -> Option<String> {
    if let Some(id) = nodes.iter().find(|n| kind_of(doc, n) == Some("body")) {
        return Some(id.clone());
    }
    let state = doc.doc.as_ref()?;
    let mut bodies = state.nodes.iter().filter(|n| (n.kind == "body" || n.kind == "sheet") && n.effective_visible);
    match (bodies.next(), bodies.next()) {
        (Some(b), None) => Some(b.id.clone()),
        _ => None,
    }
}

/// RoboCAD's `_boolean_for` modifier order (ui/tools.py:882-890): Shift
/// subtract, Ctrl (Command) union, Alt intersect, else new. Whether a
/// target exists is decided by the run ([`calls`]).
pub fn boolean_for(shift: bool, ctrl: bool, alt: bool) -> &'static str {
    if shift {
        "subtract"
    } else if ctrl {
        "union"
    } else if alt {
        "intersect"
    } else {
        "new"
    }
}

/// Sketch `id`'s own plane at the shown revision (RoboCAD's `n.sketch.plane`).
fn sketch_plane(doc: &CadDocument, id: &str, sketches: Option<&CadSketches>) -> Result<PlaneFrame, String> {
    match sketches.and_then(|c| c.sketch(id)) {
        Some(g) => g.plane.ok_or_else(|| format!("RoboCAD gave no plane for sketch {}", doc.node_name(id))),
        None => Err(unread(doc, id, sketches)),
    }
}

/// RoboCAD's `ExtrudeTool._plane` (ui/tools.py:849-853): a sketch source's
/// own plane, else the active plane (XY when none).
fn source_plane(doc: &CadDocument, source: &ExtrudeSource, sketches: Option<&CadSketches>, plane: Option<&CadActivePlane>) -> Result<PlaneFrame, String> {
    match source {
        ExtrudeSource::Node(id) if kind_of(doc, id) == Some("sketch") => sketch_plane(doc, id, sketches),
        _ => plane.map_or(Ok(PlaneFrame::XY), CadActivePlane::frame_or_xy),
    }
}

// ---- Start and run ---------------------------------------------------------

/// `Flow::Extrude` starts: RoboCAD's `ExtrudeTool.activate` (the source;
/// the height 10). Never refused: RoboCAD refuses at apply.
pub(in crate::cad) fn begin(doc: &mut CadDocument, env: &Env, revolve: bool) -> Result<(), String> {
    let nodes = selected_nodes(&doc.selection);
    let source = source(doc, &nodes, env.sketches);
    doc.ops.extrude = Some(ExtrudeState { revolve, source, height: START_HEIGHT, drag: None });
    Ok(())
}

/// A finite number parameter.
fn number(values: &Map<String, Value>, name: &str) -> Result<f64, String> {
    let v = values.get(name).ok_or_else(|| format!("{name} is required"))?;
    v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| format!("{name} must be a finite number (got {v})"))
}

/// `Shape::Extrude`: RoboCAD's `ExtrudeTool._apply` (ui/tools.py:910-924)
/// on the run's selection (`r.nodes`, the selection or a REST run's items):
/// the source by [`source`]; the boolean from `values["boolean"]` against
/// [`body_under_selection`], "new" with no target when there is none
/// (RoboCAD's `mods & Shift and target`). Extrude: `extrude(source,
/// distance, None, taper, False, op, target)`; revolve: `revolve(source,
/// plane.origin, plane.x_axis, angle or 360, op, target)` with the source
/// sketch's plane, else the active plane.
pub(crate) fn calls(entry: &OpEntry, revolve: bool, r: &Resolved, values: &Map<String, Value>, doc: &CadDocument, env: &Env) -> Result<Built, String> {
    let source = match source(doc, &r.nodes, env.sketches) {
        ExtrudeSource::Node(id) => id,
        ExtrudeSource::Reading(id) => return Err(unread(doc, &id, env.sketches)),
        ExtrudeSource::Nothing => return Err(NO_SOURCE.to_string()),
    };
    let wanted = values.get("boolean").and_then(Value::as_str).unwrap_or("new");
    if !OPS.contains(&wanted) {
        return Err(format!("boolean must be one of {} (got {wanted})", OPS.join(", ")));
    }
    let (op, target) = match body_under_selection(doc, &r.nodes) {
        Some(t) if wanted != "new" => (wanted, Some(t)),
        _ => ("new", None),
    };
    let with = target.as_ref().map_or_else(String::new, |t| format!(", {op} with {}", doc.node_name(t)));
    let name = doc.node_name(&source);
    let target_arg = target.map_or(Value::Null, Value::from);
    let call = if revolve {
        let angle = number(values, "angle")?;
        // RoboCAD's `angle or 360.0`.
        let angle = if angle == 0.0 { 360.0 } else { angle };
        let plane = source_plane(doc, &ExtrudeSource::Node(source.clone()), env.sketches, env.plane)?;
        let label = format!("{} {name}: {}{with}", history(entry.route), fa(angle));
        OpCall { name: entry.route, args: vec![json!(source), json!(plane.origin), json!(plane.x_axis), json!(angle), json!(op), target_arg], kwargs: Map::new(), label }
    } else {
        let distance = number(values, "distance")?;
        let taper = number(values, "taper")?;
        let taper_text = if taper == 0.0 { String::new() } else { format!(", taper {}", fa(taper)) };
        let label = format!("{} {name}: {}{taper_text}{with}", history(entry.route), fl(distance));
        OpCall { name: entry.route, args: vec![json!(source), json!(distance), Value::Null, json!(taper), json!(false), json!(op), target_arg], kwargs: Map::new(), label }
    };
    let label = call.label.clone();
    Ok(Built::Edit { calls: vec![call], label })
}

// ---- The interaction ---------------------------------------------------------

/// The pointer's state across frames.
#[derive(Default)]
struct Pointer {
    /// The readout is this module's (cleared when the tool ends).
    readout: bool,
    /// A command surface was open at the end of the last frame's SimSync,
    /// i.e. when this frame's Input saw the press.
    surface_open: bool,
    /// The unread sketch the status last named. Kept while the fallback
    /// source resolves (so the same sketch re-read at a later revision,
    /// e.g. after the run's own edit, does not overwrite the run's
    /// message); cleared when the selection supplies the source or the
    /// tool ends.
    announced: Option<String>,
}

/// RoboCAD's readout for a revolve press: its release revolves 360°.
const REVOLVE_READOUT: &str = "revolve 360° on release • Tab for an angle";

/// The active extrude or revolve entry.
fn active(doc: &CadDocument) -> Option<(&'static OpEntry, bool)> {
    let e = entry(doc.ops.active?)?;
    match e.flow {
        Flow::Extrude { revolve } => Some((e, revolve)),
        _ => None,
    }
}

/// SimSync: the source following the selection, the press, the height
/// drag and the release (see the module doc).
#[allow(clippy::too_many_arguments)]
fn pointer(
    doc: Option<ResMut<CadDocument>>,
    view: Option<Res<CadView>>,
    sketches: Option<Res<CadSketches>>,
    plane: Option<Res<CadActivePlane>>,
    focus: Option<Res<CadInputFocus>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    hover: Option<Res<HoverMap>>,
    nodes: Query<(), With<Node>>,
    mut out: MessageWriter<Act<CadAction>>,
    mut state: Local<Pointer>,
) {
    let (Some(mut doc), Some(view)) = (doc, view) else { return };
    let state = &mut *state;
    // The surface as this frame's Input saw it: an outside press closed it in Actions, before this system.
    let surface_was_open = std::mem::replace(&mut state.surface_open, doc.ops.surface.is_some());
    let (Some((e, revolve)), Some(before)) = (active(&doc), doc.ops.extrude.clone()) else {
        // The tool ended elsewhere (Escape, another tool or op): its state and readout go.
        if doc.ops.extrude.is_some() && active(&doc).is_none() {
            doc.ops.extrude = None;
        }
        if std::mem::take(&mut state.readout) && doc.tool_state.readout.is_some() {
            doc.tool_state.readout = None;
        }
        state.announced = None;
        return;
    };
    let sketches = sketches.as_deref();
    let mut tool = before.clone();
    tool.revolve = revolve;

    // The source follows the selection; an unread sketch is named once in
    // the status (not again when the same sketch is re-read at a new
    // revision, which would overwrite the run's own message).
    let selected = selected_nodes(&doc.selection);
    tool.source = source(&doc, &selected, sketches);
    match &tool.source {
        ExtrudeSource::Reading(id) if state.announced.as_ref() != Some(id) => {
            let why = unread(&doc, id, sketches);
            doc.show(Ok(format!("{} • {why}", e.hint)));
            state.announced = Some(id.clone());
        }
        // The selection supplies the source: a later fallback is news again.
        ExtrudeSource::Node(id) if selected.contains(id) => state.announced = None,
        _ => {}
    }

    let held = |codes: &[KeyCode]| keys.as_ref().is_some_and(|k| k.any_pressed(codes.iter().copied()));
    let shift = held(&[KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let ctrl = held(&[KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]);
    let alt = held(&[KeyCode::AltLeft, KeyCode::AltRight]);
    let focused = focus.is_some_and(|f| f.0);
    let window = windows.single().ok();
    let cursor = if view.valid { cursor_in_view(window, &view, hover.as_deref(), &nodes) } else { None };
    // A press while a command surface is open (or was, when the press came) only closes it.
    let pressed = buttons.as_ref().is_some_and(|b| b.just_pressed(MouseButton::Left)) && !surface_was_open && doc.ops.surface.is_none() && !focused && doc.tool == CadTool::Select;
    let down = buttons.as_ref().is_some_and(|b| b.pressed(MouseButton::Left));
    let mut just_started = false;
    let mut readout = None;

    if pressed && let Some(c) = cursor {
        // RoboCAD's `press`: `_mods` for a later Tab commit (both tools) …
        let op = boolean_for(shift, ctrl, alt);
        if let Some(i) = e.params.iter().position(|p| p.name == "boolean")
            && doc.ops.form.as_ref().is_some_and(|f| f.op == e.id && f.texts.get(i).is_some_and(|t| t != op))
            && let Some(form) = doc.ops.form.as_mut()
        {
            form.texts[i] = op.to_string();
            doc.touch();
        }
        // … and the drag's start on the source's plane (RoboCAD's `self.start`, both tools).
        match source_plane(&doc, &tool.source, sketches, plane.as_deref()) {
            Err(why) => doc.show(Err(why)),
            Ok(frame) => {
                let (origin, normal) = (v3(frame.origin), v3(frame.normal).try_normalize().unwrap_or(Vec3::Z));
                let start = view.ray(c).and_then(|(o, d)| ray_plane(o, d, origin, normal));
                tool.drag = start.map(|start| ExtrudeDrag { start, normal, began: doc.shown_revision(), cursor: c });
                just_started = tool.drag.is_some();
                if just_started && revolve {
                    readout = Some(REVOLVE_READOUT.to_string());
                }
            }
        }
    }
    if let Some(drag) = tool.drag.clone()
        && !just_started
    {
        if down {
            // RoboCAD's `drag`: only when the pointer moved (kept while over a
            // panel). Revolve's height is never used (its release turns 360°).
            if !revolve
                && let Some(c) = window.and_then(Window::cursor_position).filter(|c| *c != drag.cursor)
                && let Some(h) = push_distance(&view, c, drag.start, drag.normal, view_back(&view), ctrl)
            {
                let h = f64::from(h);
                tool.height = if h.abs() > 1e-6 { h } else { 0.001 };
                tool.drag = Some(ExtrudeDrag { cursor: c, ..drag });
                readout = Some(format!("extrude {}", fl(tool.height)));
            }
        } else {
            // RoboCAD's `release`: `_apply(self.h, self.taper, mods)` with the
            // release's modifiers; `self.taper` stays 0.0 and a revolve turns
            // `angle or 360.0` with `angle=None` (see the module doc).
            tool.drag = None;
            if tool.height.abs() > 1e-6 {
                let mut params = Map::new();
                if revolve {
                    params.insert("angle".into(), json!("360"));
                } else {
                    params.insert("distance".into(), json!(round6(tool.height)));
                    params.insert("taper".into(), json!(0.0));
                }
                params.insert("boolean".into(), json!(boolean_for(shift, ctrl, alt)));
                out.write(Act::ui(CadAction::CadRun { id: e.id.to_string(), params, items: None, revision: Some(drag.began) }));
            }
        }
    }
    if let Some(r) = readout {
        if doc.tool_state.readout.as_deref() != Some(r.as_str()) {
            doc.tool_state.readout = Some(r);
        }
        state.readout = true;
    }
    if tool != before {
        doc.ops.extrude = Some(tool);
    }
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

// ---- The preview ---------------------------------------------------------------

/// The preview's segments (mm, RoboCAD's frame; display only): a sketch
/// source's closed curves (all, when none is closed: `Ops._profile`,
/// commands.py:458-476) at the base and moved by the height along the
/// sketch's normal, with [`CONNECTORS`] lines per curve between them; a
/// curve or sheet source's topology edges moved along +Z. Nothing for
/// revolve (RoboCAD previews none) or an unknown source.
fn preview_lines(tool: &ExtrudeState, doc: &CadDocument, sketches: Option<&CadSketches>, topology: Option<&CadTopology>) -> Vec<(Vec3, Vec3)> {
    let ExtrudeSource::Node(id) = &tool.source else { return Vec::new() };
    if tool.revolve {
        return Vec::new();
    }
    let h = tool.height as f32;
    let mut loops: Vec<Vec<Vec3>> = Vec::new();
    let shift;
    if kind_of(doc, id) == Some("sketch") {
        // Display only: the last read sketch, so the preview does not blink while it is refetched.
        let Some(geo) = sketches.and_then(|c| c.sketch_last(id)) else { return Vec::new() };
        let Some(plane) = geo.plane else { return Vec::new() };
        let closed: Vec<_> = geo.curves.iter().filter(|c| c.closed || matches!(c.kind.as_str(), "circle" | "ellipse" | "slot")).collect();
        let curves: Vec<_> = if closed.is_empty() { geo.curves.iter().collect() } else { closed };
        loops = curves.iter().map(|c| c.sample(SAMPLES).into_iter().map(|[u, v]| v3(plane.to_world(u, v, 0.0))).collect()).collect();
        shift = v3(plane.normal).try_normalize().unwrap_or(Vec3::Z) * h;
    } else {
        let Some(t) = topology.and_then(|t| t.get(id)) else { return Vec::new() };
        for edge in &t.edges {
            let points: Vec<Vec3> = if edge.points.len() >= 2 {
                edge.points.iter().map(|p| v3(*p)).collect()
            } else if let (Some(a), Some(b)) = (edge.start, edge.end) {
                vec![v3(a), v3(b)]
            } else {
                continue;
            };
            loops.push(points);
        }
        shift = Vec3::Z * h;
    }
    let mut out = Vec::new();
    for points in &loops {
        for w in points.windows(2) {
            out.push((w[0], w[1]));
            out.push((w[0] + shift, w[1] + shift));
        }
        let step = (points.len() / CONNECTORS).max(1);
        for p in points.iter().step_by(step).take(CONNECTORS) {
            out.push((*p, *p + shift));
        }
    }
    out
}

/// Present: the extrude preview's lines (display only).
fn draw(doc: Option<Res<CadDocument>>, view: Option<Res<CadView>>, sketches: Option<Res<CadSketches>>, topology: Option<Res<CadTopology>>, mut gizmos: Gizmos<ToolGizmos>) {
    let (Some(doc), Some(view)) = (doc, view) else { return };
    let Some(tool) = doc.ops.extrude.as_ref().filter(|_| active(&doc).is_some()) else { return };
    if !view.valid {
        return;
    }
    let w = |p: Vec3| view.world_from_model.transform_point3(p);
    for (a, b) in preview_lines(tool, &doc, sketches.as_deref(), topology.as_deref()) {
        gizmos.line(w(a), w(b), PREVIEW);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cad::document::CadTarget;
    use crate::cad::ops::values;
    use crate::cad::sketch::cache::Geometry;
    use crate::cad::sketch::{ActivePlane, BasePlane};
    use crate::ui_kit::form::FieldKind;
    use sim_runtime::cad_client::{DocState, NodeSummary, SketchCurve, SketchGeometry};
    use std::sync::Arc;

    fn node(id: &str, kind: &str, name: &str, visible: bool) -> NodeSummary {
        NodeSummary { id: id.into(), kind: kind.into(), name: name.into(), visible, effective_visible: visible, ..Default::default() }
    }

    fn document(nodes: Vec<NodeSummary>) -> CadDocument {
        let mut doc = CadDocument::new(CadTarget::Service("http://127.0.0.1:8420".into()));
        doc.doc = Some(DocState { nodes, revision: 4, ..Default::default() });
        doc.doc_key = Some((None, 4));
        doc
    }

    /// Sketch 1, Body 2 (b1), Body 3 (b2), a curve and a sheet, all visible.
    fn model() -> CadDocument {
        document(vec![node("sk1", "sketch", "Sketch 1", true), node("b1", "body", "Body 2", true), node("b2", "body", "Body 3", true), node("c1", "curve", "Path", true), node("s1", "sheet", "Skin", true)])
    }

    fn sketch(plane: PlaneFrame, curves: usize) -> Geometry {
        let line = SketchCurve { kind: "line".into(), points: vec![[0.0, 0.0], [10.0, 0.0]], ..Default::default() };
        Geometry::Sketch(Arc::new(SketchGeometry { name: "Sketch".into(), plane: Some(plane), curves: vec![line; curves] }))
    }

    fn resolved(nodes: &[&str]) -> Resolved {
        Resolved { nodes: nodes.iter().map(|n| n.to_string()).collect(), revision: 4, ..Default::default() }
    }

    fn op(id: &str) -> &'static OpEntry {
        entry(id).unwrap_or_else(|| panic!("{id} is not in the catalogue"))
    }

    fn run(id: &str, nodes: &[&str], given: Value, doc: &CadDocument, env: &Env) -> Result<OpCall, String> {
        let e = op(id);
        let Value::Object(given) = given else { panic!("parameters are an object") };
        let values = values(e, &given)?;
        let Shape::Extrude { revolve } = e.shape else { panic!("{id} is not an extrude shape") };
        match calls(e, revolve, &resolved(nodes), &values, doc, env)? {
            Built::Edit { mut calls, .. } if calls.len() == 1 => Ok(calls.remove(0)),
            other => panic!("expected one call, got {other:?}"),
        }
    }
    use crate::cad::ops::Shape;

    /// `extrude(source, distance, None, taper, False, op, target)`; each
    /// modifier's op with the first selected body as the target.
    #[test]
    fn extrude_sends_robocad_positional_call_with_the_modifier_boolean() {
        let doc = model();
        let env = Env::default();
        for op_name in ["union", "subtract", "intersect"] {
            let c = run("tool.extrude", &["sk1", "b1"], json!({"distance": 12, "taper": 3, "boolean": op_name}), &doc, &env).unwrap();
            assert_eq!(c.name, "extrude");
            assert_eq!(c.args, vec![json!("sk1"), json!(12.0), Value::Null, json!(3.0), json!(false), json!(op_name), json!("b1")]);
            assert!(c.kwargs.is_empty());
            assert!(c.label.starts_with("Extrude Sketch 1: ") && c.label.ends_with(&format!(", {op_name} with Body 2")), "{}", c.label);
            assert!(c.label.contains("taper"), "{}", c.label);
        }
        // No modifier: a new body, no target, even with a body selected.
        let c = run("tool.extrude", &["sk1", "b1"], json!({"distance": 5}), &doc, &env).unwrap();
        assert_eq!(c.args, vec![json!("sk1"), json!(5.0), Value::Null, json!(0.0), json!(false), json!("new"), Value::Null]);
        assert!(!c.label.contains("taper") && !c.label.contains("with"), "{}", c.label);
    }

    /// `body_under_selection`: with no body selected and two visible bodies
    /// there is no target, so a modifier still makes a new body; with
    /// exactly one visible body or sheet, that one is the target.
    #[test]
    fn without_a_body_under_the_selection_the_extrude_is_new() {
        let doc = model();
        let env = Env::default();
        let c = run("tool.extrude", &["sk1"], json!({"boolean": "subtract"}), &doc, &env).unwrap();
        assert_eq!(&c.args[5..], &[json!("new"), Value::Null]);
        let one = document(vec![node("sk1", "sketch", "Sketch 1", true), node("b1", "body", "Body 2", true), node("b2", "body", "Hidden", false)]);
        let c = run("tool.extrude", &["sk1"], json!({"boolean": "union"}), &one, &env).unwrap();
        assert_eq!(&c.args[5..], &[json!("union"), json!("b1")]);
        // A sheet counts as a body there (`Document.bodies`), not as a selected body.
        let sheet = document(vec![node("c1", "curve", "Path", true), node("s1", "sheet", "Skin", true)]);
        let c = run("tool.extrude", &["c1"], json!({"boolean": "intersect"}), &sheet, &env).unwrap();
        assert_eq!(&c.args[5..], &[json!("intersect"), json!("s1")]);
        assert_eq!(body_under_selection(&doc, &["s1".into()]), None, "two visible bodies and a sheet: none");
    }

    /// `revolve(source, plane.origin, plane.x_axis, angle or 360, op, target)`
    /// about the source sketch's own plane; a curve's about the active plane.
    #[test]
    fn revolve_turns_about_the_source_sketch_plane_x_axis() {
        let doc = model();
        let frame = PlaneFrame { origin: [1.0, 2.0, 3.0], normal: [0.0, 0.0, 1.0], x_axis: [0.0, 1.0, 0.0] };
        let mut cache = CadSketches::default();
        cache.insert("sk1", 4, sketch(frame, 1));
        let env = Env { sketches: Some(&cache), ..Default::default() };
        let c = run("tool.revolve", &["sk1"], json!({}), &doc, &env).unwrap();
        assert_eq!(c.name, "revolve");
        assert_eq!(c.args, vec![json!("sk1"), json!([1.0, 2.0, 3.0]), json!([0.0, 1.0, 0.0]), json!(360.0), json!("new"), Value::Null]);
        let c = run("tool.revolve", &["sk1", "b2"], json!({"angle": 0, "boolean": "union"}), &doc, &env).unwrap();
        assert_eq!(&c.args[3..], &[json!(360.0), json!("union"), json!("b2")], "angle 0 is RoboCAD's `angle or 360`");
        let c = run("tool.revolve", &["sk1"], json!({"angle": 90}), &doc, &env).unwrap();
        assert_eq!(c.args[3], json!(90.0));

        // A curve turns about the active plane (YZ: origin 0, x axis +Y), XY when none.
        let plane = CadActivePlane { plane: Some(ActivePlane::Base(BasePlane::Yz)), ..Default::default() };
        let env_yz = Env { sketches: Some(&cache), plane: Some(&plane), ..Default::default() };
        let c = run("tool.revolve", &["c1"], json!({}), &doc, &env_yz).unwrap();
        assert_eq!(&c.args[..3], &[json!("c1"), json!([0.0, 0.0, 0.0]), json!([0.0, 1.0, 0.0])]);
        let c = run("tool.revolve", &["c1"], json!({}), &doc, &Env::default()).unwrap();
        assert_eq!(c.args[2], json!([1.0, 0.0, 0.0]));

        // A sketch whose plane is not read yet is refused by name, nothing guessed.
        let err = run("tool.revolve", &["sk1"], json!({}), &doc, &Env::default()).unwrap_err();
        assert!(err.contains("Sketch 1") && err.contains("still being read"), "{err}");
    }

    /// `activate`'s rule: the last selected sketch, curve or sheet; else the
    /// first visible sketch with curves; an unread sketch before it is not guessed.
    #[test]
    fn the_source_is_robocad_activate_rule() {
        let doc = model();
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(source(&doc, &ids(&["c1", "sk1", "b1"]), None), ExtrudeSource::Node("sk1".into()));
        assert_eq!(source(&doc, &ids(&["sk1", "s1"]), None), ExtrudeSource::Node("s1".into()));
        assert_eq!(selected_nodes(&[SelectionItem("s1".into(), "face".into(), 2), SelectionItem("c1".into(), "body".into(), 0), SelectionItem("s1".into(), "face".into(), 3)]), ids(&["s1", "c1"]));

        let tree = document(vec![node("hidden", "sketch", "Hidden", false), node("empty", "sketch", "Empty", true), node("sk2", "sketch", "Sketch 2", true), node("b1", "body", "Body", true)]);
        let mut cache = CadSketches::default();
        cache.insert("hidden", 4, sketch(PlaneFrame::XY, 1));
        cache.insert("sk2", 4, sketch(PlaneFrame::XY, 2));
        // The empty sketch is not read yet: it might have curves.
        assert_eq!(source(&tree, &ids(&["b1"]), Some(&cache)), ExtrudeSource::Reading("empty".into()));
        let env = Env { sketches: Some(&cache), ..Default::default() };
        let err = run("tool.extrude", &["b1"], json!({}), &tree, &env).unwrap_err();
        assert!(err.contains("Empty") && err.contains("still being read"), "{err}");
        cache.insert("empty", 4, sketch(PlaneFrame::XY, 0));
        assert_eq!(source(&tree, &ids(&["b1"]), Some(&cache)), ExtrudeSource::Node("sk2".into()));
        let env = Env { sketches: Some(&cache), ..Default::default() };
        let c = run("tool.extrude", &[], json!({}), &tree, &env).unwrap();
        assert_eq!(c.args[0], json!("sk2"));

        // Nothing to extrude: RoboCAD's message.
        let bare = document(vec![node("b1", "body", "Body", true)]);
        assert_eq!(run("tool.extrude", &["b1"], json!({}), &bare, &Env::default()).unwrap_err(), NO_SOURCE);
    }

    /// `_boolean_for`'s order: Shift, then Ctrl, then Alt.
    #[test]
    fn the_modifier_picks_the_boolean_in_robocad_order() {
        assert_eq!(boolean_for(false, false, false), "new");
        assert_eq!(boolean_for(true, true, true), "subtract");
        assert_eq!(boolean_for(false, true, true), "union");
        assert_eq!(boolean_for(false, false, true), "intersect");
    }

    /// `begin` keeps RoboCAD's start height and never refuses.
    #[test]
    fn begin_takes_the_source_and_never_refuses() {
        let mut doc = model();
        doc.selection = vec![SelectionItem("b1".into(), "body".into(), 0), SelectionItem("c1".into(), "body".into(), 0)];
        begin(&mut doc, &Env::default(), false).unwrap();
        assert_eq!(doc.ops.extrude, Some(ExtrudeState { revolve: false, source: ExtrudeSource::Node("c1".into()), height: START_HEIGHT, drag: None }));
        let mut bare = document(Vec::new());
        begin(&mut bare, &Env::default(), true).unwrap();
        assert_eq!(bare.ops.extrude.as_ref().map(|t| &t.source), Some(&ExtrudeSource::Nothing));
    }

    /// The Create entries: RoboCAD's fields, defaults, needs and messages.
    #[test]
    fn the_create_entries_carry_robocad_fields_and_messages() {
        let extrude = op("tool.extrude");
        assert_eq!((extrude.keys, extrude.flow, extrude.category), (&["X"][..], Flow::Extrude { revolve: false }, "Create"));
        let v = values(extrude, &Map::new()).unwrap();
        assert_eq!(Value::Object(v), json!({"distance": 10.0, "taper": 0.0, "boolean": "new"}));
        let revolve = op("tool.revolve");
        assert_eq!((revolve.keys, revolve.flow), (&["Shift+R"][..], Flow::Extrude { revolve: true }));
        assert_eq!(Value::Object(values(revolve, &Map::new()).unwrap()), json!({"angle": 360.0, "boolean": "new"}));
        assert!(matches!(extrude.params[2].kind, FieldKind::Choice { options } if options == ["new", "union", "subtract", "intersect"]));

        use crate::cad::ops::{Arg, Fan, Needs};
        let sweep = op("tool.sweep");
        assert_eq!(sweep.needs, Needs::Nodes { min: 2, max: None, kinds: &["sketch", "curve"] });
        assert_eq!(sweep.args, &[Arg::Target, Arg::Second, Arg::Keyed("twist_deg", "twist")]);
        assert_eq!(values(sweep, &Map::new()).unwrap().get("twist"), Some(&json!(0.0)));
        assert!(values(sweep, &serde_json::from_value(json!({"twist": 4000})).unwrap()).is_err(), "-3600..3600");
        let pipe = op("tool.pipe");
        assert_eq!((pipe.args, pipe.fan), (&[Arg::Node, Arg::Param("diameter")][..], Fan::PerNode));
        assert_eq!(values(pipe, &Map::new()).unwrap().get("diameter"), Some(&json!(4.0)));
        assert_eq!((op("tool.loft").args, op("tool.loft").refusal), (&[Arg::Nodes][..], "Select two or more sketches to loft"));
        assert_eq!((op("tool.fill").args, op("tool.fill").refusal), (&[Arg::Node][..], "Select a closed curve"));
        // RoboCAD's registry order (ui/app.py:332-337).
        let ids: Vec<&str> = crate::cad::ops::CATALOGUE.iter().map(|e| e.id).filter(|id| ["tool.extrude", "tool.revolve", "tool.sweep", "tool.pipe", "tool.loft", "tool.fill"].contains(id)).collect();
        assert_eq!(ids, ["tool.extrude", "tool.revolve", "tool.sweep", "tool.pipe", "tool.loft", "tool.fill"]);
    }
}

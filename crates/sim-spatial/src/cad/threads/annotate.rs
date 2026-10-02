//! Annotate (N) and Reattach… (ui/comments.py:98-125 `AnnotateTool`,
//! 326-346 `begin`, 488-489 `reattach`).
//!
//! - **Start** ([`start`]): refused while a draft is open (RoboCAD's
//!   "Post or cancel your current draft before placing another pin"); the
//!   Select tool replaces any other tool or catalogue interaction (RoboCAD's
//!   `set_tool`), then Annotate takes the 3D view's clicks
//!   (`threads::takes_clicks`: the selection's click stands aside) and the
//!   status line shows RoboCAD's hint. Another tool or interaction started
//!   later ends it, as RoboCAD's `set_tool` does; Escape ends it (`input`).
//! - **Click** ([`click`], Input): a left press over the 3D view (not over
//!   a panel or a pin, not with Alt, which orbits, not while a command
//!   surface is open or the keyboard is held) casts the cursor ray on the
//!   drawn bodies (`transform::ray_hit`) and reads the face only through
//!   `CadMeshes::face_at` at the shown revision; a mesh node has no face
//!   (RoboCAD's hit is then not a face). Nothing hit: RoboCAD's "Click a
//!   visible surface to place the annotation", the tool stays. A hit writes
//!   `cad_threads {op: place}` with the node, the point (mm), the face,
//!   RoboCAD's camera now (`isolation::camera_dict`) and the revision.
//! - **Place** ([`place`]): Annotate ends (RoboCAD returns to the Select
//!   tool first); the Comments dock opens; a new pin becomes the pending
//!   anchor (the "+" pin, "New annotation on …", the composer takes the
//!   keyboard, "Pin placed • write your annotation, then Post annotation");
//!   Reattach's click is one `PATCH /threads/{id}` with the new `node_id`,
//!   `point`, `face` and `view` ([`reattach`]), and the thread is opened.
use super::{HINT, ThreadsArgs, ThreadsOp, Tool, UPDATE, read};
use crate::app::actions::{Act, Call};
use crate::app::{ViewerMode};
use crate::cad::actions::{CadAction, Cx};
use crate::cad::display::CadDisplay;
use crate::cad::document::{CadDocument, CadTool};
use crate::cad::mesh::{CadBody, CadMeshes};
use crate::cad::transform::{cursor_in_view, ray_hit};
use crate::cad::view::CadView;
use crate::cad::views::CadViews;
use bevy::picking::hover::HoverMap;
use bevy::picking::mesh_picking::ray_cast::MeshRayCast;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use serde_json::{Map, Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::AnchorStatus;

/// RoboCAD's refusal for a click on nothing (comments.py:121).
pub(crate) const MISSED: &str = "Click a visible surface to place the annotation";

/// Start Annotate, or Reattach for thread `reattach` (see the module doc).
pub(super) fn start(cx: &mut Cx, call: &mut Call, reattach: Option<String>) -> Outcome {
    if cx.doc.threads.drafting() {
        return Outcome::Done(Err("Post or cancel your current draft before placing another pin".into()));
    }
    if let Some(id) = &reattach
        && read::thread(cx.doc, id).is_none()
    {
        return Outcome::Done(Err(format!("no comment thread {id} in RoboCAD's comments as last read")));
    }
    // RoboCAD's `set_tool`: Annotate replaces the active tool or catalogue interaction.
    if (cx.doc.tool != CadTool::Select || cx.doc.ops.active.is_some())
        && let Outcome::Done(Err(e)) = crate::cad::transform::handle(&CadAction::CadTool { tool: CadTool::Select }, call, cx)
    {
        return Outcome::Done(Err(e));
    }
    cx.doc.threads.tool = Some(Tool { thread: reattach.clone() });
    cx.doc.show(Ok(HINT.into()));
    Outcome::Done(Ok(json!({"annotate": true, "reattach": reattach, "hint": HINT})))
}

/// End Annotate or Reattach (Escape).
pub(super) fn cancel(doc: &mut CadDocument) -> Value {
    let ended = doc.threads.tool.take().is_some();
    if ended {
        doc.show(Ok("Annotate ended".into()));
    }
    json!({"ended": ended})
}

/// Annotate's or Reattach's click (see the module doc).
pub(super) fn place(args: &ThreadsArgs, call: &mut Call, cx: &mut Cx) -> Outcome {
    let done = |r: Result<Value, String>| Outcome::Done(r);
    let Some(tool) = cx.doc.threads.tool.clone() else { return done(Err("Annotate is not active: start Annotate model (N) first".into())) };
    let (Some(node), Some(point)) = (args.node.clone(), args.point) else { return done(Err(MISSED.into())) };
    let shown = cx.doc.shown_revision();
    match args.revision {
        None => return done(Err("place needs revision: the RoboCAD revision the click was made at".into())),
        Some(r) if r != shown => return done(Err(format!("the click was made at revision {r}; RoboCAD's faces may be renumbered since (now {shown}): click again"))),
        Some(_) => {}
    }
    if !cx.doc.doc.as_ref().is_some_and(|d| d.nodes.iter().any(|n| n.id == node)) {
        return done(Err("annotation part does not exist".into()));
    }
    // `begin`: the Select tool comes back first, whatever follows.
    cx.doc.threads.tool = None;
    cx.doc.touch();
    if cx.doc.threads.drafting() {
        cx.doc.threads.claim = Some(super::Field::Compose);
        return done(Err("Post or cancel your current draft before placing another pin".into()));
    }
    cx.doc.threads.open = true;
    let view = args.view.clone().unwrap_or_default();
    match tool.thread {
        Some(id) => {
            let outcome = moved(cx, call, &id, node, point, args.face, view, Some(shown));
            if !matches!(outcome, Outcome::Done(Err(_))) {
                let _ = super::open(cx.doc, &id);
            }
            outcome
        }
        None => {
            let name = cx.doc.node_name(&node);
            let st = &mut cx.doc.threads;
            st.pending = Some(super::Pending { node: node.clone(), point, face: args.face, view, revision: shown });
            st.editing = None;
            st.error = None;
            st.claim = Some(super::Field::Compose);
            cx.doc.show(Ok("Pin placed • write your annotation, then Post annotation".into()));
            done(Ok(json!({"pending": {"node": node, "name": name, "point": point, "face": args.face}})))
        }
    }
}

/// `cad_threads {op: reattach, thread, node, point, face?, view?}`: the pin moved now.
pub(super) fn reattach(args: &ThreadsArgs, id: &str, call: &mut Call, cx: &mut Cx) -> Outcome {
    let (Some(node), Some(point)) = (args.node.clone(), args.point) else { return Outcome::Done(Err("reattach needs node and point: [x, y, z] mm on the part's surface".into())) };
    if !cx.doc.doc.as_ref().is_some_and(|d| d.nodes.iter().any(|n| n.id == node)) {
        return Outcome::Done(Err("annotation part does not exist".into()));
    }
    let began = Some(args.revision.unwrap_or_else(|| cx.doc.shown_revision()));
    moved(cx, call, id, node, point, args.face, args.view.clone().unwrap_or_default(), began)
}

/// One `PATCH /threads/{id}` moving the pin (`update_thread(node_id, point, face, view)`).
#[allow(clippy::too_many_arguments)]
fn moved(cx: &mut Cx, call: &mut Call, id: &str, node: String, point: [f64; 3], face: Option<i64>, view: Map<String, Value>, began: Option<u64>) -> Outcome {
    let Some(t) = read::thread(cx.doc, id).cloned() else { return Outcome::Done(Err(format!("no comment thread {id} in RoboCAD's comments as last read"))) };
    let mut thread = super::source::thread_of(&t);
    let node_name = cx.doc.node_name(&node);
    let pin = super::CadAnchor::Surface { node_id: node, point, face: None, face_index: face, view, state: AnchorStatus::Attached, node_name };
    match thread.targets.first_mut() {
        Some(first) => *first = pin,
        None => thread.targets.push(pin),
    }
    super::ops::put(cx, call, began, UPDATE, thread)
}

/// Whether a command surface was open when this frame's press was made (that press closes it).
#[derive(Default)]
struct ClickState {
    surface_open: bool,
}

/// Input: Annotate's clicks (see the module doc).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn click(
    doc: Option<ResMut<CadDocument>>,
    (view, meshes, views, display): (Option<Res<CadView>>, Option<Res<CadMeshes>>, Option<Res<CadViews>>, Option<Res<CadDisplay>>),
    windows: Query<&Window, With<PrimaryWindow>>,
    (buttons, keys): (Option<Res<ButtonInput<MouseButton>>>, Option<Res<ButtonInput<KeyCode>>>),
    hover: Option<Res<HoverMap>>,
    nodes: Query<(), With<Node>>,
    mut cast: MeshRayCast,
    bodies: Query<&CadBody>,
    held: crate::cad::keys::Held,
    mut out: MessageWriter<Act<CadAction>>,
    mut state: Local<ClickState>,
) {
    let Some(mut doc) = doc else { return };
    let was_open = std::mem::replace(&mut state.surface_open, doc.ops.surface.is_some());
    // Read first: a `ResMut` deref would mark the document changed every frame.
    if doc.threads.tool.is_none() {
        return;
    }
    // Another tool or interaction replaced Annotate (RoboCAD's `set_tool`).
    if doc.tool != CadTool::Select || doc.ops.active.is_some() {
        doc.threads.tool = None;
        doc.touch();
        return;
    }
    let (Some(view), Some(meshes)) = (view, meshes) else { return };
    let pressed = buttons.as_ref().is_some_and(|b| b.just_pressed(MouseButton::Left));
    if !view.valid || !pressed || was_open || doc.ops.surface.is_some() || held.get() {
        return;
    }
    // Alt+left is RoboCAD's orbit, not a pick.
    if keys.as_ref().is_some_and(|k| k.any_pressed([KeyCode::AltLeft, KeyCode::AltRight])) {
        return;
    }
    let Some(cursor) = cursor_in_view(windows.single().ok(), &view, hover.as_deref(), &nodes) else { return };
    let shown = doc.shown_revision();
    let Some(hit) = ray_hit(&doc, &mut cast, &view, cursor, &bodies) else {
        doc.show(Err(MISSED.into()));
        return;
    };
    let mesh_node = doc.doc.as_ref().and_then(|d| d.nodes.iter().find(|n| n.id == hit.node)).is_some_and(|n| n.kind == "mesh");
    let face = hit.triangle.and_then(|t| meshes.face_at(&hit.node, t, shown));
    if face.is_none() && !mesh_node {
        doc.show(Err("The 3D view is still being redrawn for RoboCAD's latest revision: click the surface again in a moment".into()));
        return;
    }
    let point = [f64::from(hit.point.x), f64::from(hit.point.y), f64::from(hit.point.z)];
    let camera = super::isolation::camera_dict(views.as_deref(), display.as_deref()).unwrap_or_default();
    out.write(Act::ui(ThreadsArgs { op: ThreadsOp::Place, node: Some(hit.node), point: Some(point), face, view: Some(camera), revision: Some(shown), ..ThreadsArgs::default() }.action()));
}

/// CadPlugin: Annotate's 3D clicks (Input).
pub(super) fn build(app: &mut App) {
    app.add_systems(Update, click.in_set(crate::app::InputSet::Window).run_if(in_state(ViewerMode::Cad)));
}

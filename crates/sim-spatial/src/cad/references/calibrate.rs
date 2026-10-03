//! Calibrate scale (references.py:211-214) and its tool (`ImageCalibrateTool`,
//! ui/tools.py:1210-1241).
//!
//! - **Start** ([`start`]): align on the image (RoboCAD's `calibrate` aligns
//!   first), then the tool: "Click two points on the image, then type their
//!   real distance" (tools.py:1212). The selection's click stands aside while it takes
//!   clicks (`takes_clicks`, read by `pick`).
//! - **Clicks** ([`click`], Input): a left press over the 3D view (not over a
//!   panel, not Alt, which orbits; not while a command surface is open) is
//!   the cursor ray (`CadView::ray`) met with the image's plane, in f64 (RoboCAD's
//!   `world_on_plane`; no face is picked: an image has no B-rep). Written as
//!   `op: calibrate_pick {point, picked_at}`, checked by [`pick`]: the tool
//!   active, picked at the shown revision, the image's placement read at it
//!   and the point on its plane; a second point equal to the first is
//!   refused with RoboCAD's text before anything is sent. After two points
//!   the tool asks for the real distance: the dock's "Real distance" field
//!   (mm, RoboCAD's numeric field, prefilled with the picked distance).
//! - **Distance** ([`distance`]): one `calibrate_reference(id, first,
//!   second, distance)` through `edit_at` with the picks' revision; refused
//!   by name before sending for a non-positive distance (RoboCAD's
//!   "Pick two distinct points and enter a positive distance") or picks of
//!   an older revision. The tool ends when RoboCAD answers ("Reference
//!   calibrated • Ctrl+Z undoes").
//! - **Escape** ([`escape`]) ends the tool (and is consumed, so the Select
//!   tool's Escape does not also clear the selection); so does the distance
//!   field's Escape, another tool, or an interaction.
//! - **Markers** ([`markers`], Present): "Point 1", "Point 2" as markers
//!   (RoboCAD's `vp.annotations`), display only.
use super::{Focus, ReferencesArgs, ReferencesOp, reads};
use crate::app::actions::{Act, Call};
use crate::app::{ViewerMode, ViewerSet};
use crate::cad::actions::{CadAction, Cx, edit_at};
use crate::cad::document::{CadDocument, CadTool, EditDone};
use crate::cad::sync::value;
use crate::cad::transform::{ToolGizmos, cursor_in_view, fl, marker};
use crate::cad::view::CadView;
use bevy::math::DVec3;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use serde_json::{Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::ImagePlacement;

/// RoboCAD's refusal (references.py `calibrate_reference`, references.py:70).
pub(crate) const DISTINCT: &str = "Pick two distinct points and enter a positive distance";

/// The active calibrate tool.
#[derive(Clone, Debug, PartialEq)]
pub struct Calibrate {
    /// The image node.
    pub id: String,
    /// The picked points (mm, on the image's plane), at most two.
    pub picks: Vec<[f64; 3]>,
    /// The shown revision the picks were made at.
    pub picked_at: Option<u64>,
    /// The distance field's text.
    pub distance: String,
    /// Why the last pick or distance sent nothing, or RoboCAD's refusal.
    pub error: Option<String>,
}
impl Calibrate {
    /// The tool's status line (RoboCAD's hint and status texts).
    pub(crate) fn hint(&self) -> &'static str {
        match self.picks.len() {
            // RoboCAD's tool hint (ui/tools.py:1212).
            0 | 1 => "Click two points on the image, then type their real distance",
            _ => "Type the real distance below, then press Enter • Esc cancels",
        }
    }
    pub(crate) fn json(&self) -> Value {
        json!({"id": self.id, "picks": self.picks, "picked_at": self.picked_at, "distance": self.distance, "error": self.error, "hint": self.hint()})
    }
}

/// `op: calibrate`: align on the image, then the tool, which replaces the
/// active tool or interaction (RoboCAD's `set_tool`).
pub(crate) fn start(cx: &mut Cx, call: &mut Call, id: Option<&String>) -> Result<Value, String> {
    let id = super::target(cx.doc, id)?;
    super::reads::current_placement(cx.doc, &id)?;
    for replace in [(cx.doc.tool != CadTool::Select).then_some(CadAction::CadTool { tool: CadTool::Select }), (cx.doc.ops.active.is_some() || cx.doc.ops.form.is_some()).then_some(CadAction::CadFormCancel)].into_iter().flatten() {
        if let Outcome::Done(Err(e)) = crate::cad::actions::handle(&replace, call, cx) {
            return Err(e);
        }
    }
    super::align::align(cx, &id)?;
    let tool = Calibrate { id: id.clone(), picks: Vec::new(), picked_at: None, distance: String::new(), error: None };
    let hint = tool.hint();
    let st = &mut cx.doc.references;
    st.calibrate = Some(tool);
    // The tool's distance field shows in the dock.
    st.open = true;
    if st.focus == Some(Focus::Distance) {
        st.focus = None;
    }
    cx.doc.show(Ok(hint.to_string()));
    Ok(json!({"calibrating": id, "hint": hint}))
}

/// `op: cancel`: the tool ends.
pub(crate) fn cancel(doc: &mut CadDocument) -> Value {
    let was = doc.references.calibrate.take();
    if was.is_some() {
        if doc.references.focus == Some(Focus::Distance) {
            doc.references.focus = None;
        }
        doc.show(Ok("Calibration cancelled".into()));
    }
    json!({"cancelled": was.map(|c| c.id)})
}

/// Whether `p` lies on `placement`'s plane (mm; a tolerance for rounding).
fn on_plane(placement: &ImagePlacement, p: [f64; 3]) -> bool {
    let n = DVec3::from_array(placement.plane.normal).normalize_or_zero();
    let d = DVec3::from_array(p) - DVec3::from_array(placement.plane.origin);
    d.dot(n).abs() <= 1e-4 * (1.0 + DVec3::from_array(p).abs().max_element())
}

/// `op: calibrate_pick` (see the module doc).
pub(crate) fn pick(doc: &mut CadDocument, args: &ReferencesArgs) -> Result<Value, String> {
    let tool = doc.references.calibrate.clone().ok_or("no calibrate tool is active: press Calibrate scale first")?;
    let point = args.point.ok_or("calibrate_pick takes point: [x, y, z] mm on the image's plane")?;
    let shown = doc.shown_revision();
    let at = args.picked_at.ok_or("pass picked_at: the RoboCAD revision the point was picked at")?;
    let result = check_pick(doc, &tool, point, at, shown);
    let Some(t) = doc.references.calibrate.as_mut() else { return Err("no calibrate tool is active".into()) };
    match result {
        Err(e) => {
            t.error = Some(e.clone());
            doc.touch();
            Err(e)
        }
        Ok(restart) => {
            if restart {
                t.picks.clear();
            }
            t.picks.push(point);
            t.picked_at = Some(at);
            t.error = None;
            let hint = t.hint();
            if t.picks.len() == 2 {
                // RoboCAD's `NumericField("real distance", v_dist(*picks))`.
                t.distance = fl(DVec3::from_array(t.picks[0]).distance(DVec3::from_array(t.picks[1])));
                doc.references.claim = Some(Focus::Distance);
            }
            let picks = t.picks.clone();
            doc.show(Ok(hint.to_string()));
            Ok(json!({"picks": picks, "hint": hint}))
        }
    }
}

/// Why a pick is refused, or whether the picks start over (the first was
/// made at an older revision).
fn check_pick(doc: &CadDocument, tool: &Calibrate, point: [f64; 3], at: u64, shown: u64) -> Result<bool, String> {
    if tool.picks.len() >= 2 {
        return Err("Type the real distance below, then press Enter • Esc cancels".into());
    }
    if !point.iter().all(|x| x.is_finite()) {
        return Err("the point must be three finite coordinates (mm)".into());
    }
    if at != shown {
        return Err(format!("the point was picked at revision {at}; the image may have moved since (now {shown}): click again"));
    }
    let placement = reads::current_placement(doc, &tool.id)?;
    if !on_plane(placement, point) {
        return Err(format!("the point is not on the plane of {}", doc.node_name(&tool.id)));
    }
    // A first point of an older revision: this one is the first again.
    let restart = tool.picked_at.is_some_and(|r| r != at);
    if !restart
        && let Some(first) = tool.picks.first()
        && DVec3::from_array(*first).distance(DVec3::from_array(point)) < 1e-9
    {
        return Err(DISTINCT.into());
    }
    Ok(restart)
}

/// What `calibrate_distance` sends, or why nothing.
fn check_distance(doc: &CadDocument, distance: Option<f64>) -> Result<(Calibrate, [f64; 3], [f64; 3], f64), String> {
    let tool = doc.references.calibrate.clone().ok_or("no calibrate tool is active: press Calibrate scale first")?;
    let distance = distance.ok_or("calibrate_distance takes distance: the real distance in mm")?;
    let (first, second) = match tool.picks.as_slice() {
        [a, b] => (*a, *b),
        picks => return Err(format!("click two points on the image first ({} clicked)", picks.len())),
    };
    if !distance.is_finite() || distance <= 0.0 || DVec3::from_array(first).distance(DVec3::from_array(second)) < 1e-9 {
        return Err(DISTINCT.to_string());
    }
    let shown = doc.shown_revision();
    if tool.picked_at != Some(shown) {
        return Err(format!("the document changed since the points were picked (revision {}, now {shown}): pick them again", tool.picked_at.unwrap_or(0)));
    }
    Ok((tool, first, second, distance))
}

/// `op: calibrate_distance` (see the module doc).
pub(crate) fn distance(doc: &mut CadDocument, call: &mut Call, distance: Option<f64>) -> Outcome {
    let result = check_distance(doc, distance);
    let (tool, first, second, distance) = match result {
        Ok(v) => v,
        Err(e) => {
            if let Some(t) = doc.references.calibrate.as_mut() {
                // Stale points start over.
                if e.starts_with("the document changed") {
                    t.picks.clear();
                    t.picked_at = None;
                }
                t.error = Some(e.clone());
                doc.touch();
            }
            return Outcome::Done(Err(e));
        }
    };
    let name = doc.node_name(&tool.id);
    let id = tool.id.clone();
    let outcome = edit_at(doc, call, tool.picked_at, format!("Calibrate {name}"), move |c| c.calibrate_reference(&id, first, second, distance).map(|r| EditDone { message: "Reference calibrated • Ctrl+Z undoes".into(), result: value(&r) }));
    match &outcome {
        Outcome::Done(Err(e)) => {
            if let Some(t) = doc.references.calibrate.as_mut() {
                t.error = Some(e.clone());
                doc.touch();
            }
        }
        _ => doc.references.pending = Some((doc.edit_seq, super::Pending::Calibrate)),
    }
    outcome
}

/// Where the cursor ray meets `placement`'s plane (mm, f64), in front of the camera.
pub(crate) fn on_image_plane(view: &CadView, cursor: Vec2, placement: &ImagePlacement) -> Option<[f64; 3]> {
    let (origin, direction) = view.ray(cursor)?;
    let (o, d) = (origin.as_dvec3(), direction.as_dvec3());
    let n = DVec3::from_array(placement.plane.normal).normalize_or_zero();
    let denom = d.dot(n);
    if denom.abs() < 1e-12 {
        return None;
    }
    let t = (DVec3::from_array(placement.plane.origin) - o).dot(n) / denom;
    (t >= 0.0).then(|| (o + d * t).to_array())
}

/// Input: the tool's presses (see the module doc).
#[allow(clippy::too_many_arguments)]
fn click(
    doc: Option<Res<CadDocument>>,
    view: Option<Res<CadView>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    (buttons, keys): (Option<Res<ButtonInput<MouseButton>>>, Option<Res<ButtonInput<KeyCode>>>),
    hover: Option<Res<HoverMap>>,
    nodes: Query<(), With<Node>>,
    mut out: MessageWriter<Act<CadAction>>,
    mut surface_open: Local<bool>,
) {
    let Some(doc) = doc else { return };
    let was_open = std::mem::replace(&mut *surface_open, doc.ops.surface.is_some());
    if !super::takes_clicks(&doc) {
        return;
    }
    let Some(view) = view else { return };
    let pressed = buttons.as_ref().is_some_and(|b| b.just_pressed(MouseButton::Left));
    if !view.valid || !pressed || was_open || doc.ops.surface.is_some() {
        return;
    }
    // Alt+left is RoboCAD's orbit, not a pick.
    if keys.as_ref().is_some_and(|k| k.any_pressed([KeyCode::AltLeft, KeyCode::AltRight])) {
        return;
    }
    let Some(cursor) = cursor_in_view(windows.single().ok(), &view, hover.as_deref(), &nodes) else { return };
    let Some(tool) = doc.references.calibrate.as_ref() else { return };
    // Only on the placement of the shown revision (`world_on_plane` of the image as it is).
    let Ok(placement) = reads::current_placement(&doc, &tool.id) else { return };
    let Some(point) = on_image_plane(&view, cursor, placement) else { return };
    out.write(Act::ui(ReferencesArgs { point: Some(point), picked_at: Some(doc.shown_revision()), ..ReferencesArgs::of(ReferencesOp::CalibratePick) }.action()));
}

/// Input, in `CadKeySet::EscapeTool` (after the key gate, before RoboCAD's
/// shortcuts, the later `CadKeySet::Escape` readers and the Select tool's
/// keys) and only with the keys free (no field typing, no pending chord:
/// `keys::free`): Escape ends the active tool and is consumed, so the
/// threads' Escape (`CadKeySet::Escape`) never sees the same press.
/// An open file form (`files::form`) takes Escape first: it closes itself.
fn escape(doc: Option<Res<CadDocument>>, keys: Option<ResMut<ButtonInput<KeyCode>>>, files: Option<Res<crate::cad::files::CadFiles>>, mut out: MessageWriter<Act<CadAction>>) {
    let (Some(doc), Some(mut keys)) = (doc, keys) else { return };
    if doc.references.calibrate.is_none() || !keys.just_pressed(KeyCode::Escape) || files.as_ref().is_some_and(|f| f.form.is_some()) {
        return;
    }
    keys.clear_just_pressed(KeyCode::Escape);
    out.write(Act::ui(ReferencesArgs::of(ReferencesOp::Cancel).action()));
}

/// RoboCAD's annotation colour for picked points (display only).
const POINT_COLOUR: Color = Color::srgb(1.0, 0.85, 0.2);

/// Present: the picked points.
fn markers(doc: Option<Res<CadDocument>>, view: Option<Res<CadView>>, mut gizmos: Gizmos<ToolGizmos>) {
    let (Some(doc), Some(view)) = (doc, view) else { return };
    let Some(tool) = doc.references.calibrate.as_ref().filter(|_| view.valid) else { return };
    for p in &tool.picks {
        marker(&mut gizmos, &view, Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32), 9.0, POINT_COLOUR);
    }
}

/// CadPlugin: the clicks and Escape (Input, before the CAD keys) and the markers.
pub(super) fn build(app: &mut App) {
    app.add_systems(
        Update,
        (
            click.in_set(crate::app::InputSet::Window),
            escape.run_if(crate::cad::keys::free).in_set(crate::cad::CadKeySet::EscapeTool),
            markers.in_set(ViewerSet::Present),
        )
            .run_if(in_state(ViewerMode::Cad)),
    );
}

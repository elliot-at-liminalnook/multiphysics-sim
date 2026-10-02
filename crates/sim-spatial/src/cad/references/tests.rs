//! The references without a window: the controls' REST round trip, Apply
//! placement's one `update_reference` with RoboCAD's plane choices, the
//! calibrate tool's refusals, the align camera against RoboCAD's formula,
//! Open in builder's switch, the status line and the drops' mode gate.
use super::calibrate::{self, Calibrate, DISTINCT};
use super::form::{self, Values};
use super::reads::{self, Placed};
use super::system_link::{self, LINK_FIRST, READING};
use super::{BrowseKind, Focus, PlaneChoice, ReferencesArgs, ReferencesOp, command_action, controls_of, specs};
use crate::app::ViewerMode;
use crate::app::actions::{Act, Action, Call, Origin, Replies, control_matches};
use crate::app::switch::{Document, WindowAction};
use crate::cad::actions::{CadAction, Cx};
use crate::cad::document::{CadDocument, CadTarget, Connection};
use crate::cad::selection::Fixture;
use crate::cad::sketch::{ActivePlane, BasePlane, CadActivePlane};
use bevy::ecs::message::Messages;
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::window::FileDragAndDrop;
use serde_json::{Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::{CadClient, DocState, Health, ImagePlacement, LinkState, NodeSummary, PlaneJson, SystemStatus, SystemSummary};
use std::path::PathBuf;

const SYSTEM: &str = "/work/arm/arm.system.json";

fn node(id: &str, kind: &str, name: &str) -> NodeSummary {
    NodeSummary { id: id.into(), kind: kind.into(), name: name.into(), visible: true, effective_visible: true, locked: kind == "image", ..Default::default() }
}

/// A 100 × 50 mm image on Front (XZ), its corner at the origin.
fn front() -> ImagePlacement {
    ImagePlacement { path: "/work/arm/plan.png".into(), plane: PlaneJson { origin: [0.0; 3], normal: [0.0, -1.0, 0.0], x_axis: [1.0, 0.0, 0.0] }, width: 100.0, height: 50.0, opacity: 0.6, rotation_deg: 0.0 }
}

/// RoboCAD at revision 4 with two images and a body; `connected` with a client.
fn document(connected: bool) -> CadDocument {
    let mut doc = CadDocument::new(CadTarget::Service("http://127.0.0.1:8420".into()));
    if connected {
        doc.client = Some(CadClient::new("http://127.0.0.1:8420").unwrap());
        doc.connection = Connection::Connected;
    }
    doc.health = Some(Health { ok: true, app: "robocad".into(), revision: 4, ..Default::default() });
    doc.doc_key = Some((None, 4));
    doc.doc = Some(DocState { nodes: vec![node("i1", "image", "plan.png"), node("i2", "image", "side.webp"), node("b1", "body", "Bracket")], revision: 4, ..Default::default() });
    doc.references.reads.placements.insert("i1".into(), Placed { revision: 4, placement: Ok(front()) });
    reads::follow_form(&mut doc);
    doc
}

fn status(doc: &mut CadDocument, state: LinkState) {
    let s = SystemStatus {
        state,
        path: (state != LinkState::Unlinked).then(|| SYSTEM.to_string()),
        error: (state == LinkState::Missing).then(|| "No such file".to_string()),
        link: Some(SystemSummary { title: "Arm".into(), revision: 2, ..Default::default() }),
        now: Some(SystemSummary { title: "Arm".into(), revision: 3, definitions: 5, ..Default::default() }),
    };
    doc.references.reads.status = Some(((doc.generation, doc.shown_revision()), Ok(s)));
}

/// Applies `args` through the one handler REST and `system_ui` use (no window).
fn apply(args: ReferencesArgs, doc: &mut CadDocument, plane: &mut CadActivePlane) -> (Outcome, Vec<crate::camera::CameraAction>) {
    apply_in(args, doc, plane, None)
}

/// [`apply`] with the window's view (`view`: CAD mode's 3D view, as in a window).
fn apply_in(args: ReferencesArgs, doc: &mut CadDocument, plane: &mut CadActivePlane, view: Option<&crate::cad::view::CadView>) -> (Outcome, Vec<crate::camera::CameraAction>) {
    let mut f = Fixture::at(4);
    let (mut continuation, mut replies) = (Value::Null, Replies::default());
    let mut call = Call { origin: Origin::Ui, continuation: &mut continuation, cancelled: false, replies: &mut replies };
    let mut cx = Cx { settings: &mut crate::app::settings::SettingsOwner::default(), doc, shared: f.shared(), meshes: None, topology: None, view, plane, sketches: None, display: None, views: None, files: None, components: &mut crate::cad::components::ComponentsState::default(), composition: &mut crate::cad::composition::CadCompositionState::default(), experiments: &mut crate::cad::experiments::ExperimentsState::default(), review: &mut crate::cad::experiment_review::ReviewState::default(), motion: &mut crate::cad::motion::MotionState::default(), camera: Vec::new() };
    let outcome = crate::cad::actions::handle(&args.action(), &mut call, &mut cx);
    (outcome, cx.camera)
}

/// Every control fits `cad:references:<id>` and its REST form parses back to
/// the action its button writes; the spec's example parses; the registry's two
/// commands are the dock and Add reference images.
#[test]
fn controls_fit_the_pattern_and_round_trip_through_rest() {
    let mut doc = document(true);
    status(&mut doc, LinkState::Changed);
    super::browse(&mut doc, BrowseKind::System, true);
    doc.references.calibrate = Some(Calibrate { id: "i1".into(), picks: vec![], picked_at: None, distance: String::new(), error: None });
    let controls = controls_of(&doc);
    let ids: Vec<&str> = controls.iter().map(|c| c.0.as_str()).collect();
    for want in ["dock", "add", "link", "accept", "open_builder", "unlink", "browse_close", "image-i1", "visible-i2", "plane-keep", "plane-active", "locked", "apply", "align", "calibrate", "sketch", "remove", "calibrate_cancel"] {
        assert!(ids.contains(&format!("cad:references:{want}").as_str()), "{want}: {ids:?}");
    }
    for (id, _, action, _) in &controls {
        assert!(control_matches("cad:references:<id>", id), "{id}");
        let Value::Object(mut args) = crate::cad::rest_form::rest_form(action) else { panic!("{id}") };
        let name = args.remove("command").and_then(|v| v.as_str().map(str::to_string)).unwrap();
        assert_eq!(name, "cad_references");
        let parsed = <CadAction as Action>::parse(&sim_api::Command { command: name, args: Value::Object(args) }).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(&parsed, action, "{id}");
    }
    let ready = |id: &str| controls.iter().find(|c| c.0 == format!("cad:references:{id}")).unwrap().3.clone();
    assert!(ready("accept").is_ok() && ready("unlink").is_ok() && ready("apply").is_ok());
    for s in specs() {
        <CadAction as Action>::parse(&sim_api::Command { command: s.name.into(), args: s.example.clone() }).unwrap_or_else(|e| panic!("{}: {e}", s.name));
    }
    assert_eq!(command_action("view.references"), Some(ReferencesArgs { open: Some(true), ..ReferencesArgs::of(ReferencesOp::Dock) }.action()));
    assert!(matches!(command_action("reference.import"), Some(CadAction::CadReferences(ReferencesArgs { op: ReferencesOp::Browse, kind: Some(BrowseKind::Images), .. }))));
    assert_eq!(command_action("view.comments"), None);
    // Accept changes is ready only for a changed file; Unlink only with a link.
    status(&mut doc, LinkState::Current);
    assert!(controls_of(&doc).iter().find(|c| c.0 == "cad:references:accept").unwrap().3.is_err());
    status(&mut doc, LinkState::Unlinked);
    let c = controls_of(&doc);
    assert!(c.iter().find(|c| c.0 == "cad:references:unlink").unwrap().3.is_err());
    assert_eq!(c.iter().find(|c| c.0 == "cad:references:open_builder").unwrap().3, Err(LINK_FIRST.to_string()));
    // An argument of another op is refused by name.
    let mut plane = CadActivePlane::default();
    let (out, _) = apply(ReferencesArgs { distance: Some(1.0), ..ReferencesArgs::of(ReferencesOp::Dock) }, &mut doc, &mut plane);
    assert!(matches!(out, Outcome::Done(Err(e)) if e.starts_with("distance does not belong to op dock")));
}

/// Apply placement is one `update_reference` with every value RoboCAD's
/// `commit` sends: Keep sends no plane, Front "xz", Side "yz", Top "xy",
/// Active the active plane (else XY); opacity % → 0..1; ranges refused.
#[test]
fn apply_placement_builds_robocads_one_update() {
    let doc = document(true);
    let f = doc.references.form.clone().expect("the current image's form is loaded");
    assert_eq!(f.id, "i1");
    assert_eq!(f.texts, ["100.00", "0.00", "0.00", "0.00", "0.00", "60"].map(String::from));
    let v = f.values().unwrap();
    assert_eq!(v, Values { width: 100.0, origin: [0.0; 3], rotation_deg: 0.0, opacity_pct: 60.0 });
    let none = CadActivePlane::default();
    let mut node = CadActivePlane::default();
    node.plane = Some(ActivePlane::Node { id: "p7".into(), frame: None });
    let cases = [(PlaneChoice::Keep, &none, None), (PlaneChoice::Front, &none, Some(json!("xz"))), (PlaneChoice::Side, &none, Some(json!("yz"))), (PlaneChoice::Top, &none, Some(json!("xy"))), (PlaneChoice::Active, &none, Some(json!("xy"))), (PlaneChoice::Active, &node, Some(json!("p7")))];
    for (choice, active, want) in cases {
        let u = form::update(v, choice, true, active).unwrap();
        assert_eq!(u.plane, want, "{choice:?}");
        assert_eq!((u.width, u.opacity, u.origin, u.rotation_deg, u.locked), (Some(100.0), Some(0.6), Some([0.0; 3]), Some(0.0), Some(true)));
        assert_eq!((u.visible, u.name), (None, None));
    }
    // Typed values as the spin boxes hold them (two decimals, unit expressions).
    let mut typed = f.clone();
    typed.texts[0] = "12.345 cm".into();
    typed.texts[5] = "33.6".into();
    typed.plane = PlaneChoice::Side;
    let args = typed.apply().unwrap();
    assert_eq!((args.width, args.opacity_pct, args.plane, args.revision), (Some(123.45), Some(34.0), Some(PlaneChoice::Side), Some(4)));
    typed.texts[5] = "120".into();
    assert!(typed.apply().unwrap_err().starts_with("Opacity:"));
    assert!(form::update(Values { width: 0.0, ..v }, PlaneChoice::Keep, true, &none).unwrap_err().starts_with("Width:"));
    // Refused by name with nothing sent: values of an older revision, an out-of-range value.
    let mut doc = document(true);
    let mut plane = CadActivePlane::default();
    let stale = ReferencesArgs { revision: Some(3), ..f.apply().unwrap() };
    let (out, _) = apply(stale, &mut doc, &mut plane);
    assert!(matches!(&out, Outcome::Done(Err(e)) if e.contains("changed since these values were taken")), "{out:?}");
    assert!(doc.edit.is_none() && doc.references.pending.is_none());
    assert!(doc.references.form.as_ref().unwrap().error.is_some(), "the refusal shows under Apply");
    let (out, _) = apply(ReferencesArgs { opacity_pct: Some(150.0), ..ReferencesArgs::on(ReferencesOp::Placement, "i1") }, &mut doc, &mut plane);
    assert!(matches!(&out, Outcome::Done(Err(e)) if e.starts_with("Opacity: 150 is outside")), "{out:?}");
    assert!(doc.edit.is_none());
    // The form's choice is display state.
    let (out, _) = apply(ReferencesArgs { plane: Some(PlaneChoice::Top), locked: Some(false), ..ReferencesArgs::of(ReferencesOp::Form) }, &mut doc, &mut plane);
    assert!(matches!(out, Outcome::Done(Ok(_))));
    let f = doc.references.form.as_ref().unwrap();
    assert_eq!((f.plane, f.locked, f.error.clone()), (PlaneChoice::Top, false, None));
}

/// The calibrate tool: picks on the image's plane at the shown revision;
/// stale and off-plane picks, an equal second point and a non-positive
/// distance are refused by name before anything is sent; points of an older
/// revision start over.
#[test]
fn calibrate_refuses_equal_points_and_stale_picks() {
    let mut doc = document(false);
    let pick = |point: [f64; 3], at: u64| ReferencesArgs { point: Some(point), picked_at: Some(at), ..ReferencesArgs::of(ReferencesOp::CalibratePick) };
    assert!(calibrate::pick(&mut doc, &pick([10.0, 0.0, 10.0], 4)).unwrap_err().starts_with("no calibrate tool"));
    doc.references.calibrate = Some(Calibrate { id: "i1".into(), picks: vec![], picked_at: None, distance: String::new(), error: None });
    assert!(super::takes_clicks(&doc));
    assert!(calibrate::pick(&mut doc, &pick([10.0, 0.0, 10.0], 3)).unwrap_err().starts_with("the point was picked at revision 3"));
    assert!(calibrate::pick(&mut doc, &pick([10.0, 5.0, 10.0], 4)).unwrap_err().starts_with("the point is not on the plane of plan.png"));
    calibrate::pick(&mut doc, &pick([10.0, 0.0, 10.0], 4)).unwrap();
    assert_eq!(calibrate::pick(&mut doc, &pick([10.0, 0.0, 10.0], 4)), Err(DISTINCT.to_string()));
    calibrate::pick(&mut doc, &pick([60.0, 0.0, 10.0], 4)).unwrap();
    let tool = doc.references.calibrate.clone().unwrap();
    assert_eq!((tool.picks.len(), tool.picked_at, tool.distance.as_str()), (2, Some(4), crate::cad::transform::fl(50.0).as_str()));
    assert!(!super::takes_clicks(&doc), "two points: the distance field, not clicks");
    assert_eq!(doc.references.claim, Some(Focus::Distance));
    assert!(super::input::distance_value("5 cm").is_ok_and(|v| (v - 50.0).abs() < 1e-9));
    let (mut continuation, mut replies) = (Value::Null, Replies::default());
    let mut call = Call { origin: Origin::Ui, continuation: &mut continuation, cancelled: false, replies: &mut replies };
    assert!(matches!(calibrate::distance(&mut doc, &mut call, Some(0.0)), Outcome::Done(Err(e)) if e == DISTINCT));
    assert!(matches!(calibrate::distance(&mut doc, &mut call, Some(-3.0)), Outcome::Done(Err(e)) if e == DISTINCT));
    // RoboCAD moved on: the points are stale, refused, and start over.
    doc.health.as_mut().unwrap().revision = 5;
    doc.doc_key = Some((None, 5));
    assert!(matches!(calibrate::distance(&mut doc, &mut call, Some(25.0)), Outcome::Done(Err(e)) if e.starts_with("the document changed since the points were picked")));
    assert!(doc.references.calibrate.as_ref().unwrap().picks.is_empty() && doc.edit.is_none());
    assert_eq!(calibrate::cancel(&mut doc)["cancelled"], json!("i1"));
    assert!(doc.references.calibrate.is_none());
}

/// Align view is RoboCAD's camera: trackball rows [x_axis, y_axis, normal],
/// orthographic, target `to_world(w/2, h/2)`, distance
/// `max(h, w/aspect) · 0.6 / tan(fov/2)`, mapped to the display frame; the
/// image's plane becomes the active plane when it is XY, XZ, YZ or a plane node.
#[test]
fn the_align_camera_is_robocads_formula() {
    let p = front();
    let fov = 45f64.to_radians();
    let cam = super::align::camera(&p, 2.0, fov);
    let distance = 50f64.max(100.0 / 2.0) * 0.6 / (fov / 2.0).tan();
    assert!((f64::from(cam.radius) - distance / 1000.0).abs() < 1e-7, "{cam:?}");
    // Target (50, 0, 25) mm is display (0.05, 0.025, 0) m.
    assert!((Vec3::from_array(cam.focus) - Vec3::new(0.05, 0.025, 0.0)).length() < 1e-6, "{cam:?}");
    assert!(cam.orthographic && cam.fov_deg.is_none() && cam.seconds.is_none());
    // XZ's right +X, up +Z, back −Y: the display camera's own axes (identity).
    let q = Quat::from_array(cam.trackball.unwrap());
    assert!(q.angle_between(Quat::IDENTITY) < 1e-5, "{q:?}");
    assert!(cam.yaw.abs() < 1e-5 && cam.pitch.abs() < 1e-5);
    // A wide view: the height fits.
    let tall = super::align::camera(&p, 0.5, fov);
    assert!((f64::from(tall.radius) - 200.0 * 0.6 / (fov / 2.0).tan() / 1000.0).abs() < 1e-7);
    // Through the handler: the camera action, the active plane XZ, display only.
    let mut doc = document(true);
    let mut plane = CadActivePlane::default();
    let (out, camera) = apply(ReferencesArgs::on(ReferencesOp::Align, "i1"), &mut doc, &mut plane);
    assert!(matches!(out, Outcome::Done(Ok(_))));
    assert!(matches!(camera.as_slice(), [crate::camera::CameraAction::Set { state }] if state.orthographic));
    assert_eq!(plane.plane, Some(ActivePlane::Base(BasePlane::Xz)));
    assert!(doc.edit.is_none(), "nothing is sent to RoboCAD");
    // Off the named planes and with no plane node there: the active plane stays.
    let mut off = front();
    off.plane.origin = [0.0, 5.0, 0.0];
    assert_eq!(super::align::plane_for(&off, &doc, None), None);
}

/// Open in builder with a linked file writes one window action: a switch to
/// Build mode on the linked file (no process); unlinked or missing is
/// RoboCAD's refusal; with nothing read yet it asks for the status.
#[test]
fn open_in_builder_switches_this_window_to_build_on_the_linked_file() {
    let mut world = World::default();
    world.init_resource::<Messages<Act<WindowAction>>>();
    world.init_resource::<Messages<Act<CadAction>>>();
    let mut doc = document(false);
    let mut plane = CadActivePlane::default();
    let (out, _) = apply(ReferencesArgs::of(ReferencesOp::OpenBuilder), &mut doc, &mut plane);
    assert!(matches!(&out, Outcome::Done(Err(e)) if e.starts_with(READING)), "{out:?}");
    assert!(doc.references.reads.want_status.load(std::sync::atomic::Ordering::Relaxed), "the status is read for it");
    for state in [LinkState::Unlinked, LinkState::Missing] {
        status(&mut doc, state);
        let (out, _) = apply(ReferencesArgs::of(ReferencesOp::OpenBuilder), &mut doc, &mut plane);
        assert!(matches!(&out, Outcome::Done(Err(e)) if e == LINK_FIRST), "{state:?}: {out:?}");
    }
    status(&mut doc, LinkState::Current);
    // Without a window there is nothing to switch: refused, nothing requested.
    let (out, _) = apply(ReferencesArgs::of(ReferencesOp::OpenBuilder), &mut doc, &mut plane);
    assert!(matches!(&out, Outcome::Done(Err(e)) if e.ends_with("there is no window to switch")), "{out:?}");
    assert!(doc.references.switch_to.is_none());
    let view = crate::cad::view::CadView::default();
    let (out, _) = apply_in(ReferencesArgs::of(ReferencesOp::OpenBuilder), &mut doc, &mut plane, Some(&view));
    assert!(matches!(out, Outcome::Done(Ok(_))));
    world.insert_resource(doc);
    world.run_system_once(reads::receive).unwrap();
    let sent: Vec<Act<WindowAction>> = world.resource_mut::<Messages<Act<WindowAction>>>().drain().collect();
    assert_eq!(sent.len(), 1, "one switch request");
    let WindowAction::Switch(switch) = &sent[0].action else { panic!("not a switch") };
    assert_eq!(switch.mode, ViewerMode::Build);
    assert_eq!(switch.document, Some(Document::Path(PathBuf::from(SYSTEM))));
    assert!(world.resource::<CadDocument>().references.switch_to.is_none());
    world.run_system_once(reads::receive).unwrap();
    assert!(world.resource_mut::<Messages<Act<WindowAction>>>().drain().next().is_none(), "once");
}

/// The dock's status line: "Reading the linked system file…" until read,
/// then RoboCAD's texts (references.py:90-103).
#[test]
fn the_system_status_line_is_robocads() {
    let mut doc = document(false);
    assert_eq!(system_link::line(&doc), READING);
    status(&mut doc, LinkState::Unlinked);
    assert_eq!(system_link::line(&doc), "System file: none linked. Link a .system.json to build circuits and subsystems for this model.");
    status(&mut doc, LinkState::Missing);
    assert_eq!(system_link::line(&doc), format!("System file missing: {SYSTEM}"));
    status(&mut doc, LinkState::Current);
    assert_eq!(system_link::line(&doc), "System: Arm · revision 3 · 5 definitions");
    status(&mut doc, LinkState::Changed);
    assert_eq!(system_link::line(&doc), "System: Arm · revision 3 · 5 definitions · CHANGED since linked (was revision 2)");
    doc.references.reads.status = Some(((doc.generation, 4), Err("POST /ops/system_status: refused".into())));
    assert!(system_link::line(&doc).starts_with("The linked system file's status could not be read:"));
}

/// Dropped files are one import in CAD mode; in another mode the reader does
/// not run, and the drops buffered from it are not taken on entering CAD mode.
#[test]
fn drops_are_taken_in_cad_mode_only() {
    let window = Entity::PLACEHOLDER;
    let dropped = |p: &str| FileDragAndDrop::DroppedFile { window, path_buf: PathBuf::from(p) };
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin)).insert_state(ViewerMode::Cad).add_message::<FileDragAndDrop>().add_message::<Act<CadAction>>();
    app.add_systems(Update, super::drop::drops.run_if(in_state(ViewerMode::Cad)));
    let taken = |app: &mut App| -> Vec<CadAction> { app.world_mut().resource_mut::<Messages<Act<CadAction>>>().drain().map(|a| a.action).collect() };
    app.update();
    assert!(taken(&mut app).is_empty());
    app.world_mut().write_message(dropped("/work/a.png"));
    app.world_mut().write_message(dropped("/work/b.jpg"));
    app.world_mut().write_message(FileDragAndDrop::HoveredFile { window, path_buf: PathBuf::from("/work/c.png") });
    app.update();
    let want = ReferencesArgs { paths: Some(vec!["/work/a.png".into(), "/work/b.jpg".into()]), ..ReferencesArgs::of(ReferencesOp::Add) }.action();
    assert_eq!(taken(&mut app), vec![want]);
    // Build mode: not CAD's (the builder's reader runs there).
    app.world_mut().resource_mut::<NextState<ViewerMode>>().set(ViewerMode::Build);
    app.world_mut().write_message(dropped("/work/system.definition.json"));
    app.update();
    assert!(taken(&mut app).is_empty());
    // Back in CAD mode: the drop made in Build mode is not imported.
    app.world_mut().resource_mut::<NextState<ViewerMode>>().set(ViewerMode::Cad);
    app.update();
    app.update();
    assert!(taken(&mut app).is_empty());
}

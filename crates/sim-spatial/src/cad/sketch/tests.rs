//! The sketch tools without a window: the in-progress blocker, Escape
//! sending nothing, the builders' kernel calls for all 13 shapes (clicks
//! and Tab values), line chaining, a press refused before it is taken, the
//! polygon's remembered sides, which sketch a shape goes to
//! (tools.py:675-686), the corner-fillet simulation, the sketch edits and
//! the `cad_sketch` refusals, the target's candidate order, a plane node
//! without a valid frame and a sketch read with dropped curves. "Sends nothing" is checked where it is
//! decided: no edit started (`CadDocument::edit`), no action returned.
use super::cache::Geometry;
use super::edits::{self, fillet_corner, fillet_plan};
use super::interact::{Step, begin, finish_action, finish_check, press, readout, reset_after_finish};
use super::specs::{self, from_points, from_values, spec, target};
use super::*;
use crate::app::actions::{Call, Origin, Replies};
use crate::cad::actions::{CadAction, Cx};
use crate::cad::document::{CadTarget, Connection};
use crate::cad::ops::{Built, Env, FormState, Resolved, entry};
use crate::cad::transform::fl;
use serde_json::{Map, Value, json};
use sim_api::Outcome;
use sim_runtime::cad_client::{CadClient, DocState, Health, NodeSummary, SelectionItem, SketchCall, SketchCurve, SketchGeometry};
use std::sync::Arc;

fn node(id: &str, kind: &str, name: &str, visible: bool) -> NodeSummary {
    NodeSummary { id: id.into(), kind: kind.into(), name: name.into(), visible, effective_visible: visible, ..Default::default() }
}

/// A connected document at revision 4 (shown and RoboCAD's) with `nodes`,
/// its client pointed at `url`.
fn document(url: &str, nodes: Vec<NodeSummary>) -> CadDocument {
    let mut doc = CadDocument::new(CadTarget::Service(url.into()));
    doc.client = Some(CadClient::new(url).unwrap());
    doc.connection = Connection::Connected;
    doc.health = Some(Health { ok: true, app: "robocad".into(), revision: 4, ..Default::default() });
    doc.doc = Some(DocState { nodes, revision: 4, ..Default::default() });
    doc.doc_key = Some((None, 4));
    doc
}

fn polyline(points: &[[f64; 2]], closed: bool) -> SketchCurve {
    SketchCurve { kind: "polyline".into(), points: points.to_vec(), closed, ..Default::default() }
}

fn line(a: [f64; 2], b: [f64; 2]) -> SketchCurve {
    SketchCurve { kind: "line".into(), points: vec![a, b], ..Default::default() }
}

/// A cache at revision 4 holding these sketches.
fn cache(sketches: &[(&str, PlaneFrame, Vec<SketchCurve>)]) -> CadSketches {
    let mut c = CadSketches::default();
    for (id, plane, curves) in sketches {
        c.insert(id, 4, Geometry::Sketch(Arc::new(SketchGeometry { name: id.to_string(), plane: Some(*plane), curves: curves.clone() })));
    }
    c
}

const SQUARE: [[f64; 2]; 4] = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];

/// Run one action through CAD mode's one handler as a click would.
fn apply(doc: &mut CadDocument, sketches: Option<&CadSketches>, action: &CadAction) -> Outcome {
    let mut shared = crate::cad::selection::Fixture::new();
    let mut plane = CadActivePlane::default();
    let mut continuation = Value::Null;
    let mut replies = Replies::default();
    let mut call = Call { origin: Origin::Ui, continuation: &mut continuation, cancelled: false, replies: &mut replies };
    let mut cx = Cx { settings: &mut crate::app::settings::SettingsOwner::default(), doc, shared: shared.shared(), meshes: None, topology: None, view: None, plane: &mut plane, sketches, display: None, views: None, files: None, components: &mut crate::cad::components::ComponentsState::default(), composition: &mut crate::cad::composition::CadCompositionState::default(), experiments: &mut crate::cad::experiments::ExperimentsState::default(), review: &mut crate::cad::experiment_review::ReviewState::default(), motion: &mut crate::cad::motion::MotionState::default(), camera: Vec::new() };
    crate::cad::actions::handle(action, &mut call, &mut cx)
}

fn refused(outcome: Outcome) -> String {
    match outcome {
        Outcome::Done(Err(e)) => e,
        _ => panic!("not refused"),
    }
}

#[test]
fn a_shape_in_progress_blocks_leaving_cad_mode() {
    let mut doc = document("http://127.0.0.1:8420", Vec::new());
    doc.ops.active = Some("sketch.line");
    doc.ops.sketch = Some(SketchState::new(SketchShape::Line));
    // No point clicked: nothing would be lost.
    assert_eq!(crate::cad::sketch_blocker(&doc), None);
    doc.ops.sketch.as_mut().unwrap().points.push([1.0, 2.0, 0.0]);
    // The function `app::switch::prepare::leaving_blockers` adds for CAD mode.
    let why = crate::cad::sketch_blocker(&doc).expect("a blocker");
    assert!(why.contains("a sketch line is in progress") && why.contains("1 point(s)"), "{why}");
    // A lone chained point is the end of a line already sent: nothing would be lost.
    let s = doc.ops.sketch.as_mut().unwrap();
    s.chained = true;
    assert!(s.in_progress() && !s.unsent());
    assert_eq!(crate::cad::sketch_blocker(&doc), None);
}

#[test]
fn escape_drops_the_shape_and_sends_nothing() {
    let mut doc = document("http://127.0.0.1:8420", Vec::new());
    doc.ops.active = Some("sketch.rectangle");
    let mut s = SketchState::new(SketchShape::Rectangle);
    s.points.push([0.0, 0.0, 0.0]);
    s.began = 4;
    doc.ops.sketch = Some(s);
    let answer = match apply(&mut doc, None, &CadAction::CadCancel) {
        Outcome::Done(Ok(v)) => v,
        _ => panic!("Escape is not refused"),
    };
    assert_eq!(answer["closed"]["sketch_in_progress"], json!(true), "{answer}");
    assert!(doc.ops.sketch.is_none() && doc.ops.active.is_none(), "the shape and the tool end");
    assert!(doc.edit.is_none(), "no edit started");
}

#[test]
fn the_rows_follow_robocads_needed_counts_in_shape_order() {
    for (i, shape) in SketchShape::ALL.iter().enumerate() {
        assert_eq!(spec(*shape).shape, *shape, "row {i}");
        let e = entry(&format!("sketch.{}", shape.name())).unwrap_or_else(|| panic!("{} has no catalogue entry", shape.name()));
        assert_eq!((e.flow, e.shape, e.route, e.category), (crate::cad::ops::Flow::Sketch(*shape), crate::cad::ops::Shape::Sketch(*shape), "sketch", "Sketch"));
        assert!(e.hint.starts_with(&format!("{}: click points on the plane", shape.name())), "{}", e.hint);
        assert!(e.params.iter().any(|p| p.name == "anchor" && p.default.is_empty()), "{}: no optional anchor", e.id);
    }
    let needed = |s: SketchShape| spec(s).finish;
    assert_eq!(needed(SketchShape::Slot), Finish::Points(3));
    assert_eq!(needed(SketchShape::Text), Finish::Points(1));
    assert_eq!(needed(SketchShape::Spline), Finish::EnterOrDouble);
    assert!(spec(SketchShape::Line).chains && !spec(SketchShape::Rectangle).chains);
    assert_eq!(entry("sketch.arc_3pt").unwrap().keys, &["A"]);
    assert_eq!(entry("sketch.polygon").unwrap().params.iter().map(|p| (p.name, p.default)).collect::<Vec<_>>(), vec![("radius", "10.0"), ("sides", ""), ("anchor", "")]);
}

#[test]
fn clicks_build_robocads_calls_for_every_shape() {
    let a = [0.0, 0.0];
    let calls = |shape, pts: &[[f64; 2]]| from_points(shape, pts, "Hi", 6).unwrap();
    assert_eq!(calls(SketchShape::Line, &[a, [10.0, 5.0]]), vec![SketchCall::Line { a, b: [10.0, 5.0] }]);
    // A rectangle from its two corners in any order: the lower corner and the sizes.
    let r = calls(SketchShape::Rectangle, &[[10.0, 5.0], a]);
    assert_eq!(r, vec![SketchCall::Rectangle { corner: [0.0, 0.0], size: [10.0, 5.0] }]);
    assert_eq!(r[0].to_json(), json!(["rectangle", [[0.0, 0.0], [10.0, 5.0]]]));
    assert_eq!(calls(SketchShape::RectangleCenter, &[a, [10.0, -5.0]]), vec![SketchCall::RectangleCenter { center: a, size: [20.0, 10.0] }]);
    assert_eq!(calls(SketchShape::Circle, &[a, [3.0, 4.0]]), vec![SketchCall::Circle { center: a, radius: 5.0 }]);
    assert_eq!(calls(SketchShape::Circle2pt, &[a, [10.0, 0.0]]), vec![SketchCall::CircleTwoPoint { a, b: [10.0, 0.0] }]);
    let tri = [a, [10.0, 0.0], [0.0, 10.0]];
    assert_eq!(calls(SketchShape::Circle3pt, &tri), vec![SketchCall::CircleThreePoint { a: tri[0], b: tri[1], c: tri[2] }]);
    assert_eq!(calls(SketchShape::Arc3pt, &tri), vec![SketchCall::ArcThreePoint { a: tri[0], b: tri[1], c: tri[2] }]);
    // Two of three points build nothing yet; collinear ones are the kernel's error.
    assert!(calls(SketchShape::Circle3pt, &tri[..2]).is_empty());
    assert!(from_points(SketchShape::Arc3pt, &[a, [1.0, 1.0], [2.0, 2.0]], "", 6).unwrap_err().contains("collinear"));
    // The polygon carries the viewer's remembered count (what its preview draws); rotation toward the second click.
    assert_eq!(calls(SketchShape::Polygon, &[a, [10.0, 0.0]]), vec![SketchCall::Polygon { center: a, radius: 10.0, sides: Some(6), rotation: 0.0 }]);
    assert_eq!(from_points(SketchShape::Polygon, &[a, [10.0, 0.0]], "", 8).unwrap(), vec![SketchCall::Polygon { center: a, radius: 10.0, sides: Some(8), rotation: 0.0 }]);
    // The slot's width: twice the third click's distance from the axis; 4 before it.
    assert_eq!(calls(SketchShape::Slot, &[a, [10.0, 0.0], [5.0, 3.0]]), vec![SketchCall::Slot { a, b: [10.0, 0.0], width: 6.0 }]);
    assert_eq!(calls(SketchShape::Slot, &[a, [10.0, 0.0]]), vec![SketchCall::Slot { a, b: [10.0, 0.0], width: 4.0 }]);
    assert_eq!(calls(SketchShape::Spline, &tri), vec![SketchCall::Spline { points: tri.to_vec(), closed: false }]);
    assert!(calls(SketchShape::Spline, &[a]).is_empty());
    assert_eq!(calls(SketchShape::Ellipse, &[a, [10.0, 0.0], [0.0, 4.0]]), vec![SketchCall::Ellipse { center: a, radius_x: 10.0, radius_y: 4.0, rotation: 0.0 }]);
    assert_eq!(calls(SketchShape::Ellipse, &[a, [10.0, 0.0]]), vec![SketchCall::Ellipse { center: a, radius_x: 10.0, radius_y: 5.0, rotation: 0.0 }]);
    assert_eq!(calls(SketchShape::Spiral, &[a, [10.0, 0.0]]), vec![SketchCall::Spiral { center: a, start_radius: 0.15 * 10.0, end_radius: 10.0, turns: 3.0 }]);
    // A clicked text is 10 mm high (`text_height` is never set).
    assert_eq!(calls(SketchShape::Text, &[a]), vec![SketchCall::Text { origin: a, text: "Hi".into(), height: 10.0 }]);
}

#[test]
fn tab_values_build_robocads_commit_at_the_anchor() {
    let a = [1.0, 2.0];
    let v = |pairs: Value| -> Map<String, Value> { pairs.as_object().unwrap().clone() };
    let commit = |shape, values: Value| from_values(shape, a, &v(values), "Hi", 6).unwrap();
    assert_eq!(commit(SketchShape::Line, json!({"length": 10.0, "angle": 0.0})), vec![SketchCall::Line { a, b: [11.0, 2.0] }]);
    assert_eq!(commit(SketchShape::Rectangle, json!({"width": 20.0, "height": 10.0})), vec![SketchCall::Rectangle { corner: a, size: [20.0, 10.0] }]);
    assert_eq!(commit(SketchShape::RectangleCenter, json!({"width": 20.0, "height": 10.0})), vec![SketchCall::RectangleCenter { center: a, size: [20.0, 10.0] }]);
    assert_eq!(commit(SketchShape::Circle, json!({"diameter": 10.0})), vec![SketchCall::Circle { center: a, radius: 5.0 }]);
    assert_eq!(commit(SketchShape::Polygon, json!({"radius": 10.0, "sides": 8})), vec![SketchCall::Polygon { center: a, radius: 10.0, sides: Some(8), rotation: 0.0 }]);
    // Without sides: the remembered count.
    assert_eq!(from_values(SketchShape::Polygon, a, &v(json!({"radius": 10.0})), "", 5).unwrap(), vec![SketchCall::Polygon { center: a, radius: 10.0, sides: Some(5), rotation: 0.0 }]);
    assert_eq!(commit(SketchShape::Slot, json!({"length": 20.0, "width": 4.0})), vec![SketchCall::Slot { a, b: [21.0, 2.0], width: 4.0 }]);
    assert_eq!(commit(SketchShape::Ellipse, json!({"radius_x": 10.0, "radius_y": 5.0})), vec![SketchCall::Ellipse { center: a, radius_x: 10.0, radius_y: 5.0, rotation: 0.0 }]);
    assert_eq!(commit(SketchShape::Spiral, json!({"start_radius": 2.0, "end_radius": 10.0, "turns": 2.5})), vec![SketchCall::Spiral { center: a, start_radius: 2.0, end_radius: 10.0, turns: 2.5 }]);
    assert_eq!(commit(SketchShape::Text, json!({"height": 7.0})), vec![SketchCall::Text { origin: a, text: "Hi".into(), height: 7.0 }]);
    for shape in [SketchShape::Circle2pt, SketchShape::Circle3pt, SketchShape::Arc3pt, SketchShape::Spline] {
        assert!(from_values(shape, a, &Map::new(), "", 6).unwrap_err().contains("no exact values"), "{shape:?}");
    }
    // `cad_run sketch.rectangle`: the anchor on the plane (XY without an active plane), a new sketch there.
    let doc = document("http://127.0.0.1:8420", Vec::new());
    let e = entry("sketch.rectangle").unwrap();
    let values = v(json!({"width": 20.0, "height": 10.0, "anchor": [1.0, 2.0, 0.0]}));
    let built = specs::calls(e, SketchShape::Rectangle, &values, &doc, &Env::default()).unwrap();
    assert_eq!(built, Built::Sketch { target: SketchTarget::New { plane: json!("xy") }, calls: vec![SketchCall::Rectangle { corner: a, size: [20.0, 10.0] }], label: "Sketch rectangle".into() });
    // The text tool needs its text.
    let t = entry("sketch.text").unwrap();
    assert!(specs::calls(t, SketchShape::Text, &v(json!({"height": 10.0})), &doc, &Env::default()).unwrap_err().contains("type the text"));
}

#[test]
fn a_finished_line_is_one_cad_sketch_and_the_next_line_starts_at_its_end() {
    let mut s = SketchState::new(SketchShape::Line);
    s.points = vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]];
    s.began = 4;
    let action = finish_action(SketchShape::Line, &s, &PlaneFrame::XY, &json!("xy"), 6).unwrap().unwrap();
    let line = SketchCall::Line { a: [0.0, 0.0], b: [10.0, 0.0] };
    assert_eq!(action, CadAction::CadSketch { node: None, plane: Some("xy".into()), calls: vec![line.to_json()], revision: Some(4) });
    assert!(reset_after_finish(spec(SketchShape::Line), &mut s));
    assert_eq!(s.points, vec![[10.0, 0.0, 0.0]], "lines chain");
    let mut r = SketchState::new(SketchShape::Rectangle);
    r.points = vec![[0.0; 3], [1.0, 1.0, 0.0]];
    assert!(!reset_after_finish(spec(SketchShape::Rectangle), &mut r) && r.points.is_empty());
    // A text click without text is refused (nothing sent).
    let mut t = SketchState::new(SketchShape::Text);
    t.points = vec![[0.0; 3]];
    assert!(finish_action(SketchShape::Text, &t, &PlaneFrame::XY, &json!("xy"), 6).unwrap_err().contains("type the text"));
    // RoboCAD's readouts.
    assert_eq!(readout(Readout::Size, [0.0, 0.0], [-20.0, 10.0]), format!("{} × {}", fl(20.0), fl(10.0)));
    assert_eq!(readout(Readout::Radius, [0.0, 0.0], [3.0, 4.0]), format!("radius {}", fl(5.0)));
}

#[test]
fn the_polygon_remembers_its_sides() {
    let mut doc = document("http://127.0.0.1:8420", Vec::new());
    let e = entry("sketch.polygon").unwrap();
    let form = |doc: &mut CadDocument| doc.ops.form = Some(FormState { op: e.id, texts: e.params.iter().map(|p| p.default.to_string()).collect(), focus: None, select_all: true, began: 4, error: None });
    form(&mut doc);
    begin(&mut doc, SketchShape::Polygon).unwrap();
    assert_eq!(doc.ops.form.as_ref().unwrap().texts[1], "6", "RoboCAD's 6 at first");
    let eight = [SketchCall::Polygon { center: [0.0; 2], radius: 5.0, sides: Some(8), rotation: 0.0 }];
    assert_eq!(specs::polygon_sides_after(&eight), Some(8));
    assert_eq!(specs::polygon_sides_after(&[SketchCall::Polygon { center: [0.0; 2], radius: 5.0, sides: None, rotation: 0.0 }]), None);
    // A polygon edit that fails, or another edit finishing, leaves the count as it was.
    specs::note_polygon_sides(&mut doc, Some(8), 1);
    specs::polygon_edit_done(&mut doc, 1, false);
    assert_eq!((doc.ops.polygon_sides, doc.ops.polygon_sides_sent), (None, None));
    specs::note_polygon_sides(&mut doc, Some(8), 2);
    specs::polygon_edit_done(&mut doc, 3, true);
    assert_eq!((doc.ops.polygon_sides, doc.ops.polygon_sides_sent), (None, None));
    // It follows once the polygon's edit succeeds; the clicked polygon then carries it.
    specs::note_polygon_sides(&mut doc, specs::polygon_sides_after(&eight), 4);
    specs::polygon_edit_done(&mut doc, 4, true);
    assert_eq!(doc.ops.polygon_sides, Some(8));
    let mut p = SketchState::new(SketchShape::Polygon);
    p.points = vec![[0.0; 3], [5.0, 0.0, 0.0]];
    let Some(CadAction::CadSketch { calls, .. }) = finish_action(SketchShape::Polygon, &p, &PlaneFrame::XY, &json!("xy"), doc.ops.polygon_sides.unwrap_or(6)).unwrap() else { panic!("no action") };
    assert_eq!(SketchCall::from_json(&calls[0]).unwrap(), SketchCall::Polygon { center: [0.0; 2], radius: 5.0, sides: Some(8), rotation: 0.0 });
    form(&mut doc);
    begin(&mut doc, SketchShape::Polygon).unwrap();
    assert_eq!(doc.ops.form.as_ref().unwrap().texts[1], "8");
    // The text tool's "Text to sketch:" takes the keyboard (RoboCAD's dialog comes first).
    let t = entry("sketch.text").unwrap();
    doc.ops.form = Some(FormState { op: t.id, texts: t.params.iter().map(|p| p.default.to_string()).collect(), focus: None, select_all: false, began: 4, error: None });
    begin(&mut doc, SketchShape::Text).unwrap();
    assert_eq!(doc.ops.form.as_ref().unwrap().focus, Some(0));
}

#[test]
fn a_shape_goes_to_the_selected_then_the_first_visible_sketch_on_its_plane() {
    let xy = PlaneFrame::XY;
    let nodes = vec![node("k1", "sketch", "Profile", true), node("k2", "sketch", "Slots", true), node("k3", "sketch", "Side", true), node("b1", "body", "Bracket", true)];
    let mut doc = document("http://127.0.0.1:8420", nodes);
    let c = cache(&[("k1", xy, vec![]), ("k2", xy, vec![]), ("k3", PlaneFrame::XZ, vec![])]);
    let env = Env { sketches: Some(&c), ..Default::default() };
    let to = |doc: &CadDocument, selection: &[SelectionItem], frame| target(doc, &Env { selection, ..env }, &json!("xy"), frame);
    // Nothing selected: the first visible sketch on the plane, in tree order.
    assert_eq!(to(&doc, &[], xy), Ok(SketchTarget::Node("k1".into())));
    // A selected sketch on the plane wins; one on another plane does not count.
    let picked = [SelectionItem("b1".into(), "body".into(), 0), SelectionItem("k3".into(), "body".into(), 0), SelectionItem("k2".into(), "body".into(), 0)];
    assert_eq!(to(&doc, &picked, xy), Ok(SketchTarget::Node("k2".into())));
    assert_eq!(to(&doc, &picked, PlaneFrame::XZ), Ok(SketchTarget::Node("k3".into())));
    // A hidden sketch is not picked unless selected; no sketch on the plane: a new one there.
    doc.doc.as_mut().unwrap().nodes[0].effective_visible = false;
    assert_eq!(to(&doc, &[], xy), Ok(SketchTarget::Node("k2".into())));
    assert_eq!(to(&doc, &[], PlaneFrame::YZ), Ok(SketchTarget::New { plane: json!("xy") }));
    // A sketch not read at the shown revision: refused, not guessed.
    let partial = cache(&[("k1", xy, vec![])]);
    let env = Env { sketches: Some(&partial), ..Default::default() };
    assert!(target(&doc, &env, &json!("xy"), xy).unwrap_err().contains("still being read"));
}

/// Only the candidates `_ensure_sketch` would look at, in its order, need
/// their geometry read: selected sketches, then visible ones in tree
/// order; the first unread candidate refuses by name, and a hidden sketch
/// that is not selected is never waited for.
#[test]
fn the_target_waits_only_for_the_candidates_in_robocads_order() {
    let xy = PlaneFrame::XY;
    let nodes = vec![node("k1", "sketch", "Profile", true), node("k2", "sketch", "Hidden", false), node("k3", "sketch", "Later", true)];
    let mut doc = document("http://127.0.0.1:8420", nodes);
    // k2 (hidden, unselected) and k3 (after the match) are unread: k1 is taken without waiting.
    let only_k1 = cache(&[("k1", xy, vec![])]);
    let env = Env { sketches: Some(&only_k1), ..Default::default() };
    assert_eq!(target(&doc, &env, &json!("xy"), xy), Ok(SketchTarget::Node("k1".into())));
    // k1 on another plane: k3 is the next candidate, unread: refused by its name.
    let k1_xz = cache(&[("k1", PlaneFrame::XZ, vec![])]);
    let env = Env { sketches: Some(&k1_xz), ..Default::default() };
    let err = target(&doc, &env, &json!("xy"), xy).unwrap_err();
    assert!(err.contains("sketch Later is still being read"), "{err}");
    // A selected sketch is a candidate even hidden: unread, it refuses first.
    let k2 = [SelectionItem("k2".into(), "body".into(), 0)];
    let env = Env { selection: &k2, sketches: Some(&only_k1), ..Default::default() };
    let err = target(&doc, &env, &json!("xy"), xy).unwrap_err();
    assert!(err.contains("sketch Hidden is still being read"), "{err}");
    // Read on the plane, the selected hidden sketch wins.
    let both = cache(&[("k1", xy, vec![]), ("k2", xy, vec![])]);
    let env = Env { selection: &k2, sketches: Some(&both), ..Default::default() };
    assert_eq!(target(&doc, &env, &json!("xy"), xy), Ok(SketchTarget::Node("k2".into())));
    // No candidate at all (nothing selected, every sketch hidden): a new sketch, nothing waited for.
    for n in &mut doc.doc.as_mut().unwrap().nodes {
        n.effective_visible = false;
    }
    let none = CadSketches::default();
    let env = Env { sketches: Some(&none), ..Default::default() };
    assert_eq!(target(&doc, &env, &json!("xy"), xy), Ok(SketchTarget::New { plane: json!("xy") }));
}

/// A plane node read without a valid `plane` is said so by name, not
/// "still being read": the cache, the active plane's frame and label, a
/// named plane and a sketch press.
#[test]
fn a_plane_node_without_a_valid_frame_is_named_not_reading() {
    let doc = document("http://127.0.0.1:8420", vec![node("p1", "plane", "Datum", true)]);
    let mut c = CadSketches::default();
    c.insert("p1", 4, Geometry::Plane(None));
    let err = c.plane_state("p1").unwrap().unwrap_err();
    assert!(err.contains("node p1 has no valid plane frame"), "{err}");
    assert_eq!(c.plane_state("p2"), None, "not read: no state");
    // The active plane: marked as the cache sync marks it.
    let mut active = CadActivePlane { plane: Some(ActivePlane::Node { id: "p1".into(), frame: None }), ..Default::default() };
    assert!(active.frame().unwrap_err().contains("being read"), "unmarked: still reading");
    active.unusable = cache::unusable(&active, &c);
    let err = active.frame_or_xy().unwrap_err();
    assert!(err.contains("node p1 has no valid plane frame"), "{err}");
    assert_eq!(active.label(&doc), "Datum (no valid plane frame)");
    // A mark left from another node is not this one's.
    let other = CadActivePlane { plane: Some(ActivePlane::Node { id: "p2".into(), frame: None }), unusable: active.unusable.clone(), ..Default::default() };
    assert!(other.frame().unwrap_err().contains("being read"));
    // `cad_sketch` on the named plane, and a sketch press on the active one.
    let env = Env { sketches: Some(&c), ..Default::default() };
    let err = edits::shape_target(Some("p1"), &doc, &env).unwrap_err();
    assert!(err.contains("plane Datum") && err.contains("no valid plane frame"), "{err}");
    let env = Env { sketches: Some(&c), plane: Some(&active), ..Default::default() };
    let err = edits::shape_target(None, &doc, &env).unwrap_err();
    assert!(err.contains("no valid plane frame"), "{err}");
    let mut s = SketchState::new(SketchShape::Text);
    s.text = "Hi".into();
    let Step::Wait(why) = press(spec(SketchShape::Text), &mut s, [0.0; 3], &doc, &env, &json!("p1")) else { panic!("the press was taken") };
    assert!(why.contains("no valid plane frame") && !why.contains("being read"), "{why}");
    // A valid frame read later clears it.
    c.insert("p1", 4, Geometry::Plane(Some(PlaneFrame::XZ)));
    assert_eq!(c.plane_state("p1"), Some(Ok(PlaneFrame::XZ)));
    assert_eq!(cache::unusable(&active, &c), None);
}

/// Curves the read dropped (no kind) make the viewer's indices wrong: the
/// edits and a `cad_sketch` call naming curves refuse by name, nothing sent.
#[test]
fn a_sketch_with_dropped_curves_refuses_index_edits() {
    let mut doc = document("http://127.0.0.1:8420", vec![node("k1", "sketch", "Profile", true)]);
    let mut c = CadSketches::default();
    let g = SketchGeometry { name: "Profile".into(), plane: Some(PlaneFrame::XY), curves: vec![polyline(&SQUARE, true), line([0.0; 2], [5.0, 5.0])] };
    c.insert_read("k1", 4, Geometry::Sketch(Arc::new(g)), vec![2]);
    assert_eq!((c.dropped("k1"), c.dropped("k2")), (&[2][..], &[][..]));
    let env = Env { sketches: Some(&c), ..Default::default() };
    let r = Resolved::default();
    let run = |id: &str, edit, values: Value| edits::calls(entry(id).unwrap(), edit, &r, values.as_object().unwrap(), &doc, &env);
    for (id, edit, values, what) in [
        ("sketch.offset", SketchEdit::Offset, json!({"distance": 1.0}), "Offset curves"),
        ("sketch.fillet", SketchEdit::FilletCorners, json!({"radius": 2.0}), "Fillet corners"),
        ("sketch.join", SketchEdit::Join, json!({}), "Join curves"),
    ] {
        let err = run(id, edit, values).unwrap_err();
        assert!(err.starts_with(&format!("{what} refused: 1 curve(s) of Profile could not be read (no kind: RoboCAD's curve 2)")), "{err}");
    }
    let sketch = |calls: Vec<Value>| CadAction::CadSketch { node: Some("k1".into()), plane: None, calls, revision: None };
    let err = refused(apply(&mut doc, Some(&c), &sketch(vec![json!(["line", [[0, 0], [1, 1]]]), json!(["offset", [0, 1.0]])])));
    assert!(err.contains("cad_sketch call 2 (offset) refused") && err.contains("Profile"), "{err}");
    assert!(doc.edit.is_none(), "no edit started");
    // Nothing dropped: no refusal.
    assert_eq!(edits::dropped_refusal(&Env { sketches: Some(&CadSketches::default()), ..Default::default() }, "k1", "Profile", "x"), Ok(()));
}

#[test]
fn fillet_corners_sends_only_the_corners_robocad_rounds() {
    // A square: all four corners, from the last vertex down.
    let calls = fillet_plan(&[polyline(&SQUARE, true)], 2.0);
    let corner = |i: i64, radius: f64| SketchCall::FilletCorner { curve: 0, vertex_index: i, radius };
    assert_eq!(calls, vec![corner(3, 2.0), corner(2, 2.0), corner(1, 2.0), corner(0, 2.0)]);
    // A straight corner is skipped (the kernel's "corner is straight").
    let straight = [[0.0, 0.0], [5.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    let calls = fillet_plan(&[polyline(&straight, true)], 2.0);
    assert_eq!(calls.iter().map(|c| if let SketchCall::FilletCorner { vertex_index, .. } = c { *vertex_index } else { -1 }).collect::<Vec<_>>(), vec![4, 3, 2, 0]);
    // Radius 6 on the square: vertex 3 rounds; vertex 2 then has only 4 mm to
    // its new neighbour (too large for the corner), vertex 1 rounds, vertex 0 is short again.
    assert_eq!(fillet_plan(&[polyline(&SQUARE, true)], 6.0), vec![corner(3, 6.0), corner(1, 6.0)]);
    // Open polylines and other curves are left alone; a fillet adds 8 points.
    assert!(fillet_plan(&[polyline(&SQUARE, false), line([0.0; 2], [1.0, 0.0])], 1.0).is_empty());
    assert_eq!(fillet_corner(&SQUARE, true, 3, 2.0).unwrap().len(), 12);
    assert!(fillet_corner(&straight, true, 1, 2.0).is_none());
    // The kernel's "cannot fillet an end vertex" of an open polyline; its inner corners round.
    assert!(fillet_corner(&SQUARE, false, 0, 2.0).is_none() && fillet_corner(&SQUARE, false, 3, 2.0).is_none());
    assert_eq!(fillet_corner(&SQUARE, false, 1, 2.0).unwrap().len(), 12);
}

#[test]
fn sketch_edits_work_on_the_selected_or_first_visible_sketch() {
    let nodes = vec![node("b1", "body", "Bracket", true), node("k1", "sketch", "Profile", true), node("k2", "sketch", "Other", true)];
    let doc = document("http://127.0.0.1:8420", nodes);
    let c = cache(&[("k1", PlaneFrame::XY, vec![polyline(&SQUARE, true), line([0.0; 2], [5.0, 5.0])]), ("k2", PlaneFrame::XY, vec![line([0.0; 2], [1.0, 0.0])])]);
    let env = Env { sketches: Some(&c), ..Default::default() };
    let r = Resolved::default();
    let k2 = [SelectionItem("k2".into(), "body".into(), 0)];
    let picked = Env { selection: &k2, ..env };
    let run_with = |env: &Env, doc: &CadDocument, id: &str, edit, values: Value| edits::calls(entry(id).unwrap(), edit, &r, values.as_object().unwrap(), doc, env);
    let run = |doc: &CadDocument, id: &str, edit, values: Value| run_with(&env, doc, id, edit, values);
    let built = run(&doc, "sketch.offset", SketchEdit::Offset, json!({"distance": 1.5})).unwrap();
    assert_eq!(built, Built::Sketch { target: SketchTarget::Node("k1".into()), calls: vec![SketchCall::Offset { curve: 0, distance: 1.5 }, SketchCall::Offset { curve: 1, distance: 1.5 }], label: "Offset curves".into() });
    let Built::Sketch { calls, label, .. } = run(&doc, "sketch.fillet", SketchEdit::FilletCorners, json!({"radius": 2.0})).unwrap() else { panic!() };
    assert_eq!((calls.len(), label.as_str()), (4, "Fillet corners"));
    let Built::Sketch { calls, .. } = run(&doc, "sketch.join", SketchEdit::Join, json!({})).unwrap() else { panic!() };
    assert_eq!(calls, vec![SketchCall::Join { curves: vec![0, 1] }]);
    // The selected sketch wins; one curve has nothing to join.
    assert!(run_with(&picked, &doc, "sketch.join", SketchEdit::Join, json!({})).unwrap_err().contains("Nothing to join: Other has 1 curve(s)"));
    // No sketch at all: RoboCAD's "Select a sketch".
    let bare = document("http://127.0.0.1:8420", vec![node("b1", "body", "Bracket", true)]);
    assert_eq!(run(&bare, "sketch.offset", SketchEdit::Offset, json!({"distance": 1.0})).unwrap_err(), "Select a sketch");
}

#[test]
fn cad_sketch_refusals_name_the_call_and_the_argument_and_send_nothing() {
    let mut doc = document("http://127.0.0.1:8420", vec![node("k1", "sketch", "Profile", true), node("b1", "body", "Bracket", true)]);
    let c = cache(&[("k1", PlaneFrame::XY, vec![line([0.0; 2], [1.0, 0.0]), line([1.0, 0.0], [1.0, 1.0])])]);
    let sketch = |node: Option<&str>, calls: Vec<Value>, revision| CadAction::CadSketch { node: node.map(str::to_string), plane: None, calls, revision };
    let e = refused(apply(&mut doc, Some(&c), &sketch(Some("k1"), vec![json!(["no_such_method", []])], None)));
    assert!(e.contains("no_such_method"), "{e}");
    let e = refused(apply(&mut doc, Some(&c), &sketch(Some("k1"), vec![json!(["circle", [[0.0, 0.0], "big"]])], None)));
    assert!(e.contains("circle") && e.contains("radius"), "{e}");
    // A curve index past the sketch's two curves.
    let e = refused(apply(&mut doc, Some(&c), &sketch(Some("k1"), vec![json!(["offset", [5, 1.0]])], None)));
    assert!(e.contains("offset") && e.contains('5'), "{e}");
    assert!(refused(apply(&mut doc, Some(&c), &sketch(Some("b1"), vec![json!(["line", [[0, 0], [1, 1]]])], None))).contains("not a sketch"));
    assert!(refused(apply(&mut doc, Some(&c), &sketch(Some("k1"), vec![json!(["line", [[0, 0], [1, 1]]])], Some(3)))).contains("changed since"));
    let unknown = CadAction::CadSketch { node: None, plane: Some("diagonal".into()), calls: vec![json!(["line", [[0, 0], [1, 1]]])], revision: None };
    assert!(refused(apply(&mut doc, Some(&c), &unknown)).contains("unknown plane"));
    assert!(doc.edit.is_none(), "no edit started");
}

#[test]
fn a_press_that_cannot_be_sent_is_not_taken_and_keeps_the_points() {
    let mut doc = document("http://127.0.0.1:8420", Vec::new());
    let env = Env::default();
    let xy = json!("xy");
    let line = spec(SketchShape::Line);
    // A line chained at (10, 0) after the last one was sent: its next press stamps `began`.
    let mut s = SketchState::new(SketchShape::Line);
    s.points = vec![[10.0, 0.0, 0.0]];
    s.began = 3;
    s.chained = true;
    // The shown document is being refetched: the completing press is refused by name.
    doc.stale = Some("refetching revision 5".into());
    let kept = s.clone();
    let Step::Wait(why) = press(line, &mut s, [20.0, 0.0, 0.0], &doc, &env, &xy) else { panic!("the press was taken") };
    assert!(why.starts_with("Sketch line not sent: the shown document is behind RoboCAD's") && why.ends_with("; click again when RoboCAD has caught up"), "{why}");
    assert_eq!(s, kept, "the point is not added; the chained point and its chain stay");
    assert!(doc.edit.is_none(), "no edit started");
    // Caught up: the same press is taken, at the shown revision, and finishes the line.
    doc.stale = None;
    assert_eq!(press(line, &mut s, [20.0, 0.0, 0.0], &doc, &env, &xy), Step::Finish);
    assert_eq!((s.points.len(), s.began, s.chained), (2, 4, false));
    let action = finish_action(SketchShape::Line, &s, &PlaneFrame::XY, &xy, 6).unwrap().unwrap();
    assert!(matches!(action, CadAction::CadSketch { revision: Some(4), .. }), "{action:?}");

    // A rectangle: an edit in flight or a stale document at the completing press keeps its first point.
    let rect = spec(SketchShape::Rectangle);
    let mut r = SketchState::new(SketchShape::Rectangle);
    assert_eq!(press(rect, &mut r, [0.0; 3], &doc, &env, &xy), Step::Added);
    assert_eq!((r.points.len(), r.began), (1, 4));
    doc.stale = Some("refetching revision 5".into());
    assert!(matches!(press(rect, &mut r, [5.0, 5.0, 0.0], &doc, &env, &xy), Step::Wait(_)));
    assert_eq!(r.points, vec![[0.0; 3]]);
    // RoboCAD's revision moved since the first click: that point is gone for good, said by name.
    doc.stale = None;
    doc.health.as_mut().unwrap().revision = 5;
    let Step::Dropped(why) = press(rect, &mut r, [5.0, 5.0, 0.0], &doc, &env, &xy) else { panic!("not dropped") };
    assert!(why.contains("changed since") && why.contains("click the rectangle again"), "{why}");
    assert!(r.points.is_empty() && !r.unsent());
    doc.health.as_mut().unwrap().revision = 4;

    // The target sketch not read at the shown revision: the text click waits.
    let doc = document("http://127.0.0.1:8420", vec![node("k1", "sketch", "Profile", true)]);
    let empty = CadSketches::default();
    let reading = Env { sketches: Some(&empty), ..Default::default() };
    let mut t = SketchState::new(SketchShape::Text);
    let Step::Wait(why) = press(spec(SketchShape::Text), &mut t, [0.0; 3], &doc, &reading, &xy) else { panic!("the press was taken") };
    assert!(why.contains("still being read"), "{why}");
    assert!(t.points.is_empty());
    // The spline's Enter is checked the same way; its points stay.
    let mut sp = SketchState::new(SketchShape::Spline);
    sp.points = vec![[0.0; 3], [5.0, 0.0, 0.0]];
    sp.began = 4;
    assert!(matches!(finish_check(spec(SketchShape::Spline), &mut sp, &doc, &reading, &xy), Step::Wait(_)));
    assert_eq!(sp.points.len(), 2);
    let read = cache(&[("k1", PlaneFrame::XY, vec![])]);
    let ready = Env { sketches: Some(&read), ..Default::default() };
    assert_eq!(finish_check(spec(SketchShape::Spline), &mut sp, &doc, &ready, &xy), Step::Finish);
}

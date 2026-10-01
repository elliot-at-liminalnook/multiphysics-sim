//! The tools without a window: each commit's exact Ops call (pure
//! builders), refusals that send nothing (an edit in flight, a changed
//! revision, a stale document), numeric evaluation per tool, the drag
//! maths and the preview's life. Nothing here reaches the network: a
//! commit that would be sent is checked through `commit::op_for` instead.
use super::commit::{self, dimension_call, evaluate_fields, offset_call, op_for, push_pull_call, transform_call};
use super::gizmo::{self, CENTRE, Drag};
use super::preview::settle;
use super::push_pull::{PushDrag, Target, match_face, release_action, resolve};
use super::*;
use crate::app::actions::{Call, Origin, Replies};
use crate::cad::actions::Dimension;
use crate::cad::document::{CadTarget, Connection, Edit, EditDone};
use sim_runtime::cad_client::{CadClient, DocState, Health, NodeSummary, SelectionItem};

fn node(id: &str, name: &str) -> NodeSummary {
    NodeSummary { id: id.into(), kind: "body".into(), name: name.into(), visible: true, effective_visible: true, ..Default::default() }
}

/// A connected document at revision 4 with two bodies, Bracket selected.
fn document() -> CadDocument {
    let mut doc = CadDocument::new(CadTarget::Service("http://127.0.0.1:8420".into()));
    doc.client = Some(CadClient::new("http://127.0.0.1:8420").unwrap());
    doc.connection = Connection::Connected;
    doc.health = Some(Health { ok: true, app: "robocad".into(), nodes: 2, revision: 4, ..Default::default() });
    doc.doc = Some(DocState { nodes: vec![node("b1", "Bracket"), node("b2", "Plate")], revision: 4, ..Default::default() });
    doc.doc_key = Some((None, 4));
    doc.selection = vec![SelectionItem("b1".into(), "body".into(), 0)];
    doc
}

fn in_flight() -> Edit {
    Edit { label: "Patch Bracket: visible".into(), job: crate::jobs::Job::finished(0, Ok(EditDone { message: String::new(), result: Value::Null })), started: Instant::now() }
}

fn move_by(x: f64, revision: Option<u64>) -> CadAction {
    CadAction::CadTransform { ids: None, translation: Some([x, 0.0, 0.0]), axis: None, angle_deg: None, center: None, scale: None, revision }
}

/// Run a commit as a click would (`Origin::Ui`) and return its refusal.
fn refusal(doc: &mut CadDocument, action: &CadAction) -> String {
    let mut continuation = Value::Null;
    let mut replies = Replies::default();
    let mut call = Call { origin: Origin::Ui, continuation: &mut continuation, cancelled: false, replies: &mut replies };
    match commit::commit(doc, None, &mut call, action) {
        Outcome::Done(Err(e)) => e,
        _ => panic!("{action:?} was not refused"),
    }
}

#[test]
fn transform_sends_only_the_given_keywords_with_robocads_shapes() {
    let ids = vec!["b1".to_string(), "b2".to_string()];
    let names = vec!["Bracket".to_string(), "Plate".to_string()];
    let t = transform_call(&ids, &names, Some([10.0, 0.0, 0.0]), None, None, None, None).unwrap();
    assert_eq!((t.name, t.args.clone()), ("transform", vec![json!(["b1", "b2"])]));
    assert_eq!(Value::Object(t.kwargs), json!({"translation": [10.0, 0.0, 0.0]}));
    assert_eq!(t.label, "Move 2 nodes by (10, 0, 0) mm");
    let r = transform_call(&ids[..1], &names[..1], None, Some([0.0, 0.0, 1.0]), Some(90.0), Some([5.0, 5.0, 0.0]), None).unwrap();
    assert_eq!(Value::Object(r.kwargs), json!({"axis": [0.0, 0.0, 1.0], "angle_deg": 90.0, "center": [5.0, 5.0, 0.0]}));
    assert_eq!(r.label, "Rotate Bracket by 90° about axis (0, 0, 1) through (5, 5, 0)");
    let s = transform_call(&ids[..1], &names[..1], None, None, None, Some([5.0, 5.0, 0.0]), Some(1.5)).unwrap();
    assert_eq!(Value::Object(s.kwargs), json!({"scale": 1.5, "center": [5.0, 5.0, 0.0]}));
    assert_eq!(s.label, "Scale Bracket by ×1.5 about (5, 5, 0)");
    // Refusals name what is wrong.
    assert!(transform_call(&[], &[], Some([1.0, 0.0, 0.0]), None, None, None, None).unwrap_err().contains("nothing is selected"));
    assert!(transform_call(&ids, &names, None, None, None, None, None).unwrap_err().contains("needs translation"));
    assert!(transform_call(&ids, &names, None, Some([0.0, 0.0, 1.0]), None, None, None).unwrap_err().contains("both axis"));
    assert!(transform_call(&ids, &names, None, Some([0.0; 3]), Some(5.0), None, None).unwrap_err().contains("zero vector"));
    assert!(transform_call(&ids, &names, None, None, None, None, Some(0.0)).unwrap_err().contains("positive"));
    assert!(transform_call(&ids, &names, Some([f64::NAN, 0.0, 0.0]), None, None, None, None).unwrap_err().contains("translation"));
}

#[test]
fn face_edits_send_robocads_face_references() {
    let p = push_pull_call("b1", "Bracket", 3, 5.0).unwrap();
    assert_eq!((p.name, p.args, p.label.as_str()), ("push_pull", vec![json!("b1"), json!({"node": "b1", "face": 3}), json!(5.0)], "Push/pull Bracket face 3 by 5 mm"));
    assert!(p.kwargs.is_empty());
    let o = offset_call("b1", "Bracket", &[3], 1.5).unwrap();
    assert_eq!((o.name, o.args, o.label.as_str()), ("offset_faces", vec![json!("b1"), json!([{"node": "b1", "face": 3}]), json!(1.5)], "Offset Bracket face 3 by 1.5 mm"));
    assert!(offset_call("b1", "Bracket", &[], 1.0).is_err());
    assert!(offset_call("b1", "Bracket", &[2, 2], 1.0).unwrap_err().contains("twice"));
    let d = dimension_call("b1", "Bracket", Dimension::Diameter, &[4], 8.0).unwrap();
    assert_eq!((d.name, d.args, d.label.as_str()), ("set_diameter", vec![json!("b1"), json!({"node": "b1", "face": 4}), json!(8.0)], "Set Ø of Bracket face 4 to 8 mm"));
    let d = dimension_call("b1", "Bracket", Dimension::Distance, &[0, 5], 20.0).unwrap();
    assert_eq!((d.name, d.args), ("set_distance", vec![json!("b1"), json!({"node": "b1", "face": 0}), json!({"node": "b1", "face": 5}), json!(20.0)]));
    let a = dimension_call("b1", "Bracket", Dimension::Angle, &[1, 2], 45.0).unwrap();
    assert_eq!((a.name, a.label.as_str()), ("set_angle", "Set angle of Bracket faces 1–2 to 45°"));
    assert!(dimension_call("b1", "Bracket", Dimension::Diameter, &[4, 5], 8.0).unwrap_err().contains("one cylindrical face"));
    assert!(dimension_call("b1", "Bracket", Dimension::Distance, &[3, 3], 8.0).unwrap_err().contains("two different faces"));
    assert!(dimension_call("b1", "Bracket", Dimension::Diameter, &[4], -1.0).unwrap_err().contains("positive"));
}

#[test]
fn a_commit_defaults_to_the_selection_and_names_it() {
    let doc = document();
    let op = op_for(&doc, None, &move_by(10.0, Some(4))).unwrap();
    assert_eq!((op.args, op.label.as_str()), (vec![json!(["b1"])], "Move Bracket by (10, 0, 0) mm"));
    let mut empty = document();
    empty.selection.clear();
    assert!(op_for(&empty, None, &move_by(10.0, Some(4))).unwrap_err().contains("nothing is selected"));
    let unknown = CadAction::CadPushPull { node: "b9".into(), face: 0, distance: 1.0, revision: None };
    assert!(op_for(&doc, None, &unknown).unwrap_err().contains("no node b9"));
}

#[test]
fn commits_are_refused_with_nothing_sent_while_an_edit_runs_or_after_the_document_changed() {
    let mut doc = document();
    doc.edit = Some(in_flight());
    let e = refusal(&mut doc, &move_by(10.0, Some(4)));
    assert!(e.contains("in flight") && e.contains("Patch Bracket"), "{e}");
    assert_eq!((doc.edit_seq, doc.edit_label()), (0, Some("Patch Bracket: visible")));
    // The revision moved since the drag began.
    doc.edit = None;
    let e = refusal(&mut doc, &move_by(10.0, Some(3)));
    assert!(e.contains("revision 3, now 4"), "{e}");
    assert!(doc.edit.is_none() && doc.edit_seq == 0);
    // The shown document is behind RoboCAD's.
    doc.stale = Some("refetching revision 5".into());
    assert!(refusal(&mut doc, &move_by(10.0, None)).contains("behind"));
    // A released drag's preview goes with a refused commit.
    doc.stale = None;
    doc.edit = Some(in_flight());
    let action = move_by(10.0, Some(4));
    doc.tool_state.preview = Some(Preview { generation: doc.generation, bodies: vec!["b1".into()], delta: Delta::Translate(Vec3::X * 10.0), began: 4, phase: Phase::Released { action: action.clone(), at: Instant::now() } });
    refusal(&mut doc, &action);
    assert!(doc.tool_state.preview.is_none());
}

#[test]
fn numeric_fields_evaluate_per_tool_and_errors_name_the_field_and_token() {
    let mut doc = document();
    doc.tool = CadTool::Move;
    let list = fields(&doc, None, None);
    assert_eq!(list.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["dx", "dy", "dz"]);
    assert_eq!(list.iter().map(Field::text).collect::<Vec<_>>(), ["0 mm", "0 mm", "0 mm"]);
    let v = evaluate_fields(doc.tool, &list, &["20mm + 0.3".into(), "1in".into(), "0".into()]).unwrap();
    assert!((v[0] - 20.3).abs() < 1e-12 && v[1] == 25.4 && v[2] == 0.0, "{v:?}");
    let e = evaluate_fields(doc.tool, &list, &["20 qq".into(), "0".into(), "0".into()]).unwrap_err();
    assert!(e.starts_with("dx: ") && e.contains("qq"), "{e}");
    assert!(evaluate_fields(doc.tool, &list, &["1".into()]).unwrap_err().contains("takes 3 value(s) (dx, dy, dz); got 1"));
    doc.tool = CadTool::Rotate;
    let list = fields(&doc, None, None);
    assert_eq!(list[0].text(), "0°");
    assert_eq!(evaluate_fields(doc.tool, &list, &["45deg".into()]).unwrap(), vec![45.0]);
    doc.tool = CadTool::Scale;
    let list = fields(&doc, None, None);
    assert_eq!(list[0].text(), "1");
    assert_eq!(evaluate_fields(doc.tool, &list, &["1.5".into()]).unwrap(), vec![1.5]);
    doc.tool = CadTool::PushPull;
    assert_eq!(fields(&doc, None, None)[0].name, "distance");
    doc.tool = CadTool::Measure;
    assert!(fields(&doc, None, None).is_empty());
    // The axis a typed rotation uses: the last dragged one, else Z.
    let mut state = ToolState::default();
    assert_eq!(numeric_axis(&state), Vec3::Z);
    state.axis = Some(0);
    assert_eq!(numeric_axis(&state), Vec3::X);
    state.axis = Some(CENTRE);
    state.free_axis = Some(Vec3::Y);
    assert_eq!(numeric_axis(&state), Vec3::Y);
}

#[test]
fn drag_maths_follow_robocad() {
    // Move along X, Ctrl snapping whole 10 mm steps (half to even); free moves keep everything.
    let p = Vec3::new(12.3, 4.0, 5.0);
    assert_eq!(gizmo::move_delta(Vec3::ZERO, p, Some(Vec3::X), false), Vec3::new(12.3, 0.0, 0.0));
    assert_eq!(gizmo::move_delta(Vec3::ZERO, p, Some(Vec3::X), true), Vec3::new(10.0, 0.0, 0.0));
    assert_eq!(gizmo::move_delta(Vec3::ZERO, Vec3::new(15.0, 0.0, 0.0), Some(Vec3::X), true), Vec3::new(20.0, 0.0, 0.0));
    assert_eq!(gizmo::move_delta(Vec3::ZERO, Vec3::new(25.0, 0.0, 0.0), Some(Vec3::X), true), Vec3::new(20.0, 0.0, 0.0));
    assert_eq!(gizmo::move_delta(Vec3::ZERO, p, None, true), p);
    // Rotation: right-handed about the axis; Ctrl in 15° steps.
    let a = gizmo::rotate_angle(Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z, false).unwrap();
    assert!((a - 90.0).abs() < 1e-4, "{a}");
    let a = gizmo::rotate_angle(Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::NEG_Z, false).unwrap();
    assert!((a + 90.0).abs() < 1e-4, "{a}");
    let fifty = Vec3::new(50f32.to_radians().cos(), 50f32.to_radians().sin(), 3.0);
    assert!((gizmo::rotate_angle(Vec3::ZERO, Vec3::X, fifty, Vec3::Z, true).unwrap() - 45.0).abs() < 1e-4);
    assert!(gizmo::rotate_angle(Vec3::ZERO, Vec3::Z * 4.0, Vec3::Y, Vec3::Z, false).is_none());
    // Uniform scale: the distance ratio from the pivot; Ctrl rounds to 0.1; at least 0.01.
    assert!((gizmo::scale_factor(Vec3::ZERO, Vec3::X * 10.0, Vec3::X * 15.0, false) - 1.5).abs() < 1e-6);
    assert!((gizmo::scale_factor(Vec3::ZERO, Vec3::X * 10.0, Vec3::Y * 14.6, true) - 1.5).abs() < 1e-6);
    assert_eq!(gizmo::scale_factor(Vec3::ZERO, Vec3::X * 10.0, Vec3::ZERO, false), 0.01);
    // An axis drag's plane contains the axis and faces the camera.
    let n = gizmo::drag_normal(CadTool::Move, 0, Vec3::X, Vec3::new(0.0, -1.0, 1.0).normalize());
    assert!(n.dot(Vec3::X).abs() < 1e-6 && n.length() > 0.5);
    assert_eq!(gizmo::drag_normal(CadTool::Rotate, 2, Vec3::Z, Vec3::Y), Vec3::Z);
    assert_eq!(gizmo::drag_normal(CadTool::Move, CENTRE, Vec3::Y, Vec3::Y), Vec3::Y);
}

#[test]
fn previews_are_rigid_about_the_pivot_and_a_release_commits_one_transform() {
    let c = Vec3::new(5.0, 5.0, 0.0);
    let t = Delta::Rotate { axis: Vec3::Z, angle_deg: 90.0, center: c }.transform();
    assert!((t.transform_point(c) - c).length() < 1e-4);
    assert!((t.transform_point(c + Vec3::X) - (c + Vec3::Y)).length() < 1e-4);
    let s = Delta::Scale { factor: 2.0, center: c }.transform();
    assert!((s.transform_point(c + Vec3::X) - (c + Vec3::X * 2.0)).length() < 1e-4);
    assert!(!Delta::Translate(Vec3::ZERO).significant() && Delta::Translate(Vec3::X).significant());
    let drag = Drag { tool: CadTool::Rotate, handle: 2, origin: c, axis: Vec3::Z, start: Vec3::ZERO, began: 4, ids: vec!["b1".into()], bodies: vec!["b1".into()], delta: None };
    let action = gizmo::drag_action(&drag, &Delta::Rotate { axis: Vec3::Z, angle_deg: 30.0, center: c });
    assert_eq!(action, CadAction::CadTransform { ids: Some(vec!["b1".into()]), translation: None, axis: Some([0.0, 0.0, 1.0]), angle_deg: Some(30.0), center: Some([5.0, 5.0, 0.0]), scale: None, revision: Some(4) });
    let readout = gizmo::drag_delta(&Drag { tool: CadTool::Move, handle: 0, axis: Vec3::X, ..drag }, Vec3::new(12.5, 3.0, 0.0), false).unwrap().1;
    assert_eq!(readout, "Δ = (12.5 mm, 0 mm, 0 mm)  |12.500 mm|");
}

#[test]
fn the_gizmo_is_hit_as_robocads_handles() {
    let view = crate::cad::snap::tests::top_view(200.0);
    let size = gizmo::handle_size(&view, Vec3::ZERO).unwrap();
    assert!((size / 1.657 - 90.0).abs() < 1.0, "{size}");
    let centre = view.project(Vec3::ZERO).unwrap();
    assert_eq!(gizmo::hit_test(&view, CadTool::Move, Vec3::ZERO, centre), Some(CENTRE));
    let on_x = view.project(Vec3::new(60.0, 0.0, 0.0)).unwrap();
    assert_eq!(gizmo::hit_test(&view, CadTool::Move, Vec3::ZERO, on_x), Some(0));
    let on_y = view.project(Vec3::new(0.0, 60.0, 0.0)).unwrap();
    assert_eq!(gizmo::hit_test(&view, CadTool::Scale, Vec3::ZERO, on_y), Some(1));
    // Looking down Z, the Z ring is the circle on screen.
    let on_ring = view.project(gizmo::ring_point(Vec3::ZERO, Vec3::Z, size, 6)).unwrap();
    assert_eq!(gizmo::hit_test(&view, CadTool::Rotate, Vec3::ZERO, on_ring), Some(2));
    let off = view.project(Vec3::new(40.0, 40.0, 0.0)).unwrap();
    assert_eq!(gizmo::hit_test(&view, CadTool::Move, Vec3::ZERO, off), None);
}

#[test]
fn push_pull_releases_choose_offset_as_robocad() {
    let drag = PushDrag { node: "b1".into(), face: 3, kind: "plane".into(), start: Vec3::ZERO, normal: Vec3::Z, centroid: Vec3::ZERO, began: 4, distance: 5.0, outline: Vec::new() };
    assert_eq!(release_action(&drag, false, false), Some(CadAction::CadPushPull { node: "b1".into(), face: 3, distance: 5.0, revision: Some(4) }));
    let offset = Some(CadAction::CadOffsetFaces { node: "b1".into(), faces: vec![3], distance: 5.0, revision: Some(4) });
    assert_eq!(release_action(&drag, false, true), offset);
    assert_eq!(release_action(&drag, true, false), offset);
    assert_eq!(release_action(&PushDrag { kind: "cylinder".into(), ..drag.clone() }, false, false), offset);
    assert_eq!(release_action(&PushDrag { distance: 1e-7, ..drag }, false, false), None);
}

#[test]
fn a_committed_preview_stays_until_its_meshes_land_and_goes_when_the_edit_fails() {
    let mut doc = document();
    let preview = |phase: Phase, generation: u64| Preview { generation, bodies: vec!["b1".into()], delta: Delta::Translate(Vec3::X), began: 4, phase };
    // Live without a drag (cancelled): gone.
    doc.tool_state.preview = Some(preview(Phase::Live, doc.generation));
    settle(&mut doc, |_| None);
    assert!(doc.tool_state.preview.is_none());
    // The edit runs: kept, whatever is drawn.
    doc.edit = Some(in_flight());
    doc.edit_seq = 1;
    doc.tool_state.preview = Some(preview(Phase::Committed { seq: 1, ended: None }, doc.generation));
    settle(&mut doc, |_| Some(4));
    assert!(doc.tool_state.preview.is_some());
    // It ended well: kept until b1 is drawn from a newer revision, then gone.
    doc.edit = None;
    doc.status = Some(Ok("Move Bracket by (1, 0, 0) mm".into()));
    settle(&mut doc, |_| Some(4));
    assert!(matches!(doc.tool_state.preview.as_ref().map(|p| &p.phase), Some(Phase::Committed { ended: Some(_), .. })));
    settle(&mut doc, |_| Some(4));
    assert!(doc.tool_state.preview.is_some());
    settle(&mut doc, |_| Some(5));
    assert!(doc.tool_state.preview.is_none());
    // It failed: gone at once.
    doc.status = Some(Err("RoboCAD POST /ops/transform: kernel error".into()));
    doc.tool_state.preview = Some(preview(Phase::Committed { seq: 1, ended: None }, doc.generation));
    settle(&mut doc, |_| Some(4));
    assert!(doc.tool_state.preview.is_none());
    // Another document, or a release whose commit never came: gone.
    doc.tool_state.preview = Some(preview(Phase::Committed { seq: 1, ended: None }, doc.generation + 1));
    settle(&mut doc, |_| Some(4));
    assert!(doc.tool_state.preview.is_none());
    let long_ago = Instant::now().checked_sub(Duration::from_secs(5)).unwrap();
    doc.tool_state.preview = Some(preview(Phase::Released { action: move_by(1.0, Some(4)), at: long_ago }, doc.generation));
    settle(&mut doc, |_| Some(4));
    assert!(doc.tool_state.preview.is_none());
}

#[test]
fn tool_state_reports_the_tool_and_its_labels() {
    let mut doc = document();
    doc.tool = CadTool::PushPull;
    doc.select_mode = crate::cad::document::SelectMode::Face;
    assert_eq!(mode_label(&doc), "Push Pull  ·  Face");
    let state = state_json(&doc);
    assert_eq!(state["tool"], "push_pull");
    assert_eq!(state["hint"], hint(CadTool::PushPull));
    assert!(state["numeric"]["fields"].as_array().is_some_and(Vec::is_empty));
    assert_eq!(g(1.0), "1");
    assert_eq!(g(1.25), "1.25");
    assert_eq!(num(-0.0), "0");
}

#[test]
fn a_targeted_face_is_found_again_after_an_edit_or_refused_by_name() {
    use sim_runtime::cad_client::FaceInfo;
    let plane = |index: i64, z: f64, nz: f64| FaceInfo { index, kind: "plane".into(), centroid: Some([0.0, 0.0, z]), normal: Some([0.0, 0.0, nz]), area: Some(100.0), ..Default::default() };
    let top = plane(1, 20.0, 1.0);
    // After a push/pull the faces are renumbered and the top moved up 5 mm.
    let after = crate::cad::topology::NodeTopology { revision: 5, faces: vec![plane(0, 25.0, 1.0), plane(1, 0.0, -1.0)], ..Default::default() };
    assert_eq!(match_face(&after.faces, &top).map(|f| f.index), Some(0));
    let target = Target { node: "b1".into(), face: 1, revision: 4, info: Some(top.clone()) };
    let found = resolve(&target, 5, Some(&after), "Bracket").unwrap();
    assert_eq!((found.face, found.revision), (0, 5));
    assert_eq!(resolve(&target, 4, None, "Bracket").unwrap(), target);
    let e = resolve(&Target { info: None, ..target.clone() }, 5, Some(&after), "Bracket").unwrap_err();
    assert!(e.contains("face 1 of Bracket") && e.contains("revision 4") && e.contains("press the face again"), "{e}");
    assert!(resolve(&target, 5, None, "Bracket").unwrap_err().contains("not loaded"));
}

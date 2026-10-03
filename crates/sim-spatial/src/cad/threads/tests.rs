//! RoboCAD's comment threads without a window: a commit is one RoboCAD
//! call refused by name when stale and `Committed::Pending` otherwise
//! (landed by `edit_answered`); a part link selects as REST `cad_select`
//! does; Show only linked parts and Return to assembly never touch
//! RoboCAD's visibility; every control fits `cad:threads:<id>` and
//! round-trips through its REST form; `thread_detail` maps onto the
//! annotations service's thread; a reattach or label change is one PATCH
//! with only what changed.
use super::source::{CadThreadSource, Request, patch, thread_of};
use super::{ADD, CadAnchor, REPLY, ThreadsArgs, ThreadsOp, command_action, controls_of, edit_answered, read, shown};
use crate::annotations::{Committed, ThreadSource};
use crate::app::actions::{self, Action, Call, Origin, Replies};
use crate::cad::actions::{CadAction, Cx};
use crate::cad::document::{CadDocument, CadTarget, Connection};
use crate::cad::rest_form::rest_form;
use crate::cad::selection::Fixture;
use serde_json::{Value, json};
use sim_annotate::{Anchor, Comment, ThreadCommand};
use sim_api::Outcome;
use sim_runtime::cad_client::{AnchorStatus, CadClient, CadThread, DocState, Health, NodeSummary, SelectionItem, ThreadPatch};
use std::collections::BTreeMap;

/// `thread_detail` of a pin on b1 whose geometry changed, with a reply
/// linking b2 (implied parts), then a resolved evidence thread whose part is gone.
const LIST: &str = r#"[{"id": "a1", "anchor": {"node_id": "b1", "point": [10.0, 0.0, 5.0], "geometry": "abc", "face": {"index": 2, "kind": "plane"}}, "view": {"target": [0, 0, 0], "distance": 120.0, "yaw": 30.0, "pitch": 20.0, "fov": 35.0, "orthographic": false, "mode": "turntable"}, "status": "open", "created_at": "2026-10-01T10:00:00+00:00", "updated_at": "2026-10-01T10:07:00+00:00", "comments": [{"id": "c1", "author": "You", "body": "Too thin here", "created_at": "2026-10-01T10:00:00+00:00", "updated_at": "2026-10-01T10:00:00+00:00"}, {"id": "c2", "author": "Ana", "body": "See [the plate](part:b2)", "created_at": "2026-10-01T10:05:00+00:00", "updated_at": "2026-10-01T10:07:00+00:00"}], "node_name": "Bracket", "anchor_status": "needs_review", "linked_parts": [{"node_id": "b1", "name": "Bracket", "available": true}, {"node_id": "b2", "label": "the plate", "name": "Plate", "available": true}]}, {"id": "e1", "anchor": {"node_id": null, "point": null, "geometry": null}, "view": {}, "status": "resolved", "created_at": "x", "updated_at": "x", "comments": [{"id": "c3", "author": "You", "body": "Run evidence", "created_at": "x", "updated_at": "x"}], "evidence": {"run_id": "r1", "node_ids": ["b1"]}, "part_refs": [{"node_id": "gone", "label": "Old"}], "node_name": "Experiment evidence", "anchor_status": "evidence", "linked_parts": [{"node_id": "gone", "label": "Old", "name": "Deleted part", "available": false}]}]"#;

fn threads() -> Vec<CadThread> {
    serde_json::from_str(LIST).expect("thread_detail answers")
}

fn node(id: &str, parent: Option<&str>, name: &str) -> NodeSummary {
    NodeSummary { id: id.into(), kind: "body".into(), name: name.into(), parent: parent.map(str::to_string), visible: true, effective_visible: true, ..Default::default() }
}

/// A connected document at RoboCAD's revision 4: a group with two bodies, and a body beside it.
/// The client's port is the discard port, so a started edit reaches nothing.
fn document() -> CadDocument {
    let mut doc = CadDocument::new(CadTarget::Service("http://127.0.0.1:9".into()));
    doc.client = Some(CadClient::new("http://127.0.0.1:9").unwrap());
    doc.connection = Connection::Connected;
    doc.health = Some(Health { ok: true, app: "robocad".into(), revision: 4, ..Default::default() });
    doc.doc = Some(DocState { nodes: vec![node("g1", None, "Frame"), node("b1", Some("g1"), "Bracket"), node("b2", Some("g1"), "Plate"), node("b3", None, "Base")], revision: 4, ..Default::default() });
    doc.doc_key = Some((None, 4));
    doc
}

/// The threads read at the document's current key.
fn listed(doc: &mut CadDocument) {
    let key = read::key(doc);
    doc.threads.read.listed = Some((key, threads()));
}

fn body(id: &str) -> SelectionItem {
    SelectionItem(id.into(), "body".into(), 0)
}

/// Applies `action` through the one handler REST and `system_ui` use (as a click).
fn apply(action: &CadAction, doc: &mut CadDocument, f: &mut Fixture) -> Outcome {
    let mut plane = crate::cad::sketch::CadActivePlane::default();
    let (mut continuation, mut replies) = (Value::Null, Replies::default());
    let mut call = Call { origin: Origin::Ui, continuation: &mut continuation, cancelled: false, replies: &mut replies };
    let mut cx = Cx { settings: &mut crate::app::settings::SettingsOwner::default(), doc, shared: f.shared(), meshes: None, topology: None, view: None, plane: &mut plane, sketches: None, display: None, views: None, files: None, components: &mut crate::cad::components::ComponentsState::default(), composition: &mut crate::cad::composition::CadCompositionState::default(), experiments: &mut crate::cad::experiments::ExperimentsState::default(), review: &mut crate::cad::experiment_review::ReviewState::default(), motion: &mut crate::cad::motion::MotionState::default(), camera: Vec::new() };
    crate::cad::actions::handle(action, &mut call, &mut cx)
}

/// (a) A commit goes out at the revision the threads were read at: refused
/// by name with nothing sent when RoboCAD moved since or the list is being
/// read again; otherwise one RoboCAD call, `Committed::Pending` with the
/// edit's sequence, kept in `InFlight` until `edit_answered` lands it.
#[test]
fn a_commit_is_refused_by_name_when_stale_and_pending_otherwise() {
    let mut doc = document();
    let mut f = Fixture::at(4);
    listed(&mut doc);
    doc.threads.current = Some("a1".into());
    let reply = ThreadsArgs { op: ThreadsOp::Reply, body: Some("Looks fine now".into()), ..ThreadsArgs::default() }.action();
    // RoboCAD moved on to revision 5 since the threads were read at 4.
    doc.health.as_mut().unwrap().revision = 5;
    let out = apply(&reply, &mut doc, &mut f);
    assert!(matches!(&out, Outcome::Done(Err(e)) if e.contains("revision 4, now 5") && e.contains("nothing was sent")), "the stale reply was not refused by name");
    assert!(doc.edit.is_none() && !doc.threads.in_flight.busy());
    // A landed commit makes the list stale until it is read again.
    doc.health.as_mut().unwrap().revision = 4;
    doc.threads.read.again();
    assert!(matches!(apply(&reply, &mut doc, &mut f), Outcome::Done(Err(e)) if e.contains("being read again") && e.contains("nothing was sent")));
    assert!(doc.edit.is_none());
    // Current: one reply call, pending with the edit's sequence.
    listed(&mut doc);
    let (mut continuation, mut replies) = (Value::Null, Replies::default());
    let mut call = Call { origin: Origin::Ui, continuation: &mut continuation, cancelled: false, replies: &mut replies };
    let comment = Comment { id: "new".into(), author: "Ana".into(), body: "Looks fine now".into(), created_at: "1".into(), edited_at: None, links: Vec::new() };
    let mut source = CadThreadSource::new(&mut doc, &mut call, Some(4));
    let committed = source.commit(REPLY, ThreadCommand::AddComment { thread: "a1".into(), comment });
    assert_eq!(source.sent, Some(Request::Reply { thread: "a1".into(), body: "Looks fine now".into(), author: "Ana".into() }));
    drop(source);
    let seq = doc.edit_seq;
    assert_eq!(committed, Ok(Committed::Pending(seq)));
    assert!(doc.threads.in_flight.waits(seq));
    assert_eq!(doc.edit_label(), Some(REPLY));
    // Undo is RoboCAD's: refused by name.
    let (mut continuation, mut replies) = (Value::Null, Replies::default());
    let mut call = Call { origin: Origin::Ui, continuation: &mut continuation, cancelled: false, replies: &mut replies };
    let mut source = CadThreadSource::new(&mut doc, &mut call, Some(4));
    assert!(source.commit("Undo", ThreadCommand::Undo).is_err_and(|e| e.contains("document's Undo")));
    drop(source);
    // Another edit's answer is not this source's; this one lands and the threads are read again.
    let epoch = doc.threads.read.epoch;
    edit_answered(&mut doc, seq + 7, Err("unrelated".into()));
    assert!(doc.threads.in_flight.waits(seq) && doc.threads.read.epoch == epoch);
    edit_answered(&mut doc, seq, Ok(&json!({"id": "c9", "author": "Ana", "body": "Looks fine now"})));
    assert!(!doc.threads.in_flight.busy());
    assert_eq!(doc.threads.read.epoch, epoch + 1);
    assert!(!read::current(&doc));
}

/// A placed pin's post is one create; RoboCAD's answer names the new
/// thread, which becomes the open one, and the draft ends.
#[test]
fn a_posted_pin_opens_the_new_thread_when_robocad_answers() {
    let mut doc = document();
    let mut f = Fixture::at(4);
    listed(&mut doc);
    doc.threads.pending = Some(super::Pending { node: "b2".into(), point: [1.0, 2.0, 3.0], face: Some(4), view: Default::default(), revision: 4 });
    doc.threads.compose = "Chamfer this edge".into();
    let (post, label) = super::submit_action(&doc).expect("a post");
    assert_eq!(label, "Post annotation");
    assert!(matches!(apply(&post, &mut doc, &mut f), Outcome::Done(Ok(_))));
    let seq = doc.edit_seq;
    assert_eq!((doc.threads.sending.clone(), doc.edit_label()), (Some((seq, "Chamfer this edge".to_string())), Some(ADD)));
    // The draft stays until RoboCAD takes it.
    assert!(doc.threads.pending.is_some() && doc.threads.compose == "Chamfer this edge");
    edit_answered(&mut doc, seq, Ok(&json!({"id": "t9"})));
    assert_eq!(doc.threads.current.as_deref(), Some("t9"));
    assert!(doc.threads.pending.is_none() && doc.threads.compose.is_empty() && doc.threads.release);
    // Text typed while a post was in flight is not lost when it lands.
    doc.edit = None;
    listed(&mut doc);
    doc.threads.current = Some("a1".into());
    doc.threads.release = false;
    doc.threads.compose = "Agreed".into();
    let (reply, _) = super::submit_action(&doc).expect("a reply");
    assert!(matches!(apply(&reply, &mut doc, &mut f), Outcome::Done(Ok(_))));
    let seq = doc.edit_seq;
    doc.threads.compose = "Agreed, and the fillet too".into();
    edit_answered(&mut doc, seq, Ok(&json!({"id": "c9"})));
    assert_eq!(doc.threads.compose, "Agreed, and the fillet too");
    assert!(doc.threads.sending.is_none() && !doc.threads.release);
}

/// (b) A part link selects through the shared selection exactly as REST
/// `cad_select {ids: [ID]}` does (then shows the part alone).
#[test]
fn a_part_link_selects_as_cad_select_does() {
    let mut doc = document();
    let mut f = Fixture::at(4);
    listed(&mut doc);
    let link = ThreadsArgs { op: ThreadsOp::PartLink, node: Some("b2".into()), ..ThreadsArgs::default() }.action();
    assert!(matches!(apply(&link, &mut doc, &mut f), Outcome::Done(Ok(_))));
    let mut rest_doc = document();
    let mut rest = Fixture::at(4);
    let select = CadAction::CadSelect { ids: vec!["b2".into()], items: Vec::new(), extend: false, toggle: false, picked_at: None };
    assert!(matches!(apply(&select, &mut rest_doc, &mut rest), Outcome::Done(Ok(_))));
    assert_eq!(f.items(), rest.items());
    assert_eq!(f.items(), vec![body("b2")]);
    assert!(shown(&doc, "b2") && !shown(&doc, "b1"));
    // A group's link: the group selected as `cad_select` selects it; it and its parts shown.
    let mut doc = document();
    let mut f = Fixture::at(4);
    listed(&mut doc);
    let group = ThreadsArgs { op: ThreadsOp::PartLink, node: Some("g1".into()), ..ThreadsArgs::default() }.action();
    assert!(matches!(apply(&group, &mut doc, &mut f), Outcome::Done(Ok(_))));
    let mut rest_doc = document();
    let mut rest = Fixture::at(4);
    let select = CadAction::CadSelect { ids: vec!["g1".into()], items: Vec::new(), extend: false, toggle: false, picked_at: None };
    assert!(matches!(apply(&select, &mut rest_doc, &mut rest), Outcome::Done(Ok(_))));
    assert_eq!(f.items(), rest.items());
    assert_eq!(f.items(), vec![body("g1")]);
    assert!(shown(&doc, "g1") && shown(&doc, "b1") && shown(&doc, "b2") && !shown(&doc, "b3"));
    // A deleted part's link is refused by name, nothing selected.
    let gone = ThreadsArgs { op: ThreadsOp::PartLink, node: Some("gone".into()), ..ThreadsArgs::default() }.action();
    assert!(matches!(apply(&gone, &mut doc, &mut f), Outcome::Done(Err(e)) if e.contains("deleted")));
}

/// (c) Show only linked parts and Return to assembly are display only: no
/// edit starts, RoboCAD's `visible` flags are unchanged, the other parts are
/// not shown meanwhile and are again after; the selection comes back.
#[test]
fn showing_linked_parts_alone_never_writes_visibility() {
    let mut doc = document();
    let mut f = Fixture::at(4);
    listed(&mut doc);
    f.set(vec![body("b2")]);
    let before = doc.doc.clone();
    let alone = ThreadsArgs { op: ThreadsOp::ShowParts, ids: Some(vec!["b1".into()]), ..ThreadsArgs::default() }.action();
    assert!(matches!(apply(&alone, &mut doc, &mut f), Outcome::Done(Ok(_))));
    assert!(doc.edit.is_none() && doc.edit_seq == 0);
    assert_eq!(doc.doc, before);
    assert!(shown(&doc, "b1") && !shown(&doc, "b2") && !shown(&doc, "g1"));
    assert!(super::isolating(&doc));
    assert_eq!(f.items(), vec![body("b1")]);
    let back = ThreadsArgs::of(ThreadsOp::Return).action();
    assert!(matches!(apply(&back, &mut doc, &mut f), Outcome::Done(Ok(_))));
    assert!(doc.edit.is_none() && doc.edit_seq == 0);
    assert_eq!(doc.doc, before);
    assert!(shown(&doc, "b1") && shown(&doc, "b2") && shown(&doc, "g1"));
    assert_eq!(f.items(), vec![body("b2")]);
    // The thread's own linked parts: both of a1's, everything else out.
    doc.threads.current = Some("a1".into());
    assert!(matches!(apply(&ThreadsArgs::of(ThreadsOp::ShowParts).action(), &mut doc, &mut f), Outcome::Done(Ok(_))));
    assert!(shown(&doc, "b1") && shown(&doc, "b2") && !shown(&doc, "g1"));
    assert!(doc.edit.is_none());
}

/// (d) Every threads control fits `cad:threads:<id>` and its REST form
/// parses back to the action a press writes.
#[test]
fn every_threads_control_fits_its_pattern_and_round_trips() {
    let mut doc = document();
    listed(&mut doc);
    doc.threads.open = true;
    doc.threads.filter = super::Filter::All;
    doc.threads.current = Some("a1".into());
    doc.threads.part = Some("b2".into());
    doc.threads.compose = "Hi [the plate](part:b2)".into();
    let selection = vec![body("b1")];
    let controls = controls_of(&doc, &selection);
    let ids: Vec<&str> = controls.iter().map(|c| c.id.as_str()).collect();
    for expected in [
        "open", "close", "annotate", "filter-open", "filter-all", "filter-resolved", "selected_only", "thread-a1", "thread-e1", "show", "fit", "reattach", "resolve", "link_selected", "label", "show_parts", "return", "insert_link", "part-b1", "part-b2", "edit_message-c1", "delete_message-c2",
        "delete_thread", "pins", "post", "discard", "cancel",
    ] {
        assert!(ids.contains(&format!("cad:threads:{expected}").as_str()), "{expected} is not listed: {ids:?}");
    }
    let patterns = <CadAction as Action>::controls();
    for c in &controls {
        assert!(patterns.iter().any(|p| actions::control_matches(p, &c.id)), "{} fits no registered pattern", c.id);
        let Value::Object(mut args) = rest_form(&c.action) else { panic!("{}: not an object", c.id) };
        let name = args.remove("command").and_then(|v| v.as_str().map(str::to_string)).expect("a command name");
        let parsed = <CadAction as Action>::parse(&sim_api::Command { command: name.clone(), args: Value::Object(args) }).unwrap_or_else(|e| panic!("{}: {name} does not parse: {e}", c.id));
        assert_eq!(parsed, c.action, "{}: REST form and press differ", c.id);
    }
    // A draft disables the thread actions, as RoboCAD's `update_send`.
    let find = |id: &str| controls.iter().find(|c| c.id == format!("cad:threads:{id}")).unwrap();
    assert!(find("resolve").ready.as_ref().is_err_and(|e| e.contains("draft")));
    assert!(find("post").ready.is_ok() && find("post").label == "Reply");
    assert!(find("delete_message-c2").ready.as_ref().is_err_and(|e| e.contains("draft")));
    // Without the draft a message of a two-message thread can be deleted; e1's only one cannot (RoboCAD refuses it).
    doc.threads.compose.clear();
    let controls = controls_of(&doc, &selection);
    let find = |id: &str| controls.iter().find(|c| c.id == format!("cad:threads:{id}")).unwrap();
    assert!(find("delete_message-c2").ready.is_ok() && find("resolve").ready.is_ok());
    doc.threads.current = Some("e1".into());
    let controls = controls_of(&doc, &selection);
    let only = controls.iter().find(|c| c.id == "cad:threads:delete_message-c3").unwrap();
    assert!(only.ready.as_ref().is_err_and(|e| e.contains("last comment")));
    // The commands RoboCAD's registry hands to the threads: exactly these three.
    for id in ["tool.annotate", "view.comments", "view.comment_pins"] {
        assert!(command_action(id).is_some(), "{id}");
    }
    assert!(command_action("view.references").is_none() && command_action("group.group").is_none());
}

/// (e) `thread_detail` as the annotations service's thread: the pin, the
/// linked parts, RoboCAD's times, edits and part links; evidence and a
/// deleted part; a refresh against the shown tree.
#[test]
fn thread_detail_maps_onto_the_annotations_thread() {
    let list = threads();
    let t = thread_of(&list[0]);
    assert_eq!((t.id.as_str(), t.title.as_str(), t.resolved), ("a1", "Bracket", false));
    let CadAnchor::Surface { node_id, point, face, face_index, view, state, node_name } = &t.targets[0] else { panic!("a surface pin") };
    assert_eq!((node_id.as_str(), *point, face_index, *state, node_name.as_str()), ("b1", [10.0, 0.0, 5.0], &None, AnchorStatus::NeedsReview, "Bracket"));
    assert_eq!(face.as_ref().and_then(|f| f["index"].as_i64()), Some(2));
    assert_eq!(view["distance"], json!(120.0));
    let parts: Vec<String> = t.targets[1..].iter().map(Anchor::label).collect();
    assert_eq!(parts, ["Bracket", "the plate"]);
    assert_eq!((t.comments[0].created_at.as_str(), &t.comments[0].edited_at), ("2026-10-01 10:00", &None));
    assert_eq!(t.comments[1].edited_at.as_deref(), Some("2026-10-01 10:07"));
    assert_eq!(t.comments[1].links, vec![CadAnchor::Part { node_id: "b2".into(), label: Some("the plate".into()), description: None, view: None, name: "Plate".into(), available: true }]);
    assert!(t.targets.iter().all(|a| !a.missing() && a.validate().is_ok()));
    // Evidence, resolved, its part deleted.
    let e = thread_of(&list[1]);
    assert!(e.resolved && matches!(e.targets[0], CadAnchor::Evidence { .. }));
    assert!(e.targets[1].missing() && e.targets[1].label() == "Old");
    // A refresh against the shown tree: the pin's part deleted.
    let mut pin = t.targets[0].clone();
    let index: BTreeMap<String, String> = [("b2".to_string(), "Plate".to_string())].into();
    assert!(!pin.refresh(&index) && pin.missing() && pin.label() == "Deleted part");
    // Anchors keep their shape as written.
    let back: CadAnchor = serde_json::from_value(serde_json::to_value(&t.targets[2]).unwrap()).unwrap();
    assert_eq!(back, t.targets[2]);
    // RoboCAD's limits.
    let doc = document();
    let (mut continuation, mut replies) = (Value::Null, Replies::default());
    let mut call = Call { origin: Origin::Ui, continuation: &mut continuation, cancelled: false, replies: &mut replies };
    let mut owned = doc;
    let source = CadThreadSource::new(&mut owned, &mut call, None);
    let mut long = t.clone();
    long.comments[0].body = "x".repeat(20001);
    assert_eq!(source.validate(&long), Err("Comment is too long".to_string()));
    let mut blank = t.clone();
    blank.comments[0].author = "  ".into();
    assert_eq!(source.validate(&blank), Err("Author must not be empty".to_string()));
}

/// A part's new label or a moved pin is one PATCH carrying only what changed.
#[test]
fn a_whole_thread_change_is_one_patch_of_what_changed() {
    let t = thread_of(&threads()[0]);
    let mut labelled = t.clone();
    if let CadAnchor::Part { label, .. } = &mut labelled.targets[1] {
        *label = Some("Main bracket".into());
    }
    let p = patch(&t, &labelled).unwrap();
    let refs = p.part_refs.clone().unwrap();
    assert_eq!(ThreadPatch { part_refs: None, ..p }, ThreadPatch::default());
    assert_eq!((refs[0].label.as_deref(), refs[1].label.as_deref()), (Some("Main bracket"), Some("the plate")));
    let mut moved = t.clone();
    moved.targets[0] = CadAnchor::Surface { node_id: "b2".into(), point: [1.0, 2.0, 3.0], face: None, face_index: Some(5), view: Default::default(), state: AnchorStatus::Attached, node_name: "Plate".into() };
    let p = patch(&t, &moved).unwrap();
    assert_eq!(p, ThreadPatch { node_id: Some("b2".into()), point: Some([1.0, 2.0, 3.0]), face: Some(5), ..ThreadPatch::default() });
    assert!(patch(&t, &t).is_err_and(|e| e.contains("nothing to change")));
    let mut retitled = t.clone();
    retitled.title = "Other".into();
    assert!(patch(&t, &retitled).is_err());
}

#[test]
fn unknown_mutation_requires_fresh_explicit_ack_and_retains_receipt() {
    let mut doc = document();
    let mut f = Fixture::at(4);
    doc.uncertain_edit = Some("RoboCAD may still apply this request".into());
    let ack = CadAction::CadReconcileEdit { acknowledge:true, revision:Some(4) };
    doc.stale = Some("history not inspected".into());
    assert!(matches!(apply(&ack,&mut doc,&mut f),Outcome::Done(Err(_))));
    assert!(doc.uncertain_edit.is_some() && doc.uncertain_history.is_empty());
    doc.stale = None;
    assert!(matches!(apply(&CadAction::CadReconcileEdit { acknowledge:true,revision:Some(3) },&mut doc,&mut f),Outcome::Done(Err(_))));
    assert!(doc.uncertain_edit.is_some());
    assert!(matches!(apply(&ack,&mut doc,&mut f),Outcome::Done(Ok(_))));
    assert!(doc.uncertain_edit.is_none());
    assert_eq!(doc.uncertain_history.len(),1);
    assert_eq!(doc.uncertain_history[0]["acknowledged_revision"],4);
    assert!(doc.edit.is_none(),"acknowledgment never repeats an ambiguous request");
}
#[test]
fn evidence_navigation_uses_captured_run_instead_of_a_model_pin() {
    let mut doc=document();listed(&mut doc);
    doc.threads.current=Some("e1".into());
    let controls=controls_of(&doc,&[]);
    assert!(controls.iter().find(|c|c.id=="cad:threads:show").unwrap().ready.is_ok());
    doc.client=None;
    let controls=controls_of(&doc,&[]);
    assert!(controls.iter().find(|c|c.id=="cad:threads:show").unwrap().ready.is_err());
}

/// A pin placed at revision 4 is never posted once the shown document is
/// at 5 (its face index may name another face): refused by name under the
/// composer with nothing sent, the text and the pin kept; Annotate is
/// allowed then and its click replaces the pin, keeping the text; the post
/// at the new revision goes out.
#[test]
fn a_stale_pin_is_never_sent_and_annotate_replaces_it_keeping_the_text() {
    let mut doc = document();
    let mut f = Fixture::at(4);
    listed(&mut doc);
    doc.threads.pending = Some(super::Pending { node: "b2".into(), point: [1.0, 2.0, 3.0], face: Some(4), view: Default::default(), revision: 4 });
    doc.threads.compose = "Chamfer this edge".into();
    assert_eq!(super::stale_pin(&doc), None);
    // RoboCAD's window edited something: the shown document is at revision 5, read again.
    doc.health.as_mut().unwrap().revision = 5;
    doc.doc_key = Some((None, 5));
    listed(&mut doc);
    assert_eq!(super::stale_pin(&doc), Some(4));
    let (post, _) = super::submit_action(&doc).expect("a post");
    let out = apply(&post, &mut doc, &mut f);
    assert!(matches!(&out, Outcome::Done(Err(e)) if e == super::STALE_PIN), "the stale pin was not refused by name");
    assert!(doc.edit.is_none() && !doc.threads.in_flight.busy());
    assert!(doc.threads.error.as_deref().is_some_and(|e| e == super::STALE_PIN));
    assert_eq!(doc.threads.compose, "Chamfer this edge");
    let find = |doc: &CadDocument, id: &str| controls_of(doc, &[]).into_iter().find(|c| c.id == format!("cad:threads:{id}")).unwrap();
    assert!(find(&doc, "annotate").ready.is_ok());
    assert!(find(&doc, "post").ready.is_err_and(|e| e == super::STALE_PIN));
    // Annotate, then a click at revision 5: the pin replaced, the text kept.
    assert!(matches!(apply(&ThreadsArgs::of(ThreadsOp::Annotate).action(), &mut doc, &mut f), Outcome::Done(Ok(_))));
    let click = ThreadsArgs { op: ThreadsOp::Place, node: Some("b3".into()), point: Some([4.0, 5.0, 6.0]), face: Some(1), revision: Some(5), ..ThreadsArgs::default() }.action();
    assert!(matches!(apply(&click, &mut doc, &mut f), Outcome::Done(Ok(_))));
    let pin = doc.threads.pending.clone().expect("the new pin");
    assert_eq!((pin.node.as_str(), pin.face, pin.revision), ("b3", Some(1), 5));
    assert_eq!(doc.threads.compose, "Chamfer this edge");
    assert!(doc.threads.error.is_none() && super::stale_pin(&doc).is_none());
    // A pin that is not stale still refuses a second Annotate while the draft is open.
    assert!(matches!(apply(&ThreadsArgs::of(ThreadsOp::Annotate).action(), &mut doc, &mut f), Outcome::Done(Err(e)) if e.contains("current draft")));
    let (post, _) = super::submit_action(&doc).expect("a post");
    assert!(matches!(apply(&post, &mut doc, &mut f), Outcome::Done(Ok(_))));
    assert_eq!(doc.edit_label(), Some(ADD));
}

/// A draft whose thread or edited message is gone from a list read at
/// RoboCAD's current revision (deleted in RoboCAD's window, or undone) is
/// kept and refused by name under the composer, nothing sent; while the
/// list is being read again nothing is claimed gone.
#[test]
fn a_draft_whose_target_is_gone_is_kept_and_refused_by_name() {
    let mut doc = document();
    let mut f = Fixture::at(4);
    let key = read::key(&doc);
    let without_a1: Vec<CadThread> = threads().into_iter().filter(|t| t.id != "a1").collect();
    doc.threads.read.listed = Some((key, without_a1.clone()));
    doc.threads.current = Some("a1".into());
    doc.threads.compose = "Agreed".into();
    let (reply, label) = super::submit_action(&doc).expect("a reply");
    assert_eq!(label, "Reply");
    assert!(matches!(apply(&reply, &mut doc, &mut f), Outcome::Done(Err(e)) if e.contains("no longer in RoboCAD's comments")));
    assert!(doc.edit.is_none());
    assert_eq!(doc.threads.compose, "Agreed");
    assert!(doc.threads.error.as_deref().is_some_and(|e| e.contains("copy your text")));
    let post = controls_of(&doc, &[]).into_iter().find(|c| c.id == "cad:threads:post").unwrap();
    assert!(post.ready.is_err_and(|e| e.contains("no longer")));
    // The message being edited is gone.
    doc.threads.error = None;
    doc.threads.editing = Some("c2".into());
    doc.threads.compose = "See the plate".into();
    let (save, label) = super::submit_action(&doc).expect("a save");
    assert_eq!(label, "Save edit");
    assert!(matches!(apply(&save, &mut doc, &mut f), Outcome::Done(Err(e)) if e.contains("message being edited")));
    assert!(doc.edit.is_none() && doc.threads.editing.as_deref() == Some("c2") && doc.threads.compose == "See the plate");
    // Being read again: nothing is claimed gone.
    doc.threads.read.again();
    assert_eq!(super::draft_gone(&doc), None);
    // Read again with the thread back (an undo elsewhere): the draft can go out.
    listed(&mut doc);
    assert_eq!(super::draft_gone(&doc), None);
}

/// Another mode's request to show a thread opens the Comments dock once,
/// never moves an open reply draft to another thread (RoboCAD's `select`
/// refuses it), and without a draft opens the thread with the filter All.
/// A read that failed at the current key, or a lost connection, ends it
/// with a status line; closing the dock it opened drops it.
#[test]
fn a_reveal_opens_once_keeps_an_open_drafts_thread_and_ends_when_it_cannot_land() {
    let mut doc = document();
    listed(&mut doc);
    doc.threads.current = Some("a1".into());
    doc.threads.compose = "Agreed".into();
    let ask = || super::RevealThread(Some(super::Reveal::new(CadTarget::Service("http://127.0.0.1:9".into()), "e1")));
    let step = |doc: &CadDocument, pending: &super::RevealThread| read::reveal_step(doc, pending.0.as_ref().unwrap());
    let mut pending = ask();
    assert_eq!(step(&doc, &pending), read::RevealStep::OpenDock);
    read::reveal(&mut doc, &mut pending, read::RevealStep::OpenDock);
    assert!(doc.threads.open && pending.0.as_ref().is_some_and(|r| r.opened));
    let next = step(&doc, &pending);
    assert_eq!(next, read::RevealStep::Land(true));
    read::reveal(&mut doc, &mut pending, next);
    assert!(pending.0.is_none());
    assert_eq!(doc.threads.current.as_deref(), Some("a1"));
    assert!(matches!(&doc.status, Some(Err(e)) if e.contains("current draft") && e.contains("e1")));
    // Without a draft: the thread opens.
    doc.threads.compose.clear();
    let mut pending = ask();
    read::reveal(&mut doc, &mut pending, read::RevealStep::OpenDock);
    let next = step(&doc, &pending);
    read::reveal(&mut doc, &mut pending, next);
    assert_eq!((doc.threads.current.as_deref(), doc.threads.filter), (Some("e1"), super::Filter::All));
    // Closed after it was opened for the request: dropped, not reopened.
    let mut pending = ask();
    read::reveal(&mut doc, &mut pending, read::RevealStep::OpenDock);
    doc.threads.open = false;
    assert_eq!(step(&doc, &pending), read::RevealStep::Drop);
    read::reveal(&mut doc, &mut pending, read::RevealStep::Drop);
    assert!(pending.0.is_none() && !doc.threads.open);
    // A failed read at the current key: said once, dropped.
    let key = read::key(&doc);
    doc.threads.read.error = Some((key, "boom".into()));
    let mut pending = ask();
    let failed = step(&doc, &pending);
    assert_eq!(failed, read::RevealStep::Fail("boom".into()));
    read::reveal(&mut doc, &mut pending, failed);
    assert!(pending.0.is_none() && !doc.threads.open);
    assert!(matches!(&doc.status, Some(Err(e)) if e == "Asked to show comment thread e1: RoboCAD's comments could not be read: boom"));
    // A lost connection: the same.
    doc.threads.read.error = None;
    doc.connection = Connection::Lost { error: "refused".into(), since: std::time::Instant::now() };
    let pending = ask();
    assert!(matches!(step(&doc, &pending), read::RevealStep::Fail(e) if e.contains("refused")));
}

/// A window's Resolve is refused while a draft is open, as its control is
/// (RoboCAD's `update_send` disables it); the evidence run line prints the
/// time range as RoboCAD's location line does; Show on model is refused
/// only for another thread than the draft's.
#[test]
fn the_draft_guards_follow_robocad_and_the_run_line_matches_it() {
    let mut doc = document();
    let mut f = Fixture::at(4);
    listed(&mut doc);
    doc.threads.current = Some("a1".into());
    doc.threads.compose = "Agreed".into();
    let resolve = ThreadsArgs { op: ThreadsOp::Resolve, thread: Some("a1".into()), resolved: Some(true), ..ThreadsArgs::default() }.action();
    assert!(matches!(apply(&resolve, &mut doc, &mut f), Outcome::Done(Err(e)) if e == super::DRAFTING));
    assert!(doc.edit.is_none());
    assert_eq!(super::dock::time_range(Some(&json!([0.5, 2.0]))), "[0.5, 2.0]");
    assert_eq!(super::dock::time_range(Some(&json!([0, 3]))), "[0, 3]");
    assert_eq!(super::dock::time_range(None), "[]");
    assert_eq!(super::dock::time_range(Some(&Value::Null)), "None");
    // Show on model about another thread than the draft's is refused (RoboCAD's `select` rule);
    // about the draft's own thread it runs (display only, nothing sent).
    let show = |id: &str| ThreadsArgs { op: ThreadsOp::Show, thread: Some(id.into()), ..ThreadsArgs::default() }.action();
    assert!(matches!(apply(&show("e1"), &mut doc, &mut f), Outcome::Done(Err(e)) if e.contains("current draft")));
    assert!(matches!(apply(&show("a1"), &mut doc, &mut f), Outcome::Done(Ok(_))));
    assert!(doc.edit.is_none());
}

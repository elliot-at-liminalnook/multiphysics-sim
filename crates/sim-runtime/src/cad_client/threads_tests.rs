//! The threads and comments client against the in-process fake RoboCAD of
//! `tests.rs`: answers as `annotations.thread_detail` and api.py's
//! `annotation_request` write them (json.dumps' separators), the exact
//! request line and body of every call, tolerant reads, the part-link
//! syntax of `annotations.PART_LINK`, and errors verbatim.
use super::tests::{Answer, assert_request, ok, serve};
use super::*;
use serde_json::{Map, json};

/// `thread_detail` of a thread on a box face with a reply linking a part
/// (implied `part_refs`), then an evidence thread, then a malformed one.
const LIST: &str = r#"[{"id": "a1", "anchor": {"node_id": "n1", "point": [10.0, 0.0, 5.0], "geometry": "abc", "face": {"index": 2, "kind": "plane"}}, "view": {"target": [0, 0, 0], "distance": 120.0, "yaw": 30.0, "pitch": 20.0, "fov": 35.0, "orthographic": false, "mode": "turntable"}, "status": "open", "created_at": "2026-10-01T10:00:00+00:00", "updated_at": "2026-10-01T10:05:00+00:00", "comments": [{"id": "c1", "author": "You", "body": "Too thin here", "created_at": "2026-10-01T10:00:00+00:00", "updated_at": "2026-10-01T10:00:00+00:00"}, {"id": "c2", "author": "Ana", "body": "See [the bracket](part:n2)", "created_at": "2026-10-01T10:05:00+00:00", "updated_at": "2026-10-01T10:05:00+00:00"}], "node_name": "Box", "anchor_status": "needs_review", "linked_parts": [{"node_id": "n1", "name": "Box", "available": true}, {"node_id": "n2", "label": "the bracket", "name": "Bracket", "available": true}]}, {"id": "e1", "anchor": {"node_id": null, "point": null, "geometry": null}, "view": {}, "status": "resolved", "created_at": "x", "updated_at": "x", "comments": [{"id": "c3", "author": "You", "body": "Run evidence", "created_at": "x", "updated_at": "x"}], "evidence": {"run_id": "r1", "node_ids": ["n1"]}, "part_refs": [{"node_id": "gone", "label": "Old"}], "node_name": "Experiment evidence", "anchor_status": "evidence", "linked_parts": [{"node_id": "gone", "label": "Old", "name": "Deleted part", "available": false}]}, "bad"]"#;

#[test]
fn threads_read_tolerantly_with_filters_encoded() {
    let (c, server) = serve(vec![ok(LIST), ok("[]"), ok(r#"{"threads": []}"#)]);
    let port = c.endpoint.port;
    let list = c.threads(None, None, None).unwrap();
    assert_eq!(list.len(), 2, "the malformed entry is dropped, not the list");
    let t = &list[0];
    assert_eq!((t.id.as_str(), t.node_name.as_str(), t.anchor_status, t.resolved()), ("a1", "Box", AnchorStatus::NeedsReview, false));
    assert_eq!(t.anchor.node_id.as_deref(), Some("n1"));
    assert_eq!(t.anchor.point, Some([10.0, 0.0, 5.0]));
    assert_eq!(t.anchor.face.as_ref().and_then(|f| f["index"].as_i64()), Some(2));
    assert_eq!(t.view["distance"], json!(120.0));
    assert_eq!(t.comments.iter().map(|c| c.author.as_str()).collect::<Vec<_>>(), ["You", "Ana"]);
    assert_eq!(t.part_refs, None, "implied parts are only in linked_parts");
    assert_eq!(t.linked_parts[1].part.label.as_deref(), Some("the bracket"));
    assert_eq!((t.linked_parts[1].name.as_str(), t.linked_parts[1].available), ("Bracket", true));
    let e = &list[1];
    assert_eq!((e.anchor.node_id.as_deref(), e.anchor.point, e.anchor_status, e.resolved()), (None, None, AnchorStatus::Evidence, true));
    assert_eq!(e.part_refs.as_ref().map(|r| r[0].node_id.as_str()), Some("gone"));
    assert!(!e.linked_parts[0].available);
    assert!(c.threads(Some("n 1"), Some("open"), Some("r1")).unwrap().is_empty());
    let err = c.threads(None, None, None).unwrap_err();
    assert!(err.message.contains("expected a list of threads"), "{err}");
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /threads HTTP/1.1", port, None);
    assert_request(&seen[1], "GET /threads?node_id=n%201&status=open&run_id=r1 HTTP/1.1", port, None);
}

#[test]
fn an_unknown_anchor_state_and_malformed_fields_read_as_defaults() {
    let odd = r#"{"id": "a9", "anchor": "bad", "view": {}, "status": "open", "comments": [{"id": "c"}, 7], "node_name": "Box", "anchor_status": "floating", "linked_parts": "x"}"#;
    let (c, server) = serve(vec![ok(odd)]);
    let t = c.thread("a9").unwrap();
    assert_eq!(t.anchor, ThreadAnchor::default());
    assert_eq!(t.anchor_status, AnchorStatus::Unknown);
    assert_eq!(t.comments.len(), 1);
    assert!(t.linked_parts.is_empty());
    server.join().unwrap();
}

#[test]
fn create_update_and_delete_send_annotations_shapes() {
    let detail = r#"{"id": "t9", "anchor": {"node_id": "n1", "point": [1.0, 2.0, 3.0], "geometry": "g", "face": {"index": 4}}, "view": {}, "status": "open", "created_at": "x", "updated_at": "x", "comments": [{"id": "c9", "author": "You", "body": "Hi", "created_at": "x", "updated_at": "x"}], "node_name": "Box", "anchor_status": "attached", "linked_parts": [{"node_id": "n1", "name": "Box", "available": true}]}"#;
    let (c, server) = serve(vec![Answer::Json(201, detail.into()), ok(detail), ok(detail), ok(r#"{"deleted": "t9"}"#), Answer::Json(422, r#"{"error": "Linked part does not exist: zz"}"#.into())]);
    let port = c.endpoint.port;
    let mut view = Map::new();
    view.insert("distance".into(), json!(80.0));
    let new = NewThread { node_id: "n1".into(), point: [1.0, 2.0, 3.0], body: "Hi".into(), author: "You".into(), face: Some(4), view: Some(view), part_refs: None };
    assert_eq!(c.create_thread(&new).unwrap().id, "t9", "the id is RoboCAD's");
    let resolve = ThreadPatch { status: Some("resolved".into()), ..ThreadPatch::default() };
    c.update_thread("t9", &resolve).unwrap();
    let reattach = ThreadPatch { node_id: Some("n2".into()), point: Some([0.0, 0.5, 1.0]), face: Some(1), part_refs: Some(vec![PartRef { node_id: "n2".into(), label: Some("Arm".into()), ..PartRef::default() }]), ..ThreadPatch::default() };
    c.update_thread("t9", &reattach).unwrap();
    assert_eq!(c.delete_thread("t9").unwrap().deleted, "t9");
    let bad = ThreadPatch { part_refs: Some(vec![PartRef { node_id: "zz".into(), ..PartRef::default() }]), ..ThreadPatch::default() };
    let err = c.update_thread("t9", &bad).unwrap_err();
    assert_eq!((err.status, err.message.as_str()), (Some(422), "Linked part does not exist: zz"));
    let seen = server.join().unwrap();
    assert_request(&seen[0], "POST /threads HTTP/1.1", port, Some(r#"{"node_id":"n1","point":[1.0,2.0,3.0],"body":"Hi","author":"You","face":4,"view":{"distance":80.0}}"#));
    assert_request(&seen[1], "PATCH /threads/t9 HTTP/1.1", port, Some(r#"{"status":"resolved"}"#));
    assert_request(&seen[2], "PATCH /threads/t9 HTTP/1.1", port, Some(r#"{"node_id":"n2","point":[0.0,0.5,1.0],"face":1,"part_refs":[{"node_id":"n2","label":"Arm"}]}"#));
    assert_request(&seen[3], "DELETE /threads/t9 HTTP/1.1", port, None);
}

#[test]
fn comments_reply_read_edit_and_delete() {
    let comment = r#"{"id": "c5", "body": "Agreed", "author": "Ana", "created_at": "x", "updated_at": "x"}"#;
    let read = r#"{"id": "c5", "body": "Agreed", "author": "Ana", "created_at": "x", "updated_at": "x", "thread_id": "t9"}"#;
    let last = r#"{"error": "delete the thread to remove its last comment"}"#;
    let (c, server) = serve(vec![Answer::Json(201, comment.into()), ok(read), ok(comment), ok(r#"{"deleted": "c5"}"#), Answer::Json(422, last.into())]);
    let port = c.endpoint.port;
    assert_eq!(c.add_comment("t9", "Agreed", "Ana").unwrap().id, "c5");
    assert_eq!(c.comment("c5").unwrap().thread_id.as_deref(), Some("t9"));
    assert_eq!(c.update_comment("c5", "Agreed, [arm](part:n2)").unwrap().body, "Agreed");
    assert_eq!(c.delete_comment("c5").unwrap().deleted, "c5");
    let err = c.delete_comment("c1").unwrap_err();
    assert_eq!(err.message, "delete the thread to remove its last comment");
    let seen = server.join().unwrap();
    assert_request(&seen[0], "POST /threads/t9/comments HTTP/1.1", port, Some(r#"{"body":"Agreed","author":"Ana"}"#));
    assert_request(&seen[1], "GET /comments/c5 HTTP/1.1", port, None);
    assert_request(&seen[2], "PATCH /comments/c5 HTTP/1.1", port, Some(r#"{"body":"Agreed, [arm](part:n2)"}"#));
    assert_request(&seen[3], "DELETE /comments/c5 HTTP/1.1", port, None);
}

#[test]
fn part_links_match_annotations_part_link() {
    assert_eq!(part_links("See [the bracket](part:n2) and [arm](part:a_1-b)."), [("the bracket".to_string(), "n2".to_string()), ("arm".to_string(), "a_1-b".to_string())]);
    // A label may hold `[` (the regex's `[^\]\n]+`), not a newline; an id
    // stops at its first other character and must close with `)`.
    assert_eq!(part_links("[a [b](part:x)"), [("a [b".to_string(), "x".to_string())]);
    assert!(part_links("[a\nb](part:x) [c](part:) [d](part:y z) [e](web:x)").is_empty());
    assert_eq!(part_link("Arm [left]\nside", "n7"), "[Arm (left) side](part:n7)");
    assert_eq!(part_links(&part_link("Arm [left]", "n7")), [("Arm (left)".to_string(), "n7".to_string())]);
}

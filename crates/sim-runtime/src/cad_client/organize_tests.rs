//! The outliner's organization calls against the fake RoboCAD of
//! `tests.rs`: `POST /ops/{group, move_nodes, set_active_group,
//! set_locked}` as api.py's `op` route takes them (`{"args", "kwargs"}`)
//! and answers them (`{"result", "history"}`), the one-node move as
//! `PATCH /nodes/{id} {"parent", "index"}` (api.py `patch` →
//! `Ops.move_node`), and RoboCAD's refusals verbatim.
use super::tests::{Answer, assert_request, ok, serve};

fn op_answer(result: &str, undo: &str) -> Answer {
    ok(&format!(r#"{{"result": {result}, "history": {{"undo": ["{undo}"], "redo": []}}}}"#))
}

/// `node_detail` of a box moved under group g1.
const MOVED: &str = r#"{"id": "b1", "kind": "body", "name": "Box", "parent": "g1", "children": [], "visible": true, "locked": false, "disabled": false, "material": null, "color": null, "pivot": null, "source": null, "transform": {"translation": [0, 0, 0], "axis": [0, 0, 1], "angle_deg": 0, "scale": 1}, "effective_visible": true, "component_instance": null, "component_member": null}"#;

#[test]
fn organize_ops_send_positional_args() {
    let refused = r#"{"error": "Cannot move a group into itself or its descendants"}"#;
    let (c, server) = serve(vec![
        op_answer(r#""g1""#, "Group"),
        op_answer("null", "Move in outliner"),
        op_answer("null", "Move in outliner"),
        Answer::Json(422, refused.into()),
        op_answer("null", "Move in outliner"),
        op_answer("null", "Move in outliner"),
        op_answer("null", "Lock"),
    ]);
    let port = c.endpoint.port;
    let ids = vec!["b1".to_string(), "b2".to_string()];
    let group = c.group(&ids, "Legs").unwrap();
    assert_eq!(group.result.as_str(), Some("g1"));
    assert_eq!(group.history.undo, ["Group"]);
    c.move_nodes(&ids, Some("g1"), Some(2)).unwrap();
    c.move_nodes(&ids, None, None).unwrap();
    let err = c.move_nodes(&["g1".to_string()], Some("g1"), None).unwrap_err();
    assert_eq!((err.status, err.message.as_str()), (Some(422), "Cannot move a group into itself or its descendants"));
    c.set_active_group(Some("g1")).unwrap();
    c.set_active_group(None).unwrap();
    assert_eq!(c.set_locked(&ids, true).unwrap().history.undo, ["Lock"]);
    let seen = server.join().unwrap();
    assert_request(&seen[0], "POST /ops/group HTTP/1.1", port, Some(r#"{"args":[["b1","b2"],"Legs"],"kwargs":{}}"#));
    assert_request(&seen[1], "POST /ops/move_nodes HTTP/1.1", port, Some(r#"{"args":[["b1","b2"],"g1",2],"kwargs":{}}"#));
    assert_request(&seen[2], "POST /ops/move_nodes HTTP/1.1", port, Some(r#"{"args":[["b1","b2"],null,null],"kwargs":{}}"#));
    assert_request(&seen[4], "POST /ops/set_active_group HTTP/1.1", port, Some(r#"{"args":["g1"],"kwargs":{}}"#));
    assert_request(&seen[5], "POST /ops/set_active_group HTTP/1.1", port, Some(r#"{"args":[null],"kwargs":{}}"#));
    assert_request(&seen[6], "POST /ops/set_locked HTTP/1.1", port, Some(r#"{"args":[["b1","b2"],true],"kwargs":{}}"#));
}

#[test]
fn a_one_node_move_is_a_parent_and_index_patch() {
    let (c, server) = serve(vec![ok(MOVED), ok(MOVED)]);
    let port = c.endpoint.port;
    assert_eq!(c.move_node("b1", Some("g1"), Some(0)).unwrap().summary.parent.as_deref(), Some("g1"));
    c.move_node("b1", None, None).unwrap();
    let seen = server.join().unwrap();
    // serde_json's map is sorted: "index" before "parent".
    assert_request(&seen[0], "PATCH /nodes/b1 HTTP/1.1", port, Some(r#"{"index":0,"parent":"g1"}"#));
    assert_request(&seen[1], "PATCH /nodes/b1 HTTP/1.1", port, Some(r#"{"index":null,"parent":null}"#));
}

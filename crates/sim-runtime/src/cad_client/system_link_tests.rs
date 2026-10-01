//! The linked system file calls against the fake RoboCAD of `tests.rs`:
//! `POST /ops/{link_system, unlink_system, refresh_system_link,
//! system_status}` as api.py's `op` route takes them, `system_link.status`'s
//! four answers, and RoboCAD's status line (ui/references.py:90-103).
use super::tests::{Answer, assert_request, ok, serve};
use super::*;

fn op_answer(result: &str) -> Answer {
    ok(&format!(r#"{{"result": {result}, "history": {{"undo": ["Robot setting"], "redo": []}}}}"#))
}

const LINK: &str = r#"{"path": "board.system.json", "sha256": "aa", "title": "Motor driver board", "revision": 3, "definitions": 5, "instances": 4, "linked": "2026-10-01T10:00:00"}"#;
const NOW: &str = r#"{"sha256": "bb", "title": "Motor driver board", "revision": 4, "definitions": 6, "instances": 4}"#;

#[test]
fn link_accept_and_unlink_are_one_op_each() {
    let refused = r#"{"error": "No system file is linked"}"#;
    let (c, server) = serve(vec![op_answer(LINK), op_answer(LINK), op_answer("null"), Answer::Json(422, refused.into())]);
    let port = c.endpoint.port;
    let link: SystemSummary = serde_json::from_value(c.link_system("/w/board.system.json").unwrap().result).unwrap();
    assert_eq!((link.path.as_deref(), link.revision, link.definitions), (Some("board.system.json"), 3, 5));
    c.refresh_system_link().unwrap();
    c.unlink_system().unwrap();
    let err = c.unlink_system().unwrap_err();
    assert_eq!((err.status, err.message.as_str()), (Some(422), "No system file is linked"));
    let seen = server.join().unwrap();
    assert_request(&seen[0], "POST /ops/link_system HTTP/1.1", port, Some(r#"{"args":["/w/board.system.json"],"kwargs":{}}"#));
    assert_request(&seen[1], "POST /ops/refresh_system_link HTTP/1.1", port, Some(r#"{"args":[],"kwargs":{}}"#));
    assert_request(&seen[2], "POST /ops/unlink_system HTTP/1.1", port, Some(r#"{"args":[],"kwargs":{}}"#));
}

#[test]
fn status_reads_every_state_and_writes_robocads_line() {
    let unlinked = r#"{"state": "unlinked"}"#;
    let missing = format!(r#"{{"state": "missing", "path": "/w/board.system.json", "error": "Cannot read system file: [Errno 2]", "link": {LINK}}}"#);
    let current = format!(r#"{{"state": "current", "path": "/w/board.system.json", "link": {LINK}, "now": {LINK}}}"#);
    let changed = format!(r#"{{"state": "changed", "path": "/w/board.system.json", "link": {LINK}, "now": {NOW}}}"#);
    let untitled = r#"{"state": "current", "path": "/w/x/plain.system.json", "link": {"title": "", "revision": 1}, "now": {"title": "", "revision": 1, "definitions": 2}}"#;
    let (c, server) = serve(vec![op_answer(unlinked), op_answer(&missing), op_answer(&current), op_answer(&changed), op_answer(untitled), op_answer(r#""odd""#)]);
    let port = c.endpoint.port;
    let s = c.system_status().unwrap();
    assert_eq!(s.state, LinkState::Unlinked);
    assert_eq!(s.line(), "System file: none linked. Link a .system.json to build circuits and subsystems for this model.");
    let s = c.system_status().unwrap();
    assert_eq!((s.state, s.error.as_deref()), (LinkState::Missing, Some("Cannot read system file: [Errno 2]")));
    assert_eq!(s.line(), "System file missing: /w/board.system.json");
    let s = c.system_status().unwrap();
    assert_eq!(s.line(), "System: Motor driver board · revision 3 · 5 definitions");
    let s = c.system_status().unwrap();
    assert_eq!(s.state, LinkState::Changed);
    assert_eq!(s.line(), "System: Motor driver board · revision 4 · 6 definitions · CHANGED since linked (was revision 3)");
    assert_eq!(c.system_status().unwrap().line(), "System: plain.system.json · revision 1 · 2 definitions", "an untitled file shows its file name");
    let err = c.system_status().unwrap_err();
    assert!(err.message.starts_with("unexpected answer"), "{err}");
    let seen = server.join().unwrap();
    assert_request(&seen[0], "POST /ops/system_status HTTP/1.1", port, Some(r#"{"args":[],"kwargs":{}}"#));
}

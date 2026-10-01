//! The section client against the in-process fake RoboCAD of `tests.rs`:
//! the exact request line (the plane as a query string, as api.py's route
//! reads it), answers as `json.dumps` writes `section_outline`'s list of
//! tuples, the tolerant read and errors verbatim.
use super::tests::{Answer, assert_request, ok, serve};
use super::*;

#[test]
fn section_sends_the_plane_as_a_query_string_and_reads_polylines() {
    // A 10 mm cube cut by XZ: one closed square (OCCT samples lines as two points).
    let square = r#"[[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]], [[10.0, 0.0, 0.0], [10.0, 0.0, 10.0]], [[10.0, 0.0, 10.0], [0.0, 0.0, 10.0]], [[0.0, 0.0, 10.0], [0.0, 0.0, 0.0]]]"#;
    let (c, server) = serve(vec![
        ok(square),
        ok("[]"),
        // A plane node id that needs encoding in the query (parse_qs decodes it).
        ok(r#"[[[1, 2, 3], [4, 5, 6]]]"#),
        Answer::Json(400, r#"{"error": "unknown plane 'p9' (xy/xz/yz or a plane node id)"}"#.into()),
    ]);
    let port = c.endpoint.port;
    let s = c.section("a1b2c3d4e5f6", &SectionQuery::Xz).unwrap();
    assert_eq!(s.polylines.len(), 4);
    assert_eq!(s.polylines[1], vec![[10.0, 0.0, 0.0], [10.0, 0.0, 10.0]]);
    assert_eq!(s.dropped, 0);
    assert_eq!(c.section("a1b2c3d4e5f6", &SectionQuery::Xy).unwrap(), SectionCurves::default());
    let s = c.section("n1", &SectionQuery::Node("plane 1/a".into())).unwrap();
    assert_eq!(s.polylines, vec![vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]]);
    let e = c.section("n1", &SectionQuery::Node("p9".into())).unwrap_err();
    assert_eq!(e.status, Some(400));
    assert_eq!(e.message, "unknown plane 'p9' (xy/xz/yz or a plane node id)");
    assert_eq!(e.route, "/nodes/n1/section?plane=p9");
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /nodes/a1b2c3d4e5f6/section?plane=xz HTTP/1.1", port, None);
    assert_request(&seen[1], "GET /nodes/a1b2c3d4e5f6/section?plane=xy HTTP/1.1", port, None);
    assert_request(&seen[2], "GET /nodes/n1/section?plane=plane%201%2Fa HTTP/1.1", port, None);
    assert_request(&seen[3], "GET /nodes/n1/section?plane=p9 HTTP/1.1", port, None);
}

#[test]
fn malformed_polylines_are_dropped_and_counted() {
    // A bare NaN (Python's json.dumps) reads as null: that polyline goes, the rest stay.
    let (c, server) = serve(vec![ok(r#"[[[0, 0, 0], [1, 0, 0]], [[0, 0, NaN], [1, 1, 1]], [[1, 2]], "x", []]"#), ok(r#"{"error": "nope"}"#)]);
    let s = c.section("n1", &SectionQuery::Yz).unwrap();
    assert_eq!(s.polylines, vec![vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]]);
    assert_eq!(s.dropped, 4);
    // Not a list at all: an error naming the answer.
    let e = c.section("n1", &SectionQuery::Yz).unwrap_err();
    assert!(e.message.contains("expected a list of polylines"), "{}", e.message);
    server.join().unwrap();
}

#[test]
fn named_planes_match_only_planes_through_the_origin() {
    assert_eq!(SectionQuery::named([5.0, 0.0, 7.0], [0.0, -1.0, 0.0], 1e-6), Some(SectionQuery::Xz));
    assert_eq!(SectionQuery::named([5.0, 0.0, 7.0], [0.0, 2.0, 0.0], 1e-6), Some(SectionQuery::Xz));
    assert_eq!(SectionQuery::named([0.0, 3.0, 0.0], [0.0, 0.0, 1.0], 1e-6), Some(SectionQuery::Xy));
    assert_eq!(SectionQuery::named([0.0, 3.0, 0.0], [-1.0, 0.0, 0.0], 1e-6), Some(SectionQuery::Yz));
    // Offset or tilted: no query form.
    assert_eq!(SectionQuery::named([0.0, 0.0, 2.0], [0.0, 0.0, 1.0], 1e-6), None);
    assert_eq!(SectionQuery::named([0.0, 0.0, 0.0], [0.0, 1.0, 1.0], 1e-6), None);
    assert_eq!(SectionQuery::named([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], 1e-6), None);
    assert_eq!(SectionQuery::Node("p1".into()).as_str(), "p1");
}

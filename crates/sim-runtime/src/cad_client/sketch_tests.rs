//! The sketch client against the in-process fake RoboCAD of `tests.rs`:
//! answers copied from what kernel/sketch.py (`Sketch.to_json`,
//! `Curve.to_json`) and api.py (`node_detail`, `null` for a node that is
//! not a sketch) write; the exact request lines and bodies of
//! `edit_sketch` and `create_sketch`; every call's JSON both ways and every
//! refusal; index tracking across a call list; plane frames; sampling.
use super::tests::{Answer, assert_request, ok, serve};
use super::*;
use serde_json::{Value, json};
use std::collections::BTreeSet;

const TRANSFORM: &str = r#"{"translation": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0], "angle_deg": 0.0, "scale": 1.0}"#;

/// `Curve.to_json`'s fields after `kind`, `points` and `center`, as
/// Python writes them for a curve with the given values.
fn tail(radius: &str, start: &str, end: &str, closed: bool) -> String {
    format!(r#""radius": {radius}, "radius2": 0.0, "start_angle": {start}, "end_angle": {end}, "rotation": 0.0, "degree": 3, "closed": {closed}, "text": "", "height": 10.0, "font": "", "name": """#)
}

/// `Sketch.to_json` of an XY sketch holding a closed rectangle (polyline),
/// a circle, an arc, a slot and a spline, as json.dumps writes it.
fn sketch_json() -> String {
    let curves = [
        format!(r#"{{"kind": "polyline", "points": [[0, 0], [20, 0], [20, 10], [0, 10]], "center": null, {}}}"#, tail("0.0", "0.0", "360.0", true)),
        format!(r#"{{"kind": "circle", "points": [], "center": [10, 5], {}}}"#, tail("2", "0.0", "360.0", false)),
        format!(r#"{{"kind": "arc", "points": [], "center": [0.0, 0.0], {}}}"#, tail("5.0", "0.0", "90.0", false)),
        format!(r#"{{"kind": "slot", "points": [[0, 0], [10, 0]], "center": null, {}}}"#, tail("1.5", "0.0", "360.0", true)),
        format!(r#"{{"kind": "spline", "points": [[0, 0], [5, 5], [10, 0]], "center": null, {}}}"#, tail("0.0", "0.0", "360.0", false)),
    ];
    format!(r#"{{"name": "Sketch", "plane": {{"origin": [0.0, 0.0, 0.0], "normal": [0.0, 0.0, 1.0], "x_axis": [1.0, 0.0, 0.0]}}, "curves": [{}]}}"#, curves.join(", "))
}

/// `node_detail` of a node: `node_summary` plus `extra` (`"sketch": …`).
fn detail(id: &str, kind: &str, name: &str, extra: &str) -> String {
    format!(
        r#"{{"id": "{id}", "kind": "{kind}", "name": "{name}", "parent": null, "children": [], "visible": true, "locked": false, "disabled": false, "material": null, "color": null, "pivot": null, "source": null, "transform": {TRANSFORM}, "effective_visible": true, "component_instance": null, "component_member": null{extra}}}"#
    )
}

fn close(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
}

#[test]
fn sketch_decodes_as_api_writes_it() {
    let (c, server) = serve(vec![ok(&sketch_json())]);
    let port = c.endpoint.port;
    let g = c.sketch("5a5b5c5d5e5f").unwrap();
    assert_eq!(g.name, "Sketch");
    assert_eq!(g.plane, Some(PlaneFrame::XY));
    let kinds: Vec<&str> = g.curves.iter().map(|c| c.kind.as_str()).collect();
    assert_eq!(kinds, ["polyline", "circle", "arc", "slot", "spline"]);
    let rect = &g.curves[0];
    assert_eq!((rect.points.clone(), rect.closed, rect.center), (vec![[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0]], true, None));
    let circle = &g.curves[1];
    assert_eq!((circle.center, circle.radius, circle.end_angle, circle.degree, circle.height), (Some([10.0, 5.0]), 2.0, 360.0, 3, 10.0));
    let arc = &g.curves[2];
    assert_eq!((arc.start_angle, arc.end_angle), (0.0, 90.0));
    assert!(close(arc.start().unwrap(), [5.0, 0.0]) && close(arc.end().unwrap(), [0.0, 5.0]));
    assert_eq!((g.curves[3].radius, g.curves[3].closed), (1.5, true));
    assert_eq!(g.curves[4].points.len(), 3);
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /nodes/5a5b5c5d5e5f/sketch HTTP/1.1", port, None);
}

#[test]
fn sketch_reads_missing_and_malformed_fields_tolerantly() {
    let v = json!({
        "curves": [
            {"kind": "line", "points": [[0, 0], [1, 0]]},
            {"points": [[0, 0], [1, 0]]},
            {"kind": 3},
            "not a curve",
            {"kind": "circle", "center": [1], "radius": "big", "degree": -1, "closed": "yes", "name": 5},
            {"kind": "polyline", "points": [[0, 0], [1]]},
        ]
    });
    let (g, dropped) = SketchGeometry::from_value(&v).unwrap();
    assert_eq!(dropped, 3);
    // `Sketch.from_json`'s default name; no plane.
    assert_eq!((g.name.as_str(), g.plane), ("Sketch", None));
    assert_eq!(g.curves.len(), 3);
    assert_eq!(g.curves[0], SketchCurve { kind: "line".into(), points: vec![[0.0, 0.0], [1.0, 0.0]], ..SketchCurve::default() });
    // Every malformed field takes `Curve.from_json`'s default.
    assert_eq!(g.curves[1], SketchCurve { kind: "circle".into(), ..SketchCurve::default() });
    assert!(g.curves[2].points.is_empty());
    // Through serde as well; a non-object is refused.
    let g2: SketchGeometry = serde_json::from_value(v).unwrap();
    assert_eq!(g2, g);
    assert!(SketchGeometry::from_value(&json!([1, 2])).unwrap_err().contains("a sketch is an object"));
    // A malformed plane reads as None.
    let (g3, _) = SketchGeometry::from_value(&json!({"name": "S", "plane": {"origin": [0, 0, 0], "normal": [0, 0, 0], "x_axis": [1, 0, 0]}, "curves": []})).unwrap();
    assert_eq!((g3.name.as_str(), g3.plane), ("S", None));
}

#[test]
fn a_node_that_is_not_a_sketch_is_refused_by_name() {
    let (c, server) = serve(vec![ok("null"), Answer::Json(404, r#"{"error": "no node zz"}"#.into())]);
    let port = c.endpoint.port;
    let e = c.sketch("a1b2c3d4e5f6").unwrap_err();
    assert_eq!((e.method, e.route.as_str(), e.status, e.message.as_str()), ("GET", "/nodes/a1b2c3d4e5f6/sketch", None, "a1b2c3d4e5f6 is not a sketch"));
    let e = c.sketch("zz").unwrap_err();
    assert_eq!((e.status, e.message.as_str()), (Some(404), "no node zz"));
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /nodes/a1b2c3d4e5f6/sketch HTTP/1.1", port, None);
    assert_request(&seen[1], "GET /nodes/zz/sketch HTTP/1.1", port, None);
}

#[test]
fn edit_sketch_request_and_answer() {
    let answer = detail("5a5b5c5d5e5f", "sketch", "Sketch", &format!(r#", "sketch": {}"#, sketch_json()));
    let (c, server) = serve(vec![ok(&answer)]);
    let port = c.endpoint.port;
    let calls = [SketchCall::Rectangle { corner: [0.0, 0.0], size: [20.0, 10.0] }, SketchCall::Circle { center: [10.0, 5.0], radius: 2.0 }, SketchCall::Join { curves: vec![0, 1] }];
    let d = c.edit_sketch("5a5b5c5d5e5f", &calls).unwrap();
    assert_eq!((d.summary.kind.as_str(), d.summary.name.as_str()), ("sketch", "Sketch"));
    let (g, dropped) = SketchGeometry::from_value(d.sketch.as_ref().unwrap()).unwrap();
    assert_eq!((g.curves.len(), dropped), (5, 0));
    let seen = server.join().unwrap();
    assert_request(&seen[0], "POST /nodes/5a5b5c5d5e5f/sketch HTTP/1.1", port, Some(r#"{"calls":[["rectangle",[[0.0,0.0],[20.0,10.0]]],["circle",[[10.0,5.0],2.0]],["join",[[0,1]]]]}"#));
}

#[test]
fn create_sketch_request_and_answer() {
    let sketch = r#"{"name": "Sketch", "plane": {"origin": [0.0, 0.0, 0.0], "normal": [0.0, 0.0, 1.0], "x_axis": [1.0, 0.0, 0.0]}, "curves": []}"#;
    let (c, server) = serve(vec![Answer::Json(201, detail("0f0e0d0c0b0a", "sketch", "Sketch", &format!(r#", "sketch": {sketch}"#))), Answer::Json(201, detail("1f1e1d1c1b1a", "sketch", "Profile", &format!(r#", "sketch": {sketch}"#)))]);
    let port = c.endpoint.port;
    let d = c.create_sketch(&json!("xy"), &[SketchCall::Line { a: [0.0, 0.0], b: [10.0, 0.0] }], None).unwrap();
    assert_eq!(d.summary.id, "0f0e0d0c0b0a");
    let plane = json!({"origin": [0, 0, 5], "normal": [0, 0, 1], "x_axis": [1, 0, 0]});
    let d = c.create_sketch(&plane, &[], Some("Profile")).unwrap();
    assert_eq!(d.summary.name, "Profile");
    let seen = server.join().unwrap();
    assert_request(&seen[0], "POST /nodes HTTP/1.1", port, Some(r#"{"kind":"sketch","plane":"xy","calls":[["line",[[0.0,0.0],[10.0,0.0]]]]}"#));
    // serde_json's map is sorted (no `preserve_order`): the plane's keys in order.
    assert_request(&seen[1], "POST /nodes HTTP/1.1", port, Some(r#"{"kind":"sketch","plane":{"normal":[0,0,1],"origin":[0,0,5],"x_axis":[1,0,0]},"calls":[],"name":"Profile"}"#));
}

#[test]
fn plane_of_a_plane_node() {
    let plane = r#", "plane": {"origin": [0.0, 5.0, 0.0], "normal": [0.0, -1.0, 0.0], "x_axis": [1.0, 0.0, 0.0]}"#;
    let (c, server) = serve(vec![ok(&detail("2a2b2c2d2e2f", "plane", "Plane", plane)), ok(&detail("3a3b3c3d3e3f", "group", "Group", ""))]);
    let d = c.node("2a2b2c2d2e2f").unwrap();
    assert_eq!(plane_of(&d), Some(PlaneFrame { origin: [0.0, 5.0, 0.0], ..PlaneFrame::XZ }));
    assert_eq!(plane_of(&c.node("3a3b3c3d3e3f").unwrap()), None);
    server.join().unwrap();
}

/// Every call with the JSON api.py reads for it.
fn call_cases() -> Vec<(SketchCall, Value)> {
    use SketchCall::*;
    vec![
        (Line { a: [0.0, 0.0], b: [10.0, 0.0] }, json!(["line", [[0.0, 0.0], [10.0, 0.0]]])),
        (Polyline { points: vec![[0.0, 0.0], [1.0, 1.0]], closed: false }, json!(["polyline", [[[0.0, 0.0], [1.0, 1.0]]]])),
        (Polyline { points: vec![[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], closed: true }, json!(["polyline", [[[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], true]])),
        (Spline { points: vec![[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], closed: true }, json!(["spline", [[[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]], true]])),
        (ControlCurve { points: vec![[0.0, 0.0], [1.0, 1.0]], degree: 3, closed: false }, json!(["control_curve", [[[0.0, 0.0], [1.0, 1.0]]]])),
        (ControlCurve { points: vec![[0.0, 0.0], [1.0, 1.0]], degree: 2, closed: false }, json!(["control_curve", [[[0.0, 0.0], [1.0, 1.0]], 2]])),
        (ControlCurve { points: vec![[0.0, 0.0], [1.0, 1.0]], degree: 3, closed: true }, json!(["control_curve", [[[0.0, 0.0], [1.0, 1.0]], 3, true]])),
        (Circle { center: [10.0, 5.0], radius: 2.0 }, json!(["circle", [[10.0, 5.0], 2.0]])),
        (CircleTwoPoint { a: [0.0, 0.0], b: [4.0, 0.0] }, json!(["circle_two_point", [[0.0, 0.0], [4.0, 0.0]]])),
        (CircleThreePoint { a: [0.0, 0.0], b: [4.0, 0.0], c: [2.0, 2.0] }, json!(["circle_three_point", [[0.0, 0.0], [4.0, 0.0], [2.0, 2.0]]])),
        (CircleTangent { curves: vec![0, 1, 2], radius: None, near: [0.0, 0.0] }, json!(["circle_tangent", [[0, 1, 2]]])),
        (CircleTangent { curves: vec![0, 1], radius: Some(4.0), near: [0.0, 0.0] }, json!(["circle_tangent", [[0, 1], 4.0]])),
        (CircleTangent { curves: vec![0, 1, 2], radius: None, near: [12.0, 8.0] }, json!(["circle_tangent", [[0, 1, 2], null, [12.0, 8.0]]])),
        (Ellipse { center: [0.0, 0.0], radius_x: 4.0, radius_y: 2.0, rotation: 0.0 }, json!(["ellipse", [[0.0, 0.0], 4.0, 2.0]])),
        (Ellipse { center: [0.0, 0.0], radius_x: 4.0, radius_y: 2.0, rotation: 30.0 }, json!(["ellipse", [[0.0, 0.0], 4.0, 2.0, 30.0]])),
        (Arc { center: [0.0, 0.0], radius: 5.0, start_deg: 0.0, end_deg: 90.0 }, json!(["arc", [[0.0, 0.0], 5.0, 0.0, 90.0]])),
        (ArcThreePoint { a: [5.0, 0.0], b: [0.0, 5.0], c: [-5.0, 0.0] }, json!(["arc_three_point", [[5.0, 0.0], [0.0, 5.0], [-5.0, 0.0]]])),
        (ArcTangent { prev: 0, end: [20.0, 10.0] }, json!(["arc_tangent", [0, [20.0, 10.0]]])),
        (Rectangle { corner: [0.0, 0.0], size: [20.0, 10.0] }, json!(["rectangle", [[0.0, 0.0], [20.0, 10.0]]])),
        (RectangleCenter { center: [0.0, 0.0], size: [20.0, 10.0] }, json!(["rectangle_center", [[0.0, 0.0], [20.0, 10.0]]])),
        (RectangleThreePoint { a: [0.0, 0.0], b: [10.0, 0.0], c: [10.0, 5.0] }, json!(["rectangle_three_point", [[0.0, 0.0], [10.0, 0.0], [10.0, 5.0]]])),
        (Polygon { center: [0.0, 0.0], radius: 10.0, sides: None, rotation: 0.0 }, json!(["polygon", [[0.0, 0.0], 10.0]])),
        (Polygon { center: [0.0, 0.0], radius: 10.0, sides: None, rotation: 15.0 }, json!(["polygon", [[0.0, 0.0], 10.0], {"rotation": 15.0}])),
        (Polygon { center: [0.0, 0.0], radius: 10.0, sides: Some(5), rotation: 0.0 }, json!(["polygon", [[0.0, 0.0], 10.0, 5]])),
        (Polygon { center: [0.0, 0.0], radius: 10.0, sides: Some(5), rotation: 15.0 }, json!(["polygon", [[0.0, 0.0], 10.0, 5, 15.0]])),
        (Slot { a: [0.0, 0.0], b: [10.0, 0.0], width: 3.0 }, json!(["slot", [[0.0, 0.0], [10.0, 0.0], 3.0]])),
        (Spiral { center: [0.0, 0.0], start_radius: 1.0, end_radius: 5.0, turns: 3.0 }, json!(["spiral", [[0.0, 0.0], 1.0, 5.0, 3.0]])),
        (Text { origin: [0.0, 0.0], text: "Hi".into(), height: 5.0 }, json!(["text", [[0.0, 0.0], "Hi", 5.0]])),
        (Remove { curve: 2 }, json!(["remove", [2]])),
        (Reverse { curve: 1 }, json!(["reverse", [1]])),
        (SplitAt { curve: 0, point: [5.0, 0.0] }, json!(["split_at", [0, [5.0, 0.0]]])),
        (Trim { curve: 0, cutters: vec![1, 2], click: [15.0, 0.0] }, json!(["trim", [0, [1, 2], [15.0, 0.0]]])),
        (Extend { curve: 0, targets: vec![1, 2], both: true }, json!(["extend", [0, [1, 2]]])),
        (Extend { curve: 0, targets: vec![1], both: false }, json!(["extend", [0, [1], false]])),
        (FilletCorner { curve: 0, vertex_index: 1, radius: 2.0 }, json!(["fillet_corner", [0, 1, 2.0]])),
        (Offset { curve: 0, distance: -1.5 }, json!(["offset", [0, -1.5]])),
        (Join { curves: vec![0, 1] }, json!(["join", [[0, 1]]])),
        (Unjoin { curve: 0 }, json!(["unjoin", [0]])),
        (InsertVertex { curve: 0, after: 1, point: [3.0, 4.0] }, json!(["insert_vertex", [0, 1, [3.0, 4.0]]])),
        (RemoveVertex { curve: 0, index: -1 }, json!(["remove_vertex", [0, -1]])),
        (Rebuild { curve: 0, degree: 3, spans: 4 }, json!(["rebuild", [0, 3, 4]])),
    ]
}

#[test]
fn every_call_to_json_and_back() {
    let cases = call_cases();
    let names: BTreeSet<&str> = cases.iter().map(|(c, _)| c.name()).collect();
    assert_eq!(names, SKETCH_CALLS.iter().copied().collect::<BTreeSet<_>>());
    for (call, expected) in &cases {
        assert_eq!(&call.to_json(), expected, "{}", call.name());
        assert_eq!(&SketchCall::from_json(expected).unwrap(), call, "{expected}");
    }
    let body = calls_body(&[cases[0].0.clone(), cases[28].0.clone()]);
    assert_eq!(body.to_string(), r#"{"calls":[["line",[[0.0,0.0],[10.0,0.0]]],["remove",[2]]]}"#);
}

#[test]
fn from_json_reads_keywords_and_defaults() {
    let read = |v: Value| SketchCall::from_json(&v).unwrap();
    assert_eq!(read(json!(["polygon", [[0, 0], 10], {"sides": 5}])), SketchCall::Polygon { center: [0.0, 0.0], radius: 10.0, sides: Some(5), rotation: 0.0 });
    assert_eq!(read(json!(["polygon", [[0, 0], 10, null, 30]])), SketchCall::Polygon { center: [0.0, 0.0], radius: 10.0, sides: None, rotation: 30.0 });
    assert_eq!(read(json!(["line", [], {"a": [0, 0], "b": [1, 2]}])), SketchCall::Line { a: [0.0, 0.0], b: [1.0, 2.0] });
    assert_eq!(read(json!(["text", [[1, 2], "A"]])), SketchCall::Text { origin: [1.0, 2.0], text: "A".into(), height: 10.0 });
    assert_eq!(read(json!(["extend", [0, [1]], {"both": false}])), SketchCall::Extend { curve: 0, targets: vec![1], both: false });
    assert_eq!(read(json!(["circle_tangent", [[0, 1]], {"radius": 3, "near": [1, 1]}])), SketchCall::CircleTangent { curves: vec![0, 1], radius: Some(3.0), near: [1.0, 1.0] });
    // A keyword is re-sent positionally (api.py does not convert keyword values).
    assert_eq!(read(json!(["polygon", [[0, 0], 10], {"sides": 5}])).to_json(), json!(["polygon", [[0.0, 0.0], 10.0, 5]]));
}

#[test]
fn from_json_refusals_name_the_call_and_the_argument() {
    let refuse = |v: Value| SketchCall::from_json(&v).unwrap_err();
    assert_eq!(refuse(json!(["bogus", []])), format!("no sketch call bogus (RoboCAD's: {})", SKETCH_CALLS.join(", ")));
    assert!(refuse(json!(["bogus", []])).starts_with("no sketch call bogus (RoboCAD's: line, polyline, spline,"));
    assert_eq!(refuse(json!("line")), r#"a sketch call is [name, [args…], {kwargs}?] (got "line")"#);
    assert_eq!(refuse(json!([])), "a sketch call is [name, [args…], {kwargs}?] (got [])");
    assert_eq!(refuse(json!([5, []])), "a sketch call starts with its name, a string (got 5)");
    assert_eq!(refuse(json!(["line", {"a": 1}])), r#"line: arguments must be a list [args…] (got {"a":1})"#);
    assert_eq!(refuse(json!(["line", [], 5])), "line: keyword arguments must be an object {name: value} (got 5)");
    assert_eq!(refuse(json!(["line", [[0, 0]]])), "line: missing argument b (takes a, b; 2 required)");
    assert_eq!(refuse(json!(["remove"])), "remove: missing argument curve (takes curve; 1 required)");
    assert_eq!(refuse(json!(["line", [[0, 0], [1, 1], [2, 2]]])), "line: takes at most 2 arguments (a, b), got 3");
    assert_eq!(refuse(json!(["line", [[0, 0], [1]]])), "line: argument b must be a point [u, v] of two finite numbers (got [1])");
    assert_eq!(refuse(json!(["line", [[0, 0], "x"]])), r#"line: argument b must be a point [u, v] of two finite numbers (got "x")"#);
    // Python writes a non-finite float as NaN/Infinity, which reads as null.
    assert_eq!(refuse(json!(["circle", [[0, 0], null]])), "circle: argument radius must be a finite number (got null)");
    assert_eq!(refuse(json!(["circle", [[0, 0], "2"]])), r#"circle: argument radius must be a finite number (got "2")"#);
    assert_eq!(refuse(json!(["polyline", [[[0, 0], [1, "a"]]]])), r#"polyline: argument points[1] must be a point [u, v] of two finite numbers (got [1,"a"])"#);
    assert_eq!(refuse(json!(["polyline", [[[0, 0]], "yes"]])), r#"polyline: argument closed must be true or false (got "yes")"#);
    assert_eq!(refuse(json!(["remove", [-1]])), "remove: argument curve must be a curve index (a non-negative integer) (got -1)");
    assert_eq!(refuse(json!(["remove", [1.5]])), "remove: argument curve must be a curve index (a non-negative integer) (got 1.5)");
    assert_eq!(refuse(json!(["trim", [0, [1, -2], [0, 0]]])), "trim: argument cutters[1] must be a curve index (a non-negative integer) (got -2)");
    assert_eq!(refuse(json!(["join", [3]])), "join: argument curves must be a list of curve indices (got 3)");
    assert_eq!(refuse(json!(["fillet_corner", [0, 1.5, 2]])), "fillet_corner: argument vertex_index must be an integer (got 1.5)");
    assert_eq!(refuse(json!(["polygon", [[0, 0], 10], {"sids": 5}])), "polygon: no argument sids (takes center, radius, sides, rotation)");
    assert_eq!(refuse(json!(["polygon", [[0, 0], 10, 5], {"sides": 6}])), "polygon: argument sides given twice (by position and by name)");
    assert_eq!(refuse(json!(["polygon", [[0, 0], 10, 2]])), "polygon: argument sides must be at least 3 (got 2)");
    assert_eq!(refuse(json!(["polygon", [[0, 0], 10, -6]])), "polygon: argument sides must be a non-negative integer (got -6)");
    assert_eq!(refuse(json!(["spiral", [[0, 0], 1, 5, 3], {"points_per_turn": 10}])), "spiral: no argument points_per_turn (takes center, start_radius, end_radius, turns)");
    assert_eq!(refuse(json!(["text", [[0, 0], "A", 5], {"font": "Arial"}])), "text: no argument font (takes origin, text, height)");
    assert_eq!(refuse(json!(["join", [[]]])), "join: argument curves must name at least one curve");
    assert_eq!(refuse(json!(["join", [[1, 1]]])), "join: argument curves[1] names curve 1 twice");
    assert_eq!(refuse(json!(["circle_tangent", [[0, 1]]])), "circle_tangent: argument radius is needed for fewer than three curves (got 2 curves)");
    assert_eq!(refuse(json!(["rebuild", [0, 0, 4]])), "rebuild: argument degree must be at least 1 (got 0)");
    assert_eq!(refuse(json!(["rebuild", [0, 3, 0]])), "rebuild: argument spans must be at least 1 (got 0)");
}

#[test]
fn check_calls_tracks_curve_indices() {
    use SketchCall::*;
    let line = |y: f64| Line { a: [0.0, y], b: [10.0, y] };
    // Two lines, joined into one: index 1 no longer exists.
    let calls = [line(0.0), line(1.0), Join { curves: vec![0, 1] }, Trim { curve: 0, cutters: vec![1], click: [5.0, 0.0] }];
    assert_eq!(check_calls(&calls, Some(0)).unwrap_err(), "call 4 (trim): argument cutters[0] is curve 1, but the sketch has 1 curve then");
    assert!(check_calls(&calls[..3], Some(0)).is_ok());
    // Unknown counts check nothing past them.
    assert!(check_calls(&calls, None).is_ok());
    assert!(check_calls(&[Text { origin: [0.0, 0.0], text: "A".into(), height: 5.0 }, Remove { curve: 99 }], Some(0)).is_ok());
    assert!(check_calls(&[Trim { curve: 0, cutters: vec![1], click: [0.0, 0.0] }, Remove { curve: 99 }], Some(2)).is_ok());
    assert_eq!(check_calls(&[Remove { curve: 3 }], Some(3)).unwrap_err(), "call 1 (remove): argument curve is curve 3, but the sketch has 3 curves then");
    assert_eq!(check_calls(&[line(0.0), ArcTangent { prev: 1, end: [1.0, 1.0] }], Some(0)).unwrap_err(), "call 2 (arc_tangent): argument prev is curve 1, but the sketch has 1 curve then");
    assert_eq!(check_calls(&[line(0.0), Join { curves: vec![] }], Some(0)).unwrap_err(), "call 2 (join): argument curves must name at least one curve");
    // split_at and offset add a curve; remove takes one.
    assert!(check_calls(&[line(0.0), SplitAt { curve: 0, point: [5.0, 0.0] }, Offset { curve: 1, distance: 1.0 }, Remove { curve: 2 }, Reverse { curve: 1 }], Some(0)).is_ok());
    assert!(check_calls(&[line(0.0), Remove { curve: 0 }, Reverse { curve: 0 }], Some(0)).is_err());
    // curves_after, per kernel/sketch.py.
    assert_eq!(Circle { center: [0.0, 0.0], radius: 1.0 }.curves_after(2), Some(3));
    assert_eq!(Join { curves: vec![0, 2, 1] }.curves_after(4), Some(2));
    assert_eq!(Join { curves: vec![0, 0] }.curves_after(4), None);
    assert_eq!(Unjoin { curve: 0 }.curves_after(4), None);
    assert_eq!(Rebuild { curve: 0, degree: 3, spans: 4 }.curves_after(4), Some(4));
    assert_eq!(Extend { curve: 0, targets: vec![1], both: true }.curves_after(4), Some(4));
}

#[test]
fn plane_frames_as_kernel_base_plane() {
    assert_eq!(PlaneFrame::XY.y_axis(), [0.0, 1.0, 0.0]);
    // Plane.xz: normal -Y, x +X, so y = (-Y) × X = +Z.
    assert_eq!(PlaneFrame::XZ.y_axis(), [0.0, 0.0, 1.0]);
    assert_eq!(PlaneFrame::YZ.y_axis(), [0.0, 0.0, 1.0]);
    let xz = PlaneFrame { origin: [0.0, 5.0, 0.0], ..PlaneFrame::XZ };
    assert_eq!(xz.to_world(2.0, 3.0, 1.0), [2.0, 4.0, 3.0]);
    assert_eq!(xz.to_local([2.0, 4.0, 3.0]), [2.0, 3.0, 1.0]);
    assert_eq!(xz.project([2.0, 4.0, 3.0]), [2.0, 5.0, 3.0]);
    assert_eq!(PlaneFrame::YZ.to_world(1.0, 2.0, 0.0), [0.0, 1.0, 2.0]);
    assert_eq!(PlaneFrame::XY.to_local([3.0, 4.0, 5.0]), [3.0, 4.0, 5.0]);
    assert_eq!(PlaneFrame::from_value(&PlaneFrame::XZ.json()), Some(PlaneFrame::XZ));
    assert_eq!(PlaneFrame::from_value(&json!({"origin": [0, 0, 0], "normal": [0, 0, 1]})), None);
    assert_eq!(PlaneFrame::from_value(&json!({"origin": [0, 0], "normal": [0, 0, 1], "x_axis": [1, 0, 0]})), None);
    assert_eq!(PlaneFrame::from_value(&json!({"origin": [0, 0, null], "normal": [0, 0, 1], "x_axis": [1, 0, 0]})), None);
    assert_eq!(PlaneFrame::from_value(&json!({"origin": [0, 0, 0], "normal": [0, 0, 1], "x_axis": [0, 0, 0]})), None);
    assert_eq!(PlaneFrame::from_value(&json!("xy")), None);
    let nudged = PlaneFrame { origin: [1e-12, 0.0, 0.0], ..PlaneFrame::XY };
    assert!(PlaneFrame::XY.same(&nudged, 1e-9));
    assert!(!PlaneFrame::XY.same(&PlaneFrame::XZ, 1e-9));
}

#[test]
fn sample_counts_as_the_viewport_draws() {
    let curve = |kind: &str, points: Vec<[f64; 2]>| SketchCurve { kind: kind.into(), points, ..SketchCurve::default() };
    assert_eq!(curve("line", vec![[0.0, 0.0], [1.0, 0.0]]).sample(48).len(), 2);
    let square = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let closed = SketchCurve { closed: true, ..curve("polyline", square.clone()) };
    let s = closed.sample(48);
    assert_eq!((s.len(), s[4]), (5, [0.0, 0.0]));
    assert_eq!(curve("polyline", square.clone()).sample(48).len(), 4);
    let circle = SketchCurve { center: Some([10.0, 5.0]), radius: 2.0, ..curve("circle", vec![]) };
    let s = circle.sample(48);
    assert_eq!(s.len(), 49);
    assert!(close(s[0], [12.0, 5.0]) && close(s[48], [12.0, 5.0]) && close(s[12], [10.0, 7.0]));
    let arc = SketchCurve { center: Some([0.0, 0.0]), radius: 5.0, start_angle: 0.0, end_angle: 90.0, ..curve("arc", vec![]) };
    let s = arc.sample(8);
    assert_eq!(s.len(), 9);
    assert!(close(s[8], arc.end().unwrap()));
    let ellipse = SketchCurve { center: Some([0.0, 0.0]), radius: 4.0, radius2: 2.0, rotation: 90.0, ..curve("ellipse", vec![]) };
    assert!(close(ellipse.sample(4)[0], [0.0, 4.0]));
    // _slot_points' layout, 17 points round each cap, closed as the viewport
    // closes it; the caps bulge outward as the kernel's slot (RoboCAD's
    // _slot_points turns them inward: (8.5, 0) where s[8] is).
    let slot = SketchCurve { radius: 1.5, closed: true, ..curve("slot", vec![[0.0, 0.0], [10.0, 0.0]]) };
    let s = slot.sample(48);
    assert_eq!(s.len(), 35);
    assert_eq!(s[0], s[34]);
    assert!(close(s[0], [10.0, 1.5]) && close(s[8], [11.5, 0.0]) && close(s[17], [0.0, -1.5]) && close(s[25], [-1.5, 0.0]));
    // Catmull-Rom display sampling: n // spans + 1 points per span, then the last.
    let spline = curve("spline", vec![[0.0, 0.0], [5.0, 5.0], [10.0, 0.0]]);
    let s = spline.sample(48);
    assert_eq!(s.len(), 2 * 25 + 1);
    assert!(close(s[0], [0.0, 0.0]) && close(s[25], [5.0, 5.0]) && close(s[50], [10.0, 0.0]));
    let s = SketchCurve { closed: true, ..spline.clone() }.sample(48);
    assert_eq!(s.len(), 3 * 17 + 1);
    // de Boor on clamped knots: n + 1 points from the first control point to the last.
    let control = curve("control", vec![[0.0, 0.0], [1.0, 2.0], [3.0, 2.0], [4.0, 0.0]]);
    let s = control.sample(10);
    assert_eq!(s.len(), 11);
    assert!(close(s[0], [0.0, 0.0]) && close(s[10], [4.0, 0.0]));
    // A uniform cubic Bezier (4 points, degree 3) at t = 0.5: (P0 + 3 P1 + 3 P2 + P3) / 8.
    assert!(close(s[5], [2.0, 1.5]));
    assert_eq!(SketchCurve { text: "A".into(), ..curve("text", vec![[1.0, 1.0]]) }.sample(48), vec![[1.0, 1.0]]);
    // Where Python would raise: the stored points.
    assert!(curve("circle", vec![]).sample(48).is_empty());
    assert_eq!(curve("circle", vec![]).start(), None);
}

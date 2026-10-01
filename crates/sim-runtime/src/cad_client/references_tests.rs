//! The reference image calls against the fake RoboCAD of `tests.rs`:
//! `POST /ops/{import_references, update_reference, calibrate_reference}`
//! as api.py's `op` route takes them, the placement as `node_detail`
//! writes it (no bytes; the plane as `Plane.to_json`), the
//! `GET /nodes/{id}/image` gap route's answer (base64, as
//! `api.py reference_image` writes it) and its decoding.
use super::tests::{Answer, assert_request, ok, serve};
use super::*;
use serde_json::json;

fn op_answer(result: &str, undo: &str) -> Answer {
    ok(&format!(r#"{{"result": {result}, "history": {{"undo": ["{undo}"], "redo": []}}}}"#))
}

/// `node_detail` of a reference image on the front plane (XZ: normal −Y).
const IMAGE_NODE: &str = r#"{"id": "r1", "kind": "image", "name": "front.png", "parent": null, "children": [], "visible": true, "locked": true, "disabled": false, "material": null, "color": null, "pivot": null, "source": null, "transform": {"translation": [0, 0, 0], "axis": [0, 0, 1], "angle_deg": 0, "scale": 1}, "effective_visible": true, "component_instance": null, "component_member": null, "image": {"path": "/tmp/front.png", "plane": {"origin": [-10.0, 0.0, 0.0], "normal": [0.0, -1.0, 0.0], "x_axis": [1.0, 0.0, 0.0]}, "width": 200.0, "height": 100.0, "opacity": 0.6, "rotation_deg": 0.0}}"#;

#[test]
fn reference_ops_send_only_the_given_keys() {
    let (c, server) = serve(vec![op_answer(r#"["r1", "r2"]"#, "Import references"), op_answer(r#""r1""#, "Edit reference"), op_answer(r#""r1""#, "Edit reference"), op_answer(r#""r1""#, "Edit reference")]);
    let port = c.endpoint.port;
    let paths = vec!["/tmp/front.png".to_string(), "/tmp/side.png".to_string()];
    assert_eq!(c.import_references(&paths, Some(&json!("xz"))).unwrap().result, json!(["r1", "r2"]));
    let update = ReferenceUpdate { visible: Some(false), ..ReferenceUpdate::default() };
    c.update_reference("r1", &update).unwrap();
    let placement = ReferenceUpdate { width: Some(80.0), opacity: Some(0.25), origin: Some([1.0, 2.0, 3.0]), plane: Some(json!("yz")), rotation_deg: Some(90.0), locked: Some(false), ..ReferenceUpdate::default() };
    c.update_reference("r1", &placement).unwrap();
    c.calibrate_reference("r1", [10.0, 0.0, 0.0], [30.0, 0.0, 0.0], 40.0).unwrap();
    let seen = server.join().unwrap();
    assert_request(&seen[0], "POST /ops/import_references HTTP/1.1", port, Some(r#"{"args":[["/tmp/front.png","/tmp/side.png"],"xz"],"kwargs":{}}"#));
    assert_request(&seen[1], "POST /ops/update_reference HTTP/1.1", port, Some(r#"{"args":["r1"],"kwargs":{"visible":false}}"#));
    assert_request(&seen[2], "POST /ops/update_reference HTTP/1.1", port, Some(r#"{"args":["r1"],"kwargs":{"locked":false,"opacity":0.25,"origin":[1.0,2.0,3.0],"plane":"yz","rotation_deg":90.0,"width":80.0}}"#));
    assert_request(&seen[3], "POST /ops/calibrate_reference HTTP/1.1", port, Some(r#"{"args":["r1",[10.0,0.0,0.0],[30.0,0.0,0.0],40.0],"kwargs":{}}"#));
}

#[test]
fn an_import_without_a_plane_sends_null() {
    let (c, server) = serve(vec![op_answer(r#"["r1"]"#, "Import references")]);
    let port = c.endpoint.port;
    c.import_references(&["/tmp/a.png".to_string()], None).unwrap();
    let seen = server.join().unwrap();
    assert_request(&seen[0], "POST /ops/import_references HTTP/1.1", port, Some(r#"{"args":[["/tmp/a.png"],null],"kwargs":{}}"#));
}

#[test]
fn placement_reads_from_the_node_and_places_the_corners() {
    let detail: NodeDetail = serde_json::from_str(IMAGE_NODE).unwrap();
    let p = ImagePlacement::of(&detail).unwrap();
    assert_eq!((p.width, p.height, p.opacity, p.rotation_deg), (200.0, 100.0, 0.6, 0.0));
    // XZ: y axis = normal × x = (0,-1,0) × (1,0,0) = (0,0,1).
    assert_eq!(p.plane.y_axis(), [0.0, 0.0, 1.0]);
    assert_eq!(p.corners(), [[-10.0, 0.0, 0.0], [190.0, 0.0, 0.0], [190.0, 0.0, 100.0], [-10.0, 0.0, 100.0]]);
    let mut other = detail.clone();
    other.image = None;
    assert_eq!(ImagePlacement::of(&other), None);
    other.image = Some(json!({"plane": "bad", "width": 50.0}));
    let q = ImagePlacement::of(&other).unwrap();
    assert_eq!((q.plane, q.width, q.opacity), (PlaneJson::default(), 50.0, 0.6), "a malformed plane reads as XY; missing keys take import's defaults");
}

#[test]
fn the_image_route_decodes_base64() {
    // The 3 bytes "PNG" and 4 bytes; Python's b64encode.
    let answer = r#"{"id": "r1", "revision": 7, "format": "png", "width_px": 40, "height_px": 20, "bytes": 4, "data": "iVBORw=="}"#;
    let wrong = r#"{"id": "r1", "revision": 7, "format": "png", "width_px": 1, "height_px": 1, "bytes": 9, "data": "iVBORw=="}"#;
    let (c, server) = serve(vec![ok(answer), ok(wrong), Answer::Json(404, r#"{"error": "Box is not a reference image"}"#.into())]);
    let port = c.endpoint.port;
    let image = c.reference_image("r1").unwrap();
    assert_eq!((image.revision, image.format.as_str(), image.width_px, image.height_px), (7, "png", 40, 20));
    assert_eq!(image.decode().unwrap(), [0x89, b'P', b'N', b'G']);
    assert!(c.reference_image("r1").unwrap().decode().unwrap_err().contains("4 bytes decoded, RoboCAD said 9"));
    let err = c.reference_image("b1").unwrap_err();
    assert_eq!((err.status, err.message.as_str()), (Some(404), "Box is not a reference image"));
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /nodes/r1/image HTTP/1.1", port, None);
    assert_request(&seen[2], "GET /nodes/b1/image HTTP/1.1", port, None);
}

#[test]
fn base64_matches_pythons_encoder() {
    assert_eq!(base64_decode("").unwrap(), b"");
    assert_eq!(base64_decode("TQ==").unwrap(), b"M");
    assert_eq!(base64_decode("TWE=").unwrap(), b"Ma");
    assert_eq!(base64_decode("TWFu").unwrap(), b"Man");
    assert_eq!(base64_decode("TWFu\nTWE=").unwrap(), b"ManMa", "whitespace is ignored");
    assert_eq!(base64_decode("+/8=").unwrap(), [0xfb, 0xff]);
    assert!(base64_decode("TWF").is_err());
    assert!(base64_decode("TQ==TWFu").is_err(), "padding mid-stream");
    assert!(base64_decode("T*==").is_err());
}

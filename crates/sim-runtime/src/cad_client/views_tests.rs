//! The saved-views client against the in-process fake RoboCAD of
//! `tests.rs`: answers as `api.py` `saved_view_request` writes them
//! (json.dumps' separators), the exact request lines and bodies, the
//! refusals with RoboCAD's text, a tolerant read, and the schema's keys.
use super::tests::{Answer, assert_request, ok, serve};
use super::*;
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// A state as RoboCAD's `validate_state` answers it (json.dumps).
const STATE: &str = r#"{"target": [10.0, -5.0, 2.5], "distance": 300.0, "yaw": -35.0, "pitch": 28.0, "fov": 40.0, "orthographic": true, "mode": "turntable", "rot": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], "grid": false, "comment_pins": true, "display_mode": "wireframe", "section": {"enabled": true, "plane": {"origin": [0.0, 0.0, 5.0], "normal": [0.0, 0.0, 1.0], "x_axis": [1.0, 0.0, 0.0]}}}"#;

/// The same state as the client writes it (serde_json, struct order).
const STATE_SENT: &str = r#"{"target":[10.0,-5.0,2.5],"distance":300.0,"yaw":-35.0,"pitch":28.0,"fov":40.0,"orthographic":true,"mode":"turntable","rot":[[1.0,0.0,0.0],[0.0,1.0,0.0],[0.0,0.0,1.0]],"grid":false,"display_mode":"wireframe","comment_pins":true,"section":{"enabled":true,"plane":{"origin":[0.0,0.0,5.0],"normal":[0.0,0.0,1.0],"x_axis":[1.0,0.0,0.0]}}}"#;

fn state() -> ViewState {
    ViewState {
        target: [10.0, -5.0, 2.5],
        distance: 300.0,
        orthographic: true,
        grid: false,
        display_mode: "wireframe".into(),
        section: ViewSection { enabled: true, plane: Some(ViewPlane { origin: [0.0, 0.0, 5.0], normal: [0.0, 0.0, 1.0], x_axis: [1.0, 0.0, 0.0] }) },
        ..ViewState::default()
    }
}

fn view_json(id: &str, name: &str) -> String {
    format!(r#"{{"id": "{id}", "name": "{name}", "state": {STATE}}}"#)
}

fn keys(v: &Value) -> BTreeSet<String> {
    v.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default()
}

#[test]
fn list_reads_every_view_and_drops_a_malformed_one() {
    let (c, server) = serve(vec![ok(&format!("[{}, {}, \"not a view\"]", view_json("a1b2c3d4e5f6", "Worm drive cutaway"), view_json("0f0f0f0f0f0f", "Top")))]);
    let port = c.endpoint.port;
    let views = c.views().unwrap();
    assert_eq!(views.len(), 2, "the string is dropped, not the answer");
    assert_eq!((views[0].id.as_str(), views[0].name.as_str()), ("a1b2c3d4e5f6", "Worm drive cutaway"));
    assert_eq!(views[0].state, state());
    assert_eq!(views[1].name, "Top");
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /views HTTP/1.1", port, None);
}

#[test]
fn save_sends_exactly_name_and_the_schema_state() {
    let (c, server) = serve(vec![Answer::Json(201, view_json("a1b2c3d4e5f6", "Cutaway"))]);
    let port = c.endpoint.port;
    let saved = c.save_view("Cutaway", &state()).unwrap();
    assert_eq!((saved.id.as_str(), saved.name.as_str()), ("a1b2c3d4e5f6", "Cutaway"));
    let seen = server.join().unwrap();
    assert_request(&seen[0], "POST /views HTTP/1.1", port, Some(&format!(r#"{{"name":"Cutaway","state":{STATE_SENT}}}"#)));
}

#[test]
fn rename_and_replace_patch_only_the_given_key() {
    let (c, server) = serve(vec![ok(&view_json("a1b2c3d4e5f6", "Renamed")), ok(&view_json("a1b2c3d4e5f6", "Renamed"))]);
    let port = c.endpoint.port;
    assert_eq!(c.update_view("a1b2c3d4e5f6", Some("Renamed"), None).unwrap().name, "Renamed");
    assert_eq!(c.update_view("a1b2c3d4e5f6", None, Some(&state())).unwrap().state, state());
    // Neither: refused here, nothing sent (RoboCAD would answer 400).
    let e = c.update_view("a1b2c3d4e5f6", None, None).unwrap_err();
    assert_eq!((e.method, e.status), ("PATCH", None));
    let seen = server.join().unwrap();
    assert_eq!(seen.len(), 2);
    assert_request(&seen[0], "PATCH /views/a1b2c3d4e5f6 HTTP/1.1", port, Some(r#"{"name":"Renamed"}"#));
    assert_request(&seen[1], "PATCH /views/a1b2c3d4e5f6 HTTP/1.1", port, Some(&format!(r#"{{"state":{STATE_SENT}}}"#)));
}

#[test]
fn get_and_delete_one_view_by_its_encoded_id() {
    let (c, server) = serve(vec![ok(&view_json("a1b2c3d4e5f6", "Top")), ok(r#"{"deleted": "a1b2c3d4e5f6"}"#), ok("{}")]);
    let port = c.endpoint.port;
    assert_eq!(c.view("a1b2c3d4e5f6").unwrap().name, "Top");
    assert_eq!(c.delete_view("a1b2c3d4e5f6").unwrap(), json!({"deleted": "a1b2c3d4e5f6"}));
    c.view("a b/c").unwrap();
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /views/a1b2c3d4e5f6 HTTP/1.1", port, None);
    assert_request(&seen[1], "DELETE /views/a1b2c3d4e5f6 HTTP/1.1", port, None);
    assert_request(&seen[2], "GET /views/a%20b%2Fc HTTP/1.1", port, None);
}

#[test]
fn refusals_carry_robocads_text_and_status() {
    let (c, server) = serve(vec![
        Answer::Json(404, r#"{"error": "Saved view not found"}"#.into()),
        Answer::Json(409, r#"{"error": "Headless: provide view state"}"#.into()),
        Answer::Json(422, r#"{"error": "View pitch must be between -89.5 and 89.5 degrees"}"#.into()),
        Answer::Json(422, r#"{"error": "View name must contain 1–120 characters"}"#.into()),
    ]);
    let e = c.delete_view("zz").unwrap_err();
    assert!(e.not_found(), "{e:?}");
    assert_eq!(e.to_string(), "RoboCAD DELETE /views/zz: Saved view not found (HTTP 404)");
    let e = c.save_view("Top", &ViewState::default()).unwrap_err();
    assert!(e.no_gui(), "{e:?}");
    assert_eq!(e.message, "Headless: provide view state");
    let e = c.update_view("a1", None, Some(&ViewState::default())).unwrap_err();
    assert_eq!((e.status, e.message.as_str()), (Some(422), "View pitch must be between -89.5 and 89.5 degrees"));
    let e = c.update_view("a1", Some(" "), None).unwrap_err();
    assert_eq!(e.status, Some(422));
    assert_eq!(e.message, "View name must contain 1–120 characters", "RoboCAD's text verbatim (UTF-8 kept)");
    server.join().unwrap();
}

#[test]
fn a_view_reads_tolerantly_with_robocads_defaults() {
    // An extra field (a later RoboCAD's), and only the target, pitch and mode given.
    let v: SavedView = serde_json::from_str(r#"{"id": "a1", "name": "Old", "thumbnail": "x.png", "state": {"target": [1, 2, 3], "pitch": 10, "mode": "trackball", "future": true, "section": {"enabled": false}}}"#).unwrap();
    assert_eq!(v.state.target, [1.0, 2.0, 3.0]);
    assert_eq!(v.state.pitch, 10.0);
    assert_eq!(v.state.mode, "trackball");
    let d = ViewState::default();
    assert_eq!((v.state.distance, v.state.yaw, v.state.fov, v.state.orthographic), (d.distance, d.yaw, d.fov, d.orthographic));
    assert_eq!((v.state.distance, v.state.yaw, v.state.fov), (250.0, -35.0, 40.0), "saved_views.py's defaults");
    assert_eq!(v.state.rot, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    assert!(v.state.grid && v.state.comment_pins);
    assert_eq!(v.state.display_mode, "shaded_edges");
    assert_eq!(v.state.section, ViewSection { enabled: false, plane: None });
    // Without a state at all: validate_state({}).
    let bare: SavedView = serde_json::from_str(r#"{"id": "b", "name": "B"}"#).unwrap();
    assert_eq!(bare.state, ViewState::default());
    assert_eq!(ViewState::default().check(), Ok(()));
}

/// What the client writes is exactly `validate_state`'s key set (it
/// refuses any other key), nested keys included, and reads back equal.
#[test]
fn the_state_round_trips_with_exactly_the_schemas_keys() {
    for s in [state(), ViewState::default()] {
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(keys(&v), VIEW_STATE_KEYS.iter().map(|k| k.to_string()).collect::<BTreeSet<_>>());
        assert_eq!(keys(&v["section"]), ["enabled", "plane"].map(String::from).into_iter().collect());
        if !v["section"]["plane"].is_null() {
            assert_eq!(keys(&v["section"]["plane"]), ["normal", "origin", "x_axis"].map(String::from).into_iter().collect());
        }
        assert_eq!(serde_json::from_value::<ViewState>(v).unwrap(), s);
    }
    // RoboCAD's own answer (another key order) reads as the same state.
    assert_eq!(serde_json::from_str::<ViewState>(STATE).unwrap(), state());
    assert_eq!(serde_json::to_string(&state()).unwrap(), STATE_SENT);
}

/// `check` refuses what `validate_state` refuses, with its messages.
#[test]
fn check_mirrors_validate_state() {
    let bad = |f: &dyn Fn(&mut ViewState)| {
        let mut s = ViewState::default();
        f(&mut s);
        s.check().unwrap_err()
    };
    assert_eq!(bad(&|s| s.pitch = 89.6), "View pitch must be between -89.5 and 89.5 degrees");
    assert_eq!(bad(&|s| s.distance = 0.0), "camera distance or field of view is out of range");
    assert_eq!(bad(&|s| s.fov = 171.0), "camera distance or field of view is out of range");
    assert_eq!(bad(&|s| s.mode = "fly".into()), "invalid camera mode");
    assert_eq!(bad(&|s| s.rot = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]]), "camera rotation must be an orthonormal 3 by 3 matrix", "a reflection");
    assert_eq!(bad(&|s| s.display_mode = "toon".into()), "Unknown display mode");
    assert_eq!(bad(&|s| s.section.enabled = true), "An enabled section needs a plane");
    assert_eq!(bad(&|s| s.section.plane = Some(ViewPlane { origin: [0.0; 3], normal: [0.0, 0.0, 1.0], x_axis: [0.0, 1.0, 1.0] })), "Section axes must be perpendicular");
    assert_eq!(bad(&|s| s.section.plane = Some(ViewPlane { origin: [0.0; 3], normal: [0.0; 3], x_axis: [1.0, 0.0, 0.0] })), "Section axes must be nonzero");
    assert_eq!(bad(&|s| s.target = [f64::NAN, 0.0, 0.0]), "camera target needs three finite coordinates");
    // Unnormalised but perpendicular axes are RoboCAD's to normalise.
    let mut s = ViewState::default();
    s.section.plane = Some(ViewPlane { origin: [0.0; 3], normal: [0.0, 0.0, 2.0], x_axis: [3.0, 0.0, 0.0] });
    s.pitch = -89.5;
    assert_eq!(s.check(), Ok(()));
    assert_eq!(check_view_name("  Worm drive  "), Ok("Worm drive".to_string()));
    assert_eq!(check_view_name("   "), Err("View name must contain 1–120 characters".to_string()));
    assert!(check_view_name(&"é".repeat(120)).is_ok() && check_view_name(&"é".repeat(121)).is_err(), "characters, not bytes");
}

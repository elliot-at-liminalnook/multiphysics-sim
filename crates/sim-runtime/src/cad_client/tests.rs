//! The client against an in-process fake RoboCAD (std only; no Python is
//! started): the exact request line, headers and body bytes of every
//! route, answers shaped as api.py writes them (nulls, unknown and missing
//! fields), error text, loopback refusal, and the service helpers.
use super::service::{self, START_TIMEOUT};
use super::*;
use serde_json::json;
use std::io::{ErrorKind, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::Instant;

/// One request as the fake server received it.
#[derive(Debug)]
struct Seen {
    head: String,
    body: String,
}
impl Seen {
    fn request_line(&self) -> &str {
        self.head.lines().next().unwrap_or("")
    }
    /// The header lines after the request line, in order.
    fn headers(&self) -> Vec<&str> {
        self.head.lines().skip(1).filter(|l| !l.is_empty()).collect()
    }
    fn header(&self, name: &str) -> Option<&str> {
        self.head.lines().skip(1).filter_map(|l| l.split_once(':')).find(|(k, _)| k.trim().eq_ignore_ascii_case(name)).map(|(_, v)| v.trim())
    }
}

/// What the fake server does with one connection.
enum Answer {
    /// Reads the request and closes without answering.
    Close,
    /// Reads the request, holds the connection this long without
    /// answering, then closes.
    Silent(Duration),
    /// Reads the request and answers `(status, body)` as Python's
    /// `BaseHTTPRequestHandler` does (HTTP/1.0, Content-Length, then close).
    Json(u16, String),
}

fn ok(body: &str) -> Answer {
    Answer::Json(200, body.to_string())
}

/// Accepts one connection per answer and records each request.
fn serve(answers: Vec<Answer>) -> (CadClient, JoinHandle<Vec<Seen>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let handle = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for answer in answers {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == ErrorKind::WouldBlock && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(2)),
                    Err(e) => panic!("fake RoboCAD: no connection: {e}"),
                }
            };
            // Accepted sockets may inherit the listener's non-blocking mode.
            stream.set_nonblocking(false).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut data = Vec::new();
            let mut chunk = [0u8; 4096];
            let end = loop {
                if let Some(p) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                    break p + 4;
                }
                let n = stream.read(&mut chunk).unwrap();
                assert!(n > 0, "request ended before its headers");
                data.extend_from_slice(&chunk[..n]);
            };
            let head = String::from_utf8(data[..end].to_vec()).unwrap();
            let mut probe = Seen { head, body: String::new() };
            let length: usize = probe.header("content-length").map_or(0, |v| v.parse().unwrap());
            while data.len() < end + length {
                let n = stream.read(&mut chunk).unwrap();
                assert!(n > 0, "request ended before its body");
                data.extend_from_slice(&chunk[..n]);
            }
            probe.body = String::from_utf8(data[end..end + length].to_vec()).unwrap();
            if let Answer::Silent(hold) = answer {
                std::thread::sleep(hold);
            } else if let Answer::Json(status, body) = answer {
                let reason = if status == 200 { "OK" } else { "Error" };
                write!(
                    stream,
                    "HTTP/1.0 {status} {reason}\r\nServer: robocad/0.1 Python/3.12\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
            seen.push(probe);
        }
        seen
    });
    let client = CadClient::new(&format!("http://127.0.0.1:{port}")).unwrap().with_timeout(Duration::from_secs(5));
    (client, handle)
}

/// The headers of a request without a body.
fn bare(port: u16) -> Vec<String> {
    vec![format!("Host: 127.0.0.1:{port}"), "Connection: close".into()]
}

/// The headers of a request with `body`.
fn with_body(port: u16, body: &str) -> Vec<String> {
    vec![format!("Host: 127.0.0.1:{port}"), "Content-Type: application/json".into(), format!("Content-Length: {}", body.len()), "Connection: close".into()]
}

fn assert_request(seen: &Seen, line: &str, port: u16, body: Option<&str>) {
    assert_eq!(seen.request_line(), line);
    let expected = match body {
        Some(body) => with_body(port, body),
        None => bare(port),
    };
    assert_eq!(seen.headers(), expected, "{line}");
    assert_eq!(seen.body, body.unwrap_or(""), "{line}");
}

/// A port with no listener (bound, read, released).
fn dead_port() -> u16 {
    service::free_port().unwrap()
}

const TRANSFORM: &str = r#"{"translation": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0], "angle_deg": 0.0, "scale": 1.0}"#;

/// A body as `node_summary` writes it: every field, nulls where Python has `None`.
fn body_summary() -> String {
    format!(
        r#"{{"id": "a1b2c3d4e5f6", "kind": "body", "name": "Femur", "parent": "0123456789ab", "children": [], "visible": true, "locked": false, "disabled": false, "material": "pla", "color": null, "pivot": null, "source": null, "transform": {TRANSFORM}, "effective_visible": true, "component_instance": null, "component_member": null}}"#
    )
}

/// A group summary with a colour and pivot, an unknown field, and the
/// component fields missing.
fn group_summary() -> String {
    format!(
        r#"{{"id": "0123456789ab", "kind": "group", "name": "Leg", "parent": null, "children": ["a1b2c3d4e5f6"], "visible": true, "locked": true, "disabled": false, "material": null, "color": [0.8, 0.2, 0.2], "pivot": [1, 2.5, 3], "source": null, "transform": {TRANSFORM}, "effective_visible": false, "future_field": {{"x": 1}}}}"#
    )
}

#[test]
fn health_parses_with_nulls_and_unknown_fields() {
    let (c, server) = serve(vec![ok(r#"{"ok": true, "app": "robocad", "version": "0.4.0", "path": null, "dirty": false, "gui": false, "nodes": 12, "document_id": "f00d", "revision": 7, "later": [1]}"#)]);
    let port = c.endpoint.port;
    let health = c.health().unwrap();
    assert_eq!(health, Health { ok: true, app: "robocad".into(), version: "0.4.0".into(), path: None, dirty: false, gui: false, nodes: 12, document_id: Some("f00d".into()), revision: 7 });
    assert_eq!(c.url(), format!("http://127.0.0.1:{port}"));
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET / HTTP/1.1", port, None);
}

#[test]
fn doc_state_as_api_writes_it() {
    let doc = format!(
        r#"{{"path": "/models/leg.rcad", "dirty": true, "roots": ["0123456789ab"], "active_group": null, "nodes": [{}, {}, {{"id": 5, "kind": "body"}}], "materials": [{{"id": "pla", "name": "PLA", "density": 1.24, "color": [0.85, 0.85, 0.87], "roughness": 0.6, "metallic": 0.0, "tags": ["print", "plastic"], "engineering": {{}}}}], "selection": {{"items": []}}, "view": {{}}, "history": {{"undo": ["Box", "Move"], "redo": []}}, "document_id": "d0c", "revision": 41}}"#,
        group_summary(),
        body_summary()
    );
    let (c, server) = serve(vec![ok(&doc)]);
    let port = c.endpoint.port;
    let state = c.doc().unwrap();
    assert_eq!(state.path.as_deref(), Some("/models/leg.rcad"));
    assert!(state.dirty);
    assert_eq!(state.roots, vec!["0123456789ab".to_string()]);
    assert_eq!(state.active_group, None);
    assert_eq!(state.nodes.len(), 2);
    let group = &state.nodes[0];
    assert_eq!((group.kind.as_str(), group.name.as_str(), group.parent.as_deref(), group.locked), ("group", "Leg", None, true));
    assert_eq!(group.color, Some(vec![0.8, 0.2, 0.2]));
    assert_eq!(group.pivot, Some(vec![1.0, 2.5, 3.0]));
    assert_eq!((group.component_instance.clone(), group.effective_visible), (None, false));
    let body = &state.nodes[1];
    assert_eq!((body.material.as_deref(), body.color.clone(), body.pivot.clone(), body.source.clone()), (Some("pla"), None, None, None));
    assert_eq!(body.transform["scale"], json!(1.0));
    assert_eq!(state.materials[0]["density"], json!(1.24));
    assert_eq!(state.selection, Selection::default());
    assert_eq!(state.view, json!({}));
    assert_eq!(state.history, History { undo: vec!["Box".into(), "Move".into()], redo: vec![] });
    assert_eq!((state.document_id.as_deref(), state.revision), (Some("d0c"), 41));
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /doc HTTP/1.1", port, None);
}

#[test]
fn nodes_with_and_without_kind() {
    let list = format!("[{}]", body_summary());
    let (c, server) = serve(vec![ok(&list), ok(&list), ok("[]")]);
    let port = c.endpoint.port;
    assert_eq!(c.nodes(None).unwrap()[0].id, "a1b2c3d4e5f6");
    assert_eq!(c.nodes(Some("body")).unwrap().len(), 1);
    assert!(c.nodes(Some("a b&c")).unwrap().is_empty());
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /nodes HTTP/1.1", port, None);
    assert_request(&seen[1], "GET /nodes?kind=body HTTP/1.1", port, None);
    assert_request(&seen[2], "GET /nodes?kind=a%20b%26c HTTP/1.1", port, None);
}

#[test]
fn node_detail_as_api_writes_it() {
    // node_detail: the summary plus mass, counts and the node's sections.
    let summary = body_summary();
    let detail = format!(
        r#"{}, "body_kind": "solid", "mass": {{"volume_mm3": 6000.0, "area_mm2": 2200.0, "mass_g": 7.44, "centroid": [5.0, 10.0, 15.0], "bbox_min": [0, 0, 0], "bbox_max": [10.0, 20.0, 30.0], "size": [10.0, 20.0, 30.0]}}, "face_count": 6, "edge_count": 12, "joint": {{"kind": "revolute", "axis": [0, 0, 1]}}, "robot": null, "extra": true}}"#,
        summary.trim_end_matches('}')
    );
    let (c, server) = serve(vec![ok(&detail), ok(&group_summary())]);
    let port = c.endpoint.port;
    let d = c.node("a1b2c3d4e5f6").unwrap();
    assert_eq!((d.summary.id.as_str(), d.summary.name.as_str(), d.summary.visible), ("a1b2c3d4e5f6", "Femur", true));
    assert_eq!(d.body_kind.as_deref(), Some("solid"));
    let mass = d.mass.unwrap();
    assert_eq!((mass.volume_mm3, mass.mass_g, mass.bbox_min.clone(), mass.size.clone()), (Some(6000.0), Some(7.44), vec![Some(0.0); 3], vec![Some(10.0), Some(20.0), Some(30.0)]));
    assert_eq!((d.face_count, d.edge_count), (Some(6), Some(12)));
    assert_eq!(d.joint, Some(json!({"kind": "revolute", "axis": [0, 0, 1]})));
    assert_eq!((d.robot, d.sketch, d.plane, d.mesh), (None, None, None, None));
    // A node without geometry: no mass block or counts.
    let g = c.node("0123456789ab").unwrap();
    assert_eq!((g.summary.kind.as_str(), g.mass, g.face_count), ("group", None, None));
    assert_eq!(g.summary.children, vec!["a1b2c3d4e5f6".to_string()]);
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /nodes/a1b2c3d4e5f6 HTTP/1.1", port, None);
    assert_request(&seen[1], "GET /nodes/0123456789ab HTTP/1.1", port, None);
}

#[test]
fn patch_and_delete() {
    let (c, server) = serve(vec![ok(&body_summary()), ok(r#"{"deleted": "a1b2c3d4e5f6"}"#)]);
    let port = c.endpoint.port;
    let mut attrs = Map::new();
    attrs.insert("name".into(), json!("Thigh"));
    assert_eq!(c.patch("a1b2c3d4e5f6", &attrs).unwrap().summary.id, "a1b2c3d4e5f6");
    assert_eq!(c.delete("a1b2c3d4e5f6").unwrap(), Deleted { deleted: "a1b2c3d4e5f6".into() });
    let seen = server.join().unwrap();
    assert_request(&seen[0], "PATCH /nodes/a1b2c3d4e5f6 HTTP/1.1", port, Some(r#"{"name":"Thigh"}"#));
    assert_request(&seen[1], "DELETE /nodes/a1b2c3d4e5f6 HTTP/1.1", port, None);
}

#[test]
fn mesh_and_its_absence() {
    let mesh = r#"{"vertices": [[0, 0, 0], [1.5, 0.0, 0.0], [0.0, 2.0, -1.0]], "triangles": [[0, 1, 2]], "triangle_face": [0], "face_count": 1}"#;
    let (c, server) = serve(vec![
        ok(mesh),
        Answer::Json(404, r#"{"error": "no mesh"}"#.into()),
        Answer::Json(500, r#"{"error": "KernelError: bad shape", "trace": "..."}"#.into()),
        // Only "no mesh" means no mesh: an old RoboCAD without the route,
        // or any other 404, is an error.
        Answer::Json(404, r#"{"error": "no route GET /nodes/a1b2c3d4e5f6/mesh"}"#.into()),
        Answer::Json(404, r#"{"error": "no node a1b2c3d4e5f6"}"#.into()),
    ]);
    let port = c.endpoint.port;
    let m = c.mesh("a1b2c3d4e5f6", MESH_TOLERANCE).unwrap().unwrap();
    assert_eq!(m, MeshData { vertices: vec![[0.0; 3], [1.5, 0.0, 0.0], [0.0, 2.0, -1.0]], triangles: vec![[0, 1, 2]], triangle_face: vec![0], face_count: 1 });
    assert_eq!(c.mesh("0123456789ab", 0.05).unwrap(), None);
    let e = c.mesh("a1b2c3d4e5f6", 0.1).unwrap_err();
    assert_eq!((e.status, e.message.as_str()), (Some(500), "KernelError: bad shape"));
    let e = c.mesh("a1b2c3d4e5f6", 0.1).unwrap_err();
    assert_eq!((e.status, e.message.as_str()), (Some(404), "no route GET /nodes/a1b2c3d4e5f6/mesh"));
    let e = c.mesh("a1b2c3d4e5f6", 0.1).unwrap_err();
    assert_eq!((e.status, e.message.as_str()), (Some(404), "no node a1b2c3d4e5f6"));
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /nodes/a1b2c3d4e5f6/mesh?tolerance=0.1 HTTP/1.1", port, None);
    assert_request(&seen[1], "GET /nodes/0123456789ab/mesh?tolerance=0.05 HTTP/1.1", port, None);
}

#[test]
fn ops_list_and_op() {
    let (c, server) = serve(vec![
        ok(r#"{"box": "(corner, size, name='Box')", "fillet": "(edges, radius)"}"#),
        ok(r#"{"result": "a1b2c3d4e5f6", "history": {"undo": ["Box"], "redo": []}}"#),
        ok(r#"{"job": {"id": "j1", "state": "running"}}"#),
    ]);
    let port = c.endpoint.port;
    let ops = c.ops().unwrap();
    assert_eq!(ops.get("box").map(String::as_str), Some("(corner, size, name='Box')"));
    let mut kwargs = Map::new();
    kwargs.insert("name".into(), json!("Box"));
    let r = c.op("box", &[json!([0, 0, 0]), json!([10, 20, 30])], &kwargs).unwrap();
    assert_eq!((r.result, r.history.undo, r.job), (json!("a1b2c3d4e5f6"), vec!["Box".to_string()], None));
    let job = c.op("add_component", &[], &Map::new()).unwrap();
    assert_eq!((job.job, job.result), (Some(json!({"id": "j1", "state": "running"})), Value::Null));
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /ops HTTP/1.1", port, None);
    assert_request(&seen[1], "POST /ops/box HTTP/1.1", port, Some(r#"{"args":[[0,0,0],[10,20,30]],"kwargs":{"name":"Box"}}"#));
    assert_request(&seen[2], "POST /ops/add_component HTTP/1.1", port, Some(r#"{"args":[],"kwargs":{}}"#));
}

#[test]
fn commands_and_run_command() {
    let (c, server) = serve(vec![
        ok(r#"{"view.fit": {"label": "Fit all", "category": "View", "keys": "F"}, "edit.undo": {"label": "Undo", "category": "Edit", "keys": ["Ctrl+Z", "Meta+Z"]}}"#),
        ok(r#"{"ran": "view.fit"}"#),
        Answer::Json(409, r#"{"error": "no GUI"}"#.into()),
    ]);
    let port = c.endpoint.port;
    let commands = c.commands().unwrap();
    assert_eq!(commands["view.fit"], CommandInfo { label: "Fit all".into(), category: "View".into(), keys: json!("F") });
    assert_eq!(commands["edit.undo"].keys, json!(["Ctrl+Z", "Meta+Z"]));
    assert_eq!(c.run_command("view.fit").unwrap(), Ran { ran: "view.fit".into() });
    let e = c.run_command("view.fit").unwrap_err();
    assert!(e.no_gui() && !e.not_found(), "{e:?}");
    assert_eq!(e, CadError { method: "POST", route: "/commands/view.fit".into(), status: Some(409), message: "no GUI".into() });
    assert_eq!(e.to_string(), "RoboCAD POST /commands/view.fit: no GUI (HTTP 409)");
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /commands HTTP/1.1", port, None);
    assert_request(&seen[1], "POST /commands/view.fit HTTP/1.1", port, Some("{}"));
    assert_request(&seen[2], "POST /commands/view.fit HTTP/1.1", port, Some("{}"));
}

#[test]
fn history_undo_and_redo() {
    let (c, server) = serve(vec![
        ok(r#"{"undo": ["Box"], "redo": ["Fillet"]}"#),
        ok(r#"{"undone": "Box", "history": {"undo": [], "redo": ["Box", "Fillet"]}}"#),
        ok(r#"{"redone": null, "history": {"undo": [], "redo": []}}"#),
    ]);
    let port = c.endpoint.port;
    assert_eq!(c.history().unwrap(), History { undo: vec!["Box".into()], redo: vec!["Fillet".into()] });
    let undone = c.undo().unwrap();
    assert_eq!((undone.undone.as_deref(), undone.history.redo.len()), (Some("Box"), 2));
    assert_eq!(c.redo().unwrap(), Redone { redone: None, history: History::default() });
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /history HTTP/1.1", port, None);
    assert_request(&seen[1], "POST /undo HTTP/1.1", port, Some("{}"));
    assert_request(&seen[2], "POST /redo HTTP/1.1", port, Some("{}"));
}

#[test]
fn selection_get_and_set() {
    let (c, server) = serve(vec![
        // A malformed item is dropped, not the selection.
        ok(r#"{"items": [["a1b2c3d4e5f6", "face", 3], ["bad"], ["0123456789ab", "body", 0]], "mode": "face"}"#),
        ok(r#"{"items": [["a1b2c3d4e5f6", "face", 3]], "mode": "face"}"#),
        ok(r#"{"items": [["a1b2c3d4e5f6", "body", 0]]}"#),
    ]);
    let port = c.endpoint.port;
    let s = c.selection().unwrap();
    assert_eq!(s.items, vec![SelectionItem("a1b2c3d4e5f6".into(), "face".into(), 3), SelectionItem("0123456789ab".into(), "body".into(), 0)]);
    assert_eq!(s.mode.as_deref(), Some("face"));
    let face = [SelectionItem("a1b2c3d4e5f6".into(), "face".into(), 3)];
    assert_eq!(c.set_selection(&face, Some("face")).unwrap().items, face.to_vec());
    let body = [SelectionItem("a1b2c3d4e5f6".into(), "body".into(), 0)];
    let s = c.set_selection(&body, None).unwrap();
    assert_eq!((s.items, s.mode), (body.to_vec(), None));
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /selection HTTP/1.1", port, None);
    assert_request(&seen[1], "PUT /selection HTTP/1.1", port, Some(r#"{"items":[["a1b2c3d4e5f6","face",3]],"mode":"face"}"#));
    assert_request(&seen[2], "PUT /selection HTTP/1.1", port, Some(r#"{"items":[["a1b2c3d4e5f6","body",0]]}"#));
}

#[test]
fn save_open_and_loads() {
    let load = r#"{"id": "L1", "path": "/models/leg.rcad", "state": "loading", "stage": "Checking model", "completed": 3, "total": 10, "part": "Femur", "elapsed_seconds": 1.25}"#;
    let (c, server) = serve(vec![
        ok(r#"{"saved": "/models/leg.rcad"}"#),
        ok(r#"{"saved": "/tmp/copy.rcad"}"#),
        ok(r#"{"opened": "/models/arm.rcad", "window": true, "loading": true, "load_id": "L1"}"#),
        ok(load),
        ok(r#"{"id": "L1", "path": "/models/leg.rcad", "state": "cancelling", "stage": null, "completed": 3, "total": 10}"#),
        Answer::Json(400, r#"{"error": "no path"}"#.into()),
    ]);
    let port = c.endpoint.port;
    assert_eq!(c.save(None).unwrap(), Saved { saved: "/models/leg.rcad".into() });
    assert_eq!(c.save(Some("/tmp/copy.rcad")).unwrap().saved, "/tmp/copy.rcad");
    let opened = c.open("/models/arm.rcad").unwrap();
    assert_eq!(opened, Opened { opened: "/models/arm.rcad".into(), window: true, loading: true, load_id: Some("L1".into()) });
    let status = c.load_status("L1").unwrap();
    assert_eq!((status.state.as_str(), status.stage.as_deref(), status.completed, status.total, status.part.as_deref()), ("loading", Some("Checking model"), 3, 10, Some("Femur")));
    assert_eq!((status.elapsed_seconds, status.error, status.stats), (Some(1.25), None, None));
    let cancelled = c.cancel_load("L1").unwrap();
    assert_eq!((cancelled.state.as_str(), cancelled.stage), ("cancelling", None));
    let e = c.save(None).unwrap_err();
    assert_eq!(e.to_string(), "RoboCAD POST /save: no path (HTTP 400)");
    let seen = server.join().unwrap();
    assert_request(&seen[0], "POST /save HTTP/1.1", port, Some("{}"));
    assert_request(&seen[1], "POST /save HTTP/1.1", port, Some(r#"{"path":"/tmp/copy.rcad"}"#));
    assert_request(&seen[2], "POST /open HTTP/1.1", port, Some(r#"{"path":"/models/arm.rcad"}"#));
    assert_request(&seen[3], "GET /loads/L1 HTTP/1.1", port, None);
    assert_request(&seen[4], "DELETE /loads/L1 HTTP/1.1", port, None);
}

#[test]
fn autosave_physical_and_export() {
    let (c, server) = serve(vec![
        ok(r#"{"running": false, "revision": null, "saved_revision": 4, "path": "/models/leg.autosave.rcad"}"#),
        ok(r#"{"schema": "simrobot", "version": 3, "bodies": []}"#),
        ok(r#"{"schema": "simrobot", "version": 3, "bodies": [], "flex": []}"#),
        ok(r#"{"exported": "/tmp/x.stl", "warnings": []}"#),
        ok(r#"{"exported": "/tmp/x.3mf", "warnings": ["thin wall"]}"#),
    ]);
    let port = c.endpoint.port;
    assert_eq!(c.autosave().unwrap(), Autosave { running: false, revision: None, saved_revision: Some(4), path: Some("/models/leg.autosave.rcad".into()) });
    assert_eq!(c.physical(false).unwrap()["version"], json!(3));
    assert!(c.physical(true).unwrap().get("flex").is_some());
    let stl = ExportRequest { format: "stl".into(), path: "/tmp/x.stl".into(), settings: None, ids: None };
    assert_eq!(c.export(&stl).unwrap(), Exported { exported: "/tmp/x.stl".into(), warnings: json!([]) });
    let three_mf = ExportRequest { format: "3mf".into(), path: "/tmp/x.3mf".into(), settings: Some(json!({"binary": true})), ids: Some(vec!["a".into(), "b".into()]) };
    assert_eq!(c.export(&three_mf).unwrap().warnings, json!(["thin wall"]));
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /autosave HTTP/1.1", port, None);
    assert_request(&seen[1], "GET /physical?flex=0 HTTP/1.1", port, None);
    assert_request(&seen[2], "GET /physical?flex=1 HTTP/1.1", port, None);
    // Never a `path` (RoboCAD would write a file).
    assert!(seen[1..3].iter().all(|s| !s.request_line().contains("path")));
    assert_request(&seen[3], "POST /export HTTP/1.1", port, Some(r#"{"format":"stl","path":"/tmp/x.stl"}"#));
    assert_request(&seen[4], "POST /export HTTP/1.1", port, Some(r#"{"format":"3mf","path":"/tmp/x.3mf","settings":{"binary":true},"ids":["a","b"]}"#));
}

#[test]
fn path_segments_are_percent_encoded() {
    let (c, server) = serve(vec![Answer::Json(404, r#"{"error": "no node a/b c"}"#.into()), Answer::Json(404, r#"{"error": "no op x"}"#.into())]);
    let port = c.endpoint.port;
    let e = c.node("a/b c").unwrap_err();
    assert_eq!(e.route, "/nodes/a%2Fb%20c");
    assert!(c.op("x?y", &[], &Map::new()).unwrap_err().not_found());
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /nodes/a%2Fb%20c HTTP/1.1", port, None);
    assert_eq!(seen[1].request_line(), "POST /ops/x%3Fy HTTP/1.1");
}

#[test]
fn errors_name_the_route_and_robocads_text() {
    let (c, server) = serve(vec![
        Answer::Json(404, r#"{"error": "no node x"}"#.into()),
        Answer::Json(409, r#"{"error": "Background autosave requires a desktop window"}"#.into()),
        Answer::Json(502, "<html>bad gateway</html>".into()),
        ok("not json"),
        ok(r#"{"items": 5, "mode": "face"}"#),
        ok(r#"{"items": [], "mode": 3}"#),
    ]);
    let e = c.node("x").unwrap_err();
    assert_eq!(e, CadError { method: "GET", route: "/nodes/x".into(), status: Some(404), message: "no node x".into() });
    assert!(e.not_found() && !e.no_gui());
    let text = e.to_string();
    assert!(text.contains("/nodes/x") && text.contains("no node x"), "{text}");
    assert_eq!(text, "RoboCAD GET /nodes/x: no node x (HTTP 404)");
    let e = c.autosave().unwrap_err();
    assert!(e.no_gui());
    assert_eq!(e.message, "Background autosave requires a desktop window");
    // An answer without an `error` says its status once.
    let e = c.doc().unwrap_err();
    assert_eq!(e.to_string(), "RoboCAD GET /doc: Request failed (HTTP 502)");
    let e = c.history().unwrap_err();
    assert_eq!((e.status, e.route.as_str()), (None, "/history"));
    assert!(e.message.contains("not JSON"), "{e}");
    // A malformed item list reads as empty; a field of the wrong type fails
    // the answer, naming the route.
    assert_eq!(c.selection().unwrap(), Selection { items: vec![], mode: Some("face".into()) });
    let e = c.selection().unwrap_err();
    assert_eq!((e.route.as_str(), e.status), ("/selection", None));
    assert!(e.message.contains("unexpected answer"), "{e}");
    server.join().unwrap();
}

#[test]
fn connection_refused_names_the_route() {
    let port = dead_port();
    // Short, so a port reused meanwhile cannot stall the test.
    let c = CadClient::new(&format!("http://127.0.0.1:{port}")).unwrap().with_timeout(Duration::from_secs(1));
    let e = c.doc().unwrap_err();
    assert_eq!((e.method, e.route.as_str(), e.status), ("GET", "/doc", None));
    assert!(e.message.contains("connect"), "{e}");
    let text = e.to_string();
    assert!(text.starts_with("RoboCAD GET /doc: ") && text.contains(&format!("127.0.0.1:{port}")), "{text}");
}

#[test]
fn only_loopback_urls() {
    for url in ["http://10.0.0.1:8420", "http://[::1]:8420", "https://127.0.0.1:8420", "http://127.0.0.1:8420/doc", "http://127.0.0.1"] {
        let e = CadClient::new(url).unwrap_err();
        assert_eq!((e.method, e.route.as_str(), e.status), ("-", url, None));
        assert!(e.message.starts_with(url), "{e}");
    }
    for url in ["http://10.0.0.1:8420", "http://[::1]:8420"] {
        let e = CadClient::new(url).unwrap_err();
        assert!(e.message.contains("listen on 127.0.0.1 only"), "{e}");
    }
    let c = CadClient::new(DEFAULT_URL).unwrap();
    assert_eq!((c.url(), c.timeout), ("http://127.0.0.1:8420".to_string(), REQUEST_TIMEOUT));
    assert_eq!(CadClient::new("http://localhost:8420/").unwrap().url(), "http://127.0.0.1:8420");
    assert_eq!(c.with_timeout(Duration::ZERO).timeout, Duration::from_millis(1));
}

/// A fresh, empty directory under the temp dir.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cad-client-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn interpreter_requires_the_venv() {
    let cad = temp_dir("interpreter");
    let e = service::interpreter(&cad).unwrap_err();
    assert!(e.contains(".venv") && e.contains(&cad.display().to_string()) && e.contains("run cad/run.sh once"), "{e}");
    // Never created.
    assert!(!cad.join(".venv").exists());
    #[cfg(not(windows))]
    {
        std::fs::create_dir_all(cad.join(".venv/bin")).unwrap();
        std::fs::write(cad.join(".venv/bin/python"), "").unwrap();
        assert_eq!(service::interpreter(&cad).unwrap(), cad.join(".venv/bin/python"));
    }
    let _ = std::fs::remove_dir_all(&cad);
}

#[test]
fn service_command_line() {
    let cad = temp_dir("command");
    let python = cad.join(".venv/bin/python");
    let document = cad.join("models/leg.rcad");
    let port = dead_port();
    let command = service::service_command(&cad, &python, &document, port);
    assert_eq!(command.get_program(), python.as_os_str());
    let args: Vec<String> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    assert_eq!(args, vec!["-m".to_string(), "robocad.api".into(), document.display().to_string(), "--port".into(), port.to_string(), "--host".into(), "127.0.0.1".into()]);
    assert_eq!(command.get_current_dir(), Some(cad.as_path()));
    // A relative document is made absolute (the child runs in cad/).
    let relative = service::service_command(&cad, &python, std::path::Path::new("leg.rcad"), port);
    let arg = PathBuf::from(relative.get_args().nth(2).unwrap());
    assert!(arg.is_absolute() && arg.ends_with("leg.rcad"), "{}", arg.display());
    // stderr goes to the log, which the command created.
    let log = service::log_path(port);
    assert!(log.starts_with(std::env::temp_dir()) && log.exists(), "{}", log.display());
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_dir_all(&cad);
}

#[test]
fn log_tail_keeps_whole_last_lines() {
    let dir = temp_dir("log");
    let log = dir.join("robocad.log");
    assert_eq!(service::log_tail(&log, 100), "");
    std::fs::write(&log, "Traceback (most recent call last):\n  File \"x.py\"\nOSError: [Errno 48] Address already in use\n").unwrap();
    assert_eq!(service::log_tail(&log, 1000), "Traceback (most recent call last):\n  File \"x.py\"\nOSError: [Errno 48] Address already in use");
    assert_eq!(service::log_tail(&log, 50), "OSError: [Errno 48] Address already in use");
    let _ = std::fs::remove_dir_all(&dir);
}

const HEALTH: &str = r#"{"ok": true, "app": "robocad", "version": "0.4.0", "path": "/models/leg.rcad", "dirty": false, "gui": false, "nodes": 3, "document_id": "d", "revision": 1}"#;

#[test]
fn wait_until_live_polls_until_health_answers() {
    // Two connections closed unanswered (loading), then health.
    let (c, server) = serve(vec![Answer::Close, Answer::Close, ok(HEALTH)]);
    let port = c.endpoint.port;
    let started = Instant::now();
    let mut polls = 0;
    let health = service::wait_until_live(
        &c,
        started + Duration::from_secs(10),
        || {
            polls += 1;
            Ok(())
        },
        || false,
    )
    .unwrap();
    assert_eq!((health.path.as_deref(), health.nodes), (Some("/models/leg.rcad"), 3));
    assert_eq!(polls, 3);
    // Two 150 ms pauses.
    assert!(started.elapsed() >= Duration::from_millis(300), "{:?}", started.elapsed());
    let seen = server.join().unwrap();
    assert_eq!(seen.len(), 3);
    assert!(seen.iter().all(|s| s.request_line() == "GET / HTTP/1.1"));
    assert_eq!(seen[2].headers(), bare(port));
}

#[test]
fn wait_until_live_stops_when_the_child_exits_or_is_cancelled() {
    let c = CadClient::new(&format!("http://127.0.0.1:{}", dead_port())).unwrap().with_timeout(Duration::from_secs(1));
    let far = Instant::now() + START_TIMEOUT;
    let mut calls = 0;
    let e = service::wait_until_live(
        &c,
        far,
        || {
            calls += 1;
            if calls >= 2 { Err("RoboCAD service exited (exit status: 1)".into()) } else { Ok(()) }
        },
        || false,
    )
    .unwrap_err();
    assert_eq!((e.as_str(), calls), ("RoboCAD service exited (exit status: 1)", 2));
    assert_eq!(service::wait_until_live(&c, far, || Ok(()), || true).unwrap_err(), "cancelled");
}

#[test]
fn wait_until_live_gives_up_at_the_deadline() {
    let port = dead_port();
    let c = CadClient::new(&format!("http://127.0.0.1:{port}")).unwrap().with_timeout(Duration::from_secs(1));
    let started = Instant::now();
    let e = service::wait_until_live(&c, started + Duration::from_millis(400), || Ok(()), || false).unwrap_err();
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(400) && elapsed < Duration::from_secs(5), "{elapsed:?}");
    assert!(e.contains(&format!("http://127.0.0.1:{port}")) && e.contains("did not answer within"), "{e}");
}

#[test]
fn doc_state_tolerates_a_malformed_node_and_colour() {
    // A node whose id is not a string is dropped, not the document; a
    // colour or pivot of the wrong shape reads as None.
    let odd = body_summary().replace(r#""color": null, "pivot": null"#, r#""color": "red", "pivot": [1, "x", 3]"#);
    let doc = format!(r#"{{"nodes": [{}, {{"id": ["bad"]}}, {}], "revision": 2}}"#, group_summary(), odd);
    let (c, server) = serve(vec![ok(&doc)]);
    let state = c.doc().unwrap();
    assert_eq!(state.nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), vec!["0123456789ab", "a1b2c3d4e5f6"]);
    assert_eq!((state.nodes[1].color.clone(), state.nodes[1].pivot.clone(), state.nodes[1].name.as_str()), (None, None, "Femur"));
    assert_eq!(state.nodes[0].color, Some(vec![0.8, 0.2, 0.2]));
    server.join().unwrap();
}

#[test]
fn non_finite_numbers_read_as_null() {
    let n = |text: &str| null_non_finite(text).into_owned();
    assert_eq!(n("[NaN, Infinity, -Infinity]"), "[null, null, null]");
    assert_eq!(n(r#"{"a":NaN,"b":{"c":[1,-Infinity]},"d":Infinity}"#), r#"{"a":null,"b":{"c":[1,null]},"d":null}"#);
    assert_eq!(n("NaN"), "null");
    assert_eq!(n("[\n\tNaN\n]"), "[\n\tnull\n]");
    // Inside strings, also after an escaped quote or backslash: untouched.
    let strings = r#"{"NaN": "NaN", "s": "x\"NaN", "t": "\\", "u": [Infinity], "v": "-Infinity, NaN"}"#;
    assert_eq!(n(strings), r#"{"NaN": "NaN", "s": "x\"NaN", "t": "\\", "u": [null], "v": "-Infinity, NaN"}"#);
    // Only whole value tokens.
    assert_eq!(n("[NaNa, Infinityx, 1e5, -1, NaN]"), "[NaNa, Infinityx, 1e5, -1, null]");
    // Nothing to replace: borrowed.
    assert!(matches!(null_non_finite(r#"{"a": [1, 2.5e-3, "NaN"]}"#), Cow::Borrowed(_)));
    assert!(matches!(null_non_finite("ünïcode NaN"), Cow::Owned(_)));
    assert_eq!(n(r#"["é", NaN]"#), r#"["é", null]"#);
}

#[test]
fn non_finite_mass_reads_as_none() {
    // Python's json.dumps of a degenerate body's mass properties.
    let detail = format!(
        r#"{}, "body_kind": "solid", "mass": {{"volume_mm3": NaN, "area_mm2": Infinity, "mass_g": -Infinity, "centroid": [NaN, 1.0, NaN], "bbox_min": [0, 0, 0], "bbox_max": [1, 1, 1], "size": null}}, "name_note": "NaN stays"}}"#,
        body_summary().trim_end_matches('}')
    );
    let (c, server) = serve(vec![ok(&detail)]);
    let mass = c.node("a1b2c3d4e5f6").unwrap().mass.unwrap();
    assert_eq!((mass.volume_mm3, mass.area_mm2, mass.mass_g), (None, None, None));
    assert_eq!(mass.centroid, vec![None, Some(1.0), None]);
    assert_eq!((mass.bbox_max, mass.size), (vec![Some(1.0); 3], vec![]));
    server.join().unwrap();
}

#[test]
fn an_edit_that_times_out_may_still_apply() {
    let hold = Duration::from_millis(800);
    let (c, server) = serve(vec![Answer::Silent(hold), Answer::Silent(hold)]);
    let c = c.with_timeout(Duration::from_millis(200));
    let e = c.save(None).unwrap_err();
    assert_eq!((e.method, e.route.as_str(), e.status), ("POST", "/save", None));
    assert!(e.message.ends_with("read: timed out: RoboCAD may still apply it; refresh before retrying"), "{e}");
    // A read is not an edit: the plain timeout.
    let e = c.doc().unwrap_err();
    assert!(e.message.ends_with("read: timed out"), "{e}");
    server.join().unwrap();
    assert_eq!(EDIT_TIMEOUT, Duration::from_secs(130));
}

/// A planar face as `face_json` writes it (axis fields null), and a
/// cylindrical one with an unknown field.
const FACES: &str = r#"[{"kind": "plane", "centroid": [10.0, 5.0, 5.0], "normal": [0.0, 0.0, 1.0], "area": 200.0, "axis_point": null, "axis_dir": null, "radius": null, "point": [10.0, 5.0, 5.0], "index": 0}, {"kind": "cylinder", "centroid": [40.0, 0.0, 3.0], "normal": [1.0, 0.0, 0.0], "area": 150.796, "axis_point": [40.0, 0.0, 0.0], "axis_dir": [0.0, 0.0, 1.0], "radius": 4.0, "point": [44.0, 0.0, 3.0], "index": 1, "later": {"x": 1}}]"#;

/// A line and a circle as `edge_json` writes them, without samples.
const EDGES: &str = r#"[{"index": 0, "kind": "line", "midpoint": [10.0, 0.0, 0.0], "length": 20.0, "start": [0.0, 0.0, 0.0], "end": [20.0, 0.0, 0.0], "center": null, "radius": null}, {"index": 1, "kind": "circle", "midpoint": [36.0, 0.0, 6.0], "length": 25.133, "start": [44.0, 0.0, 6.0], "end": [44.0, 0.0, 6.0], "center": [40.0, 0.0, 6.0], "radius": 4.0}]"#;

/// The same with `?samples=3`: `points` last; the circle's second point is
/// malformed (dropped) and its third non-finite (dropped).
const SAMPLED_EDGES: &str = r#"[{"index": 0, "kind": "line", "midpoint": [10.0, 0.0, 0.0], "length": 20.0, "start": [0.0, 0.0, 0.0], "end": [20.0, 0.0, 0.0], "center": null, "radius": null, "points": [[0.0, 0.0, 0.0], [20.0, 0.0, 0.0]]}, {"index": 1, "kind": "circle", "midpoint": [36.0, 0.0, 6.0], "length": 25.133, "start": [44.0, 0.0, 6.0], "end": [44.0, 0.0, 6.0], "center": [40.0, 0.0, 6.0], "radius": 4.0, "points": [[44.0, 0.0, 6.0], [36.0, "x", 6.0], [NaN, 0.0, 6.0], [44.0, 0.0, 6.0]]}]"#;

#[test]
fn faces_edges_vertices_and_solids_as_api_writes_them() {
    let (c, server) = serve(vec![
        ok(FACES),
        ok(EDGES),
        ok(SAMPLED_EDGES),
        ok(r#"[{"index": 0, "point": [0.0, 0.0, 0.0]}, {"index": 1, "point": [20.0, 0.0, 0.0]}]"#),
        ok(r#"{"node_id": "a1b2c3d4e5f6", "revision": 9, "units": "mm", "solids": [{"index": 0, "bbox_min": [0.0, 0.0, 0.0], "bbox_max": [20.0, 10.0, 5.0]}]}"#),
        ok(r#"{"node_id": "0123456789ab", "revision": 9, "units": "mm", "solids": []}"#),
    ]);
    let port = c.endpoint.port;
    let faces = c.faces("a1b2c3d4e5f6").unwrap();
    let plane = FaceInfo { index: 0, kind: "plane".into(), centroid: Some([10.0, 5.0, 5.0]), normal: Some([0.0, 0.0, 1.0]), area: Some(200.0), axis_point: None, axis_dir: None, radius: None, point: Some([10.0, 5.0, 5.0]) };
    assert_eq!(faces[0], plane);
    assert_eq!((faces[1].index, faces[1].kind.as_str(), faces[1].radius), (1, "cylinder", Some(4.0)));
    assert_eq!((faces[1].axis_point, faces[1].axis_dir), (Some([40.0, 0.0, 0.0]), Some([0.0, 0.0, 1.0])));
    let edges = c.edges("a1b2c3d4e5f6", None).unwrap();
    let line = EdgeInfo { index: 0, kind: "line".into(), midpoint: Some([10.0, 0.0, 0.0]), length: Some(20.0), start: Some([0.0; 3]), end: Some([20.0, 0.0, 0.0]), center: None, radius: None, points: vec![] };
    assert_eq!(edges[0], line);
    assert_eq!((edges[1].kind.as_str(), edges[1].center, edges[1].radius, edges[1].points.len()), ("circle", Some([40.0, 0.0, 6.0]), Some(4.0), 0));
    let sampled = c.edges("a1b2c3d4e5f6", Some(3)).unwrap();
    assert_eq!(sampled[0], EdgeInfo { points: vec![[0.0; 3], [20.0, 0.0, 0.0]], ..line });
    assert_eq!(sampled[1].points, vec![[44.0, 0.0, 6.0], [44.0, 0.0, 6.0]]);
    assert_eq!(sampled[1].center, Some([40.0, 0.0, 6.0]));
    let vertices = c.vertices("a1b2c3d4e5f6").unwrap();
    assert_eq!(vertices, vec![VertexInfo { index: 0, point: Some([0.0; 3]) }, VertexInfo { index: 1, point: Some([20.0, 0.0, 0.0]) }]);
    let solids = c.solids("a1b2c3d4e5f6").unwrap();
    assert_eq!((solids.node_id.as_str(), solids.revision, solids.units.as_str(), solids.solids.len()), ("a1b2c3d4e5f6", 9, "mm", 1));
    assert_eq!(solids.solids[0]["bbox_max"], json!([20.0, 10.0, 5.0]));
    assert!(c.solids("0123456789ab").unwrap().solids.is_empty());
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /nodes/a1b2c3d4e5f6/faces HTTP/1.1", port, None);
    assert_request(&seen[1], "GET /nodes/a1b2c3d4e5f6/edges HTTP/1.1", port, None);
    assert_request(&seen[2], "GET /nodes/a1b2c3d4e5f6/edges?samples=3 HTTP/1.1", port, None);
    assert_request(&seen[3], "GET /nodes/a1b2c3d4e5f6/vertices HTTP/1.1", port, None);
    assert_request(&seen[4], "GET /nodes/a1b2c3d4e5f6/solids HTTP/1.1", port, None);
    assert_request(&seen[5], "GET /nodes/0123456789ab/solids HTTP/1.1", port, None);
}

#[test]
fn topology_tolerates_nulls_and_non_finite_values() {
    // Null or non-finite (bare NaN/Infinity, read as null) reads as None,
    // as does a malformed vector; nothing is filled in.
    let face = r#"[{"kind": "bspline", "centroid": null, "normal": [NaN, 0.0, 1.0], "area": NaN, "axis_point": null, "axis_dir": null, "radius": null, "point": [1.0, 2.0], "index": 4}]"#;
    let edge = r#"[{"index": 2, "kind": "bspline", "midpoint": [1.0, 1.0, 1.0], "length": Infinity, "start": null, "end": [0, 0, 0], "center": null, "radius": null, "points": "none"}]"#;
    let (c, server) = serve(vec![ok(face), ok(edge), ok(r#"[{"index": 0, "point": [NaN, 0.0, 0.0]}]"#)]);
    let f = &c.faces("n1").unwrap()[0];
    assert_eq!((f.index, f.kind.as_str(), f.centroid, f.normal, f.area, f.point), (4, "bspline", None, None, None, None));
    let e = &c.edges("n1", None).unwrap()[0];
    assert_eq!((e.length, e.start, e.end, e.midpoint), (None, None, Some([0.0; 3]), Some([1.0; 3])));
    assert!(e.points.is_empty());
    assert_eq!(c.vertices("n1").unwrap(), vec![VertexInfo { index: 0, point: None }]);
    server.join().unwrap();
}

#[test]
fn topology_routes_encode_the_id_and_report_no_geometry() {
    let no_geometry = || Answer::Json(404, r#"{"error": "Sketch has no geometry"}"#.into());
    let (c, server) = serve(vec![no_geometry(), no_geometry(), no_geometry(), no_geometry(), Answer::Json(404, r#"{"error": "no node a/b c"}"#.into())]);
    let port = c.endpoint.port;
    let e = c.faces("a/b c").unwrap_err();
    assert!(e.not_found(), "{e:?}");
    assert_eq!(e, CadError { method: "GET", route: "/nodes/a%2Fb%20c/faces".into(), status: Some(404), message: "Sketch has no geometry".into() });
    assert!(c.edges("a/b c", None).unwrap_err().not_found());
    assert_eq!(c.edges("a/b c", Some(24)).unwrap_err().route, "/nodes/a%2Fb%20c/edges?samples=24");
    assert!(c.vertices("a/b c").unwrap_err().not_found());
    assert!(c.solids("a/b c").unwrap_err().not_found());
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /nodes/a%2Fb%20c/faces HTTP/1.1", port, None);
    assert_request(&seen[1], "GET /nodes/a%2Fb%20c/edges HTTP/1.1", port, None);
    assert_request(&seen[2], "GET /nodes/a%2Fb%20c/edges?samples=24 HTTP/1.1", port, None);
    assert_request(&seen[3], "GET /nodes/a%2Fb%20c/vertices HTTP/1.1", port, None);
    assert_request(&seen[4], "GET /nodes/a%2Fb%20c/solids HTTP/1.1", port, None);
}

#[test]
fn moded_selection_headless_and_with_a_window() {
    let (c, server) = serve(vec![
        // Headless: set_selection answers `selection()`, which has no mode.
        ok(r#"{"items": [["n1", "face", 2]]}"#),
        ok(r#"{"items": [["n1", "face", 2]]}"#),
        // A desktop window writes the viewport's mode.
        ok(r#"{"items": [["n1", "edge", 5]], "mode": "edge"}"#),
    ]);
    let port = c.endpoint.port;
    let face = [SelectionItem("n1".into(), "face".into(), 2)];
    assert_eq!(c.set_selection(&face, Some("face")).unwrap(), Selection { items: face.to_vec(), mode: None });
    assert_eq!(c.selection().unwrap(), Selection { items: face.to_vec(), mode: None });
    assert_eq!(c.selection().unwrap(), Selection { items: vec![SelectionItem("n1".into(), "edge".into(), 5)], mode: Some("edge".into()) });
    let seen = server.join().unwrap();
    assert_request(&seen[0], "PUT /selection HTTP/1.1", port, Some(r#"{"items":[["n1","face",2]],"mode":"face"}"#));
    assert_request(&seen[1], "GET /selection HTTP/1.1", port, None);
    assert_request(&seen[2], "GET /selection HTTP/1.1", port, None);
}

#[test]
fn clipboard_copy_and_paste() {
    let clip = r#"{"robocad_clipboard": true, "items": [{"node": {"id": "a1", "kind": "body", "name": "Box", "body_kind": "solid"}, "brep": "4442", "sketch": null}]}"#;
    let (c, server) = serve(vec![
        ok(clip),
        ok(r#"{"pasted": ["b2"], "revision": 14, "history": {"undo": ["Box", "Paste"], "redo": []}}"#),
        ok(r#"{"pasted": []}"#),
        Answer::Json(400, r#"{"error": "Clipboard has no robocad content"}"#.into()),
        Answer::Json(404, r#"{"error": "no node zz"}"#.into()),
    ]);
    let port = c.endpoint.port;
    let copied = c.copy_nodes(&["a1".to_string()]).unwrap();
    assert_eq!(copied, serde_json::from_str::<Value>(clip).unwrap());
    let pasted = c.paste(&copied).unwrap();
    assert_eq!(pasted, Pasted { pasted: vec!["b2".into()], revision: Some(14), history: History { undo: vec!["Box".into(), "Paste".into()], redo: vec![] } });
    let empty = json!({"robocad_clipboard": true, "items": []});
    assert_eq!(c.paste(&empty).unwrap(), Pasted::default());
    let e = c.paste(&json!({"items": []})).unwrap_err();
    assert_eq!(e, CadError { method: "POST", route: "/clipboard/paste".into(), status: Some(400), message: "Clipboard has no robocad content".into() });
    assert!(c.copy_nodes(&["zz".to_string()]).unwrap_err().not_found());
    let seen = server.join().unwrap();
    assert_request(&seen[0], "POST /clipboard/copy HTTP/1.1", port, Some(r#"{"ids":["a1"]}"#));
    let sent = format!(r#"{{"clip":{}}}"#, serde_json::to_string(&copied).unwrap());
    assert_request(&seen[1], "POST /clipboard/paste HTTP/1.1", port, Some(&sent));
    // The clip is sent as given (serde_json's key order, whatever its features).
    assert_request(&seen[2], "POST /clipboard/paste HTTP/1.1", port, Some(&format!(r#"{{"clip":{}}}"#, serde_json::to_string(&empty).unwrap())));
    assert_request(&seen[3], "POST /clipboard/paste HTTP/1.1", port, Some(r#"{"clip":{"items":[]}}"#));
    assert_request(&seen[4], "POST /clipboard/copy HTTP/1.1", port, Some(r#"{"ids":["zz"]}"#));
}

#[test]
fn analysis_reads_as_api_writes_them() {
    let (c, server) = serve(vec![
        ok(r#"{"node": "a1", "face": 2, "rows": [[[0.0, 0.0, 5.0], [0.0, 10.0, 5.0]], [[20.0, 0.0, 5.0], [20.0, 10.0, 5.0]]]}"#),
        ok(r#"{"node": "c3", "lines": [[[5.0, 0.0, 0.0], [4.0, 0.0, 0.0]], [[0.0, 5.0, 0.0], [0.0, 4.0, 0.0]]]}"#),
        ok(r#"{"node": "s4", "lines": []}"#),
        ok(r#"{"node": "a1", "edges": [{"index": 0, "continuity": "G0", "points": [[0.0, 0.0, 0.0], [20.0, 0.0, 0.0]]}, {"index": 1, "continuity": "boundary", "points": [[1.0, 2.0, 3.0]]}], "counts": {"G0": 1, "G1": 0, "G2": 0, "boundary": 1}}"#),
    ]);
    let port = c.endpoint.port;
    let cp = c.control_points("a1", 2).unwrap();
    assert_eq!(cp, ControlPoints { node: "a1".into(), face: 2, rows: vec![vec![[0.0, 0.0, 5.0], [0.0, 10.0, 5.0]], vec![[20.0, 0.0, 5.0], [20.0, 10.0, 5.0]]] });
    let comb = c.curvature_comb("c3").unwrap();
    assert_eq!(comb, CurvatureComb { node: "c3".into(), lines: vec![[[5.0, 0.0, 0.0], [4.0, 0.0, 0.0]], [[0.0, 5.0, 0.0], [0.0, 4.0, 0.0]]] });
    assert!(c.curvature_comb("s4").unwrap().lines.is_empty());
    let cont = c.continuity("a1").unwrap();
    assert_eq!(cont.edges[0], EdgeContinuity { index: 0, continuity: "G0".into(), points: vec![[0.0; 3], [20.0, 0.0, 0.0]] });
    assert_eq!((cont.edges[1].index, cont.edges[1].continuity.as_str()), (1, "boundary"));
    let counts: Vec<(&str, u64)> = cont.counts.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    assert_eq!(counts, vec![("G0", 1), ("G1", 0), ("G2", 0), ("boundary", 1)]);
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /nodes/a1/control_points?face=2 HTTP/1.1", port, None);
    assert_request(&seen[1], "GET /nodes/c3/curvature_comb HTTP/1.1", port, None);
    assert_request(&seen[2], "GET /nodes/s4/curvature_comb HTTP/1.1", port, None);
    assert_request(&seen[3], "GET /nodes/a1/continuity HTTP/1.1", port, None);
}

#[test]
fn analysis_errors_name_the_route() {
    let (c, server) = serve(vec![
        Answer::Json(400, r#"{"error": "face index 9 out of range (0..5)"}"#.into()),
        Answer::Json(400, r#"{"error": "Box is a body: the curvature comb is drawn on curves and sketches"}"#.into()),
        Answer::Json(404, r#"{"error": "Sketch has no geometry"}"#.into()),
    ]);
    let port = c.endpoint.port;
    let e = c.control_points("a/b", 9).unwrap_err();
    assert_eq!(e, CadError { method: "GET", route: "/nodes/a%2Fb/control_points?face=9".into(), status: Some(400), message: "face index 9 out of range (0..5)".into() });
    assert_eq!(c.curvature_comb("a1").unwrap_err().status, Some(400));
    assert!(c.continuity("s4").unwrap_err().not_found());
    let seen = server.join().unwrap();
    assert_request(&seen[0], "GET /nodes/a%2Fb/control_points?face=9 HTTP/1.1", port, None);
    assert_request(&seen[1], "GET /nodes/a1/curvature_comb HTTP/1.1", port, None);
    assert_request(&seen[2], "GET /nodes/s4/continuity HTTP/1.1", port, None);
}

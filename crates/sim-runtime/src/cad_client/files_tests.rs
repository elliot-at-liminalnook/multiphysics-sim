//! The file routes against the in-process fake RoboCAD of `tests.rs`: the
//! exact request lines and bodies of import, save with thumbnail, new and
//! the unit guess, answers shaped as api.py writes them (and tolerant of
//! missing fields), RoboCAD's refusals verbatim, the render query, and the
//! render's PNG bytes (a raw one-shot server: the shared fake answers text).
use super::tests::{Answer, assert_request, ok, serve};
use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;

#[test]
fn import_sends_the_unit_only_when_given() {
    let (c, server) = serve(vec![ok(r#"{"imported": ["a1b2c3d4e5f6"]}"#), ok(r#"{"imported": ["0f0f0f0f0f0f", "1e1e1e1e1e1e"]}"#)]);
    let port = c.endpoint.port;
    assert_eq!(c.import("/tmp/part.step", None).unwrap().imported, vec![serde_json::json!("a1b2c3d4e5f6")]);
    assert_eq!(c.import("/tmp/scan.stl", Some("in")).unwrap().imported.len(), 2);
    let seen = server.join().unwrap();
    assert_request(&seen[0], "POST /import HTTP/1.1", port, Some(r#"{"path":"/tmp/part.step"}"#));
    assert_request(&seen[1], "POST /import HTTP/1.1", port, Some(r#"{"path":"/tmp/scan.stl","unit":"in"}"#));
}

#[test]
fn save_with_thumbnail_new_file_and_unit_guess_as_api_writes_them() {
    let (c, server) = serve(vec![
        ok(r#"{"saved": "/tmp/a.rcad", "thumbnail": true}"#),
        ok(r#"{"saved": "/tmp/own.rcad"}"#),
        Answer::Json(201, r#"{"created": "/tmp/new.rcad"}"#.into()),
        ok(r#"{"path": "/tmp/my scan.stl", "extent": 0.5, "guess": "m", "units": ["mm", "cm", "m", "in", "ft"]}"#),
    ]);
    let port = c.endpoint.port;
    assert_eq!(c.save_with_thumbnail(Some("/tmp/a.rcad")).unwrap(), ThumbnailSaved { saved: "/tmp/a.rcad".into(), thumbnail: true });
    // A missing field reads as its default (an older answer without the flag).
    assert_eq!(c.save_with_thumbnail(None).unwrap(), ThumbnailSaved { saved: "/tmp/own.rcad".into(), thumbnail: false });
    assert_eq!(c.new_file("/tmp/new.rcad").unwrap().created, "/tmp/new.rcad");
    let units = c.mesh_units("/tmp/my scan.stl").unwrap();
    assert_eq!((units.extent, units.guess.as_str(), units.units.len()), (Some(0.5), "m", 5));
    let seen = server.join().unwrap();
    assert_request(&seen[0], "POST /save/thumbnail HTTP/1.1", port, Some(r#"{"path":"/tmp/a.rcad"}"#));
    assert_request(&seen[1], "POST /save/thumbnail HTTP/1.1", port, Some("{}"));
    assert_request(&seen[2], "POST /new HTTP/1.1", port, Some(r#"{"path":"/tmp/new.rcad"}"#));
    assert_request(&seen[3], "GET /import/units?path=%2Ftmp%2Fmy%20scan.stl HTTP/1.1", port, None);
}

#[test]
fn refusals_come_back_verbatim_with_their_status() {
    let (c, server) = serve(vec![
        Answer::Json(409, r#"{"error": "/tmp/new.rcad exists: choose a new file name"}"#.into()),
        Answer::Json(400, r#"{"error": "/tmp/a.step: not a mesh file (.stl, .obj, .3mf, .fbx, .ply, .glb, .gltf); only meshes ask for units"}"#.into()),
        Answer::Json(422, r#"{"error": "export blocked by validation:\nBracket: not watertight"}"#.into()),
    ]);
    let e = c.new_file("/tmp/new.rcad").unwrap_err();
    assert_eq!((e.status, e.message.as_str()), (Some(409), "/tmp/new.rcad exists: choose a new file name"));
    let e = c.mesh_units("/tmp/a.step").unwrap_err();
    assert_eq!(e.status, Some(400));
    assert!(e.message.contains("not a mesh file"));
    let e = c.import("/tmp/x.stl", Some("mm")).unwrap_err();
    assert_eq!(e.status, Some(422));
    assert!(e.to_string().starts_with("RoboCAD POST /import: export blocked"), "{e}");
    server.join().unwrap();
}

#[test]
fn render_query_follows_api_order_and_encodes_values() {
    assert_eq!(RenderRequest::default().route(), "/render");
    let r = RenderRequest {
        view: Some("1,-1,0.5".into()),
        w: Some(800),
        h: Some(600),
        mode: Some("xray".into()),
        section: Some("y:10".into()),
        ids: Some(vec!["a1".into(), "b 2".into()]),
        highlight: None,
        labels: Some(true),
        edges: Some(false),
        focus: Some("a1".into()),
        tolerance: Some(0.05),
        title: Some("Bracket & base".into()),
    };
    assert_eq!(r.route(), "/render?view=1%2C-1%2C0.5&w=800&h=600&mode=xray&section=y%3A10&ids=a1,b%202&labels=1&edges=0&focus=a1&tolerance=0.05&title=Bracket%20%26%20base");
}

/// One connection answered with `head` and `body` bytes as Python's
/// `BaseHTTPRequestHandler` writes a PNG (Content-Type image/png); the
/// request line received.
fn serve_bytes(status: u16, content_type: &str, body: Vec<u8>) -> (CadClient, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let content_type = content_type.to_string();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut data = Vec::new();
        let mut chunk = [0u8; 1024];
        while !data.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = stream.read(&mut chunk).unwrap();
            assert!(n > 0, "request ended before its headers");
            data.extend_from_slice(&chunk[..n]);
        }
        let mut answer = format!("HTTP/1.0 {status} OK\r\nServer: robocad/0.1 Python/3.12\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
        answer.extend_from_slice(&body);
        stream.write_all(&answer).unwrap();
        String::from_utf8_lossy(&data).lines().next().unwrap_or("").to_string()
    });
    let client = CadClient::new(&format!("http://127.0.0.1:{port}")).unwrap().with_timeout(Duration::from_secs(5));
    (client, handle)
}

#[test]
fn render_returns_the_png_bytes_unchanged() {
    // A PNG signature, then bytes that are not UTF-8.
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend_from_slice(&[0, 0, 0, 13, 0xff, 0xfe, 0x80, 0x81]);
    let (c, server) = serve_bytes(200, "image/png", png.clone());
    let got = c.render(&RenderRequest { view: Some("top".into()), w: Some(256), h: Some(192), ..Default::default() }).unwrap();
    assert_eq!(got, png);
    assert_eq!(server.join().unwrap(), "GET /render?view=top&w=256&h=192 HTTP/1.1");
}

#[test]
fn render_refuses_an_answer_that_is_not_a_png_and_surfaces_errors() {
    let (c, server) = serve_bytes(200, "application/json", br#"{"ok": true}"#.to_vec());
    let e = c.render(&RenderRequest::default()).unwrap_err();
    assert_eq!((e.method, e.route.as_str()), ("GET", "/render"));
    assert!(e.message.starts_with("the answer is not a PNG image"), "{e}");
    server.join().unwrap();
    let (c, server) = serve_bytes(400, "application/json", br#"{"error": "view is one of ['iso'] or 'dx,dy,dz'"}"#.to_vec());
    let e = c.render(&RenderRequest { view: Some("sideways".into()), ..Default::default() }).unwrap_err();
    assert_eq!(e.status, Some(400));
    assert!(e.message.starts_with("view is one of"), "{e}");
    server.join().unwrap();
}

#[test]
fn extensions_and_tables() {
    assert_eq!(extension("/a/B.STEP"), "step");
    assert_eq!(extension("/a/noext"), "");
    for m in MESH_EXTENSIONS {
        assert!(IMPORT_EXTENSIONS.contains(m), "{m}");
    }
    assert_eq!(IMPORT_UNITS, ["mm", "cm", "m", "in", "ft"]);
}

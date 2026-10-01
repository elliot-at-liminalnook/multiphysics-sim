//! A typed client of RoboCAD's loopback REST API (`cad/robocad/api.py`),
//! for the native viewer's CAD mode, and the helpers that start a headless
//! RoboCAD service for it ([`service`]).
//!
//! RoboCAD owns the document: geometry, the undo stack, saving. This client
//! only sends what api.py's `_route` accepts and reads what it answers
//! ([`types`]). Every request is one blocking HTTP/1.1 exchange over the
//! shared loopback transport ([`crate::loopback_http`]: 127.0.0.1 only,
//! connect/read/write timeouts, a 64 MiB answer cap), so call it from a job
//! thread, never the UI thread.
//!
//! - **Errors** ([`CadError`]): the method and the route as sent, and
//!   RoboCAD's `{"error": …}` text verbatim with its HTTP status (404 for an
//!   unknown node, 409 for a desktop-only route on a headless service, 422 for
//!   a kernel error, 504 when the GUI did not answer within its 120 s).
//! - **Paths**: node ids, command ids, op names and load ids are
//!   percent-encoded as path segments, query values likewise
//!   ([`encode_uri_component`]). RoboCAD does not decode path segments, so
//!   an id needing encoding would not be found; RoboCAD's own ids (hex) and
//!   command ids (`view.fit`) pass unchanged.
//! - **Bodies**: compact JSON written by serde_json, `Content-Type:
//!   application/json`; no other headers (the API has no auth).
//! - **Edits and timeouts**: a mutating request (patch, delete, op, paste,
//!   undo, redo, save, run_command, open, export) should go through a client with
//!   [`EDIT_TIMEOUT`] (the caller sets it), which outlasts RoboCAD's 120 s
//!   GUI wait; any non-`GET` request that times out after connecting says
//!   RoboCAD may still apply it.
//! - **Non-finite numbers**: Python's `json.dumps` writes bare `NaN`,
//!   `Infinity` and `-Infinity`; they read as `null` (never as a number).
//! - **No file writes by accident**: [`CadClient::physical`] never passes
//!   `path` (RoboCAD would write the description to that file).
pub mod service;
pub mod types;
#[cfg(test)]
mod tests;

pub use types::*;

use crate::hardware_client::encode_uri_component;
use crate::loopback_http::{self, Endpoint, Request};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::time::Duration;

/// Where `python -m robocad.api` and the desktop app listen by default.
pub const DEFAULT_URL: &str = "http://127.0.0.1:8420";
/// Read/write timeout per request that does not edit the document. RoboCAD's
/// GUI marshals each request onto its Qt thread and waits up to 120 s; 30 s
/// is our bound for reads (edits use [`EDIT_TIMEOUT`]).
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Read/write timeout for a request that edits the document (patch, delete,
/// op, undo, redo, save, run_command, open, export; `set_selection` is not
/// an edit): `client.clone().with_timeout(EDIT_TIMEOUT)`. A desktop RoboCAD
/// marshals each request onto its Qt thread and waits up to 120 s
/// (`api.py` `_run_on_main`); only a request not yet started by then is
/// cancelled (504), one already running is waited for to the end. So the
/// client must wait longer than 120 s, or it could report an edit failed
/// that RoboCAD then applies. An edit that runs longer still times out here
/// with a message saying RoboCAD may still apply it ([`CadError`]).
pub const EDIT_TIMEOUT: Duration = Duration::from_secs(130);
/// api.py's default mesh tolerance (`/nodes/{id}/mesh?tolerance=`, mm).
pub const MESH_TOLERANCE: f64 = 0.1;

/// Appended to the timeout of a request that may edit the document.
const MAY_STILL_APPLY: &str = "RoboCAD may still apply it; refresh before retrying";
/// Appended to an edit whose 2xx answer could not be decoded.
const APPLIED_UNREAD: &str = "RoboCAD applied it, but its answer could not be read; refresh before retrying";

/// Why a RoboCAD request failed.
#[derive(Clone, Debug, PartialEq)]
pub struct CadError {
    /// `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, or `-` when nothing was sent
    /// (a refused URL).
    pub method: &'static str,
    /// The path as sent (e.g. `/nodes/abc/mesh?tolerance=0.1`), or the URL
    /// refused.
    pub route: String,
    /// The HTTP status of an error answer.
    pub status: Option<u16>,
    /// RoboCAD's `error` text verbatim, or the transport's reason.
    pub message: String,
}
impl CadError {
    /// A transport timeout of a request that may edit the document (any
    /// method but `GET`, after connecting: a connect timeout sent nothing)
    /// adds that RoboCAD may still apply it.
    fn from_transport(method: &'static str, route: &str, e: loopback_http::Error) -> CadError {
        let (status, message) = match e {
            loopback_http::Error::Server { status, error } => (Some(status), error),
            loopback_http::Error::Transport(m) if method != "GET" && m.contains("timed out") && !m.contains(": connect: ") => {
                let sep = if m.ends_with("timed out") { ": " } else { " (timed out): " };
                (None, format!("{m}{sep}{MAY_STILL_APPLY}"))
            }
            // A 2xx answer that could not be decoded: RoboCAD applied the edit.
            loopback_http::Error::Decode(m) if method != "GET" && (m.contains(": the answer is not JSON") || m.contains(": unexpected answer: ")) => (None, format!("{m}: {APPLIED_UNREAD}")),
            // The request may have reached RoboCAD: the connection closed
            // after it was written, or the answer could not be read.
            loopback_http::Error::Transport(m) | loopback_http::Error::Decode(m) if method != "GET" && may_have_arrived(method, &m) => (None, format!("{m}: {MAY_STILL_APPLY}")),
            loopback_http::Error::NotLoopback(m) | loopback_http::Error::Transport(m) | loopback_http::Error::Decode(m) => (None, m),
        };
        CadError { method, route: route.to_string(), status, message }
    }
    /// RoboCAD answered 404 (no such node, command, op or load).
    pub fn not_found(&self) -> bool {
        self.status == Some(404)
    }
    /// RoboCAD answered 409: the route needs a desktop window (or the
    /// document changed under the request).
    pub fn no_gui(&self) -> bool {
        self.status == Some(409)
    }
}
/// Whether a failed request may have reached RoboCAD: anything after the
/// connection was made and the request written (a closed connection, a
/// failed write or read, an answer that could not be decoded), not a
/// refusal before sending, a failed connect or a failed socket setup.
fn may_have_arrived(method: &str, message: &str) -> bool {
    message.starts_with(&format!("{method} ")) && !message.contains(": connect: ") && !message.contains(": set timeout: ")
}
impl std::fmt::Display for CadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RoboCAD {} {}: {}", self.method, self.route, self.message)?;
        if let Some(status) = self.status {
            if !self.message.contains(&format!("HTTP {status}")) {
                write!(f, " (HTTP {status})")?;
            }
        }
        Ok(())
    }
}
impl std::error::Error for CadError {}

/// A client of one RoboCAD service. Cheap to clone; each request opens its
/// own connection.
#[derive(Clone, Debug)]
pub struct CadClient {
    pub endpoint: Endpoint,
    /// Read/write timeout per request ([`REQUEST_TIMEOUT`]).
    pub timeout: Duration,
}

/// `PUT /selection`'s body.
#[derive(Serialize)]
struct SelectionBody<'a> {
    items: &'a [SelectionItem],
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<&'a str>,
}

/// `POST /ops/{name}`'s body.
#[derive(Serialize)]
struct OpBody<'a> {
    args: &'a [Value],
    kwargs: &'a Map<String, Value>,
}

/// `POST /clipboard/copy`'s body.
#[derive(Serialize)]
struct CopyBody<'a> {
    ids: &'a [String],
}

/// `POST /clipboard/paste`'s body.
#[derive(Serialize)]
struct PasteBody<'a> {
    clip: &'a Value,
}

/// `POST /save` and `/open`'s body (`{}` without a path).
#[derive(Serialize)]
struct PathBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<&'a str>,
}

impl CadClient {
    /// A client of the RoboCAD service at `url` (`http://127.0.0.1:PORT` or
    /// `http://localhost:PORT`; anything else is refused, naming the URL).
    /// No I/O.
    pub fn new(url: &str) -> Result<CadClient, CadError> {
        let endpoint = Endpoint::parse(url).map_err(|e| CadError::from_transport("-", url, e))?;
        Ok(CadClient { endpoint, timeout: REQUEST_TIMEOUT })
    }
    /// `http://127.0.0.1:PORT`.
    pub fn url(&self) -> String {
        self.endpoint.origin()
    }
    /// The same client with another read/write timeout, at least 1 ms (a
    /// zero timeout is an error for `set_read_timeout`).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout.max(Duration::from_millis(1));
        self
    }

    /// `GET /`: whether the service is up, its document and revision.
    pub fn health(&self) -> Result<Health, CadError> {
        self.get("/")
    }
    /// `GET /doc`: tree, materials, selection, view and history.
    pub fn doc(&self) -> Result<DocState, CadError> {
        self.get("/doc")
    }
    /// `GET /nodes` (every node) or `/nodes?kind=K`.
    pub fn nodes(&self, kind: Option<&str>) -> Result<Vec<NodeSummary>, CadError> {
        // Lenient like `/doc`'s node list: one malformed node is dropped, not the answer.
        let route = match kind {
            Some(kind) => format!("/nodes?kind={}", encode_uri_component(kind)),
            None => "/nodes".to_string(),
        };
        let value: Value = self.get(&route)?;
        match value {
            Value::Array(items) => Ok(items.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect()),
            other => Err(CadError { method: "GET", route, status: None, message: format!("unexpected answer: expected a list of nodes, got {other}") }),
        }
    }
    /// `GET /nodes/{id}`: details including mass properties.
    pub fn node(&self, id: &str) -> Result<NodeDetail, CadError> {
        self.get(&node_route(id))
    }
    /// `PATCH /nodes/{id}` with `attrs` as given (api.py accepts name,
    /// visible, locked, disabled, material, color, pivot, transform, parent,
    /// index, tessellation_tolerance, plane, sketch); the node afterwards.
    pub fn patch(&self, id: &str, attrs: &Map<String, Value>) -> Result<NodeDetail, CadError> {
        self.send("PATCH", &node_route(id), Some(attrs))
    }
    /// `DELETE /nodes/{id}`.
    pub fn delete(&self, id: &str) -> Result<Deleted, CadError> {
        self.call("DELETE", &node_route(id), None)
    }
    /// `GET /nodes/{id}/mesh?tolerance=T` (mm); `None` when RoboCAD answers
    /// 404 "no mesh" exactly (a node without geometry; api.py also says so
    /// for an unknown id). Any other 404 (an unknown route, …) is an error.
    pub fn mesh(&self, id: &str, tolerance: f64) -> Result<Option<MeshData>, CadError> {
        let route = format!("{}/mesh?tolerance={}", node_route(id), encode_uri_component(&tolerance.to_string()));
        match self.get(&route) {
            Ok(mesh) => Ok(Some(mesh)),
            Err(e) if e.not_found() && e.message == "no mesh" => Ok(None),
            Err(e) => Err(e),
        }
    }
    /// `GET /nodes/{id}/faces`: the body's faces in kernel order (`index`
    /// is the face's index in `[node, "face", index]` and
    /// `triangle_face`). A node without geometry is a 404 ("X has no
    /// geometry", [`CadError::not_found`]).
    pub fn faces(&self, id: &str) -> Result<Vec<FaceInfo>, CadError> {
        self.get(&format!("{}/faces", node_route(id)))
    }
    /// `GET /nodes/{id}/edges`, or `/edges?samples=N` with each edge's
    /// sampled polyline in `points` (N points along a curve, 2 along a
    /// line; RoboCAD accepts 2..=256 and answers 400 otherwise). A node
    /// without geometry is a 404 as for [`CadClient::faces`].
    pub fn edges(&self, id: &str, samples: Option<u32>) -> Result<Vec<EdgeInfo>, CadError> {
        let route = match samples {
            Some(n) => format!("{}/edges?samples={n}", node_route(id)),
            None => format!("{}/edges", node_route(id)),
        };
        self.get(&route)
    }
    /// `GET /nodes/{id}/vertices`. A node without geometry is a 404 as for
    /// [`CadClient::faces`].
    pub fn vertices(&self, id: &str) -> Result<Vec<VertexInfo>, CadError> {
        self.get(&format!("{}/vertices", node_route(id)))
    }
    /// `GET /nodes/{id}/solids`: the solid inventory of the node's body
    /// (an empty list for a node without geometry; 404 for an unknown id).
    pub fn solids(&self, id: &str) -> Result<Solids, CadError> {
        self.get(&format!("{}/solids", node_route(id)))
    }
    /// `POST /clipboard/copy` `{"ids": [...]}`: RoboCAD's "Copy with
    /// Placement" (`Document.copy_nodes`): `{"robocad_clipboard": true,
    /// "items": [{"node", "brep", "sketch"}, …]}`, kept as given for
    /// [`CadClient::paste`]. Changes nothing (a read; an unknown id is a 404).
    pub fn copy_nodes(&self, ids: &[String]) -> Result<Value, CadError> {
        self.send("POST", "/clipboard/copy", Some(&CopyBody { ids }))
    }
    /// `POST /clipboard/paste` `{"clip": clip}`: RoboCAD's "Paste with
    /// Placement", one undo step "Paste" (an edit: use [`EDIT_TIMEOUT`]).
    /// A clip without `robocad_clipboard` is a 400 "Clipboard has no
    /// robocad content".
    pub fn paste(&self, clip: &Value) -> Result<Pasted, CadError> {
        self.send("POST", "/clipboard/paste", Some(&PasteBody { clip }))
    }
    /// `GET /nodes/{id}/control_points?face=i`: the face's B-spline poles.
    /// A face index out of range is a 400; a node without geometry a 404.
    pub fn control_points(&self, id: &str, face: i64) -> Result<ControlPoints, CadError> {
        self.get(&format!("{}/control_points?face={face}", node_route(id)))
    }
    /// `GET /nodes/{id}/curvature_comb` with RoboCAD's defaults (scale 5,
    /// 48 samples per edge). A node that is not a curve or sketch is a 400.
    pub fn curvature_comb(&self, id: &str) -> Result<CurvatureComb, CadError> {
        self.get(&format!("{}/curvature_comb", node_route(id)))
    }
    /// `GET /nodes/{id}/continuity`: each edge's G0/G1/G2/boundary grade
    /// with its polyline, and the counts. A node without geometry is a 404.
    pub fn continuity(&self, id: &str) -> Result<Continuity, CadError> {
        self.get(&format!("{}/continuity", node_route(id)))
    }
    /// `GET /ops`: each callable op's name and Python signature.
    pub fn ops(&self) -> Result<BTreeMap<String, String>, CadError> {
        self.get("/ops")
    }
    /// `POST /ops/{name}` `{"args": [...], "kwargs": {...}}`.
    pub fn op(&self, name: &str, args: &[Value], kwargs: &Map<String, Value>) -> Result<OpResult, CadError> {
        self.send("POST", &format!("/ops/{}", encode_uri_component(name)), Some(&OpBody { args, kwargs }))
    }
    /// `GET /commands`: the GUI's command registry (`{}` headless).
    pub fn commands(&self) -> Result<BTreeMap<String, CommandInfo>, CadError> {
        self.get("/commands")
    }
    /// `POST /commands/{id}` with `{}` (409 "no GUI" headless).
    pub fn run_command(&self, id: &str) -> Result<Ran, CadError> {
        self.send("POST", &format!("/commands/{}", encode_uri_component(id)), Some(&Map::new()))
    }
    /// `GET /history`.
    pub fn history(&self) -> Result<History, CadError> {
        self.get("/history")
    }
    /// `POST /undo` with `{}`.
    pub fn undo(&self) -> Result<Undone, CadError> {
        self.send("POST", "/undo", Some(&Map::new()))
    }
    /// `POST /redo` with `{}`.
    pub fn redo(&self) -> Result<Redone, CadError> {
        self.send("POST", "/redo", Some(&Map::new()))
    }
    /// `GET /selection`.
    pub fn selection(&self) -> Result<Selection, CadError> {
        self.get("/selection")
    }
    /// `PUT /selection` `{"items": [[node, kind, index], …]}`, plus `"mode"`
    /// only when given.
    pub fn set_selection(&self, items: &[SelectionItem], mode: Option<&str>) -> Result<Selection, CadError> {
        self.send("PUT", "/selection", Some(&SelectionBody { items, mode }))
    }
    /// `POST /save` with `{}` (the document's own path) or `{"path": p}`.
    pub fn save(&self, path: Option<&str>) -> Result<Saved, CadError> {
        self.send("POST", "/save", Some(&PathBody { path }))
    }
    /// `POST /open` `{"path": p}`: a desktop window opens it in a new
    /// window and loads it asynchronously ([`CadClient::load_status`]);
    /// headless answers 409.
    pub fn open(&self, path: &str) -> Result<Opened, CadError> {
        self.send("POST", "/open", Some(&PathBody { path: Some(path) }))
    }
    /// `GET /loads/{id}`.
    pub fn load_status(&self, load_id: &str) -> Result<LoadStatus, CadError> {
        self.get(&format!("/loads/{}", encode_uri_component(load_id)))
    }
    /// `DELETE /loads/{id}`: cancels the load; its status afterwards.
    pub fn cancel_load(&self, load_id: &str) -> Result<LoadStatus, CadError> {
        self.call("DELETE", &format!("/loads/{}", encode_uri_component(load_id)), None)
    }
    /// `GET /autosave` (409 headless).
    pub fn autosave(&self) -> Result<Autosave, CadError> {
        self.get("/autosave")
    }
    /// `GET /physical?flex=1|0`: the physical assembly description
    /// (simrobot v3, SI). Never passes `path`: RoboCAD would write a file.
    pub fn physical(&self, flex: bool) -> Result<Value, CadError> {
        self.get(if flex { "/physical?flex=1" } else { "/physical?flex=0" })
    }
    /// `POST /export` `{"format", "path", "settings"?, "ids"?}`: RoboCAD
    /// writes the file at `path`.
    pub fn export(&self, request: &ExportRequest) -> Result<Exported, CadError> {
        self.send("POST", "/export", Some(request))
    }

    /// `GET route`, decoded.
    fn get<T: DeserializeOwned>(&self, route: &str) -> Result<T, CadError> {
        self.call("GET", route, None)
    }
    /// `method route` with `body` serialised as JSON, decoded.
    fn send<B: Serialize + ?Sized, T: DeserializeOwned>(&self, method: &'static str, route: &str, body: Option<&B>) -> Result<T, CadError> {
        let text = match body {
            Some(body) => Some(serde_json::to_string(body).map_err(|e| CadError { method, route: route.to_string(), status: None, message: format!("could not write the request: {e}") })?),
            None => None,
        };
        self.call(method, route, text)
    }
    /// One request: `Content-Type: application/json` with a body, no other
    /// header; the answer, with bare non-finite numbers read as `null`
    /// ([`null_non_finite`]), decoded as `T`.
    fn call<T: DeserializeOwned>(&self, method: &'static str, route: &str, body: Option<String>) -> Result<T, CadError> {
        let headers: &[(&str, &str)] = if body.is_some() { &[("Content-Type", "application/json")] } else { &[] };
        let request = Request { method, path: route, headers, body: body.as_deref(), closed_hint: "" };
        let fail = |e: loopback_http::Error| CadError::from_transport(method, route, e);
        let text = loopback_http::exchange(&self.endpoint, self.timeout, &request).map_err(fail)?;
        let value = loopback_http::json(route, &null_non_finite(&text)).map_err(fail)?;
        loopback_http::decode(route, value).map_err(fail)
    }
}

/// `/nodes/{id}` with the id percent-encoded.
fn node_route(id: &str) -> String {
    format!("/nodes/{}", encode_uri_component(id))
}

/// `text` with every bare `NaN`, `Infinity` and `-Infinity` value token
/// outside JSON strings replaced by `null` (Python's `json.dumps` writes
/// non-finite floats so; JSON has no such token). A token is replaced only
/// as a whole value: at the start or after `[`, `,`, `:` or whitespace, and
/// followed by the end, `,`, `]`, `}` or whitespace. String contents
/// (including escaped quotes) are never touched. Borrows when there is
/// nothing to replace.
fn null_non_finite(text: &str) -> Cow<'_, str> {
    const TOKENS: [&str; 3] = ["-Infinity", "Infinity", "NaN"];
    let is_space = |b: u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r');
    let bytes = text.as_bytes();
    let mut out: Option<String> = None;
    // Bytes of `text` before this index are already in `out`.
    let mut copied = 0;
    let mut in_string = false;
    let mut escaped = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if b == b'"' {
            in_string = true;
            i += 1;
            continue;
        }
        if matches!(b, b'N' | b'I' | b'-') && (i == 0 || matches!(bytes[i - 1], b'[' | b',' | b':') || is_space(bytes[i - 1])) {
            if let Some(token) = TOKENS.iter().find(|t| bytes[i..].starts_with(t.as_bytes())) {
                let end = i + token.len();
                if end == bytes.len() || matches!(bytes[end], b',' | b']' | b'}') || is_space(bytes[end]) {
                    // `i` and `end` sit on ASCII bytes: char boundaries.
                    let out = out.get_or_insert_with(|| String::with_capacity(text.len()));
                    out.push_str(&text[copied..i]);
                    out.push_str("null");
                    copied = end;
                    i = end;
                    continue;
                }
            }
        }
        i += 1;
    }
    match out {
        Some(mut out) => {
            out.push_str(&text[copied..]);
            Cow::Owned(out)
        }
        None => Cow::Borrowed(text),
    }
}

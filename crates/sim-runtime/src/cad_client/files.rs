//! RoboCAD's file routes beyond `save`, `open` and `export` (cad-views-export):
//! import with units, the headless render as PNG bytes, and the three gap
//! routes api.py added for the native viewer's file workflows: `POST
//! /save/thumbnail` (/save with the desktop's thumbnail), `POST /new` (an
//! empty `.rcad` written beside the open document, which is not touched)
//! and `GET /import/units` (the mesh unit prompt's guess).
//!
//! Answers are tolerant as the other types are: a missing or `null` field
//! reads as its default, an unknown one is ignored.
use super::{CadClient, CadError, PathBody};
use crate::hardware_client::encode_uri_component;
use crate::loopback_http::{self, Request};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

/// Read/write timeout for an export, an import, a render or a save with
/// its thumbnail: RoboCAD runs them to the end on its request thread (a
/// headless service under its document lock; a desktop on its Qt thread,
/// which waits up to 120 s to start one), and a large model's STEP export
/// or software render can take minutes. Use with
/// `client.clone().with_timeout(FILE_TIMEOUT)`.
pub const FILE_TIMEOUT: Duration = Duration::from_secs(600);

/// The mesh files RoboCAD imports through `importers.import_mesh` with a
/// unit (`MainWindow.import_path`; `GET /import/units` answers for these).
pub const MESH_EXTENSIONS: &[&str] = &["stl", "obj", "3mf", "fbx", "ply", "glb", "gltf"];
/// Every file `POST /import` takes: the desktop's Import filter
/// (`MainWindow.import_file`: STEP, IGES, STL, OBJ, 3MF, FBX, SVG, PNG, JPEG)
/// plus the meshes `import_path` also reads (PLY, glTF).
pub const IMPORT_EXTENSIONS: &[&str] = &["step", "stp", "iges", "igs", "stl", "obj", "3mf", "fbx", "ply", "glb", "gltf", "svg", "png", "jpg", "jpeg"];
/// The mesh unit prompt's choices (`UnitsDialog`, ui/widgets.py), in order.
pub const IMPORT_UNITS: &[&str] = &["mm", "cm", "m", "in", "ft"];
/// `GET /render`'s named views (`Service.render`'s presets), in its order;
/// `"dx,dy,dz"` is also accepted.
pub const RENDER_VIEWS: &[&str] = &["iso", "front", "back", "right", "left", "top", "bottom", "iso2", "under"];
/// `GET /render`'s modes (`io/snapshot.py` `render`).
pub const RENDER_MODES: &[&str] = &["shaded", "xray", "wireframe"];

/// A path's extension, lower case, without the dot ("" when none).
pub fn extension(path: &str) -> String {
    std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// `POST /import`'s answer: the new nodes' ids.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Imported {
    pub imported: Vec<Value>,
}

/// `POST /new`'s answer: the file written.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct NewFile {
    pub created: String,
}

/// `POST /save/thumbnail`'s answer: the file written and whether a
/// thumbnail went in (a failed one saves without, as the desktop's Save).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ThumbnailSaved {
    pub saved: String,
    pub thumbnail: bool,
}

/// `GET /import/units`'s answer: the mesh's largest raw extent (in the
/// file's own units), RoboCAD's guess and the prompt's choices.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct MeshUnits {
    pub path: String,
    pub extent: Option<f64>,
    pub guess: String,
    pub units: Vec<String>,
}

/// `GET /render`'s query (`Service.render`; absent options take RoboCAD's
/// defaults: iso, 1200×900, shaded, edges on, labels off, tolerance 0.15).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct RenderRequest {
    /// A preset ([`RENDER_VIEWS`]) or `"dx,dy,dz"`.
    pub view: Option<String>,
    pub w: Option<u32>,
    pub h: Option<u32>,
    /// [`RENDER_MODES`].
    pub mode: Option<String>,
    /// `"x|y|z:value"` (mm).
    pub section: Option<String>,
    /// Only these nodes.
    pub ids: Option<Vec<String>>,
    pub highlight: Option<Vec<String>>,
    pub labels: Option<bool>,
    pub edges: Option<bool>,
    /// Frame this node.
    pub focus: Option<String>,
    /// Tessellation tolerance (mm).
    pub tolerance: Option<f64>,
    pub title: Option<String>,
}
impl RenderRequest {
    /// The route with its query, each value percent-encoded (ids joined
    /// with `,`, booleans as 1/0), in api.py's documented order.
    pub fn route(&self) -> String {
        let mut q: Vec<String> = Vec::new();
        let mut put = |key: &str, value: Option<String>| {
            if let Some(v) = value {
                q.push(format!("{key}={v}"));
            }
        };
        let text = |s: &Option<String>| s.as_deref().map(encode_uri_component);
        let list = |ids: &Option<Vec<String>>| ids.as_ref().map(|ids| ids.iter().map(|i| encode_uri_component(i)).collect::<Vec<_>>().join(","));
        let flag = |b: Option<bool>| b.map(|b| if b { "1" } else { "0" }.to_string());
        put("view", text(&self.view));
        put("w", self.w.map(|w| w.to_string()));
        put("h", self.h.map(|h| h.to_string()));
        put("mode", text(&self.mode));
        put("section", text(&self.section));
        put("ids", list(&self.ids));
        put("highlight", list(&self.highlight));
        put("labels", flag(self.labels));
        put("edges", flag(self.edges));
        put("focus", text(&self.focus));
        put("tolerance", self.tolerance.map(|t| encode_uri_component(&t.to_string())));
        put("title", text(&self.title));
        if q.is_empty() { "/render".to_string() } else { format!("/render?{}", q.join("&")) }
    }
}

/// `POST /import`'s body: `unit` only when given (api.py's default is mm).
#[derive(Serialize)]
struct ImportBody<'a> {
    path: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    unit: Option<&'a str>,
}

/// The PNG signature.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

impl CadClient {
    /// `POST /import` `{"path", "unit"?}`: RoboCAD adds the file's
    /// contents to the document (STEP, IGES, SVG, an image, else a mesh in
    /// `unit`). An edit: use [`FILE_TIMEOUT`].
    pub fn import(&self, path: &str, unit: Option<&str>) -> Result<Imported, CadError> {
        self.send("POST", "/import", Some(&ImportBody { path, unit }))
    }
    /// `POST /save/thumbnail` `{}` (the document's own path) or `{"path"}`:
    /// /save with the desktop's thumbnail.
    pub fn save_with_thumbnail(&self, path: Option<&str>) -> Result<ThumbnailSaved, CadError> {
        self.send("POST", "/save/thumbnail", Some(&PathBody { path }))
    }
    /// `POST /new` `{"path"}`: RoboCAD writes an empty document to `path`
    /// (a `.rcad` that must not exist: 409 otherwise); the open document
    /// is not touched.
    pub fn new_file(&self, path: &str) -> Result<NewFile, CadError> {
        self.send("POST", "/new", Some(&PathBody { path: Some(path) }))
    }
    /// `GET /import/units?path=…`: the unit prompt's guess for a mesh file
    /// (400 for another kind of file, 404 when it does not exist, 422 when
    /// it cannot be read). Changes nothing.
    pub fn mesh_units(&self, path: &str) -> Result<MeshUnits, CadError> {
        self.get(&format!("/import/units?path={}", encode_uri_component(path)))
    }
    /// `GET /render?…`: the PNG's bytes (an answer that is not a PNG is an
    /// error naming the route). Changes nothing.
    pub fn render(&self, request: &RenderRequest) -> Result<Vec<u8>, CadError> {
        let route = request.route();
        let raw = Request { method: "GET", path: &route, headers: &[], body: None, closed_hint: "" };
        let bytes = loopback_http::exchange_bytes(&self.endpoint, self.timeout, &raw).map_err(|e| CadError::from_transport("GET", &route, e))?;
        if !bytes.starts_with(PNG) {
            return Err(CadError { method: "GET", route, status: None, message: format!("the answer is not a PNG image ({} bytes)", bytes.len()) });
        }
        Ok(bytes)
    }
}

//! Reference images (references.py, api.py) as typed calls:
//!
//! - `import_references(paths, plane)`: one image node per file (locked,
//!   100 mm wide, opacity 0.6, on `plane`; RoboCAD reads the files on its
//!   side and embeds their bytes); one undo step "Import references";
//!   `result` is the new node ids.
//! - `update_reference(node_id, width, opacity, origin, plane,
//!   rotation_deg, visible, locked, name)`: only the given keys; one undo
//!   step "Edit reference"; `result` is the node id. A new plane resets the
//!   rotation; `origin` keeps the plane's axes.
//! - `calibrate_reference(node_id, first, second, distance)`: rescales so
//!   the two picked points (mm, world) lie `distance` mm apart, keeping the
//!   first stationary (through `update_reference`, one undo step).
//! - `GET /nodes/{id}/image` (the cad-organize gap route, read-only): the
//!   stored bytes, base64, with their format and pixel size.
//!
//! The writes are RoboCAD edits: use a client with [`super::EDIT_TIMEOUT`].
//! Images are fetched whole; the loopback transport caps an answer at
//! 64 MiB, so stored bytes over about 48 MiB (base64 adds a third) cannot
//! be read through [`CadClient::reference_image`].
//!
//! A reference image node's placement is `NodeDetail::image` ([`ImagePlacement::of`]):
//! `node_detail` strips the bytes and writes the plane as `Plane.to_json`.
use super::{CadClient, CadError, NodeDetail, OpResult, node_route};
use crate::hardware_client::lenient;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// A plane as RoboCAD's `Plane.to_json` writes it (mm, unit vectors).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct PlaneJson {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
    pub x_axis: [f64; 3],
}
impl Default for PlaneJson {
    /// `Plane()`: XY through the origin.
    fn default() -> Self {
        PlaneJson { origin: [0.0; 3], normal: [0.0, 0.0, 1.0], x_axis: [1.0, 0.0, 0.0] }
    }
}
impl PlaneJson {
    /// `Plane.y_axis`: normal × x_axis through `v_unit` (kernel/base.py:
    /// below a length of 1e-12 it is (0, 0, 1)).
    pub fn y_axis(&self) -> [f64; 3] {
        let (n, x) = (self.normal, self.x_axis);
        let c = [n[1] * x[2] - n[2] * x[1], n[2] * x[0] - n[0] * x[2], n[0] * x[1] - n[1] * x[0]];
        let len = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
        if len < 1e-12 { [0.0, 0.0, 1.0] } else { [c[0] / len, c[1] / len, c[2] / len] }
    }
    /// `Plane.to_world(u, v)`.
    pub fn to_world(&self, u: f64, v: f64) -> [f64; 3] {
        let y = self.y_axis();
        std::array::from_fn(|i| self.origin[i] + self.x_axis[i] * u + y[i] * v)
    }
}

/// A reference image's placement (`node_detail`'s `image` without the
/// bytes): the plane (its origin is the image's corner, u along `x_axis`,
/// v along the y axis), width and height in mm, opacity 0..1, rotation in
/// degrees and the source path.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ImagePlacement {
    pub path: String,
    #[serde(deserialize_with = "lenient")]
    pub plane: PlaneJson,
    pub width: f64,
    pub height: f64,
    pub opacity: f64,
    pub rotation_deg: f64,
}
impl Default for ImagePlacement {
    /// `import_references`' defaults: 100 mm wide, opacity 0.6.
    fn default() -> Self {
        ImagePlacement { path: String::new(), plane: PlaneJson::default(), width: 100.0, height: 100.0, opacity: 0.6, rotation_deg: 0.0 }
    }
}
impl ImagePlacement {
    /// From `NodeDetail::image` (None when the node is not an image or the
    /// value is malformed).
    pub fn of(detail: &NodeDetail) -> Option<ImagePlacement> {
        detail.image.as_ref().and_then(|v| serde_json::from_value(v.clone()).ok())
    }
    /// The four corners in RoboCAD's frame (mm): (0,0), (w,0), (w,h), (0,h).
    pub fn corners(&self) -> [[f64; 3]; 4] {
        let p = &self.plane;
        [p.to_world(0.0, 0.0), p.to_world(self.width, 0.0), p.to_world(self.width, self.height), p.to_world(0.0, self.height)]
    }
}

/// `update_reference`'s keyword arguments: only the given ones are sent.
/// `plane` is what `ArgConverter.plane` reads: "xy" | "xz" | "yz", a plane
/// node id, or `{origin, normal, x_axis}`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ReferenceUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plane: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation_deg: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// `GET /nodes/{id}/image`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ReferenceImage {
    pub id: String,
    /// RoboCAD's document revision the bytes were read at.
    pub revision: u64,
    /// Pillow's format name, lower case ("png", "jpeg", "webp", "bmp").
    pub format: String,
    pub width_px: u32,
    pub height_px: u32,
    pub bytes: u64,
    /// The stored bytes, base64 (RFC 4648, padded).
    pub data: String,
}
impl ReferenceImage {
    /// The stored bytes, or why they cannot be decoded.
    pub fn decode(&self) -> Result<Vec<u8>, String> {
        let out = base64_decode(&self.data)?;
        if out.len() as u64 != self.bytes {
            return Err(format!("{}: {} bytes decoded, RoboCAD said {}", self.id, out.len(), self.bytes));
        }
        Ok(out)
    }
}

/// Standard base64 (`A–Z a–z 0–9 + /`, `=` padding; whitespace ignored),
/// as Python's `base64.b64encode` writes it.
pub fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    fn value(c: u8) -> Option<u32> {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        Some(u32::from(v))
    }
    let clean: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if clean.len() % 4 != 0 {
        return Err(format!("base64 of length {} is not a multiple of 4", clean.len()));
    }
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for (n, chunk) in clean.chunks(4).enumerate() {
        let last = n + 1 == clean.len() / 4;
        let pad = chunk.iter().rev().take_while(|b| **b == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return Err("base64 padding in the middle of the data".into());
        }
        let mut acc = 0u32;
        for (i, b) in chunk.iter().enumerate() {
            let v = if i >= 4 - pad { 0 } else { value(*b).ok_or_else(|| format!("{:?} is not a base64 character", *b as char))? };
            acc = (acc << 6) | v;
        }
        let bytes = [(acc >> 16) as u8, (acc >> 8) as u8, acc as u8];
        out.extend_from_slice(&bytes[..3 - pad]);
    }
    Ok(out)
}

impl CadClient {
    /// `import_references(paths, plane)` (`plane` None: XY): `result` is the
    /// new node ids. RoboCAD reads `paths` on its side.
    pub fn import_references(&self, paths: &[String], plane: Option<&Value>) -> Result<OpResult, CadError> {
        self.op("import_references", &[json!(paths), plane.cloned().unwrap_or(Value::Null)], &Map::new())
    }
    /// `update_reference(node_id, **update)`.
    pub fn update_reference(&self, node_id: &str, update: &ReferenceUpdate) -> Result<OpResult, CadError> {
        let kwargs = match serde_json::to_value(update) {
            Ok(Value::Object(map)) => map,
            _ => Map::new(),
        };
        self.op("update_reference", &[json!(node_id)], &kwargs)
    }
    /// `calibrate_reference(node_id, first, second, distance)` (mm).
    pub fn calibrate_reference(&self, node_id: &str, first: [f64; 3], second: [f64; 3], distance: f64) -> Result<OpResult, CadError> {
        self.op("calibrate_reference", &[json!(node_id), json!(first), json!(second), json!(distance)], &Map::new())
    }
    /// `GET /nodes/{id}/image` (read-only gap route).
    pub fn reference_image(&self, id: &str) -> Result<ReferenceImage, CadError> {
        self.get(&format!("{}/image", node_route(id)))
    }
}

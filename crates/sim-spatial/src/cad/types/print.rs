//! RoboCAD's print routes (cad-print; `api.py` `Service.print_request`
//! :300-333, `print_jobs.py`, the `/nodes/{id}/thin|validate` reads and the
//! print `Ops` methods), typed for the native viewer's Print menu.
//!
//! - `GET /print/registry`: the printing registry's printers (with the
//!   usable size `print_registry.usable_mm` computes) and filaments, its
//!   path, sha256 and revision. The printers and filaments keep the
//!   registry's order ([`Ordered`]): RoboCAD's dialogs list them in it and
//!   preselect the first.
//! - `GET /print/study` (a gap route, added with cad-print): the document's
//!   print study (`robot_settings["print_study"]`, the `/print/analyze`
//!   body RoboCAD's Check strength and Plan send) and the split groups
//!   (nodes whose `robot.print_split` is set), at the document's revision.
//! - `POST /print/split`: synchronous (seconds; the pieces are published as
//!   one undo step and the summary answered), or with `background: true` a
//!   job (RoboCAD's own menu uses the job, `split_job`), answered 200 like
//!   the synchronous split (api.py:1412 answers 202 only for the others).
//! - `POST /print/analyze|plan|assembly|coupons|strength_split`: each starts
//!   a background job and answers it, 202 ([`PrintJob`], `Job.public`).
//! - `GET /print/jobs` (every job this RoboCAD started, oldest first),
//!   `GET /print/jobs/{id}` and `DELETE /print/jobs/{id}` (cancel: the job
//!   stops at its next check and ends `cancelled`). `GET /print/jobs/{id}`
//!   reads its `wait` from the request body, which RoboCAD never parses for
//!   a `GET` (api.py:1395), so a wait is never honoured: poll instead.
//! - `GET /nodes/{id}/thin?threshold=` (`wall_thickness`: sample points
//!   whose wall is thinner, mm) and `GET /nodes/{id}/validate` (the
//!   kernel's validation report).
//! - The `Ops` methods `clearance`, `fastener_hole` and `print_split` with
//!   `commands.py`'s parameter names, each one RoboCAD undo step.
//!
//! Errors are RoboCAD's text verbatim ([`CadError`]), mapped by
//! `print_request`'s `except` clauses (api.py:330-332):
//! - An unknown job is a 404 whose text is the `KeyError`'s repr, quotes
//!   included (`'no print job x'`).
//! - The synchronous split (print_jobs.py `split`): an unknown printer is a
//!   `KeyError`, so a 404 with its repr (`"printer 'x' is not in the print
//!   registry (have: …)"`); a document changed since `expected_revision`
//!   (or before publishing) a 409; a missing `node`, a node that is not a
//!   body or a kernel error a 422. `split` reads only the `SplitOptions`
//!   keys it names; any other key is ignored, not refused.
//! - The split job (`background: true`, `split_job`) runs every one of those
//!   checks inside the job: the start answers 200 with the queued job
//!   whatever the body, and a failed check shows later as the job ending
//!   `failed` with `error` = `str(e)` (a `KeyError`'s repr, quotes
//!   included).
//! - The job-start routes (`analyze|plan|assembly|coupons|strength_split`)
//!   take their snapshot before starting, so a stale `expected_revision` is
//!   a synchronous 409 there; their other checks fail the job.
//!
//! Reads are tolerant: unknown fields are ignored, missing ones take their
//! defaults.
use sim_runtime::hardware::protocol::{lenient, lenient_items};
use serde::de::{DeserializeOwned, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use std::fmt;
use std::marker::PhantomData;

/// The joint choices of RoboCAD's "Split for printing" dialog
/// (ui/app.py:1179; `SplitOptions.joint`).
pub const SPLIT_JOINTS: [&str; 4] = ["auto", "pins+screws", "dovetail", "pins"];
/// `FastenerDialog`'s sizes and kinds (ui/widgets.py:994-996).
pub const FASTENER_SIZES: [&str; 7] = ["M2", "M2.5", "M3", "M4", "M5", "M6", "M8"];
pub const FASTENER_KINDS: [&str; 5] = ["clearance", "tap", "counterbore", "countersink", "insert"];

/// A JSON object's entries in document order; an entry whose value does
/// not parse is dropped, anything but an object reads as empty. Written
/// back as an object in the same order. The order is kept only when decoded
/// from text (`serde_json::from_str`, as `get_ordered` does): this crate's
/// `serde_json::Value` objects are BTreeMaps, so `from_value` (or any path
/// through a `Value`) hands the entries over sorted by key.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ordered<T>(pub Vec<(String, T)>);
impl<T: Serialize> Serialize for Ordered<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}
impl<T> Ordered<T> {
    #[cfg(test)]
    pub fn get(&self, key: &str) -> Option<&T> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(k, _)| k.as_str())
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
impl<'de, T: DeserializeOwned> Deserialize<'de> for Ordered<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Entries<T>(PhantomData<T>);
        impl<'de, T: DeserializeOwned> Visitor<'de> for Entries<T> {
            type Value = Ordered<T>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Ordered<T>, A::Error> {
                let mut out = Vec::new();
                while let Some((k, v)) = map.next_entry::<String, Value>()? {
                    if let Ok(v) = serde_json::from_value(v) {
                        out.push((k, v));
                    }
                }
                Ok(Ordered(out))
            }
            // Anything but an object reads as empty (tolerant, as `lenient_map`).
            fn visit_unit<E: serde::de::Error>(self) -> Result<Ordered<T>, E> {
                Ok(Ordered(Vec::new()))
            }
            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Ordered<T>, E> {
                Ok(Ordered(Vec::new()))
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Ordered<T>, E> {
                Ok(Ordered(Vec::new()))
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Ordered<T>, E> {
                Ok(Ordered(Vec::new()))
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Ordered<T>, E> {
                Ok(Ordered(Vec::new()))
            }
            fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Ordered<T>, E> {
                Ok(Ordered(Vec::new()))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Ordered<T>, A::Error> {
                while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}
                Ok(Ordered(Vec::new()))
            }
        }
        deserializer.deserialize_any(Entries(PhantomData))
    }
}

/// One printer of the registry (`{"name", "usable_mm"}`).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct PrinterInfo {
    pub name: String,
    /// The box one piece must fit in (build volume less the edge margin in
    /// x and y), mm.
    #[serde(deserialize_with = "lenient")]
    pub usable_mm: Vec<f64>,
}
impl PrinterInfo {
    /// RoboCAD's "Printer:" entry: `"{id} ({x} × {y} × {z} mm)"` with
    /// Python's `:g` numbers (ui/app.py:1175).
    pub fn label(&self, id: &str, g: impl Fn(f64) -> String) -> String {
        format!("{id} ({} mm)", self.usable_mm.iter().map(|x| g(*x)).collect::<Vec<_>>().join(" × "))
    }
}

/// One filament of the registry (`{"name", "cad_material"}`).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct FilamentInfo {
    pub name: String,
    /// The CAD material it prints (a document material id).
    pub cad_material: Option<String>,
}

/// `GET /print/registry`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct PrintRegistry {
    pub path: Option<String>,
    pub sha256: Option<String>,
    /// The registry's own revision (a number in today's file).
    pub revision: Value,
    pub printers: Ordered<PrinterInfo>,
    pub materials: Ordered<FilamentInfo>,
}

/// `GET /print/study`: the document's print study and split groups.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct PrintStudy {
    pub revision: u64,
    /// `robot_settings["print_study"]`: the `/print/analyze` body (parts
    /// with fixtures and loads, printer, material, …); null when none.
    pub study: Value,
    /// The split groups (nodes with `robot.print_split`), in tree order.
    #[serde(deserialize_with = "lenient_items")]
    pub splits: Vec<String>,
}
impl PrintStudy {
    /// The study's parts' node ids (`study["parts"][i]["node"]`).
    pub fn part_nodes(&self) -> Vec<String> {
        self.study["parts"].as_array().map_or_else(Vec::new, |parts| parts.iter().filter_map(|p| p["node"].as_str().map(str::to_string)).collect())
    }
    /// Whether the document has a print study (RoboCAD's `if not study`).
    pub fn has_study(&self) -> bool {
        match &self.study {
            Value::Null => false,
            Value::Object(m) => !m.is_empty(),
            Value::Array(a) => !a.is_empty(),
            Value::String(s) => !s.is_empty(),
            Value::Bool(b) => *b,
            Value::Number(n) => n.as_f64() != Some(0.0),
        }
    }
}

/// One print job (`Job.public`, print_jobs.py:42-45).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct PrintJob {
    pub id: String,
    /// split | analyze | plan | strength_split | assembly | coupons.
    pub kind: String,
    /// queued | running | done | failed | cancelled.
    pub state: String,
    /// 0..1 (RoboCAD rounds it to 3 decimals).
    #[serde(deserialize_with = "lenient")]
    pub fraction: f64,
    #[serde(deserialize_with = "lenient")]
    pub message: String,
    #[serde(deserialize_with = "lenient")]
    pub error: Option<String>,
    /// The job's answer once done (its shape is the kind's).
    pub result: Value,
    #[serde(deserialize_with = "lenient")]
    pub out_dir: Option<String>,
    #[serde(deserialize_with = "lenient")]
    pub seconds: f64,
}
impl PrintJob {
    /// Queued or running (RoboCAD's `state in ('queued', 'running')`).
    pub fn running(&self) -> bool {
        matches!(self.state.as_str(), "queued" | "running")
    }
}

/// `POST /print/split`'s body (print_jobs.py `split`: `node` required;
/// `SplitOptions` keys passed through `options`; `expected_revision` makes
/// RoboCAD refuse with 409 when the document moved on).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct SplitRequest {
    pub node: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub printer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub joint: Option<String>,
    /// The new group's name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
    /// A job instead of the synchronous split (RoboCAD's menu's way).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub background: bool,
    /// Other `SplitOptions` keys (screw, pin_diameter, wall, area_per_screw,
    /// max_screws, extra_planes), as RoboCAD takes them. A key naming one of
    /// the fields above ([`SPLIT_FIELDS`]) is dropped before sending: the
    /// fields win.
    #[serde(flatten)]
    pub options: Map<String, Value>,
}

/// One hardware line of a split (`SplitResult.hardware`).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Hardware {
    pub item: String,
    pub size: String,
    pub count: u64,
}

/// A finished split (the synchronous answer, or a split job's `result`):
/// `SplitResult.summary` plus the revision, group and pieces.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct SplitDone {
    pub revision: u64,
    pub group: String,
    #[serde(deserialize_with = "lenient_items")]
    pub piece_nodes: Vec<String>,
    #[serde(deserialize_with = "lenient_items")]
    pub hardware: Vec<Hardware>,
    pub printer: Option<String>,
    #[serde(deserialize_with = "lenient")]
    pub usable_mm: Vec<f64>,
    pub registry_sha256: Option<String>,
    /// cuts, pieces, seams, source: as RoboCAD writes them.
    #[serde(flatten)]
    pub summary: Map<String, Value>,
}
impl SplitDone {
    /// RoboCAD's status text once a split job is done (ui/app.py:1183):
    /// "split into N pieces; hardware: 4× screw M3, …".
    pub fn status(&self) -> String {
        format!("split into {} pieces; hardware: {}", self.piece_nodes.len(), self.hardware.iter().map(|h| format!("{}× {} {}", h.count, h.item, h.size)).collect::<Vec<_>>().join(", "))
    }
}

/// One thin region (`printing.ThinRegion`): a sample point, the wall
/// thickness there (mm) and the face index it was sampled on.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ThinRegion {
    pub point: [f64; 3],
    pub thickness: f64,
    pub face: i64,
}

/// One issue of a validation report (`ValidationIssue`).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ValidationIssue {
    /// error | warning.
    pub severity: String,
    pub message: String,
    #[serde(deserialize_with = "lenient")]
    pub location: Option<[f64; 3]>,
    #[serde(deserialize_with = "lenient")]
    pub fix: Option<String>,
}

/// `GET /nodes/{id}/validate`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Validation {
    pub valid: bool,
    pub watertight: bool,
    #[serde(deserialize_with = "lenient_items")]
    pub issues: Vec<ValidationIssue>,
    pub summary: String,
}

/// `FastenerSpec` (printing.py:31-35) as `fastener_hole`'s `spec`.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct FastenerSpec {
    /// M2, M2.5, M3, M4, M5, M6 or M8.
    pub size: String,
    /// clearance, tap, counterbore, countersink or insert.
    pub kind: String,
    /// Added to the hole diameter (print shrink), mm.
    pub extra_clearance: f64,
    /// None: through.
    pub depth: Option<f64>,
}
impl Default for FastenerSpec {
    /// `FastenerSpec`'s dataclass defaults.
    fn default() -> Self {
        FastenerSpec { size: "M3".into(), kind: "clearance".into(), extra_clearance: 0.0, depth: None }
    }
}
impl FastenerSpec {
    /// RoboCAD's undo label (`FastenerSpec.label`): "M3 clearance".
    pub fn label(&self) -> String {
        format!("{} {}", self.size, self.kind)
    }
}



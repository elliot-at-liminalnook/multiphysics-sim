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
use super::{CadClient, CadError, OpResult, node_route, null_non_finite};
use crate::hardware_client::{encode_uri_component, lenient, lenient_items};
use crate::loopback_http::{self, Request};
use serde::de::{DeserializeOwned, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value, json};
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
    pub fn get(&self, key: &str) -> Option<&T> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(k, _)| k.as_str())
    }
    pub fn len(&self) -> usize {
        self.0.len()
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
    /// Done, failed or cancelled.
    pub fn ended(&self) -> bool {
        !self.running()
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

/// The keys `SplitRequest`'s own fields send. Flattened `options` go out
/// after them, so a duplicate would be sent twice and Python's `json.loads`
/// keeps the last: `options["background"]` could turn a synchronous split
/// into a job.
pub const SPLIT_FIELDS: [&str; 6] = ["node", "printer", "joint", "name", "expected_revision", "background"];

impl SplitRequest {
    /// The body sent: this request with the call's `background` and
    /// without the `options` that duplicate a field.
    fn body(&self, background: bool) -> SplitRequest {
        let mut body = SplitRequest { background, ..self.clone() };
        for key in SPLIT_FIELDS {
            body.options.remove(key);
        }
        body
    }
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

/// A face as `ArgConverter.face` takes it (api.py:176-186).
fn face_ref(node: &str, face: i64) -> Value {
    json!({"node": node, "face": face})
}

impl CadClient {
    /// `GET /print/registry`: printers and filaments in the registry's order.
    pub fn print_registry(&self) -> Result<PrintRegistry, CadError> {
        self.get_ordered("/print/registry")
    }
    /// `GET /print/study` (gap route): the print study and the split groups.
    pub fn print_study(&self) -> Result<PrintStudy, CadError> {
        self.get("/print/study")
    }
    /// `POST /print/split` without `background`: the split, published as one
    /// undo step ("Split {name} for printing"), answered when done. Errors
    /// as the module doc lists for the synchronous split (404 unknown
    /// printer, 409 stale revision, 422 not a body).
    pub fn print_split_now(&self, request: &SplitRequest) -> Result<SplitDone, CadError> {
        self.send("POST", "/print/split", Some(&request.body(false)))
    }
    /// `POST /print/split` with `background: true`: the split job, answered
    /// 200 and queued whatever the body. RoboCAD checks nothing before
    /// starting it: a missing node, an unknown printer, a stale
    /// `expected_revision` or a node that is not a body each end the job
    /// `failed` with `error` = Python's `str(e)` (an unknown printer's text
    /// is the `KeyError`'s repr, quotes included), read with [`Self::print_job`].
    pub fn print_split_job(&self, request: &SplitRequest) -> Result<PrintJob, CadError> {
        self.send("POST", "/print/split", Some(&request.body(true)))
    }
    /// `POST /print/{kind}` for kind analyze, plan, assembly, coupons or
    /// strength_split with RoboCAD's body: the started job. Any other kind
    /// is refused here, with nothing sent.
    pub fn print_start(&self, kind: &str, body: &Value) -> Result<PrintJob, CadError> {
        if !matches!(kind, "analyze" | "plan" | "assembly" | "coupons" | "strength_split") {
            return Err(CadError { method: "-", route: format!("/print/{kind}"), status: None, message: format!("{kind} is not a print job (analyze, plan, assembly, coupons, strength_split)") });
        }
        self.send("POST", &format!("/print/{kind}"), Some(body))
    }
    /// `GET /print/jobs`: every job, oldest first (a malformed one is dropped).
    pub fn print_jobs(&self) -> Result<Vec<PrintJob>, CadError> {
        match self.get::<Value>("/print/jobs")? {
            Value::Array(items) => Ok(items.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect()),
            other => Err(CadError { method: "GET", route: "/print/jobs".into(), status: None, message: format!("unexpected answer: expected a list of print jobs, got {other}") }),
        }
    }
    /// `GET /print/jobs/{id}` (never with a wait: RoboCAD ignores it on a GET).
    pub fn print_job(&self, id: &str) -> Result<PrintJob, CadError> {
        self.get(&format!("/print/jobs/{}", encode_uri_component(id)))
    }
    /// `DELETE /print/jobs/{id}`: cancel; the job as it is now (still
    /// running until it reaches its next check).
    pub fn cancel_print_job(&self, id: &str) -> Result<PrintJob, CadError> {
        self.send::<Value, _>("DELETE", &format!("/print/jobs/{}", encode_uri_component(id)), None)
    }
    /// `GET /nodes/{id}/thin?threshold=`: sample points whose wall is
    /// thinner than `threshold` mm. A non-finite threshold is refused here,
    /// with nothing sent.
    pub fn thin_walls(&self, id: &str, threshold: f64) -> Result<Vec<ThinRegion>, CadError> {
        if !threshold.is_finite() {
            return Err(CadError { method: "-", route: format!("{}/thin", node_route(id)), status: None, message: format!("the wall threshold must be a finite number of mm, got {threshold}") });
        }
        let route = format!("{}/thin?threshold={}", node_route(id), encode_uri_component(&threshold.to_string()));
        match self.get::<Value>(&route)? {
            Value::Array(items) => Ok(items.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect()),
            other => Err(CadError { method: "GET", route, status: None, message: format!("unexpected answer: expected a list of thin regions, got {other}") }),
        }
    }
    /// `GET /nodes/{id}/validate`.
    pub fn validate_node(&self, id: &str) -> Result<Validation, CadError> {
        self.get(&format!("{}/validate", node_route(id)))
    }

    /// `clearance(node_id, faces, amount)`: grow holes / shrink bosses by
    /// `amount` mm (RoboCAD remembers it as its `last_clearance`). Undo step "Clearance".
    pub fn clearance(&self, node: &str, faces: &[i64], amount: f64) -> Result<OpResult, CadError> {
        let faces: Vec<Value> = faces.iter().map(|f| face_ref(node, *f)).collect();
        self.op("clearance", &[json!(node), Value::Array(faces), json!(amount)], &Map::new())
    }
    /// `fastener_hole(node_id, face, point, spec)`: the hole entering face
    /// `face` at `point` (mm). Undo step `spec.label()` ("M3 clearance").
    pub fn fastener_hole(&self, node: &str, face: i64, point: [f64; 3], spec: &FastenerSpec) -> Result<OpResult, CadError> {
        self.op("fastener_hole", &[json!(node), face_ref(node, face), json!(point), json!(spec)], &Map::new())
    }
    /// `print_split(node_id, **options)` (the `Ops` method: no snapshot,
    /// no job; `result` is the new group's id; undo step "Split for
    /// printing", print_split.py `apply_split`).
    ///
    /// It runs synchronously on RoboCAD's GUI thread (`POST /ops/{name}`
    /// goes through `run_on_main`) for seconds, freezing RoboCAD's window:
    /// prefer [`Self::print_split_job`]. `options` go to `SplitOptions(**options)`
    /// unfiltered, and `Service.op` maps only `RevisionConflict` (409) and
    /// `KernelError` (422): an unknown option key (`TypeError`) or an unknown
    /// printer (`KeyError`) falls through to the HTTP handler's 500, with the
    /// text `"{type}: {message}"`, e.g. `TypeError: SplitOptions.__init__()
    /// got an unexpected keyword argument 'x'` or `KeyError: "printer 'x' is
    /// not in the print registry (have: …)"`.
    pub fn print_split_op(&self, node: &str, options: &Map<String, Value>) -> Result<OpResult, CadError> {
        self.op("print_split", &[json!(node)], options)
    }

    /// `GET route`, decoded straight from RoboCAD's text, so objects keep
    /// their order ([`Ordered`]).
    fn get_ordered<T: DeserializeOwned>(&self, route: &str) -> Result<T, CadError> {
        let request = Request { method: "GET", path: route, headers: &[], body: None, closed_hint: "" };
        let fail = |e: loopback_http::Error| CadError::from_transport("GET", route, e);
        let text = loopback_http::exchange(&self.endpoint, self.timeout, &request).map_err(fail)?;
        serde_json::from_str(&null_non_finite(&text)).map_err(|e| fail(loopback_http::Error::Decode(format!("{route}: unexpected answer: {e}"))))
    }
}

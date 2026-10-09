//! Annotation threads and their messages over `api.py`'s
//! `annotation_request` (api.py:375-426) in `annotations.py`'s shapes
//! (`thread_detail`, `create_thread`, `update_thread`, `add_comment`,
//! `_change_comment`).
//!
//! - `GET /threads[?node_id&status&run_id]`, `GET /threads/{id}`: each
//!   thread as `thread_detail` writes it: the stored thread (`anchor`,
//!   `view`, `status`, times, `comments`, optional `evidence`,
//!   `part_refs`, `inspection_view`) plus `node_name`, `anchor_status`
//!   ("attached", "needs_review", "missing", "evidence") and
//!   `linked_parts` (`thread_parts` with `name` and `available`).
//! - `POST /threads` (201): `create_thread(**body)`; answers the thread.
//! - `PATCH /threads/{id}`: `update_thread(id, **body)` (status, a
//!   reattach's `node_id`/`point`/`face`/`view`, `part_refs`); answers the thread.
//! - `DELETE /threads/{id}`: `{"deleted": id}`.
//! - `POST /threads/{id}/comments` (201) `{"body", "author"}`: the new comment.
//! - `GET /comments/{id}`: the comment with `thread_id`; `PATCH` `{"body"}`:
//!   the comment; `DELETE`: `{"deleted": id}` (RoboCAD refuses the last
//!   comment of a thread: "delete the thread to remove its last comment").
//!
//! `POST /threads/{id}/show` is GUI-only (409 headless): the native viewer
//! reproduces it from [`CadThread::view`] and [`CadThread::linked_parts`]
//! (`sim_spatial::cad::threads`), so there is no client call for it.
//!
//! Reads are tolerant: a malformed thread in a list is dropped, not the
//! list; a malformed field reads as its default. Every write is a RoboCAD
//! edit (one undo step there): use a client with [`super::EDIT_TIMEOUT`].
use sim_runtime::hardware::protocol::{lenient, lenient_items};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// `annotations.py`'s `PART_LINK`: `[label](part:ID)`, ID of `[A-Za-z0-9_-]+`.
pub const PART_LINK_SCHEME: &str = "part:";

/// A thread's anchor as stored (`annotations.anchor`): the part, the
/// point (mm, RoboCAD's frame), the geometry stamp and the picked face's
/// description. Evidence threads have no part and no point.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ThreadAnchor {
    pub node_id: Option<String>,
    #[serde(deserialize_with = "lenient")]
    pub point: Option<[f64; 3]>,
    /// The anchored part's geometry fingerprint when the pin was placed.
    #[serde(deserialize_with = "lenient")]
    pub geometry: Option<String>,
    /// `Face.to_json()` of the picked face (absent without a face).
    pub face: Option<Value>,
}

/// Where a thread's pin stands now (`thread_detail`'s `anchor_status`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorStatus {
    Attached,
    /// The part's geometry changed since the pin was placed.
    NeedsReview,
    /// The part was deleted.
    Missing,
    /// An experiment-evidence thread (no part).
    Evidence,
    /// A state this client does not know, or none read (the default: a
    /// missing or malformed status never reads as attached).
    #[default]
    #[serde(other)]
    Unknown,
}

/// One message (`add_comment`): times are ISO 8601 UTC.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ThreadComment {
    pub id: String,
    pub author: String,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
    /// `GET /comments/{id}` only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

/// A linked part as stored (`validate_part_refs`): the node, and an
/// optional plain-language label (≤ 120), description (≤ 1000) and
/// saved view.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct PartRef {
    pub node_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<Value>,
}

/// A linked part as `thread_detail` answers it: the ref plus the part's
/// current name ("Deleted part" when gone) and whether it exists.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct LinkedPart {
    #[serde(flatten)]
    pub part: PartRef,
    pub name: String,
    pub available: bool,
}

/// One thread as `GET /threads` and `GET /threads/{id}` answer it.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct CadThread {
    pub id: String,
    #[serde(deserialize_with = "lenient")]
    pub anchor: ThreadAnchor,
    /// RoboCAD's camera when the pin was placed (`annotations.camera_view`:
    /// target, distance, yaw, pitch, fov, orthographic, mode, rot; `{}` when none).
    pub view: Value,
    /// "open" or "resolved".
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(deserialize_with = "lenient_items")]
    pub comments: Vec<ThreadComment>,
    pub evidence: Option<Value>,
    /// As stored; absent on older threads (their parts are implied:
    /// [`CadThread::linked_parts`] has them either way).
    #[serde(deserialize_with = "lenient_opt_items")]
    pub part_refs: Option<Vec<PartRef>>,
    pub inspection_view: Option<Value>,
    /// The anchored part's name, "Deleted part" or "Experiment evidence".
    pub node_name: String,
    #[serde(deserialize_with = "lenient")]
    pub anchor_status: AnchorStatus,
    #[serde(deserialize_with = "lenient_items")]
    pub linked_parts: Vec<LinkedPart>,
}
impl CadThread {
    pub fn resolved(&self) -> bool {
        self.status == "resolved"
    }
}

/// `deserialize_with` for an optional list: absent or `null` is `None`
/// (not stored); a list keeps the items that parse; anything else is `None`.
fn lenient_opt_items<'de, D, T>(deserializer: D) -> Result<Option<Vec<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = <Value as Deserialize>::deserialize(deserializer)?;
    Ok(match value {
        Value::Array(items) => Some(items.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect()),
        _ => None,
    })
}

/// `POST /threads`' body (`create_thread`): a part, a point (mm) and the
/// first message; the face index picked (as `GET /nodes/{id}/faces`
/// numbers them) and RoboCAD's camera are optional, as are linked parts.
/// `create_thread`'s `evidence` and `inspection_view` are deliberately not
/// sent: evidence threads belong to cad-experiments-motion.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct NewThread {
    pub node_id: String,
    pub point: [f64; 3],
    pub body: String,
    pub author: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub face: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part_refs: Option<Vec<PartRef>>,
}

/// Captured sample reference accepted by `annotations.evidence_reference`.
/// Seconds and source coordinates are retained; no live geometry is implied.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentEvidence {
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_range: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<EvidenceSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_ids: Option<Vec<String>>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceSource {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
}
impl ExperimentEvidence {
    pub fn validate(&self) -> Result<(), String> {
        let text = |v: &str, name: &str| {
            if v.trim().is_empty() || v.chars().count() > 20000 {
                Err(format!("evidence.{name}: nonempty text of at most 20000 characters required"))
            } else { Ok(()) }
        };
        text(&self.run_id, "run_id")?;
        for (name, value) in [("signal", &self.signal), ("physical_hash", &self.physical_hash)] {
            if let Some(value) = value { text(value, name)?; }
        }
        if self.time_range.is_some_and(|[a,b]| !a.is_finite() || !b.is_finite() || a < 0. || b < a) {
            return Err("evidence.time_range: ordered nonnegative finite seconds required".into());
        }
        if let Some(source) = &self.source {
            text(&source.path, "source.path")?;
            if source.line == Some(0) || source.column == Some(0) {
                return Err("evidence.source: line and column must be positive integers".into());
            }
        }
        for id in self.node_ids.iter().flatten() { text(id, "node_ids")?; }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NewEvidenceThread {
    pub body: String,
    pub author: String,
    pub evidence: ExperimentEvidence,
    pub document_id: String,
    pub expected_revision: u64,
}

/// `PATCH /threads/{id}`' body (`update_thread`): only the given keys.
/// A reattach sends `node_id` and `point` (and `face`, `view`); RoboCAD
/// reads `face` only with them.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ThreadPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub point: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub face: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part_refs: Option<Vec<PartRef>>,
}







/// The part links `[label](part:ID)` in a message body, in order (as
/// `annotations.PART_LINK` finds them: a label without `]` or a newline,
/// an id of `A-Z a-z 0-9 _ -`).
pub fn part_links(body: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(']') else { break };
        let label = &after[..close];
        let tail = &after[close + 1..];
        let ok_label = !label.is_empty() && !label.contains('\n');
        if ok_label && let Some(link) = tail.strip_prefix("(part:") {
            let id: String = link.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
            if !id.is_empty() && link[id.len()..].starts_with(')') {
                out.push((label.to_string(), id.clone()));
                rest = &link[id.len() + 1..];
                continue;
            }
        }
        rest = after;
    }
    out
}

/// A part link as RoboCAD's "Insert part link from selection" writes it:
/// `[`, `]` and newlines in the label become `(`, `)` and spaces.
pub fn part_link(label: &str, node_id: &str) -> String {
    let label: String = label.chars().map(|c| match c {
        '[' => '(',
        ']' => ')',
        '\n' => ' ',
        c => c,
    }).collect();
    format!("[{label}]({PART_LINK_SCHEME}{node_id})")
}


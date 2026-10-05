//! The linked system file (`system_link.py`, commands.py:1112-1136) as
//! typed `Ops` calls:
//!
//! - `link_system(path)`: stores `{path (relative to the .rcad when they
//!   share a tree), sha256, title, revision, definitions, instances,
//!   linked}` in `robot_settings["system"]`; one undo step; `result` is the
//!   link. A file that is not `sim.system/2` JSON is a 422.
//! - `unlink_system()`: removes it (422 "No system file is linked").
//! - `refresh_system_link()`: "Accept changes": re-links the file now at
//!   the stored path (new hash and revision).
//! - `system_status()`: `{"state": "unlinked"}`, or `{"state": "missing",
//!   "path", "error", "link"}`, or `{"state": "current" | "changed",
//!   "path" (resolved), "link", "now"}`. A read: it changes nothing (the op
//!   route still answers RoboCAD's history beside it).
//!
//! The other three are RoboCAD edits: use a client with [`super::EDIT_TIMEOUT`].
use super::{CadClient, CadError, OpResult};
use crate::hardware::protocol::lenient;
use serde::{Deserialize, Serialize};
use serde_json::{Map, json};

/// A stored link (`make_link`) or a file's summary now (`read`).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct SystemSummary {
    /// The stored path (a link only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub sha256: String,
    pub title: String,
    pub revision: i64,
    pub definitions: u64,
    pub instances: u64,
    /// When it was linked (`%Y-%m-%dT%H:%M:%S`, a link only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked: Option<String>,
}

/// `system_status()`'s state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkState {
    #[default]
    Unlinked,
    Missing,
    Current,
    Changed,
    #[serde(other)]
    Unknown,
}

/// `system_status()`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct SystemStatus {
    #[serde(deserialize_with = "lenient")]
    pub state: LinkState,
    /// The linked file's resolved path (all but unlinked).
    pub path: Option<String>,
    /// Why the file could not be read (missing).
    pub error: Option<String>,
    #[serde(deserialize_with = "lenient")]
    pub link: Option<SystemSummary>,
    /// The file as it is now (current, changed).
    #[serde(deserialize_with = "lenient")]
    pub now: Option<SystemSummary>,
}

impl SystemStatus {
    /// RoboCAD's status line (ui/references.py:90-103, `show_system`):
    /// "System file: none linked. …", "System file missing: path",
    /// "System: title · revision n · n definitions", with " · CHANGED
    /// since linked (was revision n)" when changed.
    pub fn line(&self) -> String {
        match self.state {
            LinkState::Unlinked => "System file: none linked. Link a .system.json to build circuits and subsystems for this model.".to_string(),
            LinkState::Missing => format!("System file missing: {}", self.path.as_deref().unwrap_or("")),
            LinkState::Current | LinkState::Changed | LinkState::Unknown => {
                let link = self.link.clone().unwrap_or_default();
                let now = self.now.clone().unwrap_or_default();
                let base = self.path.as_deref().map(|p| p.rsplit('/').next().unwrap_or(p).to_string()).unwrap_or_default();
                let title = if link.title.is_empty() { base } else { link.title.clone() };
                let mut text = format!("System: {title} · revision {} · {} definitions", now.revision, now.definitions);
                if self.state == LinkState::Changed {
                    text.push_str(&format!(" · CHANGED since linked (was revision {})", link.revision));
                }
                text
            }
        }
    }
}

impl CadClient {
    /// `link_system(path)`: `result` is the stored link.
    pub fn link_system(&self, path: &str) -> Result<OpResult, CadError> {
        self.op("link_system", &[json!(path)], &Map::new())
    }
    /// `unlink_system()`.
    pub fn unlink_system(&self) -> Result<OpResult, CadError> {
        self.op("unlink_system", &[], &Map::new())
    }
    /// `refresh_system_link()` ("Accept changes"): `result` is the new link.
    pub fn refresh_system_link(&self) -> Result<OpResult, CadError> {
        self.op("refresh_system_link", &[], &Map::new())
    }
    /// `system_status()`: the status from the op's `result`.
    pub fn system_status(&self) -> Result<SystemStatus, CadError> {
        let r = self.op("system_status", &[], &Map::new())?;
        serde_json::from_value(r.result.clone()).map_err(|e| CadError { method: "POST", route: "/ops/system_status".into(), status: None, message: format!("unexpected answer: {e}: {}", r.result) })
    }
}

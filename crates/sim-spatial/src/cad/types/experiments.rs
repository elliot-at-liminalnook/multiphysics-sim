//! Captured experiments. Calls block and belong in viewer jobs. Numerical
//! simulation remains in the shared runner, not this transport contract.
use super::types::MeshData;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExperimentRequest {
    pub document_id: String,
    pub expected_revision: u64,
    pub system: Value,
    pub controller: Option<Value>,
    pub parameters: BTreeMap<String, Value>,
    pub settings: BTreeMap<String, Value>,
    pub profile: String,
    pub seed: u64,
    pub preflight: bool,
    pub label: String,
    pub parent_run: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExperimentRecord {
    pub id: String, pub document_id: Option<String>, pub revision: Option<u64>,
    pub created_at: f64, pub updated_at: f64, pub state: String, pub fraction: f64,
    pub label: String, pub profile: String, pub seed: u64, pub preflight: bool,
    pub error: Option<String>, pub provenance: Value,
    #[serde(flatten)] pub extra: BTreeMap<String, Value>,
}
impl ExperimentRecord {
    pub fn terminal(&self) -> bool { matches!(self.state.as_str(), "completed" | "failed" | "cancelled") }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CaptureIdentity {
    pub document_id: Option<String>, pub revision: Option<u64>, pub source_kind: String,
    pub source_id: String, pub physical_hash: Option<String>, pub archive_hash: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapturedNode { pub id: String, pub name: String, pub source: Value, pub mesh: Option<MeshData> }
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapturedGeometry {
    pub identity: CaptureIdentity, pub units: String, pub nodes: Vec<CapturedNode>,
    pub missing_reason: Option<String>, pub provenance: Value,
}

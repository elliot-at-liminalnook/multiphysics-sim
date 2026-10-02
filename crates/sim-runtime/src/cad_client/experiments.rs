//! Captured experiments. Calls block and belong in viewer jobs. Numerical
//! simulation remains in the shared runner, not this transport contract.
use super::{CadClient, CadError, types::MeshData};
use crate::hardware_client::encode_uri_component as enc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
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
impl CadClient {
    /// Read-only bundle capture; path belongs to the service host.
    pub fn experiment_linked_sources(&self, path: &str) -> Result<Value, CadError> { self.get(&format!("/experiments/linked-sources?path={}",enc(path))) }
    pub fn experiments(&self) -> Result<Vec<ExperimentRecord>, CadError> { self.get("/experiments") }
    pub fn experiment_catalogue(&self) -> Result<Value, CadError> { self.get("/experiments/catalogue") }
    pub fn start_experiment(&self, r: &ExperimentRequest) -> Result<ExperimentRecord, CadError> { self.send("POST", "/experiments", Some(r)) }
    pub fn experiment(&self, id: &str) -> Result<ExperimentRecord, CadError> { self.get(&format!("/experiments/{}",enc(id))) }
    pub fn cancel_experiment(&self, id: &str) -> Result<ExperimentRecord, CadError> { self.send("POST", &format!("/experiments/{}/cancel",enc(id)), Some(&json!({}))) }
    pub fn experiment_geometry(&self, id: &str) -> Result<CapturedGeometry, CadError> { self.get(&format!("/experiments/{}/geometry",enc(id))) }
    pub fn experiment_result(&self, id: &str) -> Result<Value, CadError> { self.get(&format!("/experiments/{}/result",enc(id))) }
    pub fn experiment_inputs(&self, id: &str) -> Result<Value, CadError> { self.get(&format!("/experiments/{}/inputs",enc(id))) }
    pub fn experiment_diagnostics(&self, id: &str) -> Result<Value, CadError> { self.get(&format!("/experiments/{}/diagnostics",enc(id))) }
    pub fn experiment_components(&self, id: &str) -> Result<Value, CadError> { self.get(&format!("/experiments/{}/components",enc(id))) }
    pub fn experiment_sources(&self, id: &str) -> Result<Value, CadError> { self.get(&format!("/experiments/{}/sources",enc(id))) }
    pub fn experiment_partial(&self, id: &str) -> Result<Value, CadError> { self.get(&format!("/experiments/{}/partial",enc(id))) }
    pub fn experiment_compare(&self, baseline: &str, id: &str) -> Result<Value, CadError> { self.send("POST", &format!("/experiments/{}/compare",enc(id)), Some(&json!({"baseline_id":baseline}))) }
    pub fn experiment_sample(&self, id: &str, time: f64, flex_scale: f64) -> Result<Value, CadError> { self.send("POST", &format!("/experiments/{}/sample",enc(id)), Some(&json!({"time":time,"flex_scale":flex_scale}))) }
}

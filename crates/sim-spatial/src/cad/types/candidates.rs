//! Isolated staged CAD changes and guarded authoritative publication.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CandidateRequest { pub document_id: String, pub expected_revision: u64, pub label: String, pub operations: Vec<Value> }
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CandidateRecord {
    pub id: String, pub document_id: String, pub base_revision: u64, pub revision: u64,
    pub label: String, pub state: String, pub created_at: f64, pub changes: Value,
    #[serde(flatten)] pub extra: BTreeMap<String,Value>,
}

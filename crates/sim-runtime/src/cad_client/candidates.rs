//! Isolated staged CAD changes and guarded authoritative publication.
use super::{CadClient, CadError, experiments::{CapturedGeometry, ExperimentRecord, ExperimentRequest}};
use crate::hardware_client::encode_uri_component as enc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
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
impl CadClient {
    pub fn candidates(&self)->Result<Vec<CandidateRecord>,CadError>{self.get("/candidates")}
    pub fn candidate(&self,id:&str)->Result<CandidateRecord,CadError>{self.get(&format!("/candidates/{}",enc(id)))}
    pub fn create_candidate(&self,r:&CandidateRequest)->Result<CandidateRecord,CadError>{self.send("POST","/candidates",Some(r))}
    pub fn candidate_geometry(&self,id:&str)->Result<CapturedGeometry,CadError>{self.get(&format!("/candidates/{}/geometry",enc(id)))}
    pub fn accept_candidate(&self,id:&str,document_id:&str,revision:u64)->Result<CandidateRecord,CadError>{self.send("POST",&format!("/candidates/{}/accept",enc(id)),Some(&json!({"document_id":document_id,"expected_revision":revision})))}
    pub fn discard_candidate(&self,id:&str)->Result<CandidateRecord,CadError>{self.send::<Value,_>("DELETE",&format!("/candidates/{}",enc(id)),None)}
    pub fn candidate_experiment(&self,id:&str,r:&ExperimentRequest)->Result<ExperimentRecord,CadError>{self.send("POST",&format!("/candidates/{}/experiments",enc(id)),Some(r))}
    pub fn batch(&self,r:&CandidateRequest)->Result<Value,CadError>{self.send("POST","/doc/batch",Some(r))}
    pub fn model_script(&self,r:&Value)->Result<Value,CadError>{self.send("POST","/doc/script",Some(r))}
    pub fn restore_experiment_inputs(&self,id:&str,document_id:&str,revision:u64)->Result<Value,CadError>{self.send("POST",&format!("/experiments/{}/restore",enc(id)),Some(&json!({"document_id":document_id,"expected_revision":revision})))}
}

//! Reference pose and motion patterns in process (`sim_cad::pose`): what
//! pose mode calls, with the names RoboCAD's motion routes had. A
//! [`Service`] is the shown snapshot and revision, taken on the UI thread
//! and used on jobs; a request for another document or revision is refused
//! by name ("Document changed; motion draft preserved").
use crate::cad::document::CadDocument;
use crate::cad::sync::LocalSnapshot;
use crate::cad::types::components::ComponentStamp;
use crate::cad::types::motion::{PoseMetadata, PoseRequest, PoseSample};
use crate::cad::types::CadError;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct Service {
    snapshot: Arc<LocalSnapshot>,
    revision: u64,
    document_id: String,
}

/// The service for the shown document, or why there is none.
pub(crate) fn service(doc: &CadDocument) -> Result<Service, String> {
    let snapshot = doc.local.clone().ok_or("Open a CAD document first")?;
    let document_id = snapshot.archive.manifest["document_id"].as_str().unwrap_or_default().to_string();
    Ok(Service { snapshot, revision: doc.shown_revision(), document_id })
}

fn err(e: impl Into<String>) -> CadError {
    CadError::local(e)
}

fn typed<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, CadError> {
    serde_json::from_value(v).map_err(|e| err(e.to_string()))
}

impl Service {
    fn guard(&self, document_id: &str, revision: u64) -> Result<(), CadError> {
        if document_id != self.document_id || revision != self.revision {
            return Err(err("Document changed; motion draft preserved"));
        }
        Ok(())
    }
    pub fn pose_metadata(&self) -> Result<PoseMetadata, CadError> {
        typed(sim_cad::pose::metadata(&self.snapshot.archive, self.revision).map_err(err)?)
    }
    pub fn motion_programs(&self) -> Result<BTreeMap<String, Value>, CadError> {
        typed(self.snapshot.archive.manifest["robot_settings"].get("motion_programs").filter(|p| p.is_object()).cloned().unwrap_or_else(|| json!({})))
    }
    pub fn validate_motion(&self, program: &Value, stamp: &ComponentStamp) -> Result<Value, CadError> {
        self.guard(&stamp.document_id, stamp.expected_revision)?;
        let model = sim_cad::pose::PoseModel::new(&self.snapshot.archive).map_err(err)?;
        sim_cad::pose::resolve_program(&self.snapshot.archive, &model, program).map_err(err)
    }
    pub fn motion_sweep(&self, joint: &str, stamp: &ComponentStamp) -> Result<Value, CadError> {
        self.guard(&stamp.document_id, stamp.expected_revision)?;
        sim_cad::pose::sweep(&self.snapshot.archive, joint).map_err(err)
    }
    pub fn sample_pose(&self, r: &PoseRequest) -> Result<PoseSample, CadError> {
        self.guard(&r.document_id, r.expected_revision)?;
        let request = serde_json::to_value(r).map_err(|e| err(e.to_string()))?;
        typed(sim_cad::pose::sample(&self.snapshot.archive, self.revision, &request).map_err(err)?)
    }
}

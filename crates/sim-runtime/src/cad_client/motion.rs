//! Reference CAD kinematics; radians/mm, never physics or motor commands.
use super::{CadClient, CadError, components::ComponentStamp, experiments::CaptureIdentity};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PoseRequest {
    pub document_id: String,
    pub expected_revision: u64,
    pub positions: BTreeMap<String, f64>,
    pub program: Option<Value>,
    pub time: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior: Option<PoseContinuation>,
}
/// Caller-owned resolved state; validated by the reference service before it
/// seeds a solver. No remote mutable session and no cross-consumer state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PoseContinuation {
    pub identity: CaptureIdentity,
    pub positions: BTreeMap<String, f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PoseJoint {
    pub id: String,
    pub name: String,
    pub unit: String,
    pub home: f64,
    pub lower: Option<f64>,
    pub upper: Option<f64>,
    pub display_lower: f64,
    pub display_upper: f64,
    pub driver: bool,
    pub pivot: [f64; 3],
    pub axis: [f64; 3],
    pub child: String,
    pub parent: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PoseMetadata {
    pub identity: CaptureIdentity,
    pub joints: Vec<PoseJoint>,
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub focus_ids: BTreeMap<String, Vec<String>>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PoseSample {
    pub identity: CaptureIdentity,
    pub positions: BTreeMap<String, f64>,
    pub matrices: BTreeMap<String, [[f64; 4]; 4]>,
    pub closure_error_mm: f64,
    pub time: f64,
    pub program: Option<Value>,
    #[serde(default)]
    pub prior_applied: bool,
}
impl PoseSample {
    pub fn continuation(&self) -> PoseContinuation {
        PoseContinuation {
            identity: self.identity.clone(),
            positions: self.positions.clone(),
        }
    }
}
impl CadClient {
    pub fn pose_metadata(&self) -> Result<PoseMetadata, CadError> {
        self.get("/motion/pose")
    }
    pub fn sample_pose(&self, r: &PoseRequest) -> Result<PoseSample, CadError> {
        let sample: PoseSample = self.send("POST", "/motion/sample", Some(r))?;
        if r.prior.is_some() && !sample.prior_applied {
            return Err(CadError {method:"POST",route:"/motion/sample".into(),status:None,message:"motion.prior: service did not confirm validated continuation; update the reference service before continuing".into()});
        }
        Ok(sample)
    }
    pub fn motion_programs(&self) -> Result<BTreeMap<String, Value>, CadError> {
        self.get("/motion/programs")
    }
    pub fn validate_motion(
        &self,
        program: &Value,
        stamp: &ComponentStamp,
    ) -> Result<Value, CadError> {
        self.send("POST","/motion/validate",Some(&json!({"program":program,"document_id":stamp.document_id,"expected_revision":stamp.expected_revision})))
    }
    pub fn save_motion(&self, program: &Value, stamp: &ComponentStamp) -> Result<Value, CadError> {
        self.send("POST","/motion/programs/guarded",Some(&json!({"program":program,"document_id":stamp.document_id,"expected_revision":stamp.expected_revision})))
    }
    pub fn delete_motion(&self, name: &str, stamp: &ComponentStamp) -> Result<Value, CadError> {
        self.send("POST","/motion/programs/delete",Some(&json!({"name":name,"document_id":stamp.document_id,"expected_revision":stamp.expected_revision})))
    }
    pub fn motion_sweep(&self, joint: &str, stamp: &ComponentStamp) -> Result<Value, CadError> {
        self.send("POST","/motion/sweep",Some(&json!({"joint":joint,"document_id":stamp.document_id,"expected_revision":stamp.expected_revision})))
    }
}

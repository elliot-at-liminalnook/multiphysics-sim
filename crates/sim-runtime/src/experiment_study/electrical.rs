//! Shared offline electrical authoring and immutable calibrated comparisons.
use super::super::Study;
use crate::controller_refinement::{power, recording, electrical_measurements as measurements};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::atomic::{AtomicBool, Ordering}};

#[derive(Clone, Debug, Serialize)]
pub enum Command {
    SetSource(Option<power::Setup>),
    SetVoltage(Option<f64>),
    SetController(Option<power::Controller>),
    SelectPrediction(usize),
    SelectComparison(usize),
    Rejected { raw: serde_json::Value, error: String },
}
/// Unknown authored payloads are retained as explicit transactional refusals.
impl<'de> Deserialize<'de> for Command {
    fn deserialize<D:serde::Deserializer<'de>>(deserializer:D)->Result<Self,D::Error>{
        let raw=serde_json::Value::deserialize(deserializer)?;
        let parsed=(||->Result<Command,String>{
            let object=raw.as_object().ok_or("refinement.electrical.command: expected one typed command")?;
            if object.len()!=1{return Err("refinement.electrical.command: expected exactly one command".into());}
            let (name,value)=object.iter().next().expect("one command");
            match name.as_str(){
                "SetSource"=>Ok(Command::SetSource(parse_source(value)?)),
                "SetController"=>Ok(Command::SetController(parse_controller(value)?)),
                "SetVoltage"=>serde_json::from_value(value.clone()).map(Command::SetVoltage).map_err(|e|format!("draft.conditions.voltage_v: {e}")),
                "SelectPrediction"=>serde_json::from_value(value.clone()).map(Command::SelectPrediction).map_err(|e|format!("refinement.electrical.selected_prediction: {e}")),
                "SelectComparison"=>serde_json::from_value(value.clone()).map(Command::SelectComparison).map_err(|e|format!("refinement.electrical.selected_comparison: {e}")),
                "Rejected"=>{
                    keys(value,&["raw","error"],"refinement.electrical.rejected")?;
                    let error=value.get("error").and_then(|v|v.as_str()).ok_or("refinement.electrical.rejected.error: expected diagnostic")?;
                    let rejected=value.get("raw").ok_or("refinement.electrical.rejected.raw: missing exact content")?;
                    Ok(Command::Rejected{raw:rejected.clone(),error:error.into()})
                },
                _=>Err(format!("refinement.electrical.command.{name}: unsupported command retained")),
            }
        })();
        Ok(parsed.unwrap_or_else(|error|Command::Rejected{raw,error}))
    }
}
fn keys(value:&serde_json::Value,allowed:&[&str],path:&str)->Result<(),String>{
    let object=value.as_object().ok_or_else(||format!("{path}: expected object"))?;
    if let Some(key)=object.keys().find(|key|!allowed.contains(&key.as_str())){return Err(format!("{path}.{key}: unsupported field retained; no unknown payload is discarded"));}
    Ok(())
}
const LIMIT_KEYS:&[&str]=&["minimum_voltage_v","maximum_discharge_current_a","maximum_charge_current_a","maximum_winding_current_a","maximum_draw_power_w","maximum_return_power_w"];
pub fn parse_source(raw:&serde_json::Value)->Result<Option<power::Setup>,String>{
    if raw.is_null(){return Ok(None);}
    keys(raw,&["source_component","source_parameters","auxiliary_current_a","evidence","limits"],"draft.power")?;
    if let Some(limits)=raw.get("limits"){keys(limits,LIMIT_KEYS,"draft.power.limits")?;}
    serde_json::from_value(raw.clone()).map(Some).map_err(|e|format!("draft.power: {e}"))
}
pub fn parse_controller(raw:&serde_json::Value)->Result<Option<power::Controller>,String>{
    if raw.is_null(){return Ok(None);}
    keys(raw,&["sensing","nominal_voltage_for_compensation_v","limits"],"refinement.experiment.electrical")?;
    if let Some(sensing)=raw.get("sensing"){keys(sensing,&["voltage_quantum_v","supply_current_quantum_a","winding_current_quantum_a","evidence"],"refinement.experiment.electrical.sensing")?;}
    if let Some(limits)=raw.get("limits"){keys(limits,LIMIT_KEYS,"refinement.experiment.electrical.limits")?;}
    serde_json::from_value(raw.clone()).map(Some).map_err(|e|format!("refinement.experiment.electrical: {e}"))
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Operation {
    CompareServoVoltage { recording_hash: String, prediction: usize },
    CompareMeasurements { measurements: measurements::Measurements, prediction: usize },
    RejectedInput { prediction: usize, source: String },
    CapturedComparison { measurement_hash: String, prediction: usize },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResultData {
    evaluation: measurements::Evaluation,
    /// Immutable prediction, not a pointer to the mutable candidate.
    prediction: recording::Prediction,
    /// Only shared execution can produce an attachable result; deserialized evidence
    /// is review-only until validated by a fresh job. No frame scoring is needed.
    #[serde(skip)]
    validated: bool,
    #[serde(skip)]
    prediction_index: usize,
    #[serde(skip)]
    captured_unix_ns: String,
}
impl ResultData {
    pub fn evaluation(&self)->&measurements::Evaluation {&self.evaluation}
    pub fn prediction(&self)->&recording::Prediction {&self.prediction}
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Evidence {
    pub comparisons: Vec<ResultData>,
    pub selected_prediction: Option<usize>,
    pub selected_comparison: Option<usize>,
    #[serde(flatten)]
    pub retained_fields: BTreeMap<String, serde_json::Value>,
}

fn validate_limits(l:&power::Limits,path:&str)->Result<(),String>{
    for (field,value) in [("minimum_voltage_v",l.minimum_voltage_v),("maximum_discharge_current_a",l.maximum_discharge_current_a),("maximum_charge_current_a",l.maximum_charge_current_a),("maximum_winding_current_a",l.maximum_winding_current_a),("maximum_draw_power_w",l.maximum_draw_power_w),("maximum_return_power_w",l.maximum_return_power_w)] {
        if value.is_some_and(|v|!v.is_finite() || v<0.) {return Err(format!("{path}.{field}: expected finite nonnegative limit or none"));}
    }
    Ok(())
}
fn validate_source(source:&power::Setup)->Result<(),String>{
    if source.evidence.trim().is_empty(){return Err("draft.power.evidence: explicit assumption or measurement provenance required".into());}
    if !source.auxiliary_current_a.is_finite() || source.auxiliary_current_a<0. {return Err("draft.power.auxiliary_current_a: expected finite nonnegative A".into());}
    validate_limits(&source.limits,"draft.power.limits")?;
    let registry=crate::registry();
    let descriptor=registry.get(&source.source_component.as_str().into()).map_err(|e|format!("draft.power.source_component: {e}"))?;
    for terminal in ["p","n"] {
        let expected=sim_core::acausal(if terminal=="p"{"p"}else{"n"},sim_core::connectors::Electrical);
        if !descriptor.ports.iter().any(|port|port.name==terminal && port.schema==expected.schema) {return Err(format!("draft.power.source_component: registry source requires typed electrical terminal {terminal}"));}
    }
    descriptor.validate_parameters(&source.source_parameters).map_err(|e|format!("draft.power.source_parameters: {e}"))?;
    source.validate().map_err(|e|format!("draft.power: {e}"))
}
fn validate_controller(controller:&power::Controller)->Result<(),String>{
    let path="refinement.experiment.electrical";
    if controller.sensing.evidence.trim().is_empty(){return Err(format!("{path}.sensing.evidence: explicit sensing provenance required"));}
    for (field,value) in [("sensing.voltage_quantum_v",Some(controller.sensing.voltage_quantum_v)),("sensing.supply_current_quantum_a",controller.sensing.supply_current_quantum_a),("sensing.winding_current_quantum_a",controller.sensing.winding_current_quantum_a),("nominal_voltage_for_compensation_v",controller.nominal_voltage_for_compensation_v)] {
        if value.is_some_and(|v|!v.is_finite() || v<=0.){return Err(format!("{path}.{field}: expected positive finite resolution or compensation value"));}
    }
    validate_limits(&controller.limits,&format!("{path}.limits"))?;
    controller.validate().map_err(|e|format!("{path}.limits: {e}"))
}

pub fn command(s: &mut Study, command: Command) -> Result<(), String> {
    let mut next=s.clone();
    match command {
        Command::Rejected{error,..}=>return Err(error),
        Command::SetSource(source)=>{
            if let Some(source)=&source {validate_source(source)?;}
            if next.draft.power!=source {if source.is_some(){next.draft.conditions.voltage_v=None;}next.draft.power=source;next.candidate_edited();}
        },
        Command::SetVoltage(voltage)=>{
            if voltage.is_some_and(|v|!v.is_finite() || v<=0.) {return Err("draft.conditions.voltage_v: expected positive finite V or none".into());}
            if next.draft.conditions.voltage_v!=voltage {next.draft.conditions.voltage_v=voltage;next.candidate_edited();}
        },
        Command::SetController(controller)=>{
            if let Some(controller)=&controller {validate_controller(controller)?;}
            if next.refinement.experiment.electrical!=controller {next.refinement.experiment.electrical=controller;next.candidate_edited();}
        },
        Command::SelectPrediction(index)=>{
            if index>=next.refinement.predictions.len() {return Err("refinement.electrical.selected_prediction: missing retained prediction".into());}
            next.refinement_evidence.electrical.selected_prediction=Some(index);
        },
        Command::SelectComparison(index)=>{
            if index>=next.refinement.electrical_comparisons.len() {return Err("refinement.electrical.selected_comparison: missing retained comparison".into());}
            next.refinement_evidence.electrical.selected_comparison=Some(index);
        },
    }
    next.draft.validate().map_err(|e|format!("draft: {e}"))?;
    *s=next;Ok(())
}
fn sources<'a>(s:&'a Study,operation:&Operation)->Result<(&'a recording::Recording,&'a recording::Prediction,measurements::Measurements),String>{
    let (hash,index)=match operation {
        Operation::CompareServoVoltage{recording_hash,prediction}=>(recording_hash,prediction),
        Operation::CompareMeasurements{measurements,prediction}=>(&measurements.recording_hash,prediction),
        Operation::RejectedInput{..}=>return Err("refinement.electrical.measurements: rejected raw sidecar; inspect retained exact input".into()),
        Operation::CapturedComparison{..}=>return Err("refinement.electrical.measurements: receipt identity requires captured companion content".into()),
    };
    let r=s.refinement.recordings.iter().find(|r|r.fingerprint()==*hash).ok_or("refinement.electrical.recording_hash: missing exact recording")?;
    if !r.completed || r.failure.is_some() || !r.stop_verified {return Err("refinement.electrical.recording: incomplete or failed capture cannot be scored".into());}
    let p=s.refinement.predictions.get(*index).ok_or("refinement.electrical.prediction: missing retained prediction")?;
    p.validate(r).map_err(|e|format!("refinement.electrical.prediction: {e}"))?;
    let m=match operation {Operation::CompareServoVoltage{..}=>measurements::Measurements::servo_voltage(r)?,Operation::CompareMeasurements{measurements,..}=>measurements.clone(),Operation::RejectedInput{..}|Operation::CapturedComparison{..}=>unreachable!("rejected input refused before measurement construction")};
    m.validate_recording(r).map_err(|e|format!("refinement.electrical.measurements: {e}"))?;
    Ok((r,p,m))
}
pub fn prepare(s:&Study,operation:&Operation)->Result<(),String>{
    let (_,p,m)=sources(s,operation)?;
    measurements::evaluate(&m,p).map_err(|e|format!("refinement.electrical.comparison: {e}"))?;Ok(())
}
/// Capture exact typed comparison inputs into the existing bounded content owner.
/// Hosts additionally retain the original imported file bytes, including unknown fields.
pub fn capture_inputs(s:&mut Study,operation:&Operation)->Result<(),String>{
    let (r,p,m)=sources(s,operation)?;
    let bytes=serde_json::to_vec(&serde_json::json!({"recording":r,"prediction":p,"measurements":m})).map_err(|e|format!("refinement.electrical.inputs: {e}"))?;
    s.input_contents.capture(bytes);Ok(())
}
/// No retained Study, receipts or prior comparison results occur in this projection.
pub fn inputs(s:&Study,operation:&Operation)->serde_json::Value {
    match sources(s,operation) {
        Ok((r,p,m))=>{
            let bytes=serde_json::to_vec(&serde_json::json!({"recording":r,"prediction":p,"measurements":m})).expect("serializable captured electrical inputs");
            let hash=blake3::hash(&bytes).to_hex().to_string();
            serde_json::json!({"schema_version":s.version,"measurement_hash":m.fingerprint(),"recording_hash":r.fingerprint(),"prediction_hash":measurements::prediction_hash(p),"model_hash":p.model.fingerprint(),"runtime":super::super::execution_identity(),"content_ref":{"version":1,"blake3":hash,"byte_length":bytes.len()}})
        },
        Err(error)=>serde_json::json!({"operation":receipt_operation(operation),"capture_failure":error}),
    }
}
/// A receipt references exact retained content without repeating channel arrays.
pub fn receipt_operation(operation:&Operation)->Operation {
    match operation {
        Operation::CompareMeasurements{measurements,prediction}=>Operation::CapturedComparison{measurement_hash:measurements.fingerprint(),prediction:*prediction},
        operation=>operation.clone(),
    }
}
pub fn execute(s:&Study,operation:&Operation,cancel:&AtomicBool)->Result<ResultData,String>{
    if cancel.load(Ordering::Relaxed){return Err("refinement.electrical: cancelled before comparison".into());}
    let (_,p,m)=sources(s,operation)?;
    let prediction_index=match operation {Operation::CompareServoVoltage{prediction,..}|Operation::CompareMeasurements{prediction,..}=>*prediction,_=>return Err("refinement.electrical: receipt is not executable".into())};
    let result=ResultData{evaluation:measurements::evaluate(&m,p)?,prediction:p.clone(),validated:false,prediction_index,captured_unix_ns:String::new()};
    Ok(result)
}
pub fn validate_result(capture:&super::Capture,result:&ResultData)->Result<(),String>{
    let super::Operation::Electrical(operation)=&capture.operation else{return Err("refinement.electrical.result: wrong captured operation".into());};
    let (_,p,m)=sources(&capture.study,operation)?;
    if measurements::prediction_hash(p)!=measurements::prediction_hash(&result.prediction) || m!=result.evaluation.measurements {return Err("refinement.electrical.result: captured prediction or exact measurements changed".into());}
    result.evaluation.validate(p).map_err(|e|format!("refinement.electrical.result: {e}"))
}
/// Worker-only validation produces a private, nonserialized attachment capability.
pub(super) fn confirm(capture:&super::Capture,result:&mut ResultData)->Result<(),String>{
    validate_result(capture,result)?;
    result.validated=true;result.captured_unix_ns=capture.captured_unix_ns.clone();Ok(())
}
pub(super) fn belongs_to(result:&ResultData,capture:&super::Capture)->bool {
    result.validated && result.captured_unix_ns==capture.captured_unix_ns
}
pub fn apply_result(s:&mut Study,operation:&Operation,result:&ResultData)->Result<(String,usize),String>{
    if !result.validated {return Err("refinement.electrical.result: deserialized or unvalidated terminal result is review-only".into());}
    let index=match operation {Operation::CompareServoVoltage{prediction,..}|Operation::CompareMeasurements{prediction,..}=>*prediction,_=>return Err("refinement.electrical.result: metadata cannot attach".into())};
    if index!=result.prediction_index {return Err("refinement.electrical.result.prediction: captured index mismatch".into());}
    // Result attachment remains valid after candidate edits; the immutable prediction
    // and recording are its authority. The host validates against the captured Study.
    let hash=match operation {Operation::CompareServoVoltage{recording_hash,..}=>recording_hash,Operation::CompareMeasurements{measurements,..}=>&measurements.recording_hash,Operation::RejectedInput{..}|Operation::CapturedComparison{..}=>return Err("refinement.electrical.result: receipt or rejected input cannot produce comparison".into())};
    if hash!=&result.evaluation.measurements.recording_hash {return Err("refinement.electrical.result.recording_hash: operation identity mismatch".into());}
    if result.prediction.recording_hash!=*hash {return Err("refinement.electrical.result.prediction: operation recording mismatch".into());}
    if let Operation::CompareMeasurements{measurements,..}=operation {
        if measurements!=&result.evaluation.measurements {return Err("refinement.electrical.result.measurements: exact captured sidecar mismatch".into());}
    }
    // Heavy scoring and source validation run in execute/validate_result and load jobs.
    let index=s.refinement.electrical_comparisons.len();
    s.refinement.electrical_comparisons.push(result.evaluation.clone());
    s.refinement_evidence.electrical.comparisons.push(result.clone());
    Ok(("electrical_comparison".into(),index))
}
pub fn validate(s:&Study)->Result<(),String>{
    let e=&s.refinement_evidence.electrical;
    if e.selected_prediction.is_some_and(|i|i>=s.refinement.predictions.len()){return Err("refinement.electrical.selected_prediction: missing prediction".into());}
    if e.selected_comparison.is_some_and(|i|i>=s.refinement.electrical_comparisons.len()){return Err("refinement.electrical.selected_comparison: missing comparison".into());}
    for (i,result) in e.comparisons.iter().enumerate(){
        if !s.refinement.electrical_comparisons.iter().any(|evaluation|evaluation==&result.evaluation) {return Err(format!("refinement.electrical.comparisons.{i}.evaluation: missing retained comparison"));}
        if !s.refinement.predictions.iter().any(|p|measurements::prediction_hash(p)==measurements::prediction_hash(&result.prediction)) {return Err(format!("refinement.electrical.comparisons.{i}.prediction: missing exact retained prediction"));}
        let r=s.refinement.recordings.iter().find(|r|r.fingerprint()==result.evaluation.measurements.recording_hash).ok_or_else(||format!("refinement.electrical.comparisons.{i}.recording_hash: missing source"))?;
        if !r.completed || r.failure.is_some() || !r.stop_verified {return Err(format!("refinement.electrical.comparisons.{i}.recording: incomplete capture"));}
        result.evaluation.measurements.validate_recording(r)?;
        result.prediction.validate(r)?;result.evaluation.validate(&result.prediction)?;
    }
    for (i,receipt) in s.refinement_evidence.receipts.iter().enumerate(){
        if receipt.result_kind.as_deref()!=Some("electrical_comparison") {continue;}
        let path=format!("refinement_evidence.receipts.{i}");
        if receipt.cancelled || receipt.failure.is_some(){return Err(format!("{path}: failed or cancelled receipt cannot declare scored comparison"));}
        let index=receipt.result_index.ok_or_else(||format!("{path}.result_index: missing comparison"))?;
        let evaluation=s.refinement.electrical_comparisons.get(index).ok_or_else(||format!("{path}.result_index: missing comparison"))?;
        let inputs=receipt.inputs.get("electrical").ok_or_else(||format!("{path}.inputs.electrical: missing captured identity"))?;
        if inputs.get("measurement_hash").and_then(|v|v.as_str())!=Some(evaluation.measurements.fingerprint().as_str()) || inputs.get("prediction_hash").and_then(|v|v.as_str())!=Some(evaluation.prediction_hash.as_str()) {return Err(format!("{path}.inputs.electrical: comparison identity changed"));}
        let raw=inputs.get("content_ref").ok_or_else(||format!("{path}.inputs.electrical.content_ref: missing exact source content"))?;
        let reference:super::super::input_content::ContentRef=serde_json::from_value(raw.clone()).map_err(|e|format!("{path}.inputs.electrical.content_ref: {e}"))?;
        let bytes=s.input_contents.resolve(&reference.blake3)?;
        if bytes.len() as u64!=reference.byte_length || blake3::hash(bytes).to_hex().to_string()!=reference.blake3 {return Err(format!("{path}.inputs.electrical.content_ref: exact content fingerprint changed"));}
        crate::experiment_study::portable::json_bounds(bytes,crate::experiment_study::portable::MAX_OBJECT_BYTES)?;
        let exact:serde_json::Value=serde_json::from_slice(bytes).map_err(|e|format!("{path}.inputs.electrical.content_ref: {e}"))?;
        let m:measurements::Measurements=serde_json::from_value(exact["measurements"].clone()).map_err(|e|format!("{path}.inputs.measurements: {e}"))?;
        let r:recording::Recording=serde_json::from_value(exact["recording"].clone()).map_err(|e|format!("{path}.inputs.recording: {e}"))?;
        let p:recording::Prediction=serde_json::from_value(exact["prediction"].clone()).map_err(|e|format!("{path}.inputs.prediction: {e}"))?;
        if m!=evaluation.measurements || measurements::prediction_hash(&p)!=evaluation.prediction_hash || r.fingerprint()!=m.recording_hash {return Err(format!("{path}.inputs: exact comparison source differs from receipt"));}
        let prediction=match &receipt.operation {
            super::Operation::Electrical(Operation::CapturedComparison{measurement_hash,prediction})=>{
                if measurement_hash!=&m.fingerprint(){return Err(format!("{path}.operation.measurement_hash: exact sidecar changed"));}*prediction
            },
            super::Operation::Electrical(Operation::CompareServoVoltage{recording_hash,prediction})=>{
                if recording_hash!=&r.fingerprint() || m!=measurements::Measurements::servo_voltage(&r)? {return Err(format!("{path}.operation: captured servo-voltage source changed"));}*prediction
            },
            // Readable typed receipt compatibility, before metadata-only projection.
            super::Operation::Electrical(Operation::CompareMeasurements{measurements,prediction})=>{
                if measurements!=&m{return Err(format!("{path}.operation.measurements: exact sidecar changed"));}*prediction
            },
            _=>return Err(format!("{path}.operation: successful comparison requires electrical capture")),
        };
        if s.refinement.predictions.get(prediction).is_none_or(|retained|measurements::prediction_hash(retained)!=evaluation.prediction_hash){return Err(format!("{path}.operation.prediction: selected prediction identity changed"));}
        m.validate_recording(&r)?;p.validate(&r)?;evaluation.validate(&p)?;
    }
    Ok(())
}

#[cfg(test)]
#[path="electrical_fixtures.rs"]
mod fixtures;

//! One portable offline refinement command/capture/execution/application contract.
//! Hosts own scheduling and study identity; this module owns validated shared mutations.
use super::{Study, commands};
#[path="recordings.rs"] pub mod recordings;
use crate::controller_refinement::{recording, context, calibration_data as data};
use crate::controller_refinement::{authoring, calibration as cal, control};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::atomic::{AtomicBool}};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Command {
    ImportRecording{recording:recording::Recording}, SelectRecording{recording_hash:String},
    AppendContext{context:context::CaptureContext}, AssignRecording{assignment:data::Assignment},
    SelectFitCase{selection:Option<recordings::FitCaseSelection>},
    UseRecordingFit{kind:String,index:usize,device:Option<u8>}, SetPredictionPurpose(recording::Purpose),
    SetExperiment(control::Experiment), SetCoordinates(Vec<cal::Coordinate>),
    SetScenarios(Vec<cal::Variant>), SetSelection{kind:String,ids:Vec<String>}, UseFit{fit:usize,device:Option<u8>},
    SelectControllerRun(usize),
    SetDecision{kind:String,index:usize,decision:String,notes:String},
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Operation {
    PredictRecording{recording_hash:String,purpose:recording::Purpose}, FitRecordings, FitCombined{additional:Option<Box<Study>>},
    Simulate, Sensitivity{selected:Vec<String>}, Fit{train:Vec<String>,validation:Vec<String>}, Robustness,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capture {pub study:Study,pub operation:Operation,pub runtime:serde_json::Value,pub captured_unix_ns:String}
impl Capture {
    /// Bounded projection: never nests previous workspace results or receipts.
    pub fn inputs(&self)->serde_json::Value {
        let s=&self.study;
        serde_json::json!({"schema_version":s.version,"captured_unix_ns":self.captured_unix_ns,"operation":self.operation_identity(),"recordings":s.refinement.recordings.iter().map(|r|serde_json::json!({"recording_hash":r.fingerprint(),"source_hashes":r.source_hashes,"runtime":r.runtime,"experiment":r.experiment,"completed":r.completed,"timing_evidence":r.timing_evidence,"stop_request_s":r.stop_request_s,"stop_receipt_s":r.stop_receipt_s})).collect::<Vec<_>>(),"contexts":s.refinement.capture_contexts,"assignments":s.refinement.recording_assignments,"recording_exposure":s.refinement_evidence.recording_exposure,"recording_reservations":s.refinement_evidence.recording_reservations,"recording_held_out":s.refinement_evidence.recording_held_out,"archive_observation_blake3":s.archive.observation_blake3,"archive_model_blake3":s.archive.model_blake3,"trial_splits":s.archive.trials.iter().map(|t|(&t.id,&t.split)).collect::<BTreeMap<_,_>>(),"model":s.draft,"experiment":s.refinement.experiment,"coordinates":s.refinement.coordinates,"scenarios":s.refinement.scenarios,"selections":s.refinement_evidence.selections,"validation_seen":s.validation_seen,"validation_influenced":s.validation_influenced,"assumptions":"Offline exploratory controller tracking and archive model calibration. Timing evidence and task limits are captured hypotheses; no hardware validation or physical-source acceptance."})
    }
    fn operation_identity(&self)->serde_json::Value {match &self.operation {
        Operation::FitCombined{additional}=>serde_json::json!({"FitCombined":{"additional":additional.as_ref().map(|s|serde_json::json!({"schema_version":s.version,"archive_hash":data::CalibrationData::fingerprint(&s.archive),"model_hash":s.draft.fingerprint(),"contexts":s.refinement.capture_contexts,"assignments":s.refinement.recording_assignments,"trial_roles_limits":s.archive.trials.iter().map(|t|serde_json::json!({"id":t.id,"device":t.device,"split":t.split,"limits":t.limits})).collect::<Vec<_>>(),"recordings":s.refinement.recordings.iter().map(|r|serde_json::json!({"recording_hash":r.fingerprint(),"schema_version":r.version,"runtime":r.runtime,"experiment":r.experiment,"timing_evidence":r.timing_evidence,"source_hashes":r.source_hashes,"completed":r.completed})).collect::<Vec<_>>(),"validation_seen":s.validation_seen,"validation_influenced":s.validation_influenced}))}}),
        op=>serde_json::to_value(op).expect("serializable operation")}}
    pub fn label(&self)->&'static str {self.operation.label()}
    pub fn total(&self)->usize {self.operation.total(&self.study)}
}
impl Operation {
    pub fn total(&self,s:&Study)->usize {match self {
        Self::PredictRecording{recording_hash,..}=>s.refinement.recordings.iter().find(|r|r.fingerprint()==*recording_hash).map_or(0,|r|r.frames.len()*3),Self::FitRecordings|Self::FitCombined{..}=>40,
        Self::Simulate=>(s.refinement.experiment.duration_s/s.refinement.experiment.timing.period_s).ceil() as usize,
        Self::Sensitivity{..}=>s.refinement.coordinates.len(),Self::Fit{..}=>40,
        Self::Robustness=>if s.refinement.scenarios.is_empty(){4}else{s.refinement.scenarios.len()},
    }}
    pub fn label(&self)->&'static str {match self {Self::PredictRecording{purpose:recording::Purpose::RecordedCommandReplay,..}=>"Recorded command replay",Self::PredictRecording{..}=>"Own-feedback closed-loop prediction",Self::FitRecordings=>"Recording fitting",Self::FitCombined{..}=>"Combined fitting",Self::Simulate=>"Simulating PWM controller",Self::Sensitivity{..}=>"Parameter sensitivity",Self::Fit{..}=>"Bounded model fitting",Self::Robustness=>"Controller robustness"}}}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ResultData {Prediction(recording::Prediction),RecordingFit(data::RecordingFitAttempt),CombinedFit(data::CombinedFitAttempt),Controller(control::Run),Sensitivity(cal::Sensitivity),FitAttempt(cal::FitAttempt),Robustness(cal::Robustness)}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Outcome {pub capture:Capture,pub result:Result<ResultData,String>,pub cancelled:bool}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Evidence {
    pub receipts:Vec<Receipt>,pub decisions:Vec<Decision>,pub candidate_uses:Vec<CandidateUse>,
    pub selections:BTreeMap<String,Vec<String>>,
    pub controller_run:Option<usize>,
    pub selected_recording:Option<String>,pub prediction_purpose:Option<recording::Purpose>,
    pub selected_fit_case:Option<recordings::FitCaseSelection>,
    #[serde(skip)] pub fit_identities:BTreeMap<String,Vec<String>>,
    #[serde(skip)] pub recording_identities:Vec<String>,
    pub recording_held_out:std::collections::BTreeSet<String>,
    pub recording_exposure:BTreeMap<String,bool>,pub recording_reservations:BTreeMap<String,data::Assignment>,pub recording_candidate_uses:Vec<recordings::CandidateUse>,
    #[serde(flatten)] pub retained_fields:BTreeMap<String,serde_json::Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub operation:Operation,pub inputs:serde_json::Value,pub runtime:serde_json::Value,
    pub result_kind:Option<String>,pub result_index:Option<usize>,pub failure:Option<String>,pub cancelled:bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Decision {pub kind:String,pub index:usize,pub decision:String,pub notes:String}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CandidateUse {pub fit:usize,pub fit_blake3:String,pub device:Option<u8>,pub validation_seen:bool,pub validation_influenced:bool,pub draft_blake3:String}
fn family(s:&Study)->cal::Family {cal::Family{shared:s.draft.clone(),device_deltas:BTreeMap::new()}}
fn selection(s:&Study,ids:&[String],training:Option<bool>,path:&str)->Result<(),String> {
    if ids.is_empty() {return Err(format!("{path}: select at least one whole trial"));}
    let mut seen=std::collections::BTreeSet::new();
    for (i,id) in ids.iter().enumerate() {
        let t=s.archive.trials.iter().find(|t|&t.id==id).ok_or_else(||format!("{path}.{i}: unknown trial {id}"))?;
        if !seen.insert(id) {return Err(format!("{path}.{i}: duplicate trial {id}"));}
        if training.is_some_and(|train|train!=(t.split=="train")) {return Err(format!("{path}.{i}: trial {id} has frozen split {}",t.split));}
    }
    Ok(())
}
/// Validate the caller's original selection before partitioning it by frozen roles.
pub fn validate_archive_selection(s:&Study,ids:&[String],path:&str)->Result<(),String> {
    if ids.is_empty(){Ok(())}else{selection(s,ids,None,path)}
}
fn coordinate_devices(s:&Study,coords:&[cal::Coordinate],ids:Option<&[String]>)->Result<(),String> {
    for (i,c) in coords.iter().enumerate() {
        if let Some(device)=c.device {
            if !s.archive.trials.iter().any(|t|t.device==device && ids.is_none_or(|ids|ids.contains(&t.id)))
                && !(ids.is_none() && s.refinement.recordings.iter().any(|r|r.experiment.device==device)) {
                return Err(format!("refinement.coordinates.{i}.device: device {device} absent from selected source trials"));
            }
        }
    }
    Ok(())
}
/// Refused mutations preserve the entire original payload, including unknown fields.
pub fn apply(s:&mut Study,command:Command)->Result<(),String> {
    let mut next=s.clone();
    match command {
        c @ (Command::ImportRecording{..}|Command::SelectRecording{..}|Command::AppendContext{..}|Command::AssignRecording{..}|Command::UseRecordingFit{..}|Command::SelectFitCase{..})=>recordings::command(&mut next,c)?,
        Command::SetPredictionPurpose(p)=>next.refinement_evidence.prediction_purpose=Some(p),
        Command::SetExperiment(e)=>{authoring::experiment(&e)?;if next.refinement.experiment!=e {next.refinement.experiment=e;next.candidate_edited();}},
        Command::SetCoordinates(coords)=>{coordinate_devices(&next,&coords,None)?;if !coords.is_empty(){authoring::coordinates(&family(&next),&coords)?;}if next.refinement.coordinates!=coords {next.refinement.coordinates=coords;next.candidate_edited();}},
        Command::SetScenarios(v)=>{if !v.is_empty(){authoring::scenarios(&next.refinement.experiment,&v)?;}if next.refinement.scenarios!=v {next.refinement.scenarios=v;next.candidate_edited();}},
        Command::SetSelection{kind,ids}=>{
            let training=match kind.as_str(){"train"=>Some(true),"validation"=>Some(false),"sensitivity"=>None,_=>return Err("refinement.selection.kind: expected train, validation or sensitivity".into())};
            if !ids.is_empty(){selection(&next,&ids,training,&format!("refinement.selection.{kind}"))?;}
            if next.refinement_evidence.selections.get(&kind)!=Some(&ids) {
                next.candidate_edited();
                commands::expose(&mut next,&ids);
                next.refinement_evidence.selections.insert(kind,ids);
            }
        },
        Command::UseFit{fit,device}=>{
            let source=next.refinement.fits.get(fit).ok_or_else(||format!("refinement.use_fit.fit: missing fit {fit}"))?.clone();
            source.validate(&next.archive).map_err(|s|format!("refinement.use_fit.fit: {s}"))?;
            if !source.has_verified_traces() {return Err("refinement.use_fit.fit: candidate requires complete captured prediction traces; legacy summaries remain reviewable".into());}
            if source.scores.iter().any(|s|s.failure.is_some() || s.baseline.is_none() || s.candidate.is_none()) {return Err("refinement.use_fit.fit: incomplete or failed fit cannot supply a candidate".into());}
            if !source.scores.iter().any(|s|s.device==next.refinement.experiment.device) {return Err("refinement.use_fit.device: experiment device absent from fitted archive".into());}
            if let Some(d)=device {
                if d!=next.refinement.experiment.device || !source.scores.iter().any(|s|s.device==d) {return Err("refinement.use_fit.device: candidate must apply to the experiment device and captured fit trials".into());}
            } else if !source.candidate.device_deltas.is_empty() {return Err("refinement.use_fit.device: explicit device required for a family with device deviations".into());}
            let model=match device {Some(d)=>source.candidate.model(d)?,None=>source.candidate.shared.clone()};model.validate()?;
            // Inspection/use of held-out fit evidence happens before any candidate edit.
            commands::expose(&mut next,&source.validation_ids);next.candidate_edited();
            next.validation_influenced |= source.validation_influenced;
            next.draft=model;
            next.refinement_evidence.candidate_uses.push(CandidateUse{fit,fit_blake3:blake3::hash(&serde_json::to_vec(&source).map_err(|e|e.to_string())?).to_hex().to_string(),device,validation_seen:next.validation_seen,validation_influenced:next.validation_influenced,draft_blake3:next.draft.fingerprint()});
        },
        Command::SelectControllerRun(index)=>{
            if index>=next.refinement.controller_runs.len(){return Err("refinement.controller_run: missing retained run".into());}
            next.refinement_evidence.controller_run=Some(index);
        },
        Command::SetDecision{kind,index,decision,notes}=>{
            let len=match kind.as_str(){"controller"=>next.refinement.controller_runs.len(),"sensitivity"=>next.refinement.sensitivities.len(),"fit"=>next.refinement.fits.len(),"fit_attempt"=>next.refinement.fit_attempts.len(),"robustness"=>next.refinement.robustness.len(),"prediction"=>next.refinement.predictions.len(),"recording_fit"=>next.refinement.recording_fits.len(),"combined_fit"=>next.refinement.combined_fits.len(),_=>return Err("refinement.decision.kind: unknown result kind".into())};
            if index>=len {return Err("refinement.decision.index: missing result".into());}
            if decision.trim().is_empty() {return Err("refinement.decision.decision: nonempty review decision required".into());}
            if kind=="fit" {let ids=next.refinement.fits[index].validation_ids.clone();commands::expose(&mut next,&ids);}
            let ids=match kind.as_str(){"prediction"=>vec![next.refinement.predictions[index].recording_hash.clone()],"recording_fit"=>next.refinement.recording_fits[index].attempt.request.validation_ids.clone(),"combined_fit"=>next.refinement.combined_fits[index].attempt.request.validation_ids.clone(),_=>vec![]};for hash in ids{recordings::expose(&mut next,&hash,false);}
            next.refinement_evidence.decisions.push(Decision{kind,index,decision,notes});
        }
    }
    if next.validation_influenced {for v in next.refinement_evidence.recording_exposure.values_mut(){*v=true;}}
    *s=next;Ok(())
}
pub fn prepare(s:&mut Study,mut operation:Operation)->Result<Capture,String> {
    let mut snapshot=s.clone();
    if matches!(operation,Operation::Robustness) && snapshot.refinement.scenarios.is_empty() {snapshot.refinement.scenarios=default_scenarios(s);}
    let validation_source=&snapshot;
    s.draft.validate().map_err(|e|format!("draft: {e}"))?;
    authoring::experiment(&s.refinement.experiment)?;
    recordings::prepare(s,&operation)?;
    let ids=match &operation {
        Operation::PredictRecording{..}|Operation::FitRecordings|Operation::FitCombined{..}=>vec![],
        Operation::Simulate=>vec![],
        Operation::Sensitivity{selected}=>{selection(s,selected,None,"refinement.sensitivity.selected")?;coordinate_devices(s,&s.refinement.coordinates,Some(selected))?;authoring::coordinates(&family(s),&s.refinement.coordinates)?;selected.clone()},
        Operation::Fit{train,validation}=>{
            selection(s,train,Some(true),"refinement.fit.train")?;selection(s,validation,Some(false),"refinement.fit.validation")?;
            if train.iter().any(|id|validation.contains(id)) {return Err("refinement.fit.validation: split overlaps training".into());}
            coordinate_devices(s,&s.refinement.coordinates,Some(train))?;
            authoring::coordinates(&family(s),&s.refinement.coordinates)?;validation.clone()
        },
        Operation::Robustness=>{authoring::scenarios(&validation_source.refinement.experiment,&validation_source.refinement.scenarios)?;vec![]}
    };
    let captured_unix_ns=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|format!("refinement.capture.time: {e}"))?.as_nanos().to_string();
    commands::expose(s,&ids);
    s.refinement_evidence.recording_reservations=recordings::frozen(s)?;
    match &operation {
        Operation::PredictRecording{recording_hash,purpose}=>{recordings::expose(s,recording_hash,false);s.refinement_evidence.selected_recording=Some(recording_hash.clone());s.refinement_evidence.prediction_purpose=Some(*purpose);},
        Operation::FitRecordings|Operation::FitCombined{..}=>{if matches!(&operation,Operation::FitCombined{..}) {let archive_ids=s.archive.trials.iter().filter(|t|t.split!="train").map(|t|t.id.clone()).collect::<Vec<_>>();commands::expose(s,&archive_ids);}let hashes=s.refinement.recording_assignments.iter().map(|a|a.recording_hash.clone()).collect::<Vec<_>>();for hash in hashes {recordings::expose(s,&hash,false);} if let Operation::FitCombined{additional:Some(extra)}=&operation {s.validation_seen|=extra.validation_seen||extra.archive.trials.iter().any(|t|t.split!="train"&&t.device==s.refinement.experiment.device);s.validation_influenced|=extra.validation_influenced;for a in &extra.refinement.recording_assignments{s.refinement_evidence.recording_reservations.entry(a.recording_hash.clone()).or_insert_with(||a.clone());if a.role==data::Role::HeldOut{s.validation_seen=true;s.refinement_evidence.recording_exposure.entry(a.recording_hash.clone()).or_default();}}for (hash,a) in recordings::frozen(extra)?{s.refinement_evidence.recording_reservations.entry(hash).or_insert(a);}for (hash,influenced) in &extra.refinement_evidence.recording_exposure{*s.refinement_evidence.recording_exposure.entry(hash.clone()).or_default()|=*influenced;}}},_=>{}}
    snapshot.refinement_evidence=s.refinement_evidence.clone();
    snapshot.refinement_evidence.receipts.clear();
    if let Operation::FitCombined{additional:Some(extra)}=&mut operation {extra.refinement_evidence.receipts.clear();}
    snapshot.validation_seen=s.validation_seen;snapshot.validation_influenced=s.validation_influenced;
    Ok(Capture{study:snapshot,operation,runtime:super::execution_identity(),captured_unix_ns})
}
/// Existing runtime implementations are the only executors. Failure remains inspectable evidence.
pub fn execute(capture:Capture,cancel:&AtomicBool,progress:impl FnMut(usize,usize))->Result<Outcome,String> {
    let s=&capture.study;let w=&s.refinement;let f=family(s);
    let mut result=match &capture.operation {
        Operation::PredictRecording{recording_hash,purpose}=>recording::predict(recordings::source(s,recording_hash)?,&s.draft,*purpose,&recordings::limits(s,recording_hash)?,cancel,progress).map(ResultData::Prediction),
        Operation::FitRecordings=>{let dataset=data::RecordingDataset::capture(&w.recordings,&w.recording_assignments)?;let request=recordings::request(s,&dataset)?;let attempt=cal::attempt(&dataset,&request,cancel,progress);Ok(ResultData::RecordingFit(data::RecordingFitAttempt{dataset,attempt}))},
        Operation::FitCombined{additional}=>{let dataset=recordings::dataset(s,additional.as_deref())?;let request=recordings::combined_request(s,&dataset)?;let attempt=cal::attempt(&dataset,&request,cancel,progress);Ok(ResultData::CombinedFit(data::CombinedFitAttempt{dataset,attempt}))},
        Operation::Simulate=>control::simulate(&w.experiment,&s.draft,cancel,progress).map(ResultData::Controller),
        Operation::Sensitivity{selected}=>cal::sensitivity(&s.archive,&f,selected,&w.coordinates,cancel,progress).map(ResultData::Sensitivity),
        Operation::Fit{train,validation}=>Ok(ResultData::FitAttempt(cal::attempt(&s.archive,&cal::FitRequest{model:f,training_ids:train.clone(),validation_ids:validation.clone(),coordinates:w.coordinates.clone(),maximum_evaluations:40,validation_influenced:s.validation_influenced},cancel,progress))),
        Operation::Robustness=>cal::robustness(&w.experiment,&w.scenarios,cancel,progress).map(ResultData::Robustness),
    };
    if let Ok(value)=&result {if let Err(error)=recordings::validate_result(&capture,value){result=Err(format!("refinement.result: {error}"));}}
    let cancelled=match &result {Ok(ResultData::RecordingFit(r))=>r.attempt.cancelled,Ok(ResultData::CombinedFit(r))=>r.attempt.cancelled,Ok(ResultData::Prediction(_))=>false,Ok(ResultData::Controller(r))=>r.cancelled,Ok(ResultData::FitAttempt(r))=>r.cancelled,Ok(ResultData::Robustness(r))=>r.cancelled,Ok(ResultData::Sensitivity(_))=>false,Err(e)=>e.to_lowercase().contains("cancelled")};
    Ok(Outcome{capture,result,cancelled})
}
pub fn apply_outcome(s:&mut Study,outcome:Outcome) {
    let mut inputs=outcome.capture.inputs();if let Ok(result)=&outcome.result{if let Some(hash)=recordings::result_fit_identity(result){inputs["fit_result_blake3"]=serde_json::json!(hash);}}apply_outcome_with_inputs(s,outcome,inputs);
}
/// Job hosts compute the bounded input projection before dispatching terminal attachment.
pub fn apply_outcome_with_inputs(s:&mut Study,outcome:Outcome,inputs:serde_json::Value) {
    let c=&outcome.capture;let captured=&c.study;
    let receipt_operation=match &c.operation {Operation::FitCombined{..}=>Operation::FitCombined{additional:None},op=>op.clone()};
    let mut receipt=Receipt{operation:receipt_operation,inputs,runtime:c.runtime.clone(),result_kind:None,result_index:None,failure:None,cancelled:outcome.cancelled};
    // Exact input content is retained once by the existing Study owner. Only
    // bounded content references belong in receipts; merging Arc-backed inputs is
    // cheap and must precede rejection/cancellation/reservation attachment guards.
    if let Err(error)=s.input_contents.merge(&captured.input_contents) {
        receipt.failure=Some(format!("refinement.result.input_contents: {error}"));
        s.refinement_evidence.receipts.push(receipt);return;
    }
    s.validation_seen |= captured.validation_seen;s.validation_influenced |= captured.validation_influenced;
    s.refinement_evidence.recording_held_out.extend(captured.refinement_evidence.recording_held_out.iter().cloned());
    for (hash,influenced) in &captured.refinement_evidence.recording_exposure {*s.refinement_evidence.recording_exposure.entry(hash.clone()).or_default()|=*influenced||s.validation_influenced;}
    for (hash,a) in &captured.refinement_evidence.recording_reservations {
        if s.refinement_evidence.recording_reservations.get(hash).is_some_and(|old|old!=a)||s.refinement.recording_assignments.iter().any(|old|old.recording_hash==*hash&&old!=a) {
            receipt.failure=Some(format!("refinement.result.recording_reservations.{hash}: conflicts with current frozen assignment"));s.refinement_evidence.receipts.push(receipt);return;
        }
    }
    for (hash,a) in &captured.refinement_evidence.recording_reservations{s.refinement_evidence.recording_reservations.entry(hash.clone()).or_insert_with(||a.clone());}
    match outcome.result {
        Ok(ResultData::Prediction(v))=>{receipt.result_kind=Some("prediction".into());receipt.result_index=Some(s.refinement.predictions.len());s.refinement.predictions.push(v);},
        Ok(ResultData::RecordingFit(v))=>{receipt.result_kind=Some("recording_fit".into());receipt.result_index=Some(s.refinement.recording_fits.len());receipt.failure=v.attempt.failure.clone();receipt.cancelled|=v.attempt.cancelled;s.refinement.recording_fits.push(v);},
        Ok(ResultData::CombinedFit(v))=>{receipt.result_kind=Some("combined_fit".into());receipt.result_index=Some(s.refinement.combined_fits.len());receipt.failure=v.attempt.failure.clone();receipt.cancelled|=v.attempt.cancelled;s.refinement.combined_fits.push(v);},
        Ok(ResultData::Controller(v))=>{receipt.result_kind=Some("controller".into());receipt.result_index=Some(s.refinement.controller_runs.len());receipt.failure=v.failure.clone();receipt.cancelled|=v.cancelled;s.refinement.controller_runs.push(v);},
        Ok(ResultData::Sensitivity(v))=>{receipt.result_kind=Some("sensitivity".into());receipt.result_index=Some(s.refinement.sensitivities.len());s.refinement.sensitivities.push(v);},
        Ok(ResultData::FitAttempt(v))=>{receipt.result_kind=Some("fit_attempt".into());receipt.result_index=Some(s.refinement.fit_attempts.len());receipt.failure=v.failure.clone();receipt.cancelled|=v.cancelled;if let Some(f)=&v.outcome {s.refinement.fits.push(f.clone());}s.refinement.fit_attempts.push(v);},
        Ok(ResultData::Robustness(v))=>{receipt.result_kind=Some("robustness".into());receipt.result_index=Some(s.refinement.robustness.len());receipt.cancelled|=v.cancelled;let failures=v.failures.iter().cloned().chain(v.runs.iter().filter_map(|(label,_,r)|r.failure.as_ref().map(|e|format!("{label}: {e}")))).collect::<Vec<_>>();if !failures.is_empty(){receipt.failure=Some(failures.join("; "));}s.refinement.robustness.push(v);},
        Err(e)=>{s.refinement.failures.push(format!("{}: {e}",c.label()));receipt.failure=Some(e);}
    }
    if let (Some(kind),Some(index),Some(hash))=(&receipt.result_kind,receipt.result_index,receipt.inputs.get("fit_result_blake3").and_then(|v|v.as_str())) {
        if kind=="recording_fit"||kind=="combined_fit" {
            let ids=s.refinement_evidence.fit_identities.entry(kind.clone()).or_default();
            if ids.len()==index{ids.push(hash.to_owned());}
        }
    }
    s.refinement_evidence.receipts.push(receipt);
}

/// Preserves the legacy exploratory defaults, with explicit unmeasured provenance.
pub fn default_scenarios(s:&Study)->Vec<cal::Variant> {
    let timing=s.refinement.experiment.timing.clone();let mut slow=timing.clone();slow.command_delay_ticks+=1;
    let mut load=s.draft.clone();load.conditions.load_inertia*=1.25;
    vec![cal::Variant{label:"Baseline".into(),model:s.baseline.clone(),timing:timing.clone(),evidence:"Retained baseline hypothesis".into()},cal::Variant{label:"Candidate".into(),model:s.draft.clone(),timing:timing.clone(),evidence:"Current candidate hypothesis".into()},cal::Variant{label:"One additional command tick".into(),model:s.draft.clone(),timing:slow,evidence:"Exploratory timing stress; not a measured uncertainty bound".into()},cal::Variant{label:"25% additional output inertia".into(),model:load,timing,evidence:"Exploratory load stress; not a measured uncertainty bound".into()}]
}

/// Called before rendering retained refinement evidence, including historical files.
/// Merely viewing held-out outcomes must precede any subsequent authored change.
pub fn expose_review(s:&mut Study) {
    let ids=s.refinement.fits.iter().flat_map(|f|f.validation_ids.iter())
        .chain(s.refinement.fit_attempts.iter().flat_map(|f|f.request.validation_ids.iter()))
        .chain(s.refinement.sensitivities.iter().flat_map(|f|f.trial_ids.iter()))
        .cloned().collect::<Vec<_>>();
    commands::expose(s,&ids);
    let hashes=s.refinement.predictions.iter().map(|p|p.recording_hash.clone()).chain(s.refinement.recording_fits.iter().flat_map(|f|f.attempt.request.validation_ids.clone())).chain(s.refinement.combined_fits.iter().flat_map(|f|f.attempt.request.validation_ids.clone())).collect::<Vec<_>>();for hash in hashes{recordings::expose(s,&hash,false);}
}


/// Additive retained evidence is checked against its immutable workspace sources.
/// Future opaque fields remain untouched; runtime identity need not equal the current build.
pub fn validate_evidence(s:&Study)->Result<(),String> {
    recordings::validate_evidence(s)?;
    let e=&s.refinement_evidence;
    for (kind,ids) in &e.selections {
        let training=match kind.as_str(){"train"=>Some(true),"validation"=>Some(false),"sensitivity"=>None,_=>continue};
        if !ids.is_empty(){selection(s,ids,training,&format!("refinement_evidence.selections.{kind}"))?;}
    }
    if e.controller_run.is_some_and(|i|i>=s.refinement.controller_runs.len()) {return Err("refinement_evidence.controller_run: missing retained run".into());}
    for (i,d) in e.decisions.iter().enumerate() {
        let len=match d.kind.as_str(){"controller"=>s.refinement.controller_runs.len(),"sensitivity"=>s.refinement.sensitivities.len(),"fit"=>s.refinement.fits.len(),"fit_attempt"=>s.refinement.fit_attempts.len(),"robustness"=>s.refinement.robustness.len(),"prediction"=>s.refinement.predictions.len(),"recording_fit"=>s.refinement.recording_fits.len(),"combined_fit"=>s.refinement.combined_fits.len(),_=>return Err(format!("refinement_evidence.decisions.{i}.kind: unknown result kind"))};
        if d.index>=len {return Err(format!("refinement_evidence.decisions.{i}.index: missing retained result"));}
        if d.decision.trim().is_empty(){return Err(format!("refinement_evidence.decisions.{i}.decision: nonempty review required"));}
    }
    for (i,u) in e.candidate_uses.iter().enumerate() {
        let source=s.refinement.fits.get(u.fit).ok_or_else(||format!("refinement_evidence.candidate_uses.{i}.fit: missing source fit"))?;
        let hash=blake3::hash(&serde_json::to_vec(source).map_err(|e|e.to_string())?).to_hex().to_string();
        if hash!=u.fit_blake3 {return Err(format!("refinement_evidence.candidate_uses.{i}.fit_blake3: source fit changed"));}
        if !source.has_verified_traces() || source.scores.iter().any(|s|s.failure.is_some() || s.baseline.is_none() || s.candidate.is_none()) {return Err(format!("refinement_evidence.candidate_uses.{i}.fit: incomplete source candidate"));}
        let expected=match u.device {
            Some(device)=>{
                if !source.scores.iter().any(|s|s.device==device) {return Err(format!("refinement_evidence.candidate_uses.{i}.device: device absent from source fit"));}
                source.candidate.model(device).map_err(|e|format!("refinement_evidence.candidate_uses.{i}.device: {e}"))?
            },
            None=>{
                if !source.candidate.device_deltas.is_empty(){return Err(format!("refinement_evidence.candidate_uses.{i}.device: device deviations require explicit applicability"));}
                source.candidate.shared.clone()
            }
        };
        if expected.fingerprint()!=u.draft_blake3 {return Err(format!("refinement_evidence.candidate_uses.{i}.draft_blake3: adopted model differs from source"));}
        if !u.validation_seen || !u.validation_influenced || !s.validation_seen || !s.validation_influenced {return Err(format!("refinement_evidence.candidate_uses.{i}.validation_influenced: adoption must retain held-out exposure and influence"));}
    }
    Ok(())
}

#[cfg(test)]
#[path="recording_fixtures.rs"]
mod recording_fixtures;

/// Compact durable review identity. Comparing this never serializes recording frames.
#[derive(Clone, PartialEq)]
pub struct ReviewExposure {
    seen:bool,
    influenced:bool,
    recordings:std::collections::BTreeMap<String,bool>,
    reservations:std::collections::BTreeMap<String,data::Assignment>,
    held_out:std::collections::BTreeSet<String>,
}
impl ReviewExposure {
    pub fn capture(study:&Study)->Self {
        Self{seen:study.validation_seen,influenced:study.validation_influenced,recordings:study.refinement_evidence.recording_exposure.clone(),reservations:study.refinement_evidence.recording_reservations.clone(),held_out:study.refinement_evidence.recording_held_out.clone()}
    }
}

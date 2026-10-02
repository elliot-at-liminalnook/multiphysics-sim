//! One portable offline refinement command/capture/execution/application contract.
//! Hosts own scheduling and study identity; this module owns validated shared mutations.
use super::{Study, commands};
use crate::controller_refinement::{authoring, calibration as cal, control};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::atomic::{AtomicBool}};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Command {
    SetExperiment(control::Experiment), SetCoordinates(Vec<cal::Coordinate>),
    SetScenarios(Vec<cal::Variant>), SetSelection{kind:String,ids:Vec<String>}, UseFit{fit:usize,device:Option<u8>},
    SelectControllerRun(usize),
    SetDecision{kind:String,index:usize,decision:String,notes:String},
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Operation {
    Simulate, Sensitivity{selected:Vec<String>}, Fit{train:Vec<String>,validation:Vec<String>}, Robustness,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capture {pub study:Study,pub operation:Operation,pub runtime:serde_json::Value,pub captured_unix_ns:String}
impl Capture {
    /// Bounded projection: never nests previous workspace results or receipts.
    pub fn inputs(&self)->serde_json::Value {
        let s=&self.study;
        serde_json::json!({"schema_version":s.version,"captured_unix_ns":self.captured_unix_ns,"operation":self.operation,"archive_observation_blake3":s.archive.observation_blake3,"archive_model_blake3":s.archive.model_blake3,"trial_splits":s.archive.trials.iter().map(|t|(&t.id,&t.split)).collect::<BTreeMap<_,_>>(),"model":s.draft,"experiment":s.refinement.experiment,"coordinates":s.refinement.coordinates,"scenarios":s.refinement.scenarios,"selections":s.refinement_evidence.selections,"validation_seen":s.validation_seen,"validation_influenced":s.validation_influenced,"assumptions":"Offline exploratory controller tracking and archive model calibration. Timing evidence and task limits are captured hypotheses; no hardware validation or physical-source acceptance."})
    }
    pub fn label(&self)->&'static str {self.operation.label()}
    pub fn total(&self)->usize {self.operation.total(&self.study)}
}
impl Operation {
    pub fn total(&self,s:&Study)->usize {match self {
        Self::Simulate=>(s.refinement.experiment.duration_s/s.refinement.experiment.timing.period_s).ceil() as usize,
        Self::Sensitivity{..}=>s.refinement.coordinates.len(),Self::Fit{..}=>40,
        Self::Robustness=>if s.refinement.scenarios.is_empty(){4}else{s.refinement.scenarios.len()},
    }}
    pub fn label(&self)->&'static str {match self {Self::Simulate=>"Simulating PWM controller",Self::Sensitivity{..}=>"Parameter sensitivity",Self::Fit{..}=>"Bounded model fitting",Self::Robustness=>"Controller robustness"}}}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ResultData {Controller(control::Run),Sensitivity(cal::Sensitivity),FitAttempt(cal::FitAttempt),Robustness(cal::Robustness)}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Outcome {pub capture:Capture,pub result:Result<ResultData,String>,pub cancelled:bool}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Evidence {
    pub receipts:Vec<Receipt>,pub decisions:Vec<Decision>,pub candidate_uses:Vec<CandidateUse>,
    pub selections:BTreeMap<String,Vec<String>>,
    pub controller_run:Option<usize>,
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
fn coordinate_devices(s:&Study,coords:&[cal::Coordinate],ids:Option<&[String]>)->Result<(),String> {
    for (i,c) in coords.iter().enumerate() {
        if let Some(device)=c.device {
            if !s.archive.trials.iter().any(|t|t.device==device && ids.is_none_or(|ids|ids.contains(&t.id))) {
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
            let len=match kind.as_str(){"controller"=>next.refinement.controller_runs.len(),"sensitivity"=>next.refinement.sensitivities.len(),"fit"=>next.refinement.fits.len(),"fit_attempt"=>next.refinement.fit_attempts.len(),"robustness"=>next.refinement.robustness.len(),_=>return Err("refinement.decision.kind: unknown result kind".into())};
            if index>=len {return Err("refinement.decision.index: missing result".into());}
            if decision.trim().is_empty() {return Err("refinement.decision.decision: nonempty review decision required".into());}
            if kind=="fit" {let ids=next.refinement.fits[index].validation_ids.clone();commands::expose(&mut next,&ids);}
            next.refinement_evidence.decisions.push(Decision{kind,index,decision,notes});
        }
    }
    *s=next;Ok(())
}
pub fn prepare(s:&mut Study,operation:Operation)->Result<Capture,String> {
    let mut snapshot=s.clone();
    if matches!(operation,Operation::Robustness) && snapshot.refinement.scenarios.is_empty() {snapshot.refinement.scenarios=default_scenarios(s);}
    let validation_source=&snapshot;
    s.draft.validate().map_err(|e|format!("draft: {e}"))?;
    authoring::experiment(&s.refinement.experiment)?;
    let ids=match &operation {
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
    snapshot.validation_seen=s.validation_seen;snapshot.validation_influenced=s.validation_influenced;
    Ok(Capture{study:snapshot,operation,runtime:super::execution_identity(),captured_unix_ns})
}
/// Existing runtime implementations are the only executors. Failure remains inspectable evidence.
pub fn execute(capture:Capture,cancel:&AtomicBool,progress:impl FnMut(usize,usize))->Result<Outcome,String> {
    let s=&capture.study;let w=&s.refinement;let f=family(s);
    let result=match &capture.operation {
        Operation::Simulate=>control::simulate(&w.experiment,&s.draft,cancel,progress).map(ResultData::Controller),
        Operation::Sensitivity{selected}=>cal::sensitivity(&s.archive,&f,selected,&w.coordinates,cancel,progress).map(ResultData::Sensitivity),
        Operation::Fit{train,validation}=>Ok(ResultData::FitAttempt(cal::attempt(&s.archive,&cal::FitRequest{model:f,training_ids:train.clone(),validation_ids:validation.clone(),coordinates:w.coordinates.clone(),maximum_evaluations:40,validation_influenced:s.validation_influenced},cancel,progress))),
        Operation::Robustness=>cal::robustness(&w.experiment,&w.scenarios,cancel,progress).map(ResultData::Robustness),
    };
    let cancelled=match &result {Ok(ResultData::Controller(r))=>r.cancelled,Ok(ResultData::FitAttempt(r))=>r.cancelled,Ok(ResultData::Robustness(r))=>r.cancelled,Ok(ResultData::Sensitivity(_))=>false,Err(e)=>e.to_lowercase().contains("cancelled")};
    Ok(Outcome{capture,result,cancelled})
}
pub fn apply_outcome(s:&mut Study,outcome:Outcome) {
    let c=&outcome.capture;let captured=&c.study;
    let mut receipt=Receipt{operation:c.operation.clone(),inputs:c.inputs(),runtime:c.runtime.clone(),result_kind:None,result_index:None,failure:None,cancelled:outcome.cancelled};
    s.validation_seen |= captured.validation_seen;s.validation_influenced |= captured.validation_influenced;
    match outcome.result {
        Ok(ResultData::Controller(v))=>{receipt.result_kind=Some("controller".into());receipt.result_index=Some(s.refinement.controller_runs.len());receipt.failure=v.failure.clone();receipt.cancelled|=v.cancelled;s.refinement.controller_runs.push(v);},
        Ok(ResultData::Sensitivity(v))=>{receipt.result_kind=Some("sensitivity".into());receipt.result_index=Some(s.refinement.sensitivities.len());s.refinement.sensitivities.push(v);},
        Ok(ResultData::FitAttempt(v))=>{receipt.result_kind=Some("fit_attempt".into());receipt.result_index=Some(s.refinement.fit_attempts.len());receipt.failure=v.failure.clone();receipt.cancelled|=v.cancelled;if let Some(f)=&v.outcome {s.refinement.fits.push(f.clone());}s.refinement.fit_attempts.push(v);},
        Ok(ResultData::Robustness(v))=>{receipt.result_kind=Some("robustness".into());receipt.result_index=Some(s.refinement.robustness.len());receipt.cancelled|=v.cancelled;let failures=v.failures.iter().cloned().chain(v.runs.iter().filter_map(|(label,_,r)|r.failure.as_ref().map(|e|format!("{label}: {e}")))).collect::<Vec<_>>();if !failures.is_empty(){receipt.failure=Some(failures.join("; "));}s.refinement.robustness.push(v);},
        Err(e)=>{s.refinement.failures.push(format!("{}: {e}",c.label()));receipt.failure=Some(e);}
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
}


/// Additive retained evidence is checked against its immutable workspace sources.
/// Future opaque fields remain untouched; runtime identity need not equal the current build.
pub fn validate_evidence(s:&Study)->Result<(),String> {
    let e=&s.refinement_evidence;
    for (kind,ids) in &e.selections {
        let training=match kind.as_str(){"train"=>Some(true),"validation"=>Some(false),"sensitivity"=>None,_=>continue};
        if !ids.is_empty(){selection(s,ids,training,&format!("refinement_evidence.selections.{kind}"))?;}
    }
    if e.controller_run.is_some_and(|i|i>=s.refinement.controller_runs.len()) {return Err("refinement_evidence.controller_run: missing retained run".into());}
    for (i,d) in e.decisions.iter().enumerate() {
        let len=match d.kind.as_str(){"controller"=>s.refinement.controller_runs.len(),"sensitivity"=>s.refinement.sensitivities.len(),"fit"=>s.refinement.fits.len(),"fit_attempt"=>s.refinement.fit_attempts.len(),"robustness"=>s.refinement.robustness.len(),_=>return Err(format!("refinement_evidence.decisions.{i}.kind: unknown result kind"))};
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

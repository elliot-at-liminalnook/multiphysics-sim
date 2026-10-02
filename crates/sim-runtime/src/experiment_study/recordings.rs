//! Shared recording import and immutable dataset authoring; hosts provide I/O and jobs.
use super::{Command, Operation};
use crate::{experiment_study::Study, controller_refinement::{recording::Recording, context::CaptureContext, calibration_data::{self as data, CalibrationData}, calibration as cal}};
use serde::{Serialize, Deserialize};
use std::collections::BTreeMap;
#[derive(Clone, Debug)]
pub enum ImportClassification { Controller(Recording), FpgaDeferred }
pub fn classify(bytes:&[u8])->Result<ImportClassification,String> {
    let v:serde_json::Value=serde_json::from_slice(bytes).map_err(|e|format!("recording.import.json: {e}"))?;
    let controller=v.get("experiment").is_some();let fpga=v.get("plan").is_some();
    if controller==fpga {return Err("recording.import.kind: expected exactly one controller experiment or FPGA plan".into());}
    if fpga {let r:crate::controller_refinement::fpga::Recording=serde_json::from_value(v).map_err(|e|format!("recording.import.fpga: {e}"))?;r.validate_capture().map_err(|e|format!("recording.import.fpga: {e}"))?;return Ok(ImportClassification::FpgaDeferred);}
    let r:Recording=serde_json::from_value(v).map_err(|e|format!("recording.import.controller: {e}"))?;r.validate().map_err(|e|format!("recording.import.controller: {e}"))?;Ok(ImportClassification::Controller(r))
}
/// Derived presentation cache populated by host load/import jobs; not another source.
pub fn cache_identities(s:&mut Study) {s.refinement_evidence.recording_identities=s.refinement.recordings.iter().map(Recording::fingerprint).collect();}
pub fn cached_source<'a>(s:&'a Study,hash:&str)->Option<&'a Recording> {
    s.refinement_evidence.recording_identities.iter().position(|id|id==hash).and_then(|i|s.refinement.recordings.get(i))
}
pub fn source<'a>(s:&'a Study,hash:&str)->Result<&'a Recording,String> {s.refinement.recordings.iter().find(|r|r.fingerprint()==hash).ok_or_else(||format!("recording.source.{hash}: missing recording"))}
pub fn import(s:&mut Study,r:Recording)->Result<(),String> {
    r.validate().map_err(|e|format!("recording.import.controller: {e}"))?;
    cache_identities(s);
    if s.refinement.recordings.iter().any(|old|old.fingerprint()==r.fingerprint()){return Ok(());}
    // Source hashes identify code/cadence, shared across repetitions; the full content hash identifies a capture.
    let hash=r.fingerprint();s.refinement.capture_contexts.push(CaptureContext::unknown(&r));s.refinement.recordings.push(r);cache_identities(s);expose(s,&hash,false);s.refinement_evidence.selected_recording=Some(hash);Ok(())
}
pub fn context(s:&mut Study,c:CaptureContext)->Result<(),String> {
    c.validate().map_err(|e|format!("recording.context: {e}"))?;let r=source(s,&c.recording_hash)?;
    if c.fixture!=r.experiment.fixture {return Err("recording.context.fixture: captured fixture cannot be rewritten".into());}
    if let Some(id)=&r.experiment.component_id {if c.bindings.iter().any(|b|b.hardware_id==r.experiment.device && &b.cad_component_id!=id){return Err("recording.context.bindings: conflicts with captured CAD identity".into());}}
    if !s.refinement.capture_contexts.contains(&c){s.candidate_edited();s.refinement.capture_contexts.push(c);}Ok(())
}
/// Historical fit snapshots are reservations too, including studies predating the map.
pub fn frozen(s:&Study)->Result<BTreeMap<String,data::Assignment>,String> {
    let mut values=BTreeMap::new();
    let historical=s.refinement.recording_fits.iter().flat_map(|f|f.dataset.assignments.iter())
        .chain(s.refinement.combined_fits.iter().flat_map(|f|f.dataset.recordings.iter().flat_map(|d|d.assignments.iter())));
    for a in s.refinement.recording_assignments.iter().chain(s.refinement_evidence.recording_reservations.values()).chain(historical) {
        if values.get(&a.recording_hash).is_some_and(|old|old!=a) {
            return Err(format!("recording.reservations.{}: conflicting historical role, limits or rationale",a.recording_hash));
        }
        values.insert(a.recording_hash.clone(),a.clone());
    }
    Ok(values)
}
/// Reservation quarantine is monotonic even when an additional dataset is rejected.
/// Conflicting frozen declarations stay intact; a held-out identity cannot tune again.
pub fn reserve_additional(s:&mut Study,extra:&Study)->Result<(),String> {
    let existing=frozen(s)?;
    let incoming=frozen(extra)?;
    for (hash,a) in &existing {s.refinement_evidence.recording_reservations.entry(hash.clone()).or_insert_with(||a.clone());}
    for (hash,a) in incoming {
        if a.role==data::Role::HeldOut {s.refinement_evidence.recording_held_out.insert(hash.clone());s.refinement_evidence.recording_exposure.entry(hash.clone()).or_default();s.validation_seen=true;}
        if !s.refinement_evidence.recording_reservations.contains_key(&hash)
            && !existing.get(&hash).is_some_and(|old|old!=&a) {
            s.refinement_evidence.recording_reservations.insert(hash,a);
        }
    }
    s.refinement_evidence.recording_held_out.extend(extra.refinement_evidence.recording_held_out.iter().cloned());
    s.validation_seen|=extra.validation_seen;s.validation_influenced|=extra.validation_influenced;
    for (hash,influenced) in &extra.refinement_evidence.recording_exposure {*s.refinement_evidence.recording_exposure.entry(hash.clone()).or_default()|=*influenced;}
    Ok(())
}
pub fn assign(s:&mut Study,a:data::Assignment)->Result<(),String> {
    source(s,&a.recording_hash)?;
    if a.role==data::Role::Train&&s.refinement_evidence.recording_held_out.contains(&a.recording_hash) {return Err("recording.assignment.role: identity has an irrevocable held-out reservation".into());}
    if frozen(s)?.get(&a.recording_hash).is_some_and(|old|old!=&a) {return Err("recording.assignment: conflicts with frozen historical reservation".into());}
    if s.refinement_evidence.recording_reservations.get(&a.recording_hash).is_some_and(|old|old!=&a){return Err("recording.assignment: conflicts with earlier frozen reservation".into());}
    if let Some(old)=s.refinement.recording_assignments.iter().find(|old|old.recording_hash==a.recording_hash) {if old==&a{return Ok(());}return Err("recording.assignment: role, limits and rationale are frozen by recording identity".into());}
    data::RecordingDataset::capture(&s.refinement.recordings,std::slice::from_ref(&a)).map_err(|e|format!("recording.assignment: {e}"))?;
    for fit in &s.refinement.combined_fits {if let Some(data)=&fit.dataset.recordings {if let Some(old)=data.assignments.iter().find(|old|old.recording_hash==a.recording_hash){if old!=&a{return Err("recording.assignment: conflicts with earlier additional-study reservation".into());}}}}
    if a.role==data::Role::HeldOut && s.refinement_evidence.recording_exposure.contains_key(&a.recording_hash){s.validation_seen=true;s.validation_influenced|=s.refinement_evidence.recording_exposure[&a.recording_hash];}
    s.candidate_edited();if a.role==data::Role::HeldOut{s.refinement_evidence.recording_held_out.insert(a.recording_hash.clone());}s.refinement_evidence.recording_reservations.insert(a.recording_hash.clone(),a.clone());s.refinement.recording_assignments.push(a);Ok(())
}
pub fn dataset(s:&Study,additional:Option<&Study>)->Result<data::CombinedDataset,String> {
    let current_frozen=frozen(s)?;
    if s.refinement.recording_assignments.iter().any(|a|a.role==data::Role::Train&&s.refinement_evidence.recording_held_out.contains(&a.recording_hash)){return Err("recording.dataset.roles: tuning identity has an irrevocable held-out reservation".into());}
    let mut recordings=s.refinement.recordings.clone();let mut assignments=s.refinement.recording_assignments.clone();let mut archives=vec![s.archive.clone()];
    if let Some(extra)=additional {
        extra.validate().map_err(|e|format!("recording.additional.study: {e}"))?;
        for (hash,a) in frozen(extra)? {
            if a.role==data::Role::Train&&s.refinement_evidence.recording_held_out.contains(&hash){return Err("recording.additional.assignment: tuning identity has held-out reservation".into());}
            if current_frozen.get(&hash).is_some_and(|old|old!=&a) {
                return Err(format!("recording.additional.assignment.{hash}: conflicts with historical reservation"));
            }
        }
        archives.push(extra.archive.clone());
        for r in &extra.refinement.recordings {if !recordings.iter().any(|old|old.fingerprint()==r.fingerprint()) {recordings.push(r.clone());}}
        for (hash,a) in &extra.refinement_evidence.recording_reservations {if s.refinement_evidence.recording_reservations.get(hash).is_some_and(|old|old!=a)||assignments.iter().any(|old|old.recording_hash==*hash&&old!=a){return Err("recording.additional.reservations: conflicts with earlier frozen identity".into());}}
        for a in &extra.refinement.recording_assignments {if s.refinement_evidence.recording_reservations.get(&a.recording_hash).is_some_and(|old|old!=a){return Err("recording.additional.assignment: conflicts with earlier reservation".into());}if let Some(old)=assignments.iter().find(|old|old.recording_hash==a.recording_hash){if old!=a{return Err("recording.additional.assignment: conflicting frozen role, limits or rationale".into());}}else{assignments.push(a.clone());}}
    }
    let recordings=if assignments.is_empty(){None}else{Some(data::RecordingDataset::capture(&recordings,&assignments).map_err(|e|format!("recording.dataset: {e}"))?)};
    let d=data::CombinedDataset{archives,recordings};d.validate().map_err(|e|format!("recording.additional.identity: {e}"))?;Ok(d)
}
pub fn request(s:&Study,d:&impl CalibrationData)->Result<cal::FitRequest,String> {
    let cases=d.cases().map_err(|e|format!("recording.fit.dataset: {e}"))?;
    if cases.iter().any(|c|c.split=="train"&&s.refinement_evidence.recording_held_out.contains(&c.id)){return Err("recording.fit.roles: held-out identity cannot become tuning data".into());}
    let f=cal::Family{shared:s.draft.clone(),device_deltas:BTreeMap::new()};crate::controller_refinement::authoring::coordinates(&f,&s.refinement.coordinates)?;
    for (i,c) in s.refinement.coordinates.iter().enumerate(){if c.device.is_some_and(|device|!cases.iter().any(|case|case.device==device&&case.split=="train")){return Err(format!("recording.fit.coordinates.{i}.device: absent from tuning cases"));}}
    let training_ids=cases.iter().filter(|c|c.split=="train").map(|c|c.id.clone()).collect::<Vec<_>>();let validation_ids=cases.iter().filter(|c|c.split!="train").map(|c|c.id.clone()).collect::<Vec<_>>();
    if training_ids.is_empty() || validation_ids.is_empty(){return Err("recording.fit.roles: require tuning and held-out whole runs".into());}
    Ok(cal::FitRequest{model:f,training_ids,validation_ids,coordinates:s.refinement.coordinates.clone(),maximum_evaluations:40,validation_influenced:s.validation_influenced})
}
pub fn limits(s:&Study,hash:&str)->Result<crate::experiment_comparison::Limits,String>{
    source(s,hash)?;let limits=s.refinement.recording_assignments.iter().find(|a|a.recording_hash==hash).map(|a|a.limits.clone()).or_else(||s.limits.clone()).ok_or("recording.prediction.limits: explicit comparison limits required")?;
    crate::experiment_study::commands::validate_limits(&Some(limits.clone()))?;Ok(limits)
}
pub fn expose(s:&mut Study,hash:&str,influenced:bool){
    let v=s.refinement_evidence.recording_exposure.entry(hash.into()).or_default();*v|=influenced;
    let held=s.refinement_evidence.recording_reservations.get(hash).is_some_and(|a|a.role==data::Role::HeldOut)||s.refinement.recording_assignments.iter().any(|a|a.recording_hash==hash&&a.role==data::Role::HeldOut)||s.refinement.combined_fits.iter().any(|fit|fit.dataset.recordings.as_ref().is_some_and(|d|d.assignments.iter().any(|a|a.recording_hash==hash&&a.role==data::Role::HeldOut)));
    if held{s.validation_seen=true;s.validation_influenced|=influenced;}
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CandidateUse {pub kind:String,pub index:usize,pub source_blake3:String,pub device:Option<u8>,pub draft_blake3:String}
pub fn use_fit(s:&mut Study,kind:String,index:usize,device:Option<u8>)->Result<(),String>{
    let (attempt,hash)=match kind.as_str(){"recording"|"recording_fit"=>{let v=s.refinement.recording_fits.get(index).ok_or("recording.use_fit.index: missing recording fit")?;let current=data::RecordingDataset::capture(&s.refinement.recordings,&v.dataset.assignments)?;if current.fingerprint()!=v.dataset.fingerprint(){return Err("recording.use_fit.source: source recording changed".into());}v.attempt.validate(&v.dataset)?;(v.attempt.clone(),blake3::hash(&serde_json::to_vec(v).map_err(|e|e.to_string())?).to_hex().to_string())},"combined"|"combined_fit"=>{let v=s.refinement.combined_fits.get(index).ok_or("recording.use_fit.index: missing combined fit")?;if v.dataset.archives.first().is_none_or(|a|a.fingerprint()!=s.archive.fingerprint()){return Err("recording.use_fit.source: current archive differs from captured source".into());}v.attempt.validate(&v.dataset)?;(v.attempt.clone(),blake3::hash(&serde_json::to_vec(v).map_err(|e|e.to_string())?).to_hex().to_string())},_=>return Err("recording.use_fit.kind: expected recording_fit or combined_fit".into())};
    if attempt.request.training_ids.iter().any(|id|s.refinement_evidence.recording_held_out.contains(id)){return Err("recording.use_fit.roles: candidate used a now-reserved held-out identity for tuning".into());}
    if attempt.cancelled||attempt.failure.is_some(){return Err("recording.use_fit: cancelled or failed attempt cannot supply a candidate".into());}
    let fit=attempt.outcome.as_ref().ok_or("recording.use_fit: incomplete attempt cannot supply a candidate")?;
    if !fit.has_verified_traces()||fit.scores.iter().any(|score|score.failure.is_some()||score.baseline.is_none()||score.candidate.is_none()){return Err("recording.use_fit: complete verified traces required".into());}
    let target=device.unwrap_or(s.refinement.experiment.device);
    if target!=s.refinement.experiment.device||!fit.scores.iter().any(|score|score.device==target){return Err("recording.use_fit.device: candidate is not applicable to current experiment".into());}
    if device.is_none()&&!fit.candidate.device_deltas.is_empty(){return Err("recording.use_fit.device: explicit device required".into());}
    let model=fit.candidate.model(target)?;model.validate()?;
    for id in &fit.validation_ids{expose(s,id,true);}s.validation_seen=true;s.validation_influenced=true;s.candidate_edited();s.draft=model;
    s.refinement_evidence.recording_candidate_uses.push(CandidateUse{kind,index,source_blake3:hash,device:Some(target),draft_blake3:s.draft.fingerprint()});Ok(())
}
pub fn command(s:&mut Study,c:Command)->Result<(),String>{match c {Command::ImportRecording{recording}=>import(s,recording),Command::SelectRecording{recording_hash}=>{source(s,&recording_hash)?;expose(s,&recording_hash,false);s.refinement_evidence.selected_recording=Some(recording_hash);Ok(())},Command::AppendContext{context:c}=>context(s,c),Command::AssignRecording{assignment}=>assign(s,assignment),Command::UseRecordingFit{kind,index,device}=>use_fit(s,kind,index,device),_=>unreachable!()}}
pub fn prepare(s:&Study,op:&Operation)->Result<(),String>{match op {Operation::PredictRecording{recording_hash,..}=>{let r=source(s,recording_hash)?;if !r.completed||!r.stop_verified{return Err("recording.prediction.source: incomplete captures remain inspectable and unscored".into());}limits(s,recording_hash)?;},Operation::FitRecordings=>{let d=data::RecordingDataset::capture(&s.refinement.recordings,&s.refinement.recording_assignments).map_err(|e|format!("recording.fit.dataset: {e}"))?;request(s,&d)?;},Operation::FitCombined{additional}=>{let d=dataset(s,additional.as_deref())?;combined_request(s,&d)?;},_=>{}}Ok(())}
/// Immutable candidate-use links are validated against the full attempt and dataset.
pub fn validate_evidence(s:&Study)->Result<(),String>{
    frozen(s)?;
    let e=&s.refinement_evidence;
    if let Some(hash)=&e.selected_recording{source(s,hash).map_err(|e|format!("refinement_evidence.selected_recording: {e}"))?;}
    for (hash,a) in &e.recording_reservations {if &a.recording_hash!=hash||a.rationale.trim().is_empty(){return Err("refinement_evidence.recording_reservations: identity and rationale required".into());}crate::experiment_study::commands::validate_limits(&Some(a.limits.clone()))?;if s.refinement.recording_assignments.iter().any(|local|local.recording_hash==*hash&&local!=a){return Err("refinement_evidence.recording_reservations: conflicts with assignment".into());}}
    // Historical repeated imports with identical content remain readable. New imports are idempotent.
    for (i,u) in e.recording_candidate_uses.iter().enumerate(){
        let mut copy=s.clone();copy.refinement_evidence.recording_candidate_uses.clear();copy.refinement_evidence.recording_held_out.clear();if let Some(device)=u.device{copy.refinement.experiment.device=device;}
        use_fit(&mut copy,u.kind.clone(),u.index,u.device).map_err(|e|format!("refinement_evidence.recording_candidate_uses.{i}: {e}"))?;
        let expected=copy.refinement_evidence.recording_candidate_uses.last().expect("use creates link");
        if expected.source_blake3!=u.source_blake3||expected.draft_blake3!=u.draft_blake3{return Err(format!("refinement_evidence.recording_candidate_uses.{i}: candidate source or adopted model changed"));}
        if !s.validation_seen||!s.validation_influenced{return Err(format!("refinement_evidence.recording_candidate_uses.{i}: adoption must retain exposure and influence"));}
    }
    for fit in &s.refinement.combined_fits {if let Some(d)=&fit.dataset.recordings{for a in &d.assignments{for other in &s.refinement.combined_fits{if let Some(other)=&other.dataset.recordings{if other.assignments.iter().any(|b|b.recording_hash==a.recording_hash&&b!=a){return Err("refinement.combined_fits.assignments: conflicting historical reservation".into());}}}}}}
    Ok(())
}
pub fn combined_request(s:&Study,d:&data::CombinedDataset)->Result<cal::FitRequest,String>{
    let mut request=request(s,d)?;
    let selected=s.refinement_evidence.selections.get("train").into_iter().flatten().chain(s.refinement_evidence.selections.get("validation").into_iter().flatten()).cloned().collect::<Vec<_>>();
    if s.refinement_evidence.selections.contains_key("train")||s.refinement_evidence.selections.contains_key("validation") {
        let current=s.archive.trials.iter().map(|t|t.id.clone()).collect::<Vec<_>>();
        request.training_ids.retain(|id|!current.contains(id)||selected.contains(id));request.validation_ids.retain(|id|!current.contains(id)||selected.contains(id));
    }
    if let Some(extra)=d.archives.get(1){let excluded=extra.trials.iter().filter(|t|t.device!=s.refinement.experiment.device).map(|t|t.id.clone()).collect::<Vec<_>>();request.training_ids.retain(|id|!excluded.contains(id));request.validation_ids.retain(|id|!excluded.contains(id));}
    if request.training_ids.is_empty()||request.validation_ids.is_empty(){return Err("recording.combined.selection: require tuning and held-out cases after frozen selection".into());}
    let cases=d.cases()?;
    for (i,c) in request.coordinates.iter().enumerate(){if c.device.is_some_and(|device|!cases.iter().any(|case|case.device==device&&request.training_ids.contains(&case.id))){return Err(format!("recording.combined.coordinates.{i}.device: absent from selected tuning cases"));}}
    Ok(request)
}
pub fn validate_result(c:&super::Capture,r:&super::ResultData)->Result<(),String>{
    let s=&c.study;
    match (&c.operation,r){
        (Operation::PredictRecording{recording_hash,purpose},super::ResultData::Prediction(p))=>{p.validate(source(s,recording_hash)?)?;if p.purpose!=*purpose||p.model!=s.draft||p.limits!=limits(s,recording_hash)?{return Err("recording.result.prediction: differs from captured inputs".into());}},
        (Operation::FitRecordings,super::ResultData::RecordingFit(f))=>{let d=data::RecordingDataset::capture(&s.refinement.recordings,&s.refinement.recording_assignments)?;if d.fingerprint()!=f.dataset.fingerprint(){return Err("recording.result.dataset: differs from capture".into());}if serde_json::to_value(&f.attempt.request).map_err(|e|e.to_string())?!=serde_json::to_value(request(s,&d)?).map_err(|e|e.to_string())?{return Err("recording.result.request: differs from captured model, roles, bounds or budget".into());}f.attempt.validate(&f.dataset)?;if serde_json::to_value(&f.attempt.request).map_err(|e|e.to_string())?!=serde_json::to_value(request(s,&d)?).map_err(|e|e.to_string())?{return Err("recording.result.request: differs from frozen inputs".into());}},
        (Operation::FitCombined{additional},super::ResultData::CombinedFit(f))=>{let d=dataset(s,additional.as_deref())?;if d.fingerprint()!=f.dataset.fingerprint(){return Err("recording.result.additional: differs from captured sources".into());}if serde_json::to_value(&f.attempt.request).map_err(|e|e.to_string())?!=serde_json::to_value(combined_request(s,&d)?).map_err(|e|e.to_string())?{return Err("recording.result.request: differs from captured combined selection or model".into());}f.attempt.validate(&f.dataset)?;if serde_json::to_value(&f.attempt.request).map_err(|e|e.to_string())?!=serde_json::to_value(combined_request(s,&d)?).map_err(|e|e.to_string())?{return Err("recording.result.request: differs from frozen combined inputs".into());}},
        (Operation::PredictRecording{..}|Operation::FitRecordings|Operation::FitCombined{..},_)=>return Err("recording.result.kind: differs from captured operation".into()),_=>{}}
    Ok(())
}

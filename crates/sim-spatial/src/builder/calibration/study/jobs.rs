//! Captured jobs and immutable publication receipts. Never latest-wins discard.
use super::state::{DocumentCapture, StudyOwner, StudyStamp};
use crate::{document::DocumentRegistry, jobs::{Job, Pool, Progress}};
use bevy::prelude::*;
use sim_runtime::{experiment_comparison::hx_archive, experiment_study::{self, Study, Evaluation, commands::{self, EvaluationSelection}}};
use std::{path::PathBuf,sync::{Arc,Mutex}};
use serde_json::json;
use sim_runtime::experiment_study::refinement;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind { Archive, Review, Evaluate, Refinement, RecordingImport, RecordingAuthoring, Save, Export }
impl JobKind { pub fn is_load(self)->bool { matches!(self,Self::Archive|Self::Review) } }

/// The tiny gate linearizes cancel acceptance and the start of irreversible
/// create-new publication. No disk work occurs while this mutex is held.
#[derive(Default)]
pub struct PublicationGate { pub started:bool, pub cancelled:bool }
pub enum JobOutput { Loaded(Study), Evaluated(Evaluation), Refined { outcome:refinement::Outcome, inputs:serde_json::Value }, RecordingImport { prepared:Option<Box<Study>>, input:serde_json::Value, error:Option<String> }, Published }
pub struct PendingJob {
    pub id:u64,
    pub kind:JobKind,
    pub stamp:Option<StudyStamp>,
    pub document:Option<DocumentCapture>,
    pub source:String,
    pub trial_ids:Vec<String>,
    pub launch:serde_json::Value,
    pub cancel_requested:bool,
    pub job:Job<JobOutput>,
    pub(crate) selection_epoch:u64,
    pub(crate) gate:Option<Arc<Mutex<PublicationGate>>>,
    /// Full snapshot survives even when a worker cannot produce an Evaluation.
    pub captured:Option<Study>,
}
impl PendingJob {
    pub fn progress(&self)->Progress { self.job.progress() }
    pub fn snapshot(&self)->serde_json::Value {
        let p=self.progress();
        json!({"id":self.id,"kind":format!("{:?}",self.kind),"stamp":self.stamp,"source":self.source,"trial_ids":self.trial_ids,"launch":self.launch,"cancel_requested":self.cancel_requested,"progress":{"fraction":p.fraction,"steps":p.steps,"message":p.message}})
    }
    pub fn cancel(&mut self)->String {
        if let Some(gate)=&self.gate {
            let mut g=gate.lock().unwrap_or_else(|p|p.into_inner());
            if g.started { return "Publication already began; the immutable destination outcome will be retained. Cancellation cannot revoke a published file.".into(); }
            g.cancelled=true;
        }
        self.cancel_requested=true;
        self.job.cancel();
        "Cancellation requested; the job remains retained until its terminal receipt".into()
    }
}
pub struct JobReceipt {
    pub id:u64,
    pub kind:JobKind,
    pub stamp:Option<StudyStamp>,
    pub source:String,
    pub trial_ids:Vec<String>,
    pub launch:serde_json::Value,
    pub document:Option<DocumentCapture>,
    pub cancelled:bool,
    pub displaced:bool,
    pub stale:bool,
    pub message:String,
    pub error:Option<String>,
    pub captured:Option<Study>,
    /// Kept separately when a result cannot be attached to its original identity.
    pub evaluation:Option<Evaluation>,
    /// Complete orphan outcome remains inspectable when original study is missing.
    pub refinement:Option<refinement::Outcome>,
}
impl JobReceipt {
    /// Bounded-to-inputs receipt representation for the saved Study envelope.
    /// Never recursively embeds older receipts or duplicates the archive samples.
    pub fn durable(&self)->serde_json::Value {
        json!({"id":self.id,"kind":format!("{:?}",self.kind),"stamp":self.stamp,"source":self.source,"trial_ids":self.trial_ids,"launch":self.launch,"cancelled":self.cancelled,"cancellation_requested":self.cancelled,"displaced":self.displaced,"stale":self.stale,"message":self.message,"error":self.error,"document":self.document.as_ref().map(|d|json!({"id":d.id,"revision":d.revision,"source":d.source})),"inputs":self.captured.as_ref().map(|s|json!({"baseline":s.baseline,"candidate":s.draft,"limits":s.limits,"validation_seen":s.validation_seen,"validation_influenced":s.validation_influenced,"observation_blake3":s.archive.observation_blake3,"model_blake3":s.archive.model_blake3,"assumptions":experiment_study::ASSUMPTIONS}))})
    }
    pub fn snapshot(&self)->serde_json::Value {
        json!({"id":self.id,"kind":format!("{:?}",self.kind),"stamp":self.stamp,"source":self.source,"trial_ids":self.trial_ids,"launch":self.launch,"cancelled":self.cancelled,"cancellation_requested":self.cancelled,"displaced":self.displaced,"stale":self.stale,"message":self.message,"error":self.error,"evaluation":self.evaluation,"refinement":self.refinement,"captured":self.captured,"document":self.document.as_ref().map(|d|json!({"id":d.id,"revision":d.revision,"source":d.source}))})
    }
}
pub(crate) fn path(value:&str)->Result<PathBuf,String> {
    if value.trim().is_empty() { return Err("study.path: a destination/source path is required".into()); }
    Ok(PathBuf::from(value))
}
pub(crate) fn next(owner:&mut StudyOwner)->u64 { owner.next_job+=1; owner.next_job }
pub fn start_load(owner:&mut StudyOwner,value:&str,review:bool,document:Option<DocumentCapture>)->Result<u64,String> {
    let source=path(value)?;
    let id=next(owner);
    let directory=source.clone();
    let repository=crate::workspace::root()?.to_path_buf();
    let job=Job::spawn(Pool::Io,id,"measured study open",move |ctx| {
        ctx.message("Reading retained offline evidence");
        let mut study=if review {
            Study::load(&directory).map_err(|e|format!("study.load {}: {e}",directory.display()))?
        } else {
            let archive=hx_archive::load(&directory,&repository).map_err(|e|format!("study.archive {}: {e}",directory.display()))?;
            Study::new(archive).map_err(|e|format!("study.archive {}: {e}",directory.display()))?
        };
        study.validate().map_err(|e|format!("study.load {}: {e}",directory.display()))?;
        refinement::recordings::cache_identities(&mut study);
        if ctx.cancelled() { return Err("Cancelled after read; source and retained studies are unchanged".into()); }
        Ok(JobOutput::Loaded(study))
    });
    owner.pending.push(PendingJob{id,kind:if review {JobKind::Review}else{JobKind::Archive},stamp:None,document,source:source.display().to_string(),trial_ids:vec![],launch:serde_json::Value::Null,cancel_requested:false,job,selection_epoch:owner.selection_epoch,gate:None,captured:None});
    Ok(id)
}
pub fn start_evaluation(owner:&mut StudyOwner,stamp:StudyStamp,set:EvaluationSelection)->Result<u64,String> {
    if owner.pending.iter().any(|p|p.kind==JobKind::Evaluate && p.stamp.is_some_and(|s|s.id==stamp.id)) { return Err("study.evaluate: this retained study already has an evaluation pending".into()); }
    let retained=owner.get_mut(stamp.id).ok_or("study.id: missing retained study")?;
    let ids=commands::trial_ids(&retained.study,set)?;
    retained.study.draft.validate()?;
    retained.study.baseline.validate()?;
    commands::validate_limits(&retained.study.limits)?;
    let seen=retained.study.validation_seen;
    commands::expose(&mut retained.study,&ids);
    if seen!=retained.study.validation_seen { retained.revision+=1; }
    let captured=retained.study.clone();
    let capture_stamp=retained.stamp();
    let document=retained.document.clone();
    let source=retained.source.clone();
    let id=next(owner);
    let worker=captured.clone();
    let selected=ids.clone();
    let captured_document=document.clone();
    let captured_source=source.clone();
    let launch=json!({"execution":experiment_study::execution_identity(),"study":capture_stamp,"document":document.as_ref().map(|d|json!({"id":d.id,"revision":d.revision,"source":d.source})),"source":source,"trial_ids":ids,"trial_splits":ids.iter().filter_map(|id|captured.archive.trials.iter().find(|t|&t.id==id).map(|t|json!({"id":t.id,"split":t.split}))).collect::<Vec<_>>(),"observation_blake3":captured.archive.observation_blake3,"model_blake3":captured.archive.model_blake3,"assumptions":experiment_study::ASSUMPTIONS,"validation_seen":captured.validation_seen,"validation_influenced":captured.validation_influenced});
    let worker_launch=launch.clone();
    let job=Job::spawn(Pool::Dedicated,id,"measured PWM evaluation",move |ctx| {
        ctx.message("Evaluating captured baseline and candidate with the shared runtime");
        let mut evaluation=experiment_study::evaluate(&worker.archive,&selected,&worker.baseline,&worker.draft,worker.limits.clone(),worker.validation_influenced,ctx.cancel_flag(),|done,total| {ctx.steps(done as u64,total as u64);ctx.fraction(done as f64/total as f64);})?;
        evaluation.capture.insert("native_study".into(),json!(capture_stamp));
        evaluation.capture.insert("native_document".into(),captured_document.as_ref().map(|d|json!({"id":d.id,"revision":d.revision,"source":d.source})).unwrap_or(serde_json::Value::Null));
        evaluation.capture.insert("native_source".into(),json!(captured_source));
        evaluation.capture.insert("validation_seen".into(),json!(worker.validation_seen));
        evaluation.capture.insert("native_launch".into(),worker_launch);
        Ok(JobOutput::Evaluated(evaluation))
    });
    owner.pending.push(PendingJob{id,kind:JobKind::Evaluate,stamp:Some(capture_stamp),document,source,trial_ids:ids,launch,cancel_requested:false,job,selection_epoch:owner.selection_epoch,gate:None,captured:Some(captured)});
    Ok(id)
}
pub fn start_refinement(owner:&mut StudyOwner,stamp:StudyStamp,operation:refinement::Operation)->Result<u64,String> {
    super::recording_jobs::start_operation(owner,stamp,operation,None)
}

/// Additive durable metadata preserves opaque older payloads instead of replacing them.
pub(crate) fn retain_durable(study:&mut Study,key:&str,value:serde_json::Value) {
    match study.retained_fields.get_mut(key) {
        Some(serde_json::Value::Array(rows))=>rows.push(value),
        Some(_)=>{
            let mut suffix=1u64;
            while study.retained_fields.contains_key(&format!("{key}_retained_{suffix}")) {suffix+=1;}
            study.retained_fields.insert(format!("{key}_retained_{suffix}"),json!([value]));
        }
        None=>{study.retained_fields.insert(key.into(),json!([value]));}
    }
}

/// Artifact-only projection: rejected text is evidence, never a shared draft mutation.
pub(crate) fn retain_form_inputs(captured:&mut Study,stamp:StudyStamp,inputs:Option<serde_json::Value>) {
    if let Some(inputs)=inputs {
        retain_durable(captured,"native_form_inputs",json!({"study":stamp,"status":"raw rejected or unsubmitted inputs; not applied to the shared draft","inputs":inputs}));
    }
}

/// The existing publication owner captures without disk work; immutable blobs stay
/// Arc-backed in the Study snapshot until the adopted file job publishes them.
pub(super) fn prepare_publication_capture(study:&Study,stamp:StudyStamp,destination:&std::path::Path,export:bool,source:&str,document:Option<&DocumentCapture>,inputs:Option<serde_json::Value>)->Study {
    let mut captured=study.clone();
    retain_durable(&mut captured,"native_publication_captures",json!({"study":stamp,"destination":destination.display().to_string(),"kind":if export {"export_new"} else {"save_new"},"source":source,"document":document.map(|d|json!({"id":d.id,"revision":d.revision,"source":d.source})),"execution":experiment_study::execution_identity(),"status":"immutable captured revision prepared for publication"}));
    retain_form_inputs(&mut captured,stamp,inputs);
    captured
}

pub fn start_publication(owner:&mut StudyOwner,stamp:StudyStamp,value:&str,export:bool)->Result<u64,String> {
    start_publication_with_inputs(owner,stamp,value,export,None)
}
pub fn start_publication_with_inputs(owner:&mut StudyOwner,stamp:StudyStamp,value:&str,export:bool,inputs:Option<serde_json::Value>)->Result<u64,String> {
    let destination=path(value)?;
    if owner.pending.iter().any(|p|matches!(p.kind,JobKind::Save|JobKind::Export) && p.source==destination.display().to_string()) { return Err("study.path: publication to this destination is already pending".into()); }
    let retained=owner.validate_stamp(stamp)?;
    let document=retained.document.clone();
    let captured=prepare_publication_capture(&retained.study,stamp,&destination,export,&retained.source,document.as_ref(),inputs);
    // Full saved-schema validation/serialization belongs to the file job.
    let gate=Arc::new(Mutex::new(PublicationGate::default()));
    let worker_gate=gate.clone();
    let worker=captured.clone();
    let output=destination.clone();
    let id=next(owner);
    let job=Job::spawn(Pool::Io,id,"measured study immutable publication",move |ctx| {
        ctx.message("Validating immutable captured study revision");
        worker.validate().map_err(|e|format!("study.publish {}: {e}",output.display()))?;
        {
            let mut g=worker_gate.lock().unwrap_or_else(|p|p.into_inner());
            authorize_publication(&mut g, ctx.cancelled())?;
        }
        // After this linearization point cancellation cannot promise rollback.
        ctx.message("Publishing new immutable evidence; cancellation can no longer revoke publication");
        publish_artifact(&worker, &output, export, &sim_runtime::publication::NoHooks)
    }).complete_on_drop();
    owner.pending.push(PendingJob{id,kind:if export {JobKind::Export}else{JobKind::Save},stamp:Some(stamp),document,source:destination.display().to_string(),trial_ids:vec![],launch:serde_json::Value::Null,cancel_requested:false,job,selection_epoch:owner.selection_epoch,gate:Some(gate),captured:Some(captured)});
    Ok(id)
}

/// Existing cancellation authority, kept above all irreversible filesystem work.
pub(super) fn authorize_publication(gate:&mut PublicationGate,cancelled:bool)->Result<(),String> {
    if gate.cancelled || cancelled { return Err("Cancelled before immutable publication".into()); }
    gate.started=true;
    Ok(())
}
/// The same artifact/acknowledgment adapter used by the adopted job and source fixtures.
pub(super) fn publish_artifact(study:&Study,path:&std::path::Path,export:bool,hooks:&dyn sim_runtime::publication::Hooks)->Result<JobOutput,String> {
    if export { study.export_html_new_with(path,hooks) } else { study.save_new_with(path,hooks) }
        .map_err(|e|format!("study.publish {}: {e}",path.display()))?;
    Ok(JobOutput::Published)
}

pub fn poll(mut owner:ResMut<StudyOwner>,registry:Res<DocumentRegistry>) {
    let before=owner.changed;
    // Polling an empty queue or unchanged handles is observation, not an edit.
    // publish() and first document displacement explicitly advance this counter.
    poll_owner(owner.bypass_change_detection(),&registry);
    if owner.changed!=before {owner.set_changed();}
}
pub fn poll_owner(owner:&mut StudyOwner,registry:&DocumentRegistry) {
    let mut index=0;
    while index<owner.pending.len() {
        let Some(result)=owner.pending[index].job.poll() else {index+=1;continue};
        let pending=owner.pending.remove(index);
        publish(owner,registry,pending,result);
    }
    for s in &mut owner.studies {
        if s.document.as_ref().is_some_and(|d|!d.matches(registry)) && s.displaced.is_none() {
            s.displaced=Some("Captured Build document identity/revision changed; retained evidence belongs to the original document".into());
            owner.changed+=1;
        }
    }
}
fn publish(owner:&mut StudyOwner,registry:&DocumentRegistry,pending:PendingJob,result:Result<JobOutput,String>) {
    let displaced=pending.document.as_ref().is_some_and(|d|!d.matches(registry))
        || pending.stamp.is_some_and(|s|owner.get(s.id).is_none());
    let stale=pending.stamp.is_some_and(|s|owner.get(s.id).is_none_or(|r|r.revision!=s.revision));
    let mut receipt=JobReceipt{id:pending.id,kind:pending.kind,stamp:pending.stamp,source:pending.source.clone(),trial_ids:pending.trial_ids.clone(),launch:pending.launch.clone(),document:pending.document.clone(),cancelled:pending.cancel_requested,displaced,stale,message:String::new(),error:None,captured:pending.captured.clone(),evaluation:None,refinement:None};
    match result {
        Err(e)=>{
            receipt.message="Job failed or cancelled; inputs and existing evidence remain retained and unscored".into();
            if matches!(pending.kind,JobKind::Refinement|JobKind::RecordingImport|JobKind::RecordingAuthoring) {
                receipt.launch=json!({"inputs":pending.launch,"terminal":{"execution_cancelled":serde_json::Value::Null,"execution_status":"no runtime outcome returned; cancellation observation unknown","cancellation_requested":pending.cancel_requested,"stale":stale,"displaced":displaced}});
            }
            receipt.error=Some(e);
        }
        Ok(JobOutput::RecordingImport{prepared,input,error})=>{
            super::recording_jobs::attach_import(owner,&pending,&mut receipt,prepared,input,error);
        }
        Ok(JobOutput::Loaded(mut study))=>{
            if pending.cancel_requested {
                receipt.captured=Some(study.clone());
                let id=owner.retain(study,pending.source,pending.document,pending.kind==JobKind::Review,false);
                receipt.message=format!("Read completed after cancellation; study {id} is retained without changing selection");
            } else {
                let exposure=super::state::ReviewExposure::capture(&study);
                let expose=commands::validate_view(&study);
                if let Err(e)=expose {receipt.error=Some(e);receipt.captured=Some(study);receipt.message="Loaded study refused exposure validation and remains captured".into();}
                else {
                    let ids=commands::filtered_ids(&study);
                    commands::expose(&mut study,&ids);
                    refinement::expose_review(&mut study);
                    let changed=exposure!=super::state::ReviewExposure::capture(&study);
                    let activate=owner.selection_epoch==pending.selection_epoch && !displaced;
                    let id=owner.retain(study,pending.source,pending.document,pending.kind==JobKind::Review && !changed,activate);
                    receipt.displaced|=!activate;
                    receipt.message=format!("Opened retained study {id}{}",if activate {""} else {"; another selection/document is active, choose this study explicitly"});
                }
            }
        }
        Ok(JobOutput::Evaluated(mut evaluation))=>{
            // Cancellation can arrive after worker completion. Preserve the runtime's
            // actual cancelled flag, and durably retain the later request separately.
            evaluation.capture.insert("native_terminal".into(),json!({
                "job_id":pending.id,
                "study":pending.stamp,
                "document":pending.document.as_ref().map(|d|json!({"id":d.id,"revision":d.revision,"source":d.source})),
                "source":pending.source,
                "launch":pending.launch,
                "cancellation_requested":pending.cancel_requested,
                "execution_cancelled":evaluation.cancelled,
                "stale":stale,
                "displaced":displaced,
            }));
            let original=pending.stamp.and_then(|stamp|owner.get_mut(stamp.id));
            if let Some(study)=original {
                // Never attach by active index or matching revision alone.
                study.study.evaluations.push(evaluation.clone());
                study.revision+=1;
                if displaced {study.displaced=Some("Evaluation completed for a displaced document; captured inputs are retained".into());}
                receipt.message=if stale||displaced {"Evaluation retained on its original study; stale/displaced captured evidence"} else {"Evaluation retained with immutable baseline/candidate inputs"}.into();
            } else {receipt.message="Original study identity is missing; evaluation retained only in the displaced receipt".into();}
            receipt.evaluation=Some(evaluation);
        }
        Ok(JobOutput::Refined{outcome,inputs})=>{
            let execution_cancelled=outcome.cancelled;
            receipt.error=match &outcome.result {
                Err(error)=>Some(error.clone()),
                Ok(refinement::ResultData::Controller(run))=>run.failure.clone(),
                Ok(refinement::ResultData::FitAttempt(attempt))=>attempt.failure.clone(),
                Ok(refinement::ResultData::RecordingFit(fit))=>fit.attempt.failure.clone(),
                Ok(refinement::ResultData::CombinedFit(fit))=>fit.attempt.failure.clone(),
                Ok(refinement::ResultData::Robustness(result))=>{
                    let failures=result.failures.iter().cloned().chain(result.runs.iter().filter_map(|(label,_,run)|run.failure.as_ref().map(|e|format!("{label}: {e}")))).collect::<Vec<_>>();
                    (!failures.is_empty()).then(||failures.join("; "))
                },
                _=>None,
            };
            receipt.launch=json!({"inputs":pending.launch,"terminal":{"execution_cancelled":execution_cancelled,"cancellation_requested":pending.cancel_requested,"stale":stale,"displaced":displaced}});
            // Orphan outcomes retain their Study-owned content store in memory;
            // durable receipt metadata contains bounded content references only.
            if let Some(input)=inputs.get("additional_input") {receipt.launch["additional_input"]=input.clone();}
            receipt.launch["recording_applied"]=json!(false);
            if let Some(study)=pending.stamp.and_then(|stamp|owner.get_mut(stamp.id)) {
                receipt.launch["recording_applied"]=json!(!stale && !displaced && !execution_cancelled && receipt.error.is_none());
                // Prepared prediction choice is already shared-validated in the job.
                // A later edit/selection must never be overwritten by terminal delivery.
                if !stale && !displaced && matches!(&outcome.result,Ok(refinement::ResultData::Prediction(_))) {
                    study.study.refinement_evidence.selected_recording=outcome.capture.study.refinement_evidence.selected_recording.clone();
                    study.study.refinement_evidence.prediction_purpose=outcome.capture.study.refinement_evidence.prediction_purpose;
                }
                refinement::apply_outcome_with_inputs(&mut study.study,outcome.clone(),inputs);
                study.revision+=1;
                if displaced {study.displaced=Some("Refinement completed for a displaced document; captured settings remain linked to the original study".into());}
                receipt.message=if execution_cancelled || receipt.error.is_some() {"Refinement failed or cancelled; captured attempt retained and unscored"} else if stale||displaced {"Refinement retained on its original study as stale/displaced evidence"} else {"Refinement retained for review; candidate adoption requires an explicit action"}.into();
            } else {receipt.message="Original study identity is missing; refinement outcome retained in the displaced receipt".into();}
            receipt.refinement=Some(outcome);
        }
        Ok(JobOutput::Published)=>{
            if pending.kind==JobKind::Save {
                if let Some(stamp)=pending.stamp {
                    if let Some(s)=owner.get_mut(stamp.id).filter(|s|s.revision==stamp.revision) {s.saved_revision=Some(stamp.revision);}
                }
            }
            receipt.message=if stale {"Published the captured revision; later edits remain unsaved"} else if pending.kind==JobKind::Export {"Published new HTML evidence; export does not mark the draft saved"} else {"Published a new immutable review of the captured revision"}.into();
        }
    }
    if receipt.error.is_some() || matches!(receipt.kind,JobKind::Refinement|JobKind::RecordingImport|JobKind::RecordingAuthoring) {
        // Failed evaluation/publication evidence is part of the saved round trip,
        // while successful save acknowledgements must not create a new dirty edit.
        if let Some(stamp)=receipt.stamp {
            if let Some(study)=owner.get_mut(stamp.id) {
                let value=receipt.durable();
                retain_durable(&mut study.study,"native_offline_job_receipts",value);
                study.revision+=1;
            }
        }
    }
    owner.status=receipt.error.clone().unwrap_or_else(||receipt.message.clone());
    owner.receipts.push(receipt);
    owner.changed+=1;
}

#[cfg(test)]
mod publication_input_fixtures {
    //! T48 source-written fixture; NOT executed. No publication or job starts.
    use super::*;
    #[test]
    fn raw_rejected_inputs_are_artifact_evidence_without_applying_or_losing_opaque_fields() {
        let root=crate::workspace::root().unwrap();
        let archive=hx_archive::load(&root.join(super::super::super::DEFAULT_ARCHIVE),root).unwrap();
        let live=Study::new(archive).unwrap();
        let mut captured=live.clone();
        captured.retained_fields.insert("native_form_inputs".into(),json!({"opaque":"future"}));
        let stamp=StudyStamp{id:7,revision:11};
        retain_form_inputs(&mut captured,stamp,Some(json!({"field":"period_s","text":"malformed duration","error":"refinement.experiment.timing.period_s: expected number"})));
        assert_eq!(serde_json::to_value(&live.refinement.experiment).unwrap(),serde_json::to_value(&captured.refinement.experiment).unwrap());
        assert!(!live.retained_fields.contains_key("native_form_inputs"));
        let reopened:Study=serde_json::from_value(serde_json::to_value(captured).unwrap()).unwrap();
        assert_eq!(reopened.retained_fields["native_form_inputs"]["opaque"],"future");
        let row=&reopened.retained_fields["native_form_inputs_retained_1"][0];
        assert_eq!(row["study"]["revision"],11);
        assert_eq!(row["inputs"]["text"],"malformed duration");
        assert!(row["status"].as_str().unwrap().contains("not applied"));
    }
}

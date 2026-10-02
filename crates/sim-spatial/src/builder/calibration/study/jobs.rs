//! Captured jobs and immutable publication receipts. Never latest-wins discard.
use super::state::{DocumentCapture, StudyOwner, StudyStamp};
use crate::{document::DocumentRegistry, jobs::{Job, Pool, Progress}};
use bevy::prelude::*;
use sim_runtime::{experiment_comparison::hx_archive, experiment_study::{self, Study, Evaluation, commands::{self, EvaluationSelection}}};
use std::{path::PathBuf,sync::{Arc,Mutex}};
use serde_json::json;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind { Archive, Review, Evaluate, Save, Export }
impl JobKind { pub fn is_load(self)->bool { matches!(self,Self::Archive|Self::Review) } }

/// The tiny gate linearizes cancel acceptance and the start of irreversible
/// create-new publication. No disk work occurs while this mutex is held.
#[derive(Default)]
pub struct PublicationGate { pub started:bool, pub cancelled:bool }
pub enum JobOutput { Loaded(Study), Evaluated(Evaluation), Published }
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
}
impl JobReceipt {
    /// Bounded-to-inputs receipt representation for the saved Study envelope.
    /// Never recursively embeds older receipts or duplicates the archive samples.
    pub fn durable(&self)->serde_json::Value {
        json!({"id":self.id,"kind":format!("{:?}",self.kind),"stamp":self.stamp,"source":self.source,"trial_ids":self.trial_ids,"launch":self.launch,"cancelled":self.cancelled,"displaced":self.displaced,"stale":self.stale,"message":self.message,"error":self.error,"document":self.document.as_ref().map(|d|json!({"id":d.id,"revision":d.revision,"source":d.source})),"inputs":self.captured.as_ref().map(|s|json!({"baseline":s.baseline,"candidate":s.draft,"limits":s.limits,"validation_seen":s.validation_seen,"validation_influenced":s.validation_influenced,"observation_blake3":s.archive.observation_blake3,"model_blake3":s.archive.model_blake3,"assumptions":experiment_study::ASSUMPTIONS}))})
    }
    pub fn snapshot(&self)->serde_json::Value {
        json!({"id":self.id,"kind":format!("{:?}",self.kind),"stamp":self.stamp,"source":self.source,"trial_ids":self.trial_ids,"launch":self.launch,"cancelled":self.cancelled,"displaced":self.displaced,"stale":self.stale,"message":self.message,"error":self.error,"evaluation":self.evaluation,"captured":self.captured,"document":self.document.as_ref().map(|d|json!({"id":d.id,"revision":d.revision,"source":d.source}))})
    }
}
fn path(value:&str)->Result<PathBuf,String> {
    if value.trim().is_empty() { return Err("study.path: a destination/source path is required".into()); }
    Ok(PathBuf::from(value))
}
fn next(owner:&mut StudyOwner)->u64 { owner.next_job+=1; owner.next_job }
pub fn start_load(owner:&mut StudyOwner,value:&str,review:bool,document:Option<DocumentCapture>)->Result<u64,String> {
    let source=path(value)?;
    let id=next(owner);
    let directory=source.clone();
    let repository=crate::workspace::root()?.to_path_buf();
    let job=Job::spawn(Pool::Io,id,"measured study open",move |ctx| {
        ctx.message("Reading retained offline evidence");
        let study=if review {
            Study::load(&directory).map_err(|e|format!("study.load {}: {e}",directory.display()))?
        } else {
            let archive=hx_archive::load(&directory,&repository).map_err(|e|format!("study.archive {}: {e}",directory.display()))?;
            Study::new(archive).map_err(|e|format!("study.archive {}: {e}",directory.display()))?
        };
        study.validate().map_err(|e|format!("study.load {}: {e}",directory.display()))?;
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
pub fn start_publication(owner:&mut StudyOwner,stamp:StudyStamp,value:&str,export:bool)->Result<u64,String> {
    let destination=path(value)?;
    if owner.pending.iter().any(|p|matches!(p.kind,JobKind::Save|JobKind::Export) && p.source==destination.display().to_string()) { return Err("study.path: publication to this destination is already pending".into()); }
    let retained=owner.validate_stamp(stamp)?;
    let captured=retained.study.clone();
    let document=retained.document.clone();
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
            if g.cancelled || ctx.cancelled() { return Err("Cancelled before immutable publication".into()); }
            g.started=true;
        }
        // After this linearization point cancellation cannot promise rollback.
        ctx.message("Publishing new immutable evidence; cancellation can no longer revoke publication");
        if export { worker.export_html_new(&output) } else { worker.save_new(&output) }
            .map_err(|e|format!("study.publish {}: {e}",output.display()))?;
        Ok(JobOutput::Published)
    }).complete_on_drop();
    owner.pending.push(PendingJob{id,kind:if export {JobKind::Export}else{JobKind::Save},stamp:Some(stamp),document,source:destination.display().to_string(),trial_ids:vec![],launch:serde_json::Value::Null,cancel_requested:false,job,selection_epoch:owner.selection_epoch,gate:Some(gate),captured:Some(captured)});
    Ok(id)
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
    let mut receipt=JobReceipt{id:pending.id,kind:pending.kind,stamp:pending.stamp,source:pending.source.clone(),trial_ids:pending.trial_ids.clone(),launch:pending.launch.clone(),document:pending.document.clone(),cancelled:pending.cancel_requested,displaced,stale,message:String::new(),error:None,captured:pending.captured,evaluation:None};
    match result {
        Err(e)=>{receipt.message="Job failed or cancelled; inputs and existing evidence remain retained and unscored".into();receipt.error=Some(e);}
        Ok(JobOutput::Loaded(mut study))=>{
            if pending.cancel_requested {
                receipt.captured=Some(study.clone());
                let id=owner.retain(study,pending.source,pending.document,pending.kind==JobKind::Review,false);
                receipt.message=format!("Read completed after cancellation; study {id} is retained without changing selection");
            } else {
                let seen=study.validation_seen;
                let expose=commands::validate_view(&study);
                if let Err(e)=expose {receipt.error=Some(e);receipt.captured=Some(study);receipt.message="Loaded study refused exposure validation and remains captured".into();}
                else {
                    let ids=commands::filtered_ids(&study);
                    commands::expose(&mut study,&ids);
                    let changed=seen!=study.validation_seen;
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
        Ok(JobOutput::Published)=>{
            if pending.kind==JobKind::Save {
                if let Some(stamp)=pending.stamp {
                    if let Some(s)=owner.get_mut(stamp.id).filter(|s|s.revision==stamp.revision) {s.saved_revision=Some(stamp.revision);}
                }
            }
            receipt.message=if stale {"Published the captured revision; later edits remain unsaved"} else if pending.kind==JobKind::Export {"Published new HTML evidence; export does not mark the draft saved"} else {"Published a new immutable review of the captured revision"}.into();
        }
    }
    if receipt.error.is_some() {
        // Failed evaluation/publication evidence is part of the saved round trip,
        // while successful save acknowledgements must not create a new dirty edit.
        if let Some(stamp)=receipt.stamp {
            if let Some(study)=owner.get_mut(stamp.id) {
                let key="native_offline_job_receipts";
                let value=receipt.durable();
                // Preserve an unsupported older payload rather than overwriting it.
                match study.study.retained_fields.get_mut(key) {
                    Some(serde_json::Value::Array(rows))=>rows.push(value),
                    Some(_)=>{
                        let mut suffix=1u64;
                        let mut retained_key=format!("{key}_retained_{suffix}");
                        while study.study.retained_fields.contains_key(&retained_key) {
                            suffix+=1;retained_key=format!("{key}_retained_{suffix}");
                        }
                        study.study.retained_fields.insert(retained_key,json!([value]));
                    }
                    None=>{study.study.retained_fields.insert(key.into(),json!([value]));}
                }
                study.revision+=1;
            }
        }
    }
    owner.status=receipt.error.clone().unwrap_or_else(||receipt.message.clone());
    owner.receipts.push(receipt);
    owner.changed+=1;
}

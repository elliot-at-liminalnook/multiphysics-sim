//! T50 jobs integration; extends the existing StudyOwner, not another persistence owner.
//!
//! Ownership: StudyOwner's global pending queue is durable through dock/mode changes;
//! Actions alone captures submissions and JobResults alone attaches terminal evidence.
//! Actions → JobResults ordering is the existing public ViewerSet pipeline. Messages
//! are occurrences only. No entities are spawned here; existing kit presentation owns
//! its transient roots. File reads, parsing, validation/preparation and runtime execution
//! belong to adopted jobs. Frame attachment never validates samples or runs physics.
use super::{jobs::{self, JobKind, JobOutput, PendingJob, JobReceipt}, state::{StudyOwner, StudyStamp}};
use crate::jobs::{Job, Pool};
use serde_json::{json, Value};
use sim_runtime::experiment_study::{Study, refinement::{self, recordings::{self, ImportClassification}}};

fn idle(owner:&StudyOwner,stamp:StudyStamp)->Result<(),String> {
    if owner.pending.iter().any(|p|matches!(p.kind,JobKind::Refinement|JobKind::RecordingImport|JobKind::RecordingAuthoring) && p.stamp.is_some_and(|s|s.id==stamp.id)) {
        return Err("study.recording: this retained study already has import/refinement work pending".into());
    }
    owner.validate_stamp(stamp)?;
    Ok(())
}
pub fn start_import(owner:&mut StudyOwner,stamp:StudyStamp,value:&str)->Result<u64,String> {
    idle(owner,stamp)?;
    let path=jobs::path(value)?;
    let retained=owner.validate_stamp(stamp)?;
    let captured=retained.study.clone();
    let document=retained.document.clone();
    let mut worker=captured.clone();
    let source=path.display().to_string();
    let worker_source=source.clone();
    let id=jobs::next(owner);
    let job=Job::spawn(Pool::Io,id,"controller recording import",move |ctx| {
        ctx.message("Reading and classifying controller versus deferred FPGA capture");
        let bytes=std::fs::read(&path).map_err(|e|format!("study.recording.path {}: {e}",path.display()))?;
        // Raw input is retained even when classification or shared validation refuses it.
        let mut input=json!({"path":worker_source,"byte_length":bytes.len(),"raw":String::from_utf8_lossy(&bytes)});
        if std::str::from_utf8(&bytes).is_err() {input["invalid_utf8_bytes"]=json!(bytes);}
        let execution_cancelled=ctx.cancelled();
        input["execution_cancelled"]=json!(execution_cancelled);
        let result=(|| {
            if execution_cancelled {return Err("study.recording: cancelled after read; captured input retained unapplied".into());}
            match recordings::classify(&bytes)? {
                ImportClassification::Controller(recording)=>{
                    input["classification"]=json!("controller");
                    refinement::apply(&mut worker,refinement::Command::ImportRecording{recording})?;
                    Ok(Some(Box::new(worker)))
                }
                ImportClassification::FpgaDeferred=>{
                    input["classification"]=json!("fpga_deferred");
                    Ok(None)
                }
            }
        })();
        Ok(match result {
            Ok(prepared)=>JobOutput::RecordingImport{prepared,input,error:None},
            Err(error)=>JobOutput::RecordingImport{prepared:None,input,error:Some(error)},
        })
    });
    owner.pending.push(PendingJob{id,kind:JobKind::RecordingImport,stamp:Some(stamp),document,source:source.clone(),trial_ids:vec![],launch:json!({"study":stamp,"source":source,"execution":sim_runtime::experiment_study::execution_identity()}),cancel_requested:false,job,selection_epoch:owner.selection_epoch,gate:None,captured:Some(captured)});
    Ok(id)
}
/// Structured authoring and REST use the same job-owned shared validation.
pub fn start_command(owner:&mut StudyOwner,stamp:StudyStamp,command:refinement::Command)->Result<u64,String> {
    idle(owner,stamp)?;
    let retained=owner.validate_stamp(stamp)?;
    let captured=retained.study.clone();
    let document=retained.document.clone();
    let source=retained.source.clone();
    let mut worker=captured.clone();
    // Occurrence input is retained even when cancellation prevents worker startup.
    let launch=json!({"study":stamp,"input_kind":"typed_recording_command","command":command,"execution":sim_runtime::experiment_study::execution_identity()});
    let id=jobs::next(owner);
    let job=Job::spawn(Pool::Dedicated,id,"recording authoring validation",move |ctx| {
        ctx.message("Validating captured recording authoring command");
        let execution_cancelled=ctx.cancelled();
        let input=json!({"classification":"recording_command","command":command,"execution_cancelled":execution_cancelled,"cancellation_scope":"shared validation is not interruptible; cancellation observed before validation only"});
        let result=if execution_cancelled {Err("study.recording: authoring cancelled before validation".into())}
            else {refinement::apply(&mut worker,command)};
        Ok(match result {
            Ok(())=>JobOutput::RecordingImport{prepared:Some(Box::new(worker)),input,error:None},
            Err(error)=>JobOutput::RecordingImport{prepared:None,input,error:Some(error)},
        })
    });
    owner.pending.push(PendingJob{id,kind:JobKind::RecordingAuthoring,stamp:Some(stamp),document,source,trial_ids:vec![],launch,cancel_requested:false,job,selection_epoch:owner.selection_epoch,gate:None,captured:Some(captured)});
    Ok(id)
}
pub fn start_combined(owner:&mut StudyOwner,stamp:StudyStamp,additional_path:Option<String>)->Result<u64,String> {
    if let Some(value)=&additional_path {jobs::path(value).map_err(|e|format!("study.combined.additional_path: {e}"))?;}
    start_operation(owner,stamp,refinement::Operation::FitCombined{additional:None},additional_path)
}
/// Preparation itself belongs in the job. Action-time capture is an immutable clone.
/// Conservative launch exposure prevents edits during a pending held-out review from
/// escaping later influence accounting. No scores or passing assessment are implied.
pub fn start_operation(owner:&mut StudyOwner,stamp:StudyStamp,operation:refinement::Operation,additional_path:Option<String>)->Result<u64,String> {
    idle(owner,stamp)?;
    let retained=owner.get_mut(stamp.id).ok_or("study.id: missing retained study")?;
    let may_expose=matches!(&operation,refinement::Operation::PredictRecording{..}|refinement::Operation::FitRecordings|refinement::Operation::FitCombined{..}|refinement::Operation::Fit{..}|refinement::Operation::Sensitivity{..});
    if may_expose && !retained.study.validation_seen {
        retained.study.validation_seen=true;
        retained.revision+=1;
    }
    let stamp=retained.stamp();
    let captured=retained.study.clone();
    let document=retained.document.clone();
    let source=retained.source.clone();
    let mut worker=captured.clone();
    let label=operation.label().to_string();
    let trial_ids=match &operation {
        refinement::Operation::Fit{train,validation}=>train.iter().chain(validation).cloned().collect(),
        refinement::Operation::Sensitivity{selected}=>selected.clone(),
        _=>vec![],
    };
    // Do not serialize FitCombined's additional Study into launch metadata: it could
    // recursively carry old receipts. Shared Capture::inputs supplies its projection.
    let launch=json!({"study":stamp,"operation":label,"source":source,"additional_path":additional_path,"model_blake3":captured.draft.fingerprint(),"archive_observation_blake3":captured.archive.observation_blake3,"archive_model_blake3":captured.archive.model_blake3,"execution":sim_runtime::experiment_study::execution_identity(),"validation_seen":captured.validation_seen,"validation_influenced":captured.validation_influenced});
    let id=jobs::next(owner);
    let job=Job::spawn(Pool::Dedicated,id,label.clone(),move |ctx| {
        let mut operation=operation;
        if let Some(path)=additional_path {
            ctx.message("Loading additional saved study without changing the current archive");
            let mut additional=Study::load(std::path::Path::new(&path)).map_err(|e|format!("study.combined.additional_path {path}: {e}"))?;
            additional.validate().map_err(|e|format!("study.combined.additional_study: {e}"))?;
            recordings::cache_identities(&mut additional);
            operation=refinement::Operation::FitCombined{additional:Some(Box::new(additional))};
        }
        // Shared quarantine remembers held-out identities even when an additional
        // source conflicts with a frozen tuning assignment and fitting is refused.
        let reservation_error=if let refinement::Operation::FitCombined{additional:Some(extra)}=&operation {
            recordings::reserve_additional(&mut worker,extra).err()
        } else {None};
        ctx.message(format!("Preparing {label} from immutable captured inputs"));
        // Even a refused additional dataset retains bounded captured provenance via
        // the shared outcome owner, rather than losing it as a bare worker error.
        let rejected_operation=operation.clone();
        let preparation=match reservation_error {Some(error)=>Err(error),None=>refinement::prepare(&mut worker,operation)};
        let capture=match preparation {
            Ok(capture)=>capture,
            Err(error)=>{
                let capture=refinement::Capture{study:worker,operation:rejected_operation,runtime:sim_runtime::experiment_study::execution_identity(),captured_unix_ns:std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|format!("study.capture.time: {e}"))?.as_nanos().to_string()};
                let inputs=capture.inputs();
                return Ok(JobOutput::Refined{outcome:refinement::Outcome{capture,result:Err(error),cancelled:ctx.cancelled()},inputs});
            }
        };
        ctx.message(format!("Running {label} with shared runtime and captured timing assumptions"));
        let outcome=refinement::execute(capture,ctx.cancel_flag(),|done,total| {
            ctx.steps(done as u64,total as u64);
            if total>0 {ctx.fraction(done as f64/total as f64);}
        })?;
        let inputs=outcome.capture.inputs();
        Ok(JobOutput::Refined{outcome,inputs})
    });
    owner.pending.push(PendingJob{id,kind:JobKind::Refinement,stamp:Some(stamp),document,source,trial_ids,launch,cancel_requested:false,job,selection_epoch:owner.selection_epoch,gate:None,captured:Some(captured)});
    Ok(id)
}
/// Attachment is purely stamped application; parsing/validation has already run.
/// A stale import may be inspected but cannot replace newer edits or another archive.
pub(super) fn attach_import(owner:&mut StudyOwner,pending:&PendingJob,receipt:&mut JobReceipt,prepared:Option<Box<Study>>,input:Value,error:Option<String>) {
    let mut applicable=!receipt.stale && !receipt.displaced && !pending.cancel_requested && error.is_none();
    let deferred=input["classification"].as_str()==Some("fpga_deferred");
    let authoring=input["classification"].as_str()==Some("recording_command");
    receipt.error=error;
    let mut prepared=prepared;
    let execution_cancelled=input.get("execution_cancelled").cloned().unwrap_or(Value::Null);
    receipt.launch=json!({"inputs":receipt.launch,"terminal":{"execution_cancelled":execution_cancelled,"cancellation_requested":pending.cancel_requested,"stale":receipt.stale,"displaced":receipt.displaced}});
    let mut applied=false;
    if let Some(original)=pending.stamp.and_then(|s|owner.get_mut(s.id)) {
        if let Some(next)=prepared.as_mut() {
            // Review can monotonically expose an already-seen source without a
            // revision increment. Preserve every current reservation/exposure.
            next.validation_seen|=original.study.validation_seen;
            next.validation_influenced|=original.study.validation_influenced;
            for (hash,influenced) in &original.study.refinement_evidence.recording_exposure {
                *next.refinement_evidence.recording_exposure.entry(hash.clone()).or_default()|=*influenced || original.study.validation_influenced;
            }
            if original.study.validation_influenced {for influence in next.refinement_evidence.recording_exposure.values_mut(){*influence=true;}}
            next.refinement_evidence.recording_held_out.extend(original.study.refinement_evidence.recording_held_out.iter().cloned());
            for assignment in &next.refinement.recording_assignments {
                if assignment.role==sim_runtime::controller_refinement::calibration_data::Role::Train && next.refinement_evidence.recording_held_out.contains(&assignment.recording_hash)
                    && !original.study.refinement.recording_assignments.contains(assignment) {
                    applicable=false;
                    receipt.error=Some(format!("recording.attachment.held_out.{}: quarantined source cannot become tuning data",assignment.recording_hash));
                }
            }
            for (hash,reservation) in &original.study.refinement_evidence.recording_reservations {
                if next.refinement_evidence.recording_reservations.get(hash).is_some_and(|value|value!=reservation) || next.refinement.recording_assignments.iter().any(|a|a.recording_hash==*hash && a!=reservation) {
                    applicable=false;
                    receipt.error=Some(format!("recording.attachment.reservations.{hash}: conflicts with current frozen reservation"));
                } else {next.refinement_evidence.recording_reservations.insert(hash.clone(),reservation.clone());}
            }
            for (key,value) in &original.study.retained_fields {next.retained_fields.insert(key.clone(),value.clone());}
        }
        applied=prepared.is_some() && applicable;
        if applicable {
            if let Some(prepared)=prepared.take() {original.study=*prepared;original.revision+=1;}
        }
        jobs::retain_durable(&mut original.study,"native_recording_imports",json!({"job_id":pending.id,"stamp":pending.stamp,"source":pending.source,"input":input,"applied":applied,"cancellation_requested":pending.cancel_requested,"stale":receipt.stale,"displaced":receipt.displaced,"error":receipt.error}));
        original.revision+=1;
    } else {
        // Missing original identity still retains the input in the global receipt.
        receipt.launch["orphan_import"]=input;
    }
    receipt.launch["terminal"]["applied"]=json!(applied);
    receipt.launch["recording_applied"]=json!(applied);
    receipt.message=if deferred {"FPGA recording import is deferred to the retained legacy FPGA workflow; no controller capture was inserted"} else if applied && authoring {"Captured recording authoring command applied through shared validation"} else if applied {"Controller recording imported with shared identity validation; incomplete captures remain inspectable and unscored"} else {"Recording input retained unapplied: rejected, cancelled, stale or displaced captured submission"}.into();
}

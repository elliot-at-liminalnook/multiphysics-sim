//! Written lifecycle fixtures; no fixture was executed in T46.
use super::{actions::{apply_action, StudyAction},jobs::{self,JobKind,JobOutput,PendingJob,PublicationGate},state::{DocumentCapture,StudyOwner}};
use crate::{document::{DocumentRegistry,DocumentKind,Source},app::ViewerMode,jobs::Job};
use sim_runtime::experiment_study::{Study,commands::{Command,EvaluationSelection}};
use std::sync::{Arc,Mutex,atomic::AtomicBool};

#[test]
fn actual_system_ui_and_rest_shapes_parse_to_the_same_study_action_owner() {
    use crate::app::actions::Action;
    let command=sim_api::Command{command:"system_ui".into(),args:serde_json::json!({"action":{"operation":"activate","id":"study:evaluate:heldout","ui_revision":17}})};
    assert!(matches!(StudyAction::parse(&command).unwrap(),StudyAction::SystemUi{id,..} if id=="study:evaluate:heldout"));
    let command=sim_api::Command{command:"system_measured_study".into(),args:serde_json::json!({"op":"apply","stamp":{"id":1,"revision":3},"command":{"SetNotes":"review notes"}})};
    assert!(matches!(StudyAction::parse(&command).unwrap(),StudyAction::Apply{stamp,command:Command::SetNotes(notes)} if stamp.revision==3 && notes=="review notes"));
}

fn fixture()->Study {
    let root=crate::workspace::root().unwrap();
    let archive=sim_runtime::experiment_comparison::hx_archive::load(&root.join(super::super::DEFAULT_ARCHIVE),root).unwrap();
    Study::new(archive).unwrap()
}
fn owner()->(StudyOwner,DocumentRegistry) {
    let mut registry=DocumentRegistry::default();
    registry.open(ViewerMode::Build,DocumentKind::System,Source::path("fixture.system.json"));
    let mut owner=StudyOwner::default();
    owner.retain(fixture(),"fixture archive".into(),DocumentCapture::current(&registry),false,true);
    (owner,registry)
}
fn finished(owner:&mut StudyOwner,kind:JobKind,output:Result<JobOutput,String>)->u64 {
    let r=owner.active().unwrap();
    let stamp=r.stamp();let captured=r.study.clone();let document=r.document.clone();
    owner.next_job+=1;let id=owner.next_job;
    owner.pending.push(PendingJob{id,kind,stamp:Some(stamp),document,source:"new-review.json".into(),trial_ids:vec![],launch:serde_json::Value::Null,cancel_requested:false,job:Job::finished(id,output),selection_epoch:owner.selection_epoch,gate:None,captured:Some(captured)});
    id
}

#[test]
fn old_text_missing_and_same_revision_replaced_identity_are_refused_without_mutation() {
    let (mut owner,mut registry)=owner();
    let stamp=owner.active().unwrap().stamp();
    let before=owner.active().unwrap().study.notes.clone();
    let changed=StudyAction::Apply{stamp,command:Command::SetNotes("first edit".into())};
    apply_action(&mut owner,&registry,&changed).unwrap();
    assert!(apply_action(&mut owner,&registry,&changed).unwrap_err().contains("revision"));
    let current=owner.active().unwrap().stamp();
    registry.open(ViewerMode::Build,DocumentKind::System,Source::path("replacement.system.json"));
    assert_eq!(registry.entry(ViewerMode::Build).unwrap().revision,0);
    assert!(apply_action(&mut owner,&registry,&StudyAction::Apply{stamp:current,command:Command::SetNotes("late".into())}).unwrap_err().contains("document"));
    assert_ne!(owner.active().unwrap().study.notes,before);
    assert_eq!(owner.active().unwrap().study.notes,"first edit");
    let unknown=super::StudyStamp{id:999,revision:0};
    assert!(owner.validate_stamp(unknown).is_err());
}

#[test]
fn immutable_save_ack_does_not_mark_later_edits_saved_and_failure_preserves_dirty() {
    let (mut owner,registry)=owner();
    finished(&mut owner,JobKind::Save,Ok(JobOutput::Published));
    let stamp=owner.active().unwrap().stamp();
    apply_action(&mut owner,&registry,&StudyAction::Apply{stamp,command:Command::SetNotes("newer notes".into())}).unwrap();
    jobs::poll_owner(&mut owner,&registry);
    assert!(owner.active().unwrap().dirty());assert!(owner.receipts[0].stale);
    finished(&mut owner,JobKind::Save,Err("Destination exists; immutable evidence was not overwritten".into()));
    jobs::poll_owner(&mut owner,&registry);
    assert!(owner.active().unwrap().dirty());assert!(owner.receipts[1].error.is_some());
    finished(&mut owner,JobKind::Export,Ok(JobOutput::Published));
    jobs::poll_owner(&mut owner,&registry);
    assert!(owner.active().unwrap().dirty(),"HTML export is not a review-save acknowledgement");
    finished(&mut owner,JobKind::Save,Ok(JobOutput::Published));
    jobs::poll_owner(&mut owner,&registry);
    assert!(!owner.active().unwrap().dirty());
}

#[test]
fn completion_follows_original_study_identity_and_retains_displaced_cancelled_evidence() {
    let (mut owner,mut registry)=owner();
    let original=owner.active().unwrap().id;
    let snapshot=owner.active().unwrap().study.clone();
    let ids=vec![snapshot.archive.trials[0].id.clone()];
    let evaluation=sim_runtime::experiment_study::evaluate(&snapshot.archive,&ids,&snapshot.baseline,&snapshot.draft,None,false,&AtomicBool::new(true),|_,_|{}).unwrap();
    assert_eq!(evaluation.summary(&ids).unscored,1);
    finished(&mut owner,JobKind::Evaluate,Ok(JobOutput::Evaluated(evaluation)));
    let next=owner.retain(snapshot,"different retained study".into(),DocumentCapture::current(&registry),false,true);
    assert_ne!(original,next);
    registry.open(ViewerMode::Build,DocumentKind::System,Source::path("replacement.system.json"));
    jobs::poll_owner(&mut owner,&registry);
    assert!(owner.get(next).unwrap().study.evaluations.is_empty());
    let e=&owner.get(original).unwrap().study.evaluations[0];
    assert!(e.cancelled);assert_eq!(e.summary(&ids).passes,0);
    assert!(owner.receipts[0].displaced);assert!(owner.get(original).unwrap().displaced.is_some());
}

#[test]
fn load_navigation_and_cancelled_late_reads_never_erase_or_steal_selection() {
    let (mut owner,registry)=owner();
    let selected=owner.active;
    let snapshot=owner.active().unwrap().study.clone();
    finished(&mut owner,JobKind::Review,Ok(JobOutput::Loaded(snapshot.clone())));
    owner.selection_epoch+=1;
    jobs::poll_owner(&mut owner,&registry);
    assert_eq!(owner.active,selected);assert_eq!(owner.studies.len(),2);
    assert!(owner.receipts[0].displaced);
    let id=finished(&mut owner,JobKind::Review,Ok(JobOutput::Loaded(snapshot)));
    apply_action(&mut owner,&registry,&StudyAction::Cancel{job:id}).unwrap();
    jobs::poll_owner(&mut owner,&registry);
    assert_eq!(owner.studies.len(),3);assert_eq!(owner.active,selected);
    assert!(owner.receipts[1].captured.is_some());assert!(owner.receipts[1].cancelled);
}

#[test]
fn publication_cancel_gate_distinguishes_before_and_after_publication_start() {
    let (mut owner,_)=owner();
    finished(&mut owner,JobKind::Save,Ok(JobOutput::Published));
    let gate=Arc::new(Mutex::new(PublicationGate::default()));
    owner.pending[0].gate=Some(gate.clone());
    assert!(owner.pending[0].cancel().contains("requested"));
    assert!(gate.lock().unwrap().cancelled);
    finished(&mut owner,JobKind::Save,Ok(JobOutput::Published));
    let gate=Arc::new(Mutex::new(PublicationGate{started:true,cancelled:false}));
    owner.pending[1].gate=Some(gate.clone());
    assert!(owner.pending[1].cancel().contains("already began"));
    assert!(!owner.pending[1].cancel_requested);assert!(!gate.lock().unwrap().cancelled);
}

#[test]
fn closure_and_busy_refusal_keep_drafts_failed_requests_and_exposure() {
    let (mut owner,registry)=owner();
    assert!(owner.blocking_reason().unwrap().contains("unsaved"));
    let stamp=owner.active().unwrap().stamp();
    apply_action(&mut owner,&registry,&StudyAction::Apply{stamp,command:Command::Expose(EvaluationSelection::HeldOut)}).unwrap();
    assert!(owner.active().unwrap().study.validation_seen);
    let stamp=owner.active().unwrap().stamp();
    let before=owner.active().unwrap().revision;
    assert!(apply_action(&mut owner,&registry,&StudyAction::Apply{stamp,command:Command::SetStep(f64::NAN)}).is_err());
    assert_eq!(owner.active().unwrap().revision,before);assert!(owner.active().unwrap().study.validation_seen);
    finished(&mut owner,JobKind::Evaluate,Err("Cancelled before worker start".into()));
    assert!(owner.blocking_reason().unwrap().contains("pending"));
    assert!(jobs::start_evaluation(&mut owner,stamp,EvaluationSelection::HeldOut).unwrap_err().contains("pending"));
    jobs::poll_owner(&mut owner,&registry);
    assert!(owner.receipts[0].captured.is_some());assert!(owner.active().unwrap().study.validation_seen);
    let saved=serde_json::to_value(&owner.active().unwrap().study).unwrap();
    let reopened:Study=serde_json::from_value(saved).unwrap();
    assert_eq!(reopened.retained_fields["native_offline_job_receipts"][0]["error"],"Cancelled before worker start");
    assert_eq!(reopened.retained_fields["native_offline_job_receipts"][0]["inputs"]["validation_seen"],true);
}

#[test]
fn failed_receipt_roundtrip_preserves_an_existing_opaque_extension() {
    let (mut owner,registry)=owner();
    owner.get_mut(owner.active.unwrap()).unwrap().study.retained_fields.insert("native_offline_job_receipts".into(),serde_json::json!({"future":"opaque supported data"}));
    finished(&mut owner,JobKind::Save,Err("new destination conflict".into()));
    jobs::poll_owner(&mut owner,&registry);
    let saved=serde_json::to_value(&owner.active().unwrap().study).unwrap();
    let reopened:Study=serde_json::from_value(saved).unwrap();
    assert_eq!(reopened.retained_fields["native_offline_job_receipts"]["future"],"opaque supported data");
    assert_eq!(reopened.retained_fields["native_offline_job_receipts_retained_1"][0]["error"],"new destination conflict");
    assert!(owner.active().unwrap().dirty());
}

#[test]
fn full_status_exposes_held_out_data_even_with_train_only_filters() {
    let (mut owner,registry)=owner();
    let current=owner.active.unwrap();
    let study=owner.get_mut(current).unwrap();
    study.study.view.role="Tuning".into();study.study.validation_seen=false;
    study.saved_revision=Some(study.revision);
    let old_revision=study.revision;
    apply_action(&mut owner,&registry,&StudyAction::Status).unwrap();
    let study=owner.active().unwrap();
    assert!(study.study.validation_seen);assert_eq!(study.revision,old_revision+1);assert!(study.dirty());
    let stamp=study.stamp();
    apply_action(&mut owner,&registry,&StudyAction::Apply{stamp,command:Command::SetStep(0.0005)}).unwrap();
    assert!(owner.active().unwrap().study.validation_influenced);
}

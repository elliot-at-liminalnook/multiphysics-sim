//! T48 written lifecycle fixtures. These fixtures have NOT been executed.
//! Finished handles exercise the production global result owner without starting jobs.
use super::{actions::{self, StudyAction}, jobs::{self, JobKind, JobOutput, PendingJob}, state::{DocumentCapture, StudyOwner}};
use crate::{app::{ViewerMode, actions::{Call, Origin, Replies}}, document::{DocumentKind, DocumentRegistry, Source}, jobs::Job};
use sim_runtime::experiment_study::{Study, refinement::{Command, Operation, Outcome, Capture}};
use serde_json::json;

fn owner()->(StudyOwner,DocumentRegistry) {
    let root=crate::workspace::root().unwrap();
    let archive=sim_runtime::experiment_comparison::hx_archive::load(&root.join(super::super::DEFAULT_ARCHIVE),root).unwrap();
    let mut registry=DocumentRegistry::default();
    registry.open(ViewerMode::Build,DocumentKind::System,Source::path("refinement-fixture.system.json"));
    let mut owner=StudyOwner::default();
    owner.retain(Study::new(archive).unwrap(),"unexecuted fixture archive".into(),DocumentCapture::current(&registry),false,true);
    (owner,registry)
}
fn finished(owner:&mut StudyOwner,operation:Operation,cancelled:bool,requested:bool)->u64 {
    let r=owner.active().unwrap();
    let stamp=r.stamp();let captured=r.study.clone();let document=r.document.clone();
    let outcome=Outcome{capture:Capture{study:captured.clone(),operation,runtime:sim_runtime::experiment_study::execution_identity(),captured_unix_ns:"0".into()},result:Err("fixture.analysis: deliberate incomplete attempt".into()),cancelled};
    owner.next_job+=1;let id=owner.next_job;
    owner.pending.push(PendingJob{id,kind:JobKind::Refinement,stamp:Some(stamp),document,source:"fixture archive".into(),trial_ids:vec![],launch:json!({"captured_revision":stamp.revision}),cancel_requested:requested,job:Job::finished(id,Ok(JobOutput::Refined(outcome))),selection_epoch:owner.selection_epoch,gate:None,captured:Some(captured)});
    id
}
#[test]
fn edit_and_review_use_shared_commands_without_implicit_candidate_adoption() {
    let (mut owner,registry)=owner();
    let stamp=owner.active().unwrap().stamp();
    let mut e=owner.active().unwrap().study.refinement.experiment.clone();e.name="Captured native draft".into();
    actions::apply_action(&mut owner,&registry,&StudyAction::RefineApply{stamp,command:Command::SetExperiment(e)}).unwrap();
    assert!(actions::apply_action(&mut owner,&registry,&StudyAction::RefineApply{stamp,command:Command::SetCoordinates(vec![])}).unwrap_err().contains("revision"));
    let before=owner.active().unwrap().study.draft.clone();
    finished(&mut owner,Operation::Simulate,true,true);
    jobs::poll_owner(&mut owner,&registry);
    assert_eq!(serde_json::to_value(before).unwrap(),serde_json::to_value(&owner.active().unwrap().study.draft).unwrap());
    assert!(owner.active().unwrap().study.refinement.controller_runs.is_empty());
    assert!(owner.active().unwrap().study.refinement_evidence.receipts[0].failure.is_some());
    assert!(owner.blocking_reason().is_some());
}
#[test]
fn switched_edited_replaced_completion_and_cancellation_request_keep_original_identity() {
    let (mut owner,mut registry)=owner();
    let original=owner.active().unwrap().id;
    finished(&mut owner,Operation::Robustness,false,true);
    let copy=owner.active().unwrap().study.clone();
    owner.get_mut(original).unwrap().revision+=1;
    let selected=owner.retain(copy,"other study".into(),DocumentCapture::current(&registry),false,true);
    registry.open(ViewerMode::Build,DocumentKind::System,Source::path("replacement.system.json"));
    jobs::poll_owner(&mut owner,&registry);
    assert_eq!(owner.active,Some(selected));
    assert!(owner.get(selected).unwrap().study.refinement_evidence.receipts.is_empty());
    assert_eq!(owner.get(original).unwrap().study.refinement_evidence.receipts.len(),1);
    let receipt=&owner.receipts[0];assert!(receipt.cancelled);assert!(receipt.stale);assert!(receipt.displaced);
    assert!(!receipt.refinement.as_ref().unwrap().cancelled,"late cancellation request is not actual runtime cancellation");
    let terminal=&receipt.launch["terminal"];
    assert_eq!(terminal["execution_cancelled"],false);assert_eq!(terminal["cancellation_requested"],true);
    assert!(owner.get(original).unwrap().study.retained_fields.contains_key("native_offline_job_receipts"));
}
#[test]
fn sensitivity_fit_failures_and_opaque_native_receipts_survive_serialized_reopen() {
    let (mut owner,registry)=owner();
    owner.active=Some(1);
    owner.get_mut(1).unwrap().study.retained_fields.insert("native_offline_job_receipts".into(),json!({"future":"opaque retained payload"}));
    finished(&mut owner,Operation::Sensitivity{selected:vec!["captured trial".into()]},false,false);
    jobs::poll_owner(&mut owner,&registry);
    finished(&mut owner,Operation::Fit{train:vec!["training".into()],validation:vec!["heldout".into()]},false,false);
    jobs::poll_owner(&mut owner,&registry);
    let serialized=serde_json::to_value(&owner.active().unwrap().study).unwrap();
    let reopened:Study=serde_json::from_value(serialized).unwrap();
    assert_eq!(reopened.refinement_evidence.receipts.len(),2);
    assert!(reopened.refinement_evidence.receipts.iter().all(|r|r.failure.is_some() && r.result_index.is_none()));
    assert_eq!(reopened.retained_fields["native_offline_job_receipts"]["future"],"opaque retained payload");
    assert!(reopened.retained_fields.contains_key("native_offline_job_receipts_retained_1"));
    assert!(reopened.refinement.fits.is_empty(),"failed fitting supplies no candidate");
}
#[test]
fn rejected_candidate_use_keeps_input_and_path_named_error_durably() {
    let (mut owner,registry)=owner();
    let stamp=owner.active().unwrap().stamp();
    let mut continuation=serde_json::Value::Null;let mut replies=Replies::default();
    let mut call=Call{origin:Origin::Ui,continuation:&mut continuation,cancelled:false,replies:&mut replies};
    let outcome=actions::handle(&mut owner,&registry,&StudyAction::RefineApply{stamp,command:Command::UseFit{fit:99,device:None}},&mut call);
    assert!(matches!(outcome,sim_api::Outcome::Done(Err(e)) if e.contains("refinement.use_fit.fit")));
    let rejected=&owner.active().unwrap().study.retained_fields["native_refinement_rejections"][0];
    assert_eq!(rejected["stamp"]["revision"],stamp.revision);
    assert!(rejected["error"].as_str().unwrap().contains("fit"));
    assert!(owner.active().unwrap().dirty());
}
#[test]
fn idle_global_poll_is_observation_and_missing_original_retains_orphan_outcome() {
    let (mut owner,registry)=owner();
    let changed=owner.changed;jobs::poll_owner(&mut owner,&registry);assert_eq!(owner.changed,changed);
    finished(&mut owner,Operation::Simulate,true,true);
    owner.studies.clear();owner.active=None;
    jobs::poll_owner(&mut owner,&registry);
    assert!(owner.receipts[0].displaced);assert!(owner.receipts[0].refinement.is_some());
    assert!(owner.receipts[0].error.is_some());
}
#[test]
fn busy_refinement_launch_is_refused_before_capture_and_publication_ack_is_revision_scoped() {
    let (mut owner,registry)=owner();
    let stamp=owner.active().unwrap().stamp();
    finished(&mut owner,Operation::Simulate,true,true);
    assert!(jobs::start_refinement(&mut owner,stamp,Operation::Simulate).unwrap_err().contains("pending"));
    jobs::poll_owner(&mut owner,&registry);
    let r=owner.active().unwrap();let saved_stamp=r.stamp();let captured=r.study.clone();let document=r.document.clone();
    owner.next_job+=1;let id=owner.next_job;
    owner.pending.push(PendingJob{id,kind:JobKind::Save,stamp:Some(saved_stamp),document,source:"new immutable destination".into(),trial_ids:vec![],launch:json!(null),cancel_requested:false,job:Job::finished(id,Ok(JobOutput::Published)),selection_epoch:owner.selection_epoch,gate:None,captured:Some(captured)});
    let mut e=owner.active().unwrap().study.refinement.experiment.clone();e.notes="later edit remains dirty".into();
    actions::apply_action(&mut owner,&registry,&StudyAction::RefineApply{stamp:saved_stamp,command:Command::SetExperiment(e)}).unwrap();
    jobs::poll_owner(&mut owner,&registry);
    assert!(owner.active().unwrap().dirty());assert!(owner.receipts.last().unwrap().stale);
}

#[test]
fn native_cancel_action_keeps_pending_work_until_actual_terminal_receipt() {
    let (mut owner,registry)=owner();
    let id=finished(&mut owner,Operation::Simulate,false,false);
    actions::apply_action(&mut owner,&registry,&StudyAction::Cancel{job:id}).unwrap();
    assert_eq!(owner.pending.len(),1);assert!(owner.pending[0].cancel_requested);
    assert!(owner.blocking_reason().is_some());
    jobs::poll_owner(&mut owner,&registry);
    assert!(owner.pending.is_empty());assert!(owner.receipts[0].cancelled);
    assert!(!owner.receipts[0].refinement.as_ref().unwrap().cancelled);
    assert!(owner.active().unwrap().dirty());
}

//! T50 written, UNEXECUTED native recording lifecycle fixtures.
//! Finished adopted handles reach the production global poll/application owner;
//! they start no threads, run no physics and publish no files.
use super::{actions::{self,StudyAction},jobs::{self,JobKind,JobOutput,PendingJob},state::{StudyOwner,DocumentCapture}};
use crate::{app::ViewerMode,document::{DocumentRegistry,DocumentKind,Source},jobs::Job};
use sim_runtime::{experiment_study::{Study,commands,refinement::{self,Command,Operation}},controller_refinement::recording::{Recording,Purpose},physics_context::RuntimeIdentity};
use serde_json::json;
fn owner()->(StudyOwner,DocumentRegistry) {
    let root=crate::workspace::root().unwrap();
    let archive=sim_runtime::experiment_comparison::hx_archive::load(&root.join(super::super::DEFAULT_ARCHIVE),root).unwrap();
    let mut registry=DocumentRegistry::default();
    registry.open(ViewerMode::Build,DocumentKind::System,Source::path("recording-fixture.system.json"));
    let mut owner=StudyOwner::default();
    owner.retain(Study::new(archive).unwrap(),"unexecuted fixture archive".into(),DocumentCapture::current(&registry),false,true);
    (owner,registry)
}
fn incomplete(study:&Study)->Recording {
    Recording{version:1,experiment:study.refinement.experiment.clone(),runtime:RuntimeIdentity::current(),frames:vec![],stop_request_s:0.,stop_receipt_s:0.,completed:false,failure:Some("fixture acquisition ended before first frame".into()),stop_verified:false,initial_registers:json!({}),transactions_origin_host_s:0.,timing_evidence:"host seconds; no verified hardware capture".into(),source_hashes:std::collections::BTreeMap::from([("fixture".into(),"a".repeat(64))])}
}
fn finished(owner:&mut StudyOwner,kind:JobKind,output:JobOutput)->u64 {
    let r=owner.active().unwrap();let stamp=r.stamp();let captured=r.study.clone();let document=r.document.clone();
    owner.next_job+=1;let id=owner.next_job;
    owner.pending.push(PendingJob{id,kind,stamp:Some(stamp),document,source:"fixture recording.json".into(),trial_ids:vec![],launch:json!({"study":stamp}),cancel_requested:false,job:Job::finished(id,Ok(output)),selection_epoch:owner.selection_epoch,gate:None,captured:Some(captured)});id
}
#[test]
fn stale_recording_submission_reaches_one_owner_and_refuses_before_any_io_job() {
    let (mut owner,registry)=owner();let stamp=owner.active().unwrap().stamp();
    actions::apply_action(&mut owner,&registry,&StudyAction::Apply{stamp,command:commands::Command::SetNotes("later draft".into())}).unwrap();
    assert!(actions::apply_action(&mut owner,&registry,&StudyAction::ImportRecording{stamp,path:"never-read.json".into()}).unwrap_err().contains("revision"));
    assert!(owner.pending.is_empty());assert_eq!(owner.active().unwrap().study.notes,"later draft");
}
#[test]
fn prepared_incomplete_import_round_trips_source_context_and_remains_unscored() {
    let (mut owner,registry)=owner();let mut prepared=owner.active().unwrap().study.clone();let recording=incomplete(&prepared);let hash=recording.fingerprint();
    refinement::apply(&mut prepared,Command::ImportRecording{recording:recording.clone()}).unwrap();
    finished(&mut owner,JobKind::RecordingImport,JobOutput::RecordingImport{prepared:Some(Box::new(prepared)),input:json!({"classification":"controller","typed_recording":recording}),error:None});
    jobs::poll_owner(&mut owner,&registry);
    let reopened:Study=serde_json::from_value(serde_json::to_value(&owner.active().unwrap().study).unwrap()).unwrap();
    assert_eq!(reopened.refinement.recordings[0].fingerprint(),hash);
    assert_eq!(reopened.refinement.capture_contexts[0].recording_hash,hash);
    assert!(reopened.refinement.predictions.is_empty());assert!(reopened.refinement.recording_fits.is_empty());
    assert_eq!(reopened.retained_fields["native_recording_imports"][0]["applied"],true);
    assert!(owner.active().unwrap().dirty());
}
#[test]
fn terminal_import_cannot_replace_newer_context_or_another_study_and_keeps_raw_input() {
    let (mut owner,mut registry)=owner();let original=owner.active().unwrap().id;
    let mut prepared=owner.active().unwrap().study.clone();let recording=incomplete(&prepared);
    refinement::apply(&mut prepared,Command::ImportRecording{recording}).unwrap();
    let id=finished(&mut owner,JobKind::RecordingImport,JobOutput::RecordingImport{prepared:Some(Box::new(prepared)),input:json!({"classification":"controller","raw":"captured input text"}),error:None});
    actions::apply_action(&mut owner,&registry,&StudyAction::Cancel{job:id}).unwrap();
    owner.get_mut(original).unwrap().revision+=1;
    let chosen=owner.retain(owner.get(original).unwrap().study.clone(),"other archive".into(),DocumentCapture::current(&registry),false,true);
    registry.open(ViewerMode::Build,DocumentKind::System,Source::path("displaced.system.json"));
    jobs::poll_owner(&mut owner,&registry);
    assert_eq!(owner.active,Some(chosen));assert!(owner.get(chosen).unwrap().study.refinement.recordings.is_empty());
    assert!(owner.get(original).unwrap().study.refinement.recordings.is_empty());
    let row=&owner.get(original).unwrap().study.retained_fields["native_recording_imports"][0];
    assert_eq!(row["input"]["raw"],"captured input text");assert_eq!(row["applied"],false);
    assert!(owner.receipts[0].cancelled && owner.receipts[0].stale && owner.receipts[0].displaced);
}
#[test]
fn deferred_fpga_and_rejected_bytes_are_retained_without_controller_insertion() {
    let (mut owner,registry)=owner();
    finished(&mut owner,JobKind::RecordingImport,JobOutput::RecordingImport{prepared:None,input:json!({"classification":"fpga_deferred","raw":"validated FPGA capture"}),error:None});
    jobs::poll_owner(&mut owner,&registry);
    assert!(owner.status.contains("deferred"));assert!(owner.active().unwrap().study.refinement.recordings.is_empty());
    finished(&mut owner,JobKind::RecordingImport,JobOutput::RecordingImport{prepared:None,input:json!({"raw":"{malformed"}),error:Some("recording.import.json: invalid object".into())});
    jobs::poll_owner(&mut owner,&registry);
    let rows=owner.active().unwrap().study.retained_fields["native_recording_imports"].as_array().unwrap();
    assert_eq!(rows[1]["input"]["raw"],"{malformed");assert!(owner.receipts[1].error.is_some());
}
#[test]
fn both_prediction_purposes_and_additional_rejections_capture_bounded_original_identity() {
    let (mut owner,registry)=owner();let mut prepared=owner.active().unwrap().study.clone();let recording=incomplete(&prepared);let hash=recording.fingerprint();
    refinement::apply(&mut prepared,Command::ImportRecording{recording}).unwrap();
    owner.get_mut(1).unwrap().study=prepared;
    for purpose in [Purpose::RecordedCommandReplay,Purpose::ClosedLoopPrediction] {
        let study=owner.active().unwrap().study.clone();
        let capture=refinement::Capture{study,operation:Operation::PredictRecording{recording_hash:hash.clone(),purpose},runtime:sim_runtime::experiment_study::execution_identity(),captured_unix_ns:"0".into()};
        finished(&mut owner,JobKind::Refinement,JobOutput::Refined{inputs:capture.inputs(),outcome:refinement::Outcome{capture,result:Err("recording.prediction: incomplete capture remains inspectable".into()),cancelled:false}});
        jobs::poll_owner(&mut owner,&registry);
    }
    let study=owner.active().unwrap().study.clone();let observation=study.archive.observation_blake3.clone();
    let capture=refinement::Capture{study:study.clone(),operation:Operation::FitCombined{additional:Some(Box::new(study))},runtime:sim_runtime::experiment_study::execution_identity(),captured_unix_ns:"0".into()};
    let inputs=capture.inputs();assert!(inputs["operation"]["FitCombined"]["additional"].get("refinement").is_none());
    finished(&mut owner,JobKind::Refinement,JobOutput::Refined{inputs:capture.inputs(),outcome:refinement::Outcome{capture,result:Err("recording.additional.identity: ambiguous archive trial collision".into()),cancelled:true}});
    jobs::poll_owner(&mut owner,&registry);
    assert_eq!(owner.active().unwrap().study.archive.observation_blake3,observation);
    let receipts=&owner.active().unwrap().study.refinement_evidence.receipts;
    assert!(matches!(receipts[0].operation,Operation::PredictRecording{purpose:Purpose::RecordedCommandReplay,..}));
    assert!(matches!(receipts[1].operation,Operation::PredictRecording{purpose:Purpose::ClosedLoopPrediction,..}));
    assert!(matches!(receipts[2].operation,Operation::FitCombined{additional:None}));
    assert!(owner.active().unwrap().study.refinement.recording_fits.is_empty());
    assert!(owner.active().unwrap().study.refinement.combined_fits.is_empty());
}
#[test]
fn import_terminal_evidence_invalidates_older_publication_acknowledgment() {
    let (mut owner,registry)=owner();let captured=owner.active().unwrap().study.clone();
    finished(&mut owner,JobKind::RecordingImport,JobOutput::RecordingImport{prepared:None,input:json!({"raw":"rejected pending input"}),error:Some("recording.import.kind: ambiguous classification".into())});
    finished(&mut owner,JobKind::Save,JobOutput::Published);
    jobs::poll_owner(&mut owner,&registry);
    assert!(owner.active().unwrap().dirty());assert!(owner.receipts[1].stale);
    assert!(!captured.retained_fields.contains_key("native_recording_imports"));
    assert!(owner.active().unwrap().study.retained_fields.contains_key("native_recording_imports"));
}
#[test]
fn revision_equal_terminal_preserves_monotonic_review_exposure_and_opaque_evidence() {
    let (mut owner,registry)=owner();
    owner.get_mut(1).unwrap().study.validation_seen=true;
    let mut prepared=owner.active().unwrap().study.clone();let recording=incomplete(&prepared);let hash=recording.fingerprint();
    refinement::apply(&mut prepared,Command::ImportRecording{recording}).unwrap();
    finished(&mut owner,JobKind::RecordingImport,JobOutput::RecordingImport{prepared:Some(Box::new(prepared)),input:json!({"classification":"controller","execution_cancelled":false}),error:None});
    // Review can add identity exposure while the global flag was already set,
    // without changing the authoring revision. Terminal replacement must merge it.
    let original=owner.get_mut(1).unwrap();
    original.study.validation_influenced=true;
    original.study.refinement_evidence.recording_exposure.insert(hash.clone(),true);
    original.study.retained_fields.insert("future_opaque_evidence".into(),json!({"kept":"verbatim"}));
    jobs::poll_owner(&mut owner,&registry);
    assert!(owner.active().unwrap().study.validation_influenced);
    assert_eq!(owner.active().unwrap().study.refinement_evidence.recording_exposure[&hash],true);
    assert_eq!(owner.active().unwrap().study.retained_fields["future_opaque_evidence"]["kept"],"verbatim");
    assert_eq!(owner.receipts[0].launch["terminal"]["execution_cancelled"],false);
    assert_eq!(owner.receipts[0].launch["terminal"]["cancellation_requested"],false);
}
fn failed_review_source(study:&mut Study)->String {
    use sim_runtime::controller_refinement::{calibration as cal,calibration_data as data};
    let recording=incomplete(study);let hash=recording.fingerprint();
    refinement::apply(study,Command::ImportRecording{recording:recording.clone()}).unwrap();
    // Synthetic historical failed source: deliberately unscored. This fixture tests
    // terminal review/publication bookkeeping, not capture schema qualification.
    study.refinement.recording_fits.push(data::RecordingFitAttempt{dataset:data::RecordingDataset{recordings:vec![recording],assignments:vec![]},attempt:cal::FitAttempt{request:cal::FitRequest{model:cal::Family{shared:study.draft.clone(),device_deltas:std::collections::BTreeMap::new()},training_ids:vec![],validation_ids:vec![hash.clone()],coordinates:vec![],maximum_evaluations:40,validation_influenced:false},archive_hash:"a".repeat(64),runtime:RuntimeIdentity::current(),evaluations:vec![],outcome:None,partial:None,failure:Some("unscored historical fixture".into()),cancelled:false}});
    study.refinement_evidence.recording_exposure.clear();study.validation_seen=true;hash
}
#[test]
fn status_exposure_map_edit_prevents_older_publication_from_acknowledging_new_review() {
    let (mut owner,registry)=owner();let hash=failed_review_source(&mut owner.get_mut(1).unwrap().study);
    let stamp=owner.active().unwrap().stamp();finished(&mut owner,JobKind::Save,JobOutput::Published);
    actions::apply_action(&mut owner,&registry,&StudyAction::Status).unwrap();
    assert!(owner.active().unwrap().study.refinement_evidence.recording_exposure.contains_key(&hash));
    assert!(owner.active().unwrap().revision>stamp.revision,"new identity exposure is a durable edit even when global seen was already true");
    jobs::poll_owner(&mut owner,&registry);assert!(owner.active().unwrap().dirty());assert!(owner.receipts[0].stale);
}
#[test]
fn choose_and_load_detect_new_identity_exposure_when_global_seen_was_already_true() {
    let (mut owner,registry)=owner();let hash=failed_review_source(&mut owner.get_mut(1).unwrap().study);
    let before=owner.active().unwrap().revision;
    actions::apply_action(&mut owner,&registry,&StudyAction::Choose{id:1}).unwrap();
    assert!(owner.active().unwrap().revision>before);
    let mut loaded=owner.active().unwrap().study.clone();loaded.refinement_evidence.recording_exposure.clear();
    finished(&mut owner,JobKind::Review,JobOutput::Loaded(loaded));jobs::poll_owner(&mut owner,&registry);
    assert!(owner.active().unwrap().study.refinement_evidence.recording_exposure.contains_key(&hash));
    assert!(owner.active().unwrap().dirty(),"newly exposed reopened evidence is not silently acknowledged saved");
}
#[test]
fn rejected_additional_heldout_conflict_quarantines_identity_without_rewriting_frozen_train() {
    use sim_runtime::controller_refinement::calibration_data::{Assignment,Role};
    let (mut owner,registry)=owner();let r=incomplete(&owner.active().unwrap().study);let hash=r.fingerprint();
    refinement::apply(&mut owner.get_mut(1).unwrap().study,Command::ImportRecording{recording:r}).unwrap();
    let train=Assignment{recording_hash:hash.clone(),role:Role::Train,limits:sim_runtime::experiment_comparison::Limits{rmse:1.,final_abs_error:1.},rationale:"earlier frozen tuning declaration".into()};
    // Role collision is independent of completeness; no attempt can score this source.
    owner.get_mut(1).unwrap().study.refinement.recording_assignments.push(train.clone());
    let mut extra=owner.active().unwrap().study.clone();let mut held=train.clone();held.role=Role::HeldOut;held.rationale="earlier additional-study heldout reservation".into();
    extra.refinement.recording_assignments=vec![held];
    let mut worker=owner.active().unwrap().study.clone();
    refinement::recordings::reserve_additional(&mut worker,&extra).unwrap();
    assert_eq!(worker.refinement.recording_assignments[0],train);
    assert!(refinement::recordings::dataset(&worker,Some(&extra)).unwrap_err().contains("held-out"));
    let capture=refinement::Capture{study:worker,operation:Operation::FitCombined{additional:Some(Box::new(extra))},runtime:sim_runtime::experiment_study::execution_identity(),captured_unix_ns:"0".into()};
    finished(&mut owner,JobKind::Refinement,JobOutput::Refined{inputs:capture.inputs(),outcome:refinement::Outcome{capture,result:Err("recording.dataset.roles: rejected additional held-out collision".into()),cancelled:false}});
    jobs::poll_owner(&mut owner,&registry);
    let live=&owner.active().unwrap().study;
    assert!(live.refinement_evidence.recording_held_out.contains(&hash));assert_eq!(live.refinement.recording_assignments[0],train);
    assert!(refinement::recordings::dataset(live,None).unwrap_err().contains("held-out"));
    let reopened:Study=serde_json::from_value(serde_json::to_value(live).unwrap()).unwrap();assert!(reopened.refinement_evidence.recording_held_out.contains(&hash));
    // Quarantine preserves unrelated inspection and purpose authoring.
    let mut inspection=reopened.clone();
    refinement::apply(&mut inspection,Command::SetPredictionPurpose(Purpose::ClosedLoopPrediction)).unwrap();
    finished(&mut owner,JobKind::RecordingAuthoring,JobOutput::RecordingImport{prepared:Some(Box::new(inspection)),input:json!({"classification":"recording_command"}),error:None});
    jobs::poll_owner(&mut owner,&registry);assert_eq!(owner.receipts.last().unwrap().launch["recording_applied"],true);
    // A newly introduced prepared tuning assignment still fails attachment.
    let prepared=owner.active().unwrap().study.clone();
    owner.get_mut(1).unwrap().study.refinement.recording_assignments.clear();finished(&mut owner,JobKind::RecordingAuthoring,JobOutput::RecordingImport{prepared:Some(Box::new(prepared)),input:json!({"classification":"recording_command"}),error:None});
    jobs::poll_owner(&mut owner,&registry);assert_eq!(owner.receipts.last().unwrap().launch["recording_applied"],false);
    assert!(owner.receipts.last().unwrap().error.as_ref().unwrap().contains("held_out"));
}

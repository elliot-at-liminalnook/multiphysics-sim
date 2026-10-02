//! T50 shared-contract fixtures, written and deliberately unexecuted.
use super::{self as shared, recordings};
use crate::{experiment_study::Study, controller_refinement::{recording::{Recording,MeasuredFrame,Purpose},control::{Experiment,ControllerSession,Feedback},calibration_data::{Assignment,Role}},experiment_comparison::Limits};
fn study()->Study{let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");Study::new(crate::experiment_comparison::hx_archive::load(&root.join("examples/actuators/hx30hm/pwm-identification"),&root).unwrap()).unwrap()}
fn recording(complete:bool)->Recording {
    let e=Experiment::default();let mut session=ControllerSession::new(e.clone()).unwrap();
    let f=session.tick(0.015,Feedback{electrical:None,observed_s:0.01,request_s:0.009,completion_s:0.012,received_s:0.012,encoder_rad:0.1}).unwrap();
    let drive_counts=(f.applied_duty*1000.).round() as i16;
    Recording{version:1,experiment:e,runtime:crate::physics_context::RuntimeIdentity::current(),frames:vec![MeasuredFrame{control:f,command_request_s:0.016,command_receipt_s:0.02,drive_counts,voltage_v:12.,temperature_c:25.,current_raw_uncalibrated:0}],stop_request_s:0.03,stop_receipt_s:0.04,completed:complete,failure:if complete{None}else{Some("Fixture capture interrupted".into())},stop_verified:complete,initial_registers:serde_json::json!({"fixture":true}),transactions_origin_host_s:0.,timing_evidence:"Synthetic transaction windows, encoder sample age unknown".into(),source_hashes:std::collections::BTreeMap::from([("controller_host".into(),"same source across repetitions".into())])}
}
fn assignment(r:&Recording,role:Role)->Assignment{Assignment{recording_hash:r.fingerprint(),role,limits:Limits{rmse:0.01,final_abs_error:0.02},rationale:"Whole run reserved before optimization".into()}}
#[test]
fn import_classification_duplicate_and_incomplete_are_transactional(){
    let mut s=study();let r=recording(false);r.validate().unwrap();
    assert!(matches!(recordings::classify(&serde_json::to_vec(&r).unwrap()).unwrap(),recordings::ImportClassification::Controller(_)));
    shared::apply(&mut s,shared::Command::ImportRecording{recording:r.clone()}).unwrap();
    let saved=serde_json::to_value(&s).unwrap();shared::apply(&mut s,shared::Command::ImportRecording{recording:r.clone()}).unwrap();assert_eq!(saved,serde_json::to_value(&s).unwrap());
    assert!(shared::prepare(&mut s,shared::Operation::PredictRecording{recording_hash:r.fingerprint(),purpose:Purpose::RecordedCommandReplay}).unwrap_err().contains("incomplete"));
    assert!(shared::apply(&mut s,shared::Command::AssignRecording{assignment:assignment(&r,Role::Train)}).is_err());assert_eq!(saved,serde_json::to_value(&s).unwrap());
    let mut mixed=serde_json::to_value(r).unwrap();mixed["plan"]=serde_json::json!({});assert!(recordings::classify(&serde_json::to_vec(&mixed).unwrap()).unwrap_err().contains("kind"));
}
#[test]
fn role_limits_rationale_and_setup_are_frozen_and_exposure_reopens(){
    let mut s=study();let r=recording(true);shared::apply(&mut s,shared::Command::ImportRecording{recording:r.clone()}).unwrap();
    let mut draft=s.draft.clone();draft.step_s*=0.5;crate::experiment_study::commands::apply(&mut s,crate::experiment_study::commands::Command::SetCandidate(draft)).unwrap();
    let held=assignment(&r,Role::HeldOut);shared::apply(&mut s,shared::Command::AssignRecording{assignment:held.clone()}).unwrap();assert!(s.validation_seen&&s.validation_influenced);
    let original=serde_json::to_value(&s).unwrap();let mut train=held;train.role=Role::Train;assert!(shared::apply(&mut s,shared::Command::AssignRecording{assignment:train}).unwrap_err().contains("recording.assignment.role"));assert_eq!(original,serde_json::to_value(&s).unwrap());
    let mut c=s.refinement.capture_contexts[0].clone();c.properties[0].source="Independent source pending; unknown retained".into();shared::apply(&mut s,shared::Command::AppendContext{context:c}).unwrap();assert_eq!(s.refinement.capture_contexts.len(),2);assert_eq!(s.refinement.recordings[0].fingerprint(),r.fingerprint());
    let loaded:Study=serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();loaded.validate().unwrap();assert!(loaded.validation_seen&&loaded.validation_influenced);
}
#[test]
fn additional_bare_ids_and_conflicting_recording_roles_never_retarget(){
    let mut s=study();let mut extra=s.clone();assert!(recordings::dataset(&s,Some(&extra)).unwrap_err().contains("identity"));
    for t in &mut extra.archive.trials {t.id=format!("additional:{}",t.id);}
    let r=recording(true);for target in [&mut s,&mut extra]{shared::apply(target,shared::Command::ImportRecording{recording:r.clone()}).unwrap();}
    shared::apply(&mut s,shared::Command::AssignRecording{assignment:assignment(&r,Role::HeldOut)}).unwrap();shared::apply(&mut extra,shared::Command::AssignRecording{assignment:assignment(&r,Role::Train)}).unwrap();
    assert!(recordings::dataset(&s,Some(&extra)).unwrap_err().contains("assignment"));
}
#[test]
fn prediction_purposes_and_capture_projection_preserve_distinct_inputs(){
    let mut s=study();let r=recording(true);shared::apply(&mut s,shared::Command::ImportRecording{recording:r.clone()}).unwrap();s.limits=Some(Limits{rmse:0.01,final_abs_error:0.02});
    for purpose in [Purpose::RecordedCommandReplay,Purpose::ClosedLoopPrediction]{let capture=shared::prepare(&mut s,shared::Operation::PredictRecording{recording_hash:r.fingerprint(),purpose}).unwrap();assert_eq!(capture.study.refinement_evidence.prediction_purpose,Some(purpose));let inputs=capture.inputs();assert!(inputs["recordings"][0]["recording_hash"].is_string());assert!(inputs.get("receipts").is_none());}
    let mut extra=s.clone();for t in &mut extra.archive.trials{t.id=format!("extra:{}",t.id);}
    let capture=shared::Capture{study:s,operation:shared::Operation::FitCombined{additional:Some(Box::new(extra))},runtime:serde_json::json!({}),captured_unix_ns:"1".into()};let text=serde_json::to_string(&capture.inputs()).unwrap();assert!(!text.contains("receipts"));assert!(!text.contains("combined_fits"));
}
#[test]
fn historical_duplicate_rows_and_opaque_deferred_payload_round_trip(){
    let mut s=study();let r=recording(false);s.refinement.recordings=vec![r.clone(),r];
    s.refinement.retained_fields.insert("future_fpga_receipt".into(),serde_json::json!({"opaque":{"source":["kept"]}}));
    s.refinement_evidence.retained_fields.insert("future_review".into(),serde_json::json!(["unknown"]));
    let value=serde_json::to_value(&s).unwrap();let loaded:Study=serde_json::from_value(value.clone()).unwrap();loaded.validate().unwrap();assert_eq!(serde_json::to_value(loaded).unwrap(),value);
}
#[test]
fn failed_attempt_and_wrong_source_candidate_are_refused_without_mutation(){
    let mut s=study();let r=recording(true);shared::apply(&mut s,shared::Command::ImportRecording{recording:r.clone()}).unwrap();
    shared::apply(&mut s,shared::Command::AssignRecording{assignment:assignment(&r,Role::HeldOut)}).unwrap();
    let d=crate::controller_refinement::calibration_data::RecordingDataset::capture(&s.refinement.recordings,&s.refinement.recording_assignments).unwrap();
    use crate::controller_refinement::calibration_data::CalibrationData;
    let attempt=crate::controller_refinement::calibration::FitAttempt{request:crate::controller_refinement::calibration::FitRequest{model:crate::controller_refinement::calibration::Family{shared:s.draft.clone(),device_deltas:Default::default()},training_ids:vec![],validation_ids:vec![r.fingerprint()],coordinates:vec![],maximum_evaluations:40,validation_influenced:false},archive_hash:d.fingerprint(),runtime:crate::physics_context::RuntimeIdentity::current(),evaluations:vec![],outcome:None,partial:None,failure:Some("Cancelled before optimization".into()),cancelled:true};
    s.refinement.recording_fits.push(crate::controller_refinement::calibration_data::RecordingFitAttempt{dataset:d,attempt});let before=serde_json::to_value(&s).unwrap();
    assert!(shared::apply(&mut s,shared::Command::UseRecordingFit{kind:"recording_fit".into(),index:0,device:None}).unwrap_err().contains("cancelled"));assert_eq!(serde_json::to_value(&s).unwrap(),before);
}
#[test]
fn fpga_inputs_are_validated_and_classified_as_deferred(){
    let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bytes=std::fs::read(root.join("examples/actuators/hx30hm/hardware/2026-09-14-full-drive/stage-250-01/fpga-recording.json")).unwrap();
    assert!(matches!(recordings::classify(&bytes).unwrap(),recordings::ImportClassification::FpgaDeferred));
    assert!(recordings::classify(br#"{"plan":{}}"#).unwrap_err().contains("fpga"));
}
#[test]
fn old_combined_snapshot_reservation_blocks_later_additional_tuning_without_new_map(){
    use crate::controller_refinement::{calibration as cal,calibration_data as data};
    let mut s=study();let r=recording(true);
    let recordings=data::RecordingDataset::capture(std::slice::from_ref(&r),&[assignment(&r,Role::HeldOut)]).unwrap();
    let dataset=data::CombinedDataset{archives:vec![s.archive.clone()],recordings:Some(recordings)};
    let cases=data::CalibrationData::cases(&dataset).unwrap();
    let resistance=s.draft.motor["resistance"];
    let request=cal::FitRequest{model:cal::Family{shared:s.draft.clone(),device_deltas:Default::default()},training_ids:cases.iter().filter(|c|c.split=="train").map(|c|c.id.clone()).collect(),validation_ids:cases.iter().filter(|c|c.split!="train").map(|c|c.id.clone()).collect(),coordinates:vec![cal::Coordinate{path:"motor.resistance".into(),device:None,lower:resistance*0.5,upper:resistance*1.5}],maximum_evaluations:40,validation_influenced:true};
    let attempt=cal::attempt(&dataset,&request,&std::sync::atomic::AtomicBool::new(true),|_,_|{});
    s.refinement.combined_fits.push(data::CombinedFitAttempt{dataset,attempt});
    assert!(s.refinement_evidence.recording_reservations.is_empty(),"historical study lacks additive map");
    let mut extra=study();for t in &mut extra.archive.trials{t.id=format!("extra:{}",t.id);}
    shared::apply(&mut extra,shared::Command::ImportRecording{recording:r.clone()}).unwrap();
    shared::apply(&mut extra,shared::Command::AssignRecording{assignment:assignment(&r,Role::Train)}).unwrap();
    assert!(recordings::dataset(&s,Some(&extra)).unwrap_err().contains("historical reservation"));
    let reopened:Study=serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
    assert!(recordings::dataset(&reopened,Some(&extra)).is_err());
    // Reverse the historical declaration in a new captured attempt, not its old receipt.
    let mut historical=study();
    let training=data::RecordingDataset::capture(std::slice::from_ref(&r),&[assignment(&r,Role::Train)]).unwrap();
    let dataset=data::CombinedDataset{archives:vec![historical.archive.clone()],recordings:Some(training)};
    let cases=data::CalibrationData::cases(&dataset).unwrap();
    let mut request=s.refinement.combined_fits[0].attempt.request.clone();
    request.training_ids=cases.iter().filter(|c|c.split=="train").map(|c|c.id.clone()).collect();
    request.validation_ids=cases.iter().filter(|c|c.split!="train").map(|c|c.id.clone()).collect();
    let attempt=cal::attempt(&dataset,&request,&std::sync::atomic::AtomicBool::new(true),|_,_|{});
    historical.refinement.combined_fits.push(data::CombinedFitAttempt{dataset,attempt});
    let mut held=study();shared::apply(&mut held,shared::Command::ImportRecording{recording:r.clone()}).unwrap();
    shared::apply(&mut held,shared::Command::AssignRecording{assignment:assignment(&r,Role::HeldOut)}).unwrap();
    recordings::reserve_additional(&mut historical,&held).unwrap();
    assert_eq!(recordings::frozen(&historical).unwrap()[&r.fingerprint()].role,Role::Train);
    assert!(historical.refinement_evidence.recording_held_out.contains(&r.fingerprint()));
    historical.validate().unwrap();
    let loaded:Study=serde_json::from_value(serde_json::to_value(historical).unwrap()).unwrap();loaded.validate().unwrap();
}
#[test]
fn recording_device_coordinates_are_authorable_but_archive_subsets_still_validate(){
    let mut s=study();let mut r=recording(true);
    let device=(1..=253).find(|d|!s.archive.trials.iter().any(|t|t.device==*d)).unwrap();
    r.experiment.device=device;
    shared::apply(&mut s,shared::Command::ImportRecording{recording:r}).unwrap();
    let resistance=s.draft.motor["resistance"];
    let coords=vec![crate::controller_refinement::calibration::Coordinate{path:"motor.resistance".into(),device:Some(device),lower:-resistance*0.5,upper:resistance*0.5}];
    shared::apply(&mut s,shared::Command::SetCoordinates(coords)).unwrap();
    let selected=vec![s.archive.trials[0].id.clone()];
    assert!(shared::prepare(&mut s,shared::Operation::Sensitivity{selected}).unwrap_err().contains("device"));
}
#[test]
fn historical_tuning_declaration_survives_new_heldout_quarantine_for_reopen(){
    use crate::controller_refinement::calibration_data as data;
    let mut s=study();let r=recording(true);
    shared::apply(&mut s,shared::Command::ImportRecording{recording:r.clone()}).unwrap();
    shared::apply(&mut s,shared::Command::AssignRecording{assignment:assignment(&r,Role::Train)}).unwrap();
    let mut extra=study();for t in &mut extra.archive.trials{t.id=format!("extra:{}",t.id);}
    shared::apply(&mut extra,shared::Command::ImportRecording{recording:r.clone()}).unwrap();
    shared::apply(&mut extra,shared::Command::AssignRecording{assignment:assignment(&r,Role::HeldOut)}).unwrap();
    recordings::reserve_additional(&mut s,&extra).unwrap();
    assert_eq!(recordings::frozen(&s).unwrap()[&r.fingerprint()].role,Role::Train);
    assert!(s.refinement_evidence.recording_held_out.contains(&r.fingerprint()));
    let loaded:Study=serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();loaded.validate().unwrap();
    let dataset=data::RecordingDataset::capture(&loaded.refinement.recordings,&loaded.refinement.recording_assignments).unwrap();
    assert!(recordings::request(&loaded,&dataset).unwrap_err().contains("held-out"));
    let mut inspection=loaded;shared::apply(&mut inspection,shared::Command::SelectRecording{recording_hash:r.fingerprint()}).unwrap();
}

#[test]
fn direct_combined_preparation_rejects_additional_quarantine_without_host_reservation(){
    let mut primary=study();let mut additional=study();
    for t in &mut additional.archive.trials{t.id=format!("additional:{}",t.id);}
    let r=recording(true);shared::apply(&mut additional,shared::Command::ImportRecording{recording:r.clone()}).unwrap();
    shared::apply(&mut additional,shared::Command::AssignRecording{assignment:assignment(&r,Role::Train)}).unwrap();
    let resistance=primary.draft.motor["resistance"];
    primary.refinement.coordinates=vec![crate::controller_refinement::calibration::Coordinate{path:"motor.resistance".into(),device:None,lower:resistance*0.5,upper:resistance*1.5}];
    assert!(shared::prepare(&mut primary,shared::Operation::FitCombined{additional:Some(Box::new(additional.clone()))}).is_ok());
    additional.refinement_evidence.recording_held_out.insert(r.fingerprint());
    additional.validate().unwrap(); // Historical Train is retained evidence, not artifact corruption.
    let reopened:Study=serde_json::from_value(serde_json::to_value(&additional).unwrap()).unwrap();
    let before=serde_json::to_value(&primary).unwrap();
    let error=shared::prepare(&mut primary,shared::Operation::FitCombined{additional:Some(Box::new(reopened))}).unwrap_err();
    assert!(error.contains("recording.additional.assignment")&&error.contains("held-out"));
    assert_eq!(before,serde_json::to_value(&primary).unwrap());
    assert_eq!(additional.refinement.recording_assignments[0].role,Role::Train);
    // Historical-only reservations are evidence, not cases joining this request.
    // Compatibility artifacts may retain them without a current assignment.
    additional.refinement.recording_assignments.clear();
    additional.validate().unwrap();
    let capture=shared::prepare(&mut primary,shared::Operation::FitCombined{additional:Some(Box::new(additional))}).unwrap();
    let shared::Operation::FitCombined{additional:Some(extra)}=&capture.operation else{panic!("captured additional study")};
    let dataset=recordings::dataset(&capture.study,Some(extra)).unwrap();
    assert!(dataset.recordings.is_none());
}
#[test]
fn immutable_failed_fit_case_selection_reopens_and_stale_source_is_transactional(){
    use crate::controller_refinement::{calibration as cal,calibration_data as data};
    let mut s=study();let r=recording(true);
    shared::apply(&mut s,shared::Command::ImportRecording{recording:r.clone()}).unwrap();
    shared::apply(&mut s,shared::Command::AssignRecording{assignment:assignment(&r,Role::HeldOut)}).unwrap();
    let dataset=data::RecordingDataset::capture(std::slice::from_ref(&r),&[assignment(&r,Role::HeldOut)]).unwrap();
    let attempt=cal::FitAttempt{request:cal::FitRequest{model:cal::Family{shared:s.draft.clone(),device_deltas:Default::default()},training_ids:vec![],validation_ids:vec![r.fingerprint()],coordinates:vec![],maximum_evaluations:40,validation_influenced:false},archive_hash:data::CalibrationData::fingerprint(&dataset),runtime:crate::physics_context::RuntimeIdentity::current(),evaluations:vec![],outcome:None,partial:None,failure:Some("Scoring cancelled".into()),cancelled:true};
    s.refinement.recording_fits.push(data::RecordingFitAttempt{dataset,attempt});recordings::cache_identities(&mut s);
    let selection=recordings::FitCaseSelection{kind:"recording_fit".into(),index:0,fit_blake3:recordings::fit_identity(&s,"recording_fit",0).unwrap().into(),case_id:r.fingerprint()};
    shared::apply(&mut s,shared::Command::SelectFitCase{selection:Some(selection.clone())}).unwrap();assert!(s.validation_seen);
    let review=recordings::fit_case_traces(&s,&selection).unwrap();assert!(!review.case.measured.samples.is_empty());assert!(review.baseline.is_none()&&review.candidate.is_none());
    let loaded:Study=serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();loaded.validate().unwrap();assert_eq!(loaded.refinement_evidence.selected_fit_case,Some(selection.clone()));
    let before=serde_json::to_value(&s).unwrap();let mut stale=selection;stale.fit_blake3="stale".into();assert!(shared::apply(&mut s,shared::Command::SelectFitCase{selection:Some(stale)}).unwrap_err().contains("fit_blake3"));assert_eq!(before,serde_json::to_value(&s).unwrap());
    shared::apply(&mut s,shared::Command::SelectFitCase{selection:None}).unwrap();assert!(s.refinement_evidence.selected_fit_case.is_none());
}

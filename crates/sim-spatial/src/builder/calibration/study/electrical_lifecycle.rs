//! T52 UNEXECUTED lifecycle fixtures, using finished adopted handles and real poll owner.
use super::{jobs::{self,JobKind,JobOutput,PendingJob},state::StudyOwner,recording_jobs};
use crate::{jobs::Job,document::DocumentRegistry};
use sim_runtime::experiment_study::{Study,refinement::{self,electrical}};
use serde_json::json;
fn owner()->StudyOwner {
    let root=crate::workspace::root().unwrap();
    let study=Study::new(sim_runtime::experiment_comparison::hx_archive::load(&root.join(super::super::DEFAULT_ARCHIVE),root).unwrap()).unwrap();
    let mut owner=StudyOwner::default();owner.retain(study,"source fixture".into(),None,false,true);owner
}
#[test]
fn rejected_sidecar_keeps_exact_unknown_bytes_on_original_study_after_switch_and_cancel(){
    for cancelled in [false,true] {
        let mut owner=owner();let stamp=owner.active().unwrap().stamp();let mut captured=owner.active().unwrap().study.clone();
        let bytes=b"{\"unknown_calibration\": [1,2], broken".to_vec();
        let (input,error)=recording_jobs::capture_electrical(&mut captured,"fixture-sidecar.json",7,Ok(bytes.clone()),cancelled).unwrap_err();
        let reference=input["content_ref"]["blake3"].as_str().unwrap().to_owned();
        let operation=refinement::Operation::Electrical(electrical::Operation::RejectedInput{prediction:7,source:"fixture-sidecar.json".into()});
        let output=recording_jobs::rejected_output(captured.clone(),operation,error,cancelled,Some(input));
        owner.pending.push(PendingJob{id:1,kind:JobKind::Refinement,stamp:Some(stamp),document:None,source:"fixture-sidecar.json".into(),trial_ids:vec![],launch:json!({"stamp":stamp}),cancel_requested:cancelled,job:Job::finished(1,Ok(output)),selection_epoch:owner.selection_epoch,gate:None,captured:Some(captured)});
        owner.get_mut(stamp.id).unwrap().revision+=1;
        let other=owner.retain(owner.get(stamp.id).unwrap().study.clone(),"other study".into(),None,false,true);
        jobs::poll_owner(&mut owner,&DocumentRegistry::default());
        assert_eq!(owner.active,Some(other));
        assert!(owner.receipts[0].stale);assert!(owner.receipts[0].error.is_some());
        let original=&owner.get(stamp.id).unwrap().study;
        assert_eq!(original.input_contents.resolve(&reference).unwrap(),bytes);
        assert!(original.refinement.electrical_comparisons.is_empty());
        assert!(original.refinement_evidence.receipts[0].failure.is_some());
        let serialized=serde_json::to_value(original).unwrap();
        let reopened:Study=serde_json::from_value(serialized).unwrap();
        assert!(reopened.input_contents.references.contains_key(&reference));
        assert!(owner.get(other).unwrap().study.refinement_evidence.receipts.is_empty());
    }
}
#[test]
fn cancelled_capture_has_recoverable_bytes_and_bounded_metadata_without_parsing(){
    let mut study=owner().active().unwrap().study.clone();let bytes=vec![0xff,0x00,0x42];
    let(input,error)=recording_jobs::capture_electrical(&mut study,"invalid-utf8.json",0,Ok(bytes.clone()),true).unwrap_err();
    assert!(error.contains("cancelled"));assert!(input.get("raw").is_none());
    assert_eq!(study.input_contents.resolve(input["content_ref"]["blake3"].as_str().unwrap()).unwrap(),bytes);
    let captured=jobs::prepare_publication_capture(&study,super::StudyStamp{id:1,revision:3},std::path::Path::new("unexecuted-review.json"),jobs::PublicationKind::Json,"fixture",None,Some(json!({"drafts":[{"text":"exact unsent electrical intent"}]})));
    assert_eq!(captured.input_contents,study.input_contents);
    assert!(captured.retained_fields["native_form_inputs"][0]["inputs"]["drafts"][0]["text"]=="exact unsent electrical intent");
}
#[test]
fn completed_electrical_run_cancelled_before_attachment_is_retained_without_passing_vector(){
    use sim_runtime::{controller_refinement::{control,power},physics_context::RuntimeIdentity};
    let mut owner=owner();let stamp=owner.active().unwrap().stamp();let captured=owner.active().unwrap().study.clone();
    let samples=vec![power::Sample{time_s:0.,supply_voltage_v:12.,supply_current_a:0.,winding_voltage_v:0.,winding_current_a:0.,supply_power_w:0.,winding_power_w:0.,state_of_charge:None},power::Sample{time_s:0.01,supply_voltage_v:12.,supply_current_a:0.,winding_voltage_v:0.,winding_current_a:0.,supply_power_w:0.,winding_power_w:0.,state_of_charge:None}];
    let limits=power::Limits{minimum_voltage_v:Some(9.),..Default::default()};
    let trace=power::Trace{summary:power::summarize(&samples,&limits).unwrap(),samples,limits,interpretation:"Synthetic sampled trace for terminal lifecycle only".into()};
    let run=control::Run{version:1,experiment:captured.refinement.experiment.clone(),model:captured.draft.clone(),runtime:RuntimeIdentity::current(),frames:vec![],truth:vec![],electrical:Some(trace),score:None,failure:None,cancelled:false,evidence_kind:"Unexecuted lifecycle fixture".into()};
    let capture=refinement::Capture{study:captured.clone(),operation:refinement::Operation::Simulate,runtime:sim_runtime::experiment_study::execution_identity(),captured_unix_ns:"0".into()};
    let expected=serde_json::to_vec(&refinement::ResultData::Controller(run.clone())).unwrap();
    let mut outcome=refinement::Outcome{capture,result:Ok(refinement::ResultData::Controller(run)),cancelled:false};
    refinement::terminal::capture_outcome(&mut outcome).unwrap(); // worker capture, before late cancellation
    let output=JobOutput::Refined{inputs:outcome.capture.inputs(),outcome};
    owner.pending.push(PendingJob{id:1,kind:JobKind::Refinement,stamp:Some(stamp),document:None,source:"fixture".into(),trial_ids:vec![],launch:json!({}),cancel_requested:true,job:Job::finished(1,Ok(output)),selection_epoch:owner.selection_epoch,gate:None,captured:Some(captured)});
    jobs::poll_owner(&mut owner,&DocumentRegistry::default());
    assert!(owner.active().unwrap().study.refinement.controller_runs.is_empty());
    assert!(owner.active().unwrap().study.refinement_evidence.receipts[0].cancelled);
    assert!(owner.receipts[0].cancelled&&owner.receipts[0].error.is_some());
    assert!(matches!(owner.receipts[0].refinement.as_ref().unwrap().result,Ok(refinement::ResultData::Controller(_))),"actual completed execution retained separately from applied review");
    let study=&owner.active().unwrap().study;
    let reference=&study.refinement_evidence.terminals[0];
    assert!(reference.cancelled && reference.unapplied && reference.unscored);
    assert_eq!(study.input_contents.resolve(&reference.content_ref.blake3).unwrap(),expected);
    let durable=owner.receipts[0].durable();
    assert_eq!(durable["terminal_result"]["content_ref"]["blake3"],reference.content_ref.blake3);
    assert!(durable["terminal_result"]["cancelled"].as_bool().unwrap());
    assert!(durable["terminal_result"].get("study").is_none());
    // Written publication/reopen fixture only. No execution in the T52 run.
    let directory=std::env::temp_dir().join(format!("native-electrical-terminal-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&directory).unwrap();let path=directory.join("review.json");
    study.save_new(&path).unwrap();let reopened=Study::load(&path).unwrap();
    assert!(reopened.refinement.controller_runs.is_empty());
    let retained=&reopened.refinement_evidence.terminals[0];
    assert_eq!(reopened.input_contents.resolve(&retained.content_ref.blake3).unwrap(),expected);
    assert!(matches!(refinement::terminal::cached(&reopened,retained),Some(refinement::ResultData::Controller(_))));
    assert!(retained.cancelled&&retained.unapplied&&retained.unscored);
    // T53 portable relocation uses the native captured publication adapter; no sibling content exists there.
    let relocated=directory.join("relocated");std::fs::create_dir(&relocated).unwrap();
    let portable=relocated.join("terminal.simstudy");
    let captured=jobs::prepare_publication_capture(study,owner.active().unwrap().stamp(),&portable,jobs::PublicationKind::Portable,"original source",None,Some(json!({"drafts":[{"text":"raw rejected source intent"}]})));
    jobs::publish_artifact(&captured,&portable,jobs::PublicationKind::Portable,&sim_runtime::publication::NoHooks).unwrap();
    assert!(!relocated.join(".study-inputs").exists());
    let reopened=Study::load(&portable).unwrap();
    let retained=&reopened.refinement_evidence.terminals[0];
    assert_eq!(reopened.input_contents.resolve(&retained.content_ref.blake3).unwrap(),expected);
    assert!(retained.cancelled&&retained.unapplied&&retained.unscored);
    assert!(matches!(refinement::terminal::cached(&reopened,retained),Some(refinement::ResultData::Controller(_))));
    assert_eq!(reopened.retained_fields["native_form_inputs"][0]["inputs"]["drafts"][0]["text"],"raw rejected source intent");
    // This fixture preserves immutable evidence; no cleanup deletes retained output.

}

#[test]
fn late_prediction_and_comparison_native_poll_preserve_exact_reopened_diagnostics(){
    // Written only: constructs shared outcomes, then exercises the actual retained
    // native poll owner with cancellation arriving after worker completion.
    use sim_runtime::controller_refinement::{control,power,recording::{self,MeasuredFrame}};
    use std::{collections::BTreeMap,sync::atomic::AtomicBool};
    fn deliver(study:Study,outcome:refinement::Outcome){
        let mut owner=StudyOwner::default();owner.retain(study,"terminal fixture".into(),None,false,true);
        let stamp=owner.active().unwrap().stamp();
        let predictions=owner.active().unwrap().study.refinement.predictions.len();
        let comparisons=owner.active().unwrap().study.refinement.electrical_comparisons.len();
        let exact=serde_json::to_vec(outcome.result.as_ref().unwrap()).unwrap();
        let output=JobOutput::Refined{inputs:outcome.capture.inputs(),outcome};
        let captured=owner.active().unwrap().study.clone();
        owner.pending.push(PendingJob{id:9,kind:JobKind::Refinement,stamp:Some(stamp),document:None,source:"late electrical terminal".into(),trial_ids:vec![],launch:json!({}),cancel_requested:true,job:Job::finished(9,Ok(output)),selection_epoch:owner.selection_epoch,gate:None,captured:Some(captured)});
        jobs::poll_owner(&mut owner,&DocumentRegistry::default());
        let retained=&owner.active().unwrap().study;
        assert_eq!(retained.refinement.predictions.len(),predictions);
        assert_eq!(retained.refinement.electrical_comparisons.len(),comparisons);
        let reference=retained.refinement_evidence.terminals.last().unwrap();
        assert!(reference.cancelled&&reference.unapplied&&reference.unscored);
        assert!(owner.receipts[0].durable()["terminal_result"]["cancelled"].as_bool().unwrap());
        assert_eq!(retained.input_contents.resolve(&reference.content_ref.blake3).unwrap(),exact);
        let directory=std::env::temp_dir().join(format!("native-late-prediction-comparison-{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&directory).unwrap();let path=directory.join("study.json");
        retained.save_new(&path).unwrap();let reopened=Study::load(&path).unwrap();
        assert_eq!(reopened.refinement.predictions.len(),predictions);
        assert_eq!(reopened.refinement.electrical_comparisons.len(),comparisons);
        let reference=reopened.refinement_evidence.terminals.last().unwrap();
        assert!(reference.cancelled&&reference.unapplied&&reference.unscored);
        assert_eq!(reopened.input_contents.resolve(&reference.content_ref.blake3).unwrap(),exact);
        assert!(refinement::terminal::cached(&reopened,reference).is_some());
    }
    let mut study=owner().active().unwrap().study.clone();
    refinement::apply(&mut study,refinement::Command::Electrical(electrical::Command::SetSource(Some(power::Setup{source_component:"electrical.voltage_source".into(),source_parameters:BTreeMap::from([("voltage".into(),12.)]),auxiliary_current_a:0.,evidence:"Written native lifecycle ideal source hypothesis".into(),limits:Default::default()})))).unwrap();
    let experiment=control::Experiment::default();let mut session=control::ControllerSession::new(experiment.clone()).unwrap();let mut frames=vec![];
    for time in [0.01,0.03]{
        let control=session.tick(time+0.005,control::Feedback{electrical:None,observed_s:time,request_s:time-0.001,completion_s:time+0.002,received_s:time+0.002,encoder_rad:0.1}).unwrap();
        let drive_counts=(control.applied_duty*1000.).round() as i16;
        frames.push(MeasuredFrame{control,command_request_s:time+0.006,command_receipt_s:time+0.01,drive_counts,voltage_v:12.,temperature_c:25.,current_raw_uncalibrated:0});
    }
    let recording=recording::Recording{version:1,experiment,runtime:sim_runtime::physics_context::RuntimeIdentity::current(),frames,stop_request_s:0.05,stop_receipt_s:0.06,completed:true,failure:None,stop_verified:true,initial_registers:json!({}),transactions_origin_host_s:0.,timing_evidence:"Synthetic observation windows; no hardware operation".into(),source_hashes:BTreeMap::from([("fixture".into(),"a".repeat(64))])};
    let hash=recording.fingerprint();refinement::apply(&mut study,refinement::Command::ImportRecording{recording}).unwrap();
    study.limits=Some(sim_runtime::experiment_comparison::Limits{rmse:1.,final_abs_error:1.});
    let cancel=AtomicBool::new(false);
    let capture=refinement::prepare(&mut study,refinement::Operation::PredictRecording{recording_hash:hash.clone(),purpose:recording::Purpose::RecordedCommandReplay}).unwrap();
    let prediction=refinement::execute(capture,&cancel,|_,_|{}).unwrap();
    deliver(study.clone(),prediction.clone());
    refinement::apply_outcome(&mut study,prediction);
    let capture=refinement::prepare(&mut study,refinement::Operation::Electrical(electrical::Operation::CompareServoVoltage{recording_hash:hash,prediction:0})).unwrap();
    let comparison=refinement::execute(capture,&cancel,|_,_|{}).unwrap();
    deliver(study,comparison);
}

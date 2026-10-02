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
    let captured=jobs::prepare_publication_capture(&study,super::StudyStamp{id:1,revision:3},std::path::Path::new("unexecuted-review.json"),false,"fixture",None,Some(json!({"drafts":[{"text":"exact unsent electrical intent"}]})));
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
    let output=JobOutput::Refined{inputs:capture.inputs(),outcome:refinement::Outcome{capture,result:Ok(refinement::ResultData::Controller(run)),cancelled:false}};
    owner.pending.push(PendingJob{id:1,kind:JobKind::Refinement,stamp:Some(stamp),document:None,source:"fixture".into(),trial_ids:vec![],launch:json!({}),cancel_requested:true,job:Job::finished(1,Ok(output)),selection_epoch:owner.selection_epoch,gate:None,captured:Some(captured)});
    jobs::poll_owner(&mut owner,&DocumentRegistry::default());
    assert!(owner.active().unwrap().study.refinement.controller_runs.is_empty());
    assert!(owner.active().unwrap().study.refinement_evidence.receipts[0].cancelled);
    assert!(owner.receipts[0].cancelled&&owner.receipts[0].error.is_some());
    assert!(matches!(owner.receipts[0].refinement.as_ref().unwrap().result,Ok(refinement::ResultData::Controller(_))),"actual completed execution retained separately from applied review");
}

//! T52 source-reviewed fixtures. Deliberately not executed in this batch.
use super::*;
fn study()->Study {
    let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    Study::new(crate::experiment_comparison::hx_archive::load(&root.join("examples/actuators/hx30hm/pwm-identification"),&root).unwrap()).unwrap()
}
#[test]
fn refusal_preserves_original_and_unknown_electrical_payload(){
    let mut s=study();
    s.refinement_evidence.electrical.retained_fields.insert("future_sensor".into(),serde_json::json!({"raw":"exact rejected input","polarity":-1}));
    let original=serde_json::to_value(&s).unwrap();
    assert!(command(&mut s,Command::SetVoltage(Some(-1.))).unwrap_err().contains("draft.conditions.voltage_v"));
    assert_eq!(original,serde_json::to_value(&s).unwrap());
    assert!(command(&mut s,Command::SelectPrediction(usize::MAX)).unwrap_err().contains("selected_prediction"));
    assert_eq!(original,serde_json::to_value(&s).unwrap());
    let reopened:Study=serde_json::from_value(original.clone()).unwrap();
    assert_eq!(original,serde_json::to_value(reopened).unwrap());
}
#[test]
fn missing_identity_is_refused_without_changing_study(){
    let s=study();let original=serde_json::to_value(&s).unwrap();
    let op=Operation::CompareServoVoltage{recording_hash:"f".repeat(64),prediction:0};
    assert!(prepare(&s,&op).unwrap_err().contains("recording_hash"));
    assert_eq!(original,serde_json::to_value(&s).unwrap());
    let cancel=AtomicBool::new(true);
    assert!(execute(&s,&op,&cancel).unwrap_err().contains("cancelled"));
}
#[test]
fn nonhex_identity_and_unmeasured_limit_refuse(){
    let m=measurements::Measurements{version:1,recording_hash:"z".repeat(64),source_hashes:BTreeMap::from([("meter".into(),"a".repeat(64))]),timing_evidence:"Captured synchronized clock windows".into(),channels:vec![],limits:BTreeMap::new()};
    assert!(m.validate().is_err());
    let mut m=m;m.recording_hash="b".repeat(64);
    m.channels.push(measurements::Channel{name:"supply_voltage".into(),calibration:measurements::Calibration{sensor:"Meter".into(),circuit_location:"Servo supply node".into(),raw_unit:"count".into(),gain:0.1,offset:0.,evidence:"Synthetic independently declared calibration".into(),uncertainty:"Synthetic 0.1 V resolution".into()},raw_samples:vec![crate::experiment_comparison::Observation{time_s:0.,request_s:0.,completion_s:0.,value:120.},crate::experiment_comparison::Observation{time_s:0.1,request_s:0.1,completion_s:0.1,value:120.}]});
    m.validate().unwrap();
    m.limits.insert("supply_current".into(),crate::experiment_comparison::Limits{rmse:1.,final_abs_error:1.});
    assert!(m.validate().unwrap_err().contains("unmeasured"));
}
#[test]
fn source_refusal_keeps_exact_parameters_and_provenance(){
    let mut s=study();let original=serde_json::to_value(&s).unwrap();
    let source=power::Setup{source_component:"unknown-future-source".into(),source_parameters:BTreeMap::from([("unrecognized".into(),42.)]),auxiliary_current_a:0.,evidence:"Retain this exact original evidence".into(),limits:Default::default()};
    let raw=serde_json::to_value(&source).unwrap();
    assert!(command(&mut s,Command::SetSource(Some(source.clone()))).unwrap_err().contains("draft.power"));
    assert_eq!(original,serde_json::to_value(&s).unwrap());assert_eq!(raw,serde_json::to_value(source).unwrap());
}
#[test]
fn unknown_authoring_keys_become_exact_rejected_command(){
    let raw=serde_json::json!({"SetController":{"sensing":{"voltage_quantum_v":0.1,"supply_current_quantum_a":null,"winding_current_quantum_a":null,"evidence":"retain provenance","future_polarity":-1},"nominal_voltage_for_compensation_v":null,"limits":{}}});
    let rejected:Command=serde_json::from_value(raw.clone()).unwrap();
    let Command::Rejected{raw:retained,error}=&rejected else{panic!("unknown sensing field was discarded")};
    assert_eq!(retained,&raw);assert!(error.contains("sensing.future_polarity"));
    let encoded=serde_json::to_value(&rejected).unwrap();
    let reopened:Command=serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(encoded,serde_json::to_value(&reopened).unwrap());
    let mut s=study();let before=serde_json::to_value(&s).unwrap();
    assert!(command(&mut s,reopened).is_err());assert_eq!(before,serde_json::to_value(s).unwrap());
}
#[test]
fn explicit_source_atomically_clears_legacy_voltage_and_rejects_non_electrical(){
    let mut s=study();s.draft.power=None;s.draft.conditions.voltage_v=Some(12.);
    let source=power::Setup{source_component:sim_domain_electrical::elements::VOLTAGE_SOURCE.into(),source_parameters:BTreeMap::from([("voltage".into(),12.)]),auxiliary_current_a:0.,evidence:"Explicit ideal source hypothesis".into(),limits:Default::default()};
    command(&mut s,Command::SetSource(Some(source))).unwrap();
    assert_eq!(s.draft.conditions.voltage_v,None);s.draft.validate().unwrap();
    let mut controller=power::Controller{sensing:power::Sensing{voltage_quantum_v:0.1,supply_current_quantum_a:None,winding_current_quantum_a:None,evidence:"Declared voltage-only sensing".into()},nominal_voltage_for_compensation_v:None,limits:Default::default()};
    controller.limits.maximum_draw_power_w=Some(10.);
    let original=serde_json::to_value(&s).unwrap();
    assert!(command(&mut s,Command::SetController(Some(controller))).unwrap_err().contains("electrical.limits"));
    assert_eq!(original,serde_json::to_value(&s).unwrap());
}
#[test]
fn derived_power_requires_same_location_and_exact_observation_windows(){
    let calibration=measurements::Calibration{sensor:"Calibrated fixture meter".into(),circuit_location:"Supply at servo terminals".into(),raw_unit:"count".into(),gain:0.1,offset:0.,evidence:"Declared calibration receipt".into(),uncertainty:"Synthetic calibration bound".into()};
    let observations=vec![crate::experiment_comparison::Observation{time_s:0.,request_s:0.,completion_s:0.,value:120.},crate::experiment_comparison::Observation{time_s:0.1,request_s:0.1,completion_s:0.1,value:110.}];
    let voltage=measurements::Channel{name:"supply_voltage".into(),calibration:calibration.clone(),raw_samples:observations.clone()};
    let mut current=measurements::Channel{name:"supply_current".into(),calibration,raw_samples:observations};current.calibration.gain=-0.01;
    let mut m=measurements::Measurements{version:1,recording_hash:"a".repeat(64),source_hashes:BTreeMap::from([("meter".into(),"b".repeat(64))]),timing_evidence:"Same explicit sample windows".into(),channels:vec![voltage,current],limits:BTreeMap::new()};
    let traces=m.traces().unwrap();assert!(traces["supply_power"].samples[0].value<0.);
    assert_eq!(m.channels[1].calibration.gain,-0.01);
    m.channels[1].calibration.circuit_location="Battery terminals".into();
    assert!(m.traces().unwrap_err().contains("circuit locations"));
    m.channels[1].calibration.circuit_location="Supply at servo terminals".into();m.channels[1].raw_samples[1].request_s=0.099;
    assert!(m.traces().unwrap_err().contains("synchronized"));
}
#[test]
fn deserialized_fabricated_or_cancelled_terminals_cannot_attach_scored_evidence(){
    use crate::experiment_comparison::{self,Observation,Trace,Limits};
    use crate::experiment_study::refinement as shared;
    let mut s=study();
    let trace=Trace{quantity:sim_core::QuantityKind::Angle.definition_id(),unit:"rad".into(),samples:vec![Observation{time_s:0.,request_s:0.,completion_s:0.,value:0.},Observation{time_s:0.1,request_s:0.1,completion_s:0.1,value:0.}]};
    let limits=Limits{rmse:1.,final_abs_error:1.};
    let prediction=recording::Prediction{purpose:recording::Purpose::RecordedCommandReplay,recording_hash:"a".repeat(64),model:s.draft.clone(),runtime:crate::physics_context::RuntimeIdentity::current(),measured:trace.clone(),predicted:trace.clone(),model_error:experiment_comparison::compare(&trace,&trace,&limits).unwrap(),limits,simulated_frames:vec![],measured_tracking:None,simulated_tracking:None,assumptions:"Fabricated fixture: never runtime-validated".into(),electrical:None,voltage_input:None};
    let m=measurements::Measurements{version:1,recording_hash:prediction.recording_hash.clone(),source_hashes:BTreeMap::new(),timing_evidence:String::new(),channels:vec![],limits:BTreeMap::new()};
    let fabricated=ResultData{evaluation:measurements::Evaluation{measurements:m,prediction_hash:measurements::prediction_hash(&prediction),channels:vec![],supply_energy:None,method:"fabricated passing summary".into()},prediction,validated:true,prediction_index:0,captured_unix_ns:"fixture".into()};
    let reopened:ResultData=serde_json::from_value(serde_json::to_value(&fabricated).unwrap()).unwrap();
    let operation=shared::Operation::Electrical(Operation::CompareServoVoltage{recording_hash:"a".repeat(64),prediction:0});
    for(result,cancelled)in[(reopened,false),(fabricated,true)]{
        let capture=shared::Capture{study:s.clone(),operation:operation.clone(),runtime:crate::experiment_study::execution_identity(),captured_unix_ns:"fixture".into()};
        shared::apply_outcome(&mut s,shared::Outcome{capture,result:Ok(shared::ResultData::Electrical(result)),cancelled});
        assert!(s.refinement.electrical_comparisons.is_empty());
        assert!(s.refinement_evidence.receipts.last().unwrap().failure.is_some());
    }
}
#[test]
fn captured_voltage_comparison_saves_reopens_and_rejects_changed_companion_identity(){
    use crate::{experiment_study::refinement as shared,controller_refinement::{control,recording::MeasuredFrame}};
    let mut s=study();
    command(&mut s,Command::SetSource(Some(power::Setup{source_component:"electrical.voltage_source".into(),source_parameters:BTreeMap::from([("voltage".into(),12.)]),auxiliary_current_a:0.,evidence:"Synthetic ideal supply for written capture fixture".into(),limits:Default::default()}))).unwrap();
    let experiment=control::Experiment::default();let mut session=control::ControllerSession::new(experiment.clone()).unwrap();
    let mut frames=vec![];
    for time in [0.01,0.03]{
        let control=session.tick(time+0.005,control::Feedback{electrical:None,observed_s:time,request_s:time-0.001,completion_s:time+0.002,received_s:time+0.002,encoder_rad:0.1}).unwrap();
        let drive_counts=(control.applied_duty*1000.).round() as i16;
        frames.push(MeasuredFrame{control,command_request_s:time+0.006,command_receipt_s:time+0.01,drive_counts,voltage_v:12.,temperature_c:25.,current_raw_uncalibrated:0});
    }
    let recording=recording::Recording{version:1,experiment,runtime:crate::physics_context::RuntimeIdentity::current(),frames,stop_request_s:0.05,stop_receipt_s:0.06,completed:true,failure:None,stop_verified:true,initial_registers:serde_json::json!({}),transactions_origin_host_s:0.,timing_evidence:"Synthetic host observation windows, no physical hardware claim".into(),source_hashes:BTreeMap::from([("fixture".into(),"a".repeat(64))])};
    let hash=recording.fingerprint();shared::apply(&mut s,shared::Command::ImportRecording{recording}).unwrap();
    s.limits=Some(crate::experiment_comparison::Limits{rmse:1.,final_abs_error:1.});
    let cancel=AtomicBool::new(false);
    let captured=shared::prepare(&mut s,shared::Operation::PredictRecording{recording_hash:hash.clone(),purpose:recording::Purpose::RecordedCommandReplay}).unwrap();
    let outcome=shared::execute(captured,&cancel,|_,_|{}).unwrap();assert!(outcome.result.is_ok());shared::apply_outcome(&mut s,outcome);
    let captured=shared::prepare(&mut s,shared::Operation::Electrical(Operation::CompareServoVoltage{recording_hash:hash,prediction:0})).unwrap();
    let outcome=shared::execute(captured,&cancel,|_,_|{}).unwrap();assert!(outcome.result.is_ok());shared::apply_outcome(&mut s,outcome);
    assert_eq!(s.refinement.electrical_comparisons.len(),1);
    assert!(s.refinement.electrical_comparisons[0].channels.iter().all(|c|c.passes.is_none()),"voltage-only registers have no declared measurement limits");
    shared::apply(&mut s,shared::Command::SetDecision{kind:"electrical".into(),index:0,decision:"investigating".into(),notes:"Synthetic review never promotes physical source".into()}).unwrap();
    let directory=std::env::temp_dir().join(format!("t52-electrical-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&directory).unwrap();let path=directory.join("study.json");
    s.save_new(&path).unwrap();let reopened=Study::load(&path).unwrap();
    assert_eq!(reopened.refinement.electrical_comparisons,s.refinement.electrical_comparisons);
    assert_eq!(reopened.input_contents,s.input_contents);assert_eq!(reopened.refinement_evidence.decisions[0].notes,s.refinement_evidence.decisions[0].notes);
    let mut changed=reopened;let index=changed.refinement_evidence.receipts.len()-1;
    changed.refinement_evidence.receipts[index].inputs["electrical"]["prediction_hash"]=serde_json::json!("b".repeat(64));
    assert!(changed.validate().unwrap_err().contains("identity changed"));
    // Cleanup is only part of this UNEXECUTED isolated fixture, never retained user data.
    std::fs::remove_dir_all(directory).unwrap();
}

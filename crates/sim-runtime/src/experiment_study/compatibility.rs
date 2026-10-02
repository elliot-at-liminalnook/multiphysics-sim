//! Written compatibility fixtures; not executed in T46.
use super::*;
fn fixture() -> Study {
    let root=Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    Study::new(crate::experiment_comparison::hx_archive::load(&root.join("examples/actuators/hx30hm/pwm-identification"),&root).unwrap()).unwrap()
}
#[test]
fn unknown_deferred_payload_and_saved_schema_round_trip() {
    let study=fixture();
    let mut value=serde_json::to_value(&study).unwrap();
    value["future_review"] = serde_json::json!({"opaque":[1,"kept"]});
    value["refinement"]["future_controller_payload"] = serde_json::json!({"unknown":{"bytes":[4,9]}});
    let loaded:Study=serde_json::from_value(value.clone()).unwrap();
    loaded.validate().unwrap();
    let saved=serde_json::to_value(loaded).unwrap();
    assert_eq!(saved["future_review"],value["future_review"]);
    assert_eq!(saved["refinement"]["future_controller_payload"],value["refinement"]["future_controller_payload"]);
    assert_eq!(saved["baseline_source"],value["baseline_source"]);
}
#[test]
fn exposure_is_monotonic_and_refused_commands_are_transactional() {
    let mut s=fixture();
    commands::apply(&mut s,commands::Command::Expose(commands::EvaluationSelection::HeldOut)).unwrap();
    assert!(s.validation_seen);
    let original=serde_json::to_value(&s).unwrap();
    assert!(commands::apply(&mut s,commands::Command::SetStep(f64::NAN)).unwrap_err().contains("step_s"));
    assert_eq!(serde_json::to_value(&s).unwrap(),original);
    let step=s.draft.step_s*0.5;
    commands::apply(&mut s,commands::Command::SetStep(step)).unwrap();
    assert!(s.validation_influenced);
    let reopened:Study=serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
    assert!(reopened.validation_seen && reopened.validation_influenced);
    assert!(commands::trial_ids(&s,commands::EvaluationSelection::Ids(vec![s.archive.trials[0].id.clone();2])).is_err());
}
#[test]
fn cancellation_retains_exact_unscored_trial_set_and_capture() {
    let s=fixture();let ids=commands::trial_ids(&s,commands::EvaluationSelection::HeldOut).unwrap();
    let e=evaluate(&s.archive,&ids,&s.baseline,&s.draft,None,false,&AtomicBool::new(true),|_,_|{}).unwrap();
    assert_eq!(e.results.iter().map(|r|r.trial_id.clone()).collect::<Vec<_>>(),ids);
    assert_eq!(e.capture["trial_ids"],serde_json::json!(ids));
    assert_eq!(e.summary(&ids).unscored,ids.len());
    assert_eq!(e.summary(&ids).passes,0);
    let mut saved=s.clone();saved.evaluations.push(e);
    let reopened:Study=serde_json::from_value(serde_json::to_value(saved).unwrap()).unwrap();
    reopened.validate().unwrap();assert!(reopened.evaluations[0].cancelled);
}
#[test]
fn registry_paths_and_limits_refuse_nonfinite_values() {
    let mut s=fixture();
    assert!(commands::metadata().iter().any(|p|p.name=="resistance" && !p.unit.is_empty()));
    assert!(commands::apply(&mut s,commands::Command::SetParameter{group:commands::ParameterGroup::Motor,name:"invented".into(),value:1.}).is_err());
    assert!(commands::apply(&mut s,commands::Command::SetLimits(Some(Limits{rmse:-1.,final_abs_error:0.}))).unwrap_err().contains("limits.rmse"));
}

#[test]
fn candidate_only_success_is_not_a_successful_pair() {
    let s=fixture();let ids=vec![s.archive.trials[0].id.clone()];
    let mut e=evaluate(&s.archive,&ids,&s.baseline,&s.draft,None,false,&AtomicBool::new(true),|_,_|{}).unwrap();
    let t=&s.archive.trials[0];
    e.results[0].candidate=Some(Prediction{trace:t.predicted.clone(),metrics:t.comparison.clone()});
    assert_eq!(e.summary(&ids).unscored,1);assert_eq!(e.summary(&ids).passes,0);assert_eq!(e.summary(&ids).failures,0);
}
#[test]
fn immutable_destination_conflict_preserves_bytes() {
    let nonce=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let root=std::env::temp_dir().join(format!("t46-study-fixture-{}-{nonce}",std::process::id()));
    std::fs::create_dir(&root).unwrap();let path=root.join("review.json");
    std::fs::write(&path,b"preexisting evidence").unwrap();
    assert!(fixture().save_new(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(),b"preexisting evidence");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn html_outcomes_filters_and_summary_share_incomplete_pair_rule() {
    let mut s=fixture();let t=s.archive.trials.iter().find(|t|t.comparison.passes).unwrap().clone();let ids=vec![t.id.clone()];
    let mut e=evaluate(&s.archive,&ids,&s.baseline,&s.draft,None,false,&AtomicBool::new(true),|_,_|{}).unwrap();
    let prediction=Prediction{trace:t.predicted.clone(),metrics:t.comparison.clone()};
    assert!(prediction.metrics.passes);
    e.results[0].errors.clear();
    e.results[0].candidate=Some(prediction.clone());
    assert_eq!(e.results[0].outcome(),TrialOutcome::Unscored);
    s.evaluations.push(e);s.view.evaluation=Some(0);s.view.outcome="Pass".into();
    assert!(!commands::filtered_ids(&s).contains(&t.id));
    s.view.outcome="Unscored".into();assert!(commands::filtered_ids(&s).contains(&t.id));
    let html=s.render_html().unwrap();let needle=format!("<tr><td>{}<br>",t.id);let row=html.split(needle.as_str()).nth(1).unwrap().split("</tr>").next().unwrap();
    assert!(row.contains("<td>UNSCORED</td>"));assert!(row.contains(&format!("{:.6}",prediction.metrics.rmse)));
    s.evaluations[0].results[0].baseline=Some(prediction);
    s.evaluations[0].results[0].errors.push("Baseline worker failed after producing a trace".into());
    assert_eq!(s.evaluations[0].results[0].outcome(),TrialOutcome::Unscored,"errors still prohibit score despite complete traces");
    assert_eq!(s.evaluations[0].summary(&ids).unscored,1);
    let html=s.render_html().unwrap();let needle=format!("<tr><td>{}<br>",t.id);let row=html.split(needle.as_str()).nth(1).unwrap().split("</tr>").next().unwrap();
    assert!(row.contains("<td>UNSCORED</td>"));
    s.evaluations[0].results[0].errors.clear();
    assert!(s.evaluations[0].results[0].outcome().is_scored());
}

// T48 source-reading fixtures; deliberately unexecuted in this batch.
#[test]
fn refinement_mutations_are_transactional_path_named_and_opaque_compatible() {
    use super::refinement as r;
    let mut s=fixture();s.refinement.retained_fields.insert("future_fpga".into(),serde_json::json!({"opaque":[3,8]}));
    let before=serde_json::to_value(&s).unwrap();
    let mut e=s.refinement.experiment.clone();e.trajectory[1].time_s=0.;
    assert!(r::apply(&mut s,r::Command::SetExperiment(e)).unwrap_err().contains("trajectory.1.time_s"));
    assert_eq!(before,serde_json::to_value(&s).unwrap());
    assert!(r::apply(&mut s,r::Command::UseFit{fit:999,device:None}).unwrap_err().contains("use_fit.fit"));
    assert_eq!(before,serde_json::to_value(&s).unwrap());
    r::apply(&mut s,r::Command::SetCoordinates(vec![])).unwrap();
    r::apply(&mut s,r::Command::SetScenarios(vec![])).unwrap();
    let reopened:Study=serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
    assert_eq!(reopened.refinement.retained_fields,s.refinement.retained_fields);
    assert!(reopened.refinement_evidence.receipts.is_empty());
}
#[test]
fn refinement_capture_freezes_inputs_and_failed_completion_survives_reopen() {
    use super::refinement as r;
    let mut s=fixture();let c=r::prepare(&mut s,r::Operation::Robustness).unwrap();
    assert_eq!(c.study.refinement.scenarios.len(),4);
    assert_eq!(c.total(),4);
    let frozen=c.inputs();s.refinement.experiment.name="Later authored revision".into();
    r::apply_outcome(&mut s,r::Outcome{capture:c,result:Err("Cancelled before execution".into()),cancelled:true});
    let reopened:Study=serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
    assert_eq!(reopened.refinement_evidence.receipts[0].inputs,frozen);
    assert!(reopened.refinement_evidence.receipts[0].cancelled);
    assert_eq!(reopened.refinement_evidence.receipts[0].result_index,None);
    assert_eq!(reopened.refinement.experiment.name,"Later authored revision");
    assert!(reopened.refinement_evidence.receipts[0].inputs.get("refinement_evidence").is_none());
}
#[test]
fn refinement_exposure_precedes_later_edits_but_identical_submission_is_not_influence() {
    use super::refinement as r;
    let mut s=fixture();let held=commands::trial_ids(&s,commands::EvaluationSelection::HeldOut).unwrap();
    s.refinement.coordinates=vec![crate::controller_refinement::calibration::Coordinate{path:"condition.load_inertia".into(),device:None,lower:s.draft.conditions.load_inertia*0.5,upper:s.draft.conditions.load_inertia*1.5}];
    let c=r::prepare(&mut s,r::Operation::Sensitivity{selected:held}).unwrap();
    assert!(s.validation_seen && c.study.validation_seen);
    let same=s.refinement.experiment.clone();r::apply(&mut s,r::Command::SetExperiment(same)).unwrap();
    assert!(!s.validation_influenced);
    let mut e=s.refinement.experiment.clone();e.name="Exploratory revision after exposure".into();r::apply(&mut s,r::Command::SetExperiment(e)).unwrap();
    assert!(s.validation_seen && s.validation_influenced);
}
#[test]
fn refinement_fit_refuses_frozen_split_misuse_without_exposure() {
    use super::refinement as r;
    let mut s=fixture();let held=commands::trial_ids(&s,commands::EvaluationSelection::HeldOut).unwrap();
    let before=serde_json::to_value(&s).unwrap();
    assert!(r::prepare(&mut s,r::Operation::Fit{train:held.clone(),validation:held}).unwrap_err().contains("fit.train"));
    assert_eq!(serde_json::to_value(&s).unwrap(),before);
}
#[test]
fn late_cancel_request_after_final_controller_progress_does_not_cancel_execution() {
    use super::refinement as r;
    let mut s=fixture();let c=r::prepare(&mut s,r::Operation::Simulate).unwrap();
    let cancel=AtomicBool::new(false);
    let outcome=r::execute(c,&cancel,|done,total|if done==total {cancel.store(true,std::sync::atomic::Ordering::Relaxed)}).unwrap();
    assert!(!outcome.cancelled);
    match outcome.result {Ok(r::ResultData::Controller(run))=>{assert!(!run.cancelled);run.validate().unwrap();},other=>panic!("expected completed controller, got {other:?}")}
}
#[test]
fn static_rhai_authoring_does_not_run_top_level_code_and_names_source_errors() {
    use crate::controller_refinement::control::Policy;
    use super::refinement as r;
    let mut s=fixture();let mut e=s.refinement.experiment.clone();
    e.controller=Policy::Rhai{source:"throw \"must not execute during authoring\"; fn control(t,s,a,state) { a.duty=0.0; }".into(),parameters:serde_json::json!({}),duty_limit:0.2};
    r::apply(&mut s,r::Command::SetExperiment(e.clone())).unwrap();
    e.controller=Policy::Rhai{source:"fn wrong(t) { }".into(),parameters:serde_json::json!({}),duty_limit:0.2};
    let before=serde_json::to_value(&s).unwrap();
    assert!(r::apply(&mut s,r::Command::SetExperiment(e)).unwrap_err().contains("controller.source"));
    assert_eq!(serde_json::to_value(&s).unwrap(),before);
}
#[test]
fn refinement_archive_selections_are_durable_transactional_and_expose_before_later_edit() {
    use super::refinement as r;
    let mut s=fixture();let held=commands::trial_ids(&s,commands::EvaluationSelection::HeldOut).unwrap();
    let before=serde_json::to_value(&s).unwrap();
    assert!(r::apply(&mut s,r::Command::SetSelection{kind:"train".into(),ids:held.clone()}).unwrap_err().contains("selection.train"));
    assert_eq!(serde_json::to_value(&s).unwrap(),before);
    r::apply(&mut s,r::Command::SetSelection{kind:"validation".into(),ids:held.clone()}).unwrap();
    assert!(s.validation_seen);
    assert!(!s.validation_influenced);
    let reopened:Study=serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
    assert_eq!(reopened.refinement_evidence.selections["validation"],held);
    let mut e=s.refinement.experiment.clone();e.name="After selecting held-out evidence".into();r::apply(&mut s,r::Command::SetExperiment(e)).unwrap();
    assert!(s.validation_influenced);
}
#[test]
fn final_held_out_prediction_cancellation_preserves_unscored_partial_fit() {
    use crate::controller_refinement::{calibration as c,calibration_data::{CalibrationData,Case}};
    struct CancelFinal {archive:Archive,held:String,calls:std::sync::atomic::AtomicUsize}
    impl CalibrationData for CancelFinal {
        fn cases(&self)->Result<Vec<Case>,String>{self.archive.cases()}
        fn fingerprint(&self)->String{self.archive.fingerprint()}
        fn predict(&self,id:&str,_model:&ModelSettings,cancel:&AtomicBool)->Result<Trace,String>{
            if id==self.held && self.calls.fetch_add(1,std::sync::atomic::Ordering::Relaxed)==1 {
                cancel.store(true,std::sync::atomic::Ordering::Relaxed);
                return Err("Cancelled during final held-out candidate prediction".into());
            }
            Ok(self.archive.trials.iter().find(|t|t.id==id).unwrap().measured.clone())
        }
    }
    let s=fixture();let train=s.archive.trials.iter().find(|t|t.split=="train").unwrap().id.clone();
    let held=s.archive.trials.iter().find(|t|t.split!="train").unwrap().id.clone();
    let data=CancelFinal{archive:s.archive.clone(),held:held.clone(),calls:std::sync::atomic::AtomicUsize::new(0)};
    let request=c::FitRequest{model:c::Family{shared:s.draft.clone(),device_deltas:Default::default()},training_ids:vec![train],validation_ids:vec![held],coordinates:vec![c::Coordinate{path:"condition.load_inertia".into(),device:None,lower:s.draft.conditions.load_inertia*0.5,upper:s.draft.conditions.load_inertia*1.5}],maximum_evaluations:40,validation_influenced:false};
    let attempt=c::attempt(&data,&request,&AtomicBool::new(false),|_,_|{});
    assert!(attempt.cancelled && attempt.failure.is_some());assert!(attempt.outcome.is_none());
    let partial=attempt.partial.as_ref().unwrap();assert_eq!(partial.scores.len(),2);
    let last=partial.scores.last().unwrap();assert!(last.failure.is_some());assert!(last.candidate.is_none());assert!(last.baseline.is_some());
    attempt.validate(&data).unwrap();
    let reopened:c::FitAttempt=serde_json::from_value(serde_json::to_value(attempt).unwrap()).unwrap();
    reopened.validate(&data).unwrap();assert!(reopened.cancelled && reopened.outcome.is_none() && reopened.partial.is_some());
}

// Synthetic captured trace fixture; unexecuted and not measured qualification evidence.
#[test]
fn explicit_fit_use_keeps_source_identity_and_exposure_separate_from_review() {
    use super::refinement as r;
    use crate::controller_refinement::calibration::{Family,Coordinate,Fit,TrialScore};
    let mut s=fixture();let device=s.refinement.experiment.device;
    let train=s.archive.trials.iter().find(|t|t.device==device&&t.split=="train").unwrap().clone();
    let held=s.archive.trials.iter().find(|t|t.device==device&&t.split!="train").unwrap().clone();
    let baseline=Family{shared:s.draft.clone(),device_deltas:Default::default()};
    let coordinate=Coordinate{path:"condition.load_torque".into(),device:None,lower:-1.,upper:1.};
    let mut candidate=baseline.clone();candidate.shared.conditions.load_torque=0.02;
    let scores=[&train,&held].into_iter().map(|t| {
        let metrics=compare(&t.measured,&t.measured,&t.limits).unwrap();
        TrialScore{id:t.id.clone(),device:t.device,split:t.split.clone(),baseline:Some(metrics.clone()),candidate:Some(metrics),baseline_prediction:Some(t.measured.clone()),candidate_prediction:Some(t.measured.clone()),failure:None}
    }).collect();
    s.refinement.fits.push(Fit{baseline,candidate,coordinates:vec![coordinate],training_ids:vec![train.id],validation_ids:vec![held.id],optimizer:serde_json::json!({"values":[0.02]}),scores,validation_influenced:false,runtime:RuntimeIdentity::current(),status:"synthetic unexecuted captured fit".into()});
    let original=s.draft.clone();
    r::apply(&mut s,r::Command::SetDecision{kind:"fit".into(),index:0,decision:"reviewed".into(),notes:"review does not adopt".into()}).unwrap();
    assert_eq!(s.draft,original);
    r::apply(&mut s,r::Command::UseFit{fit:0,device:Some(device)}).unwrap();
    assert_ne!(s.draft,original);assert!(s.validation_seen&&s.validation_influenced);
    assert_eq!(s.refinement_evidence.candidate_uses.len(),1);
    assert_eq!(s.refinement_evidence.decisions.len(),1);
    let reopened:Study=serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
    assert_eq!(reopened.refinement_evidence.candidate_uses[0].draft_blake3,reopened.draft.fingerprint());
    assert!(!reopened.refinement_evidence.candidate_uses[0].fit_blake3.is_empty());
    assert_eq!(reopened.baseline,s.baseline);
}
#[test]
fn saved_refinement_evidence_rejects_invalid_roles_indexes_and_source_links() {
    use super::refinement as r;
    let mut s=fixture();let held=commands::trial_ids(&s,commands::EvaluationSelection::HeldOut).unwrap();
    s.refinement_evidence.selections.insert("train".into(),held);
    assert!(s.validate().unwrap_err().contains("refinement_evidence.selections.train"));
    s.refinement_evidence.selections.clear();s.refinement_evidence.controller_run=Some(999);
    assert!(s.validate().unwrap_err().contains("refinement_evidence.controller_run"));
    s.refinement_evidence.controller_run=None;s.refinement_evidence.decisions.push(r::Decision{kind:"fit".into(),index:999,decision:"Reviewed".into(),notes:String::new()});
    assert!(s.validate().unwrap_err().contains("refinement_evidence.decisions.0.index"));
    s.refinement_evidence.decisions.clear();s.refinement_evidence.candidate_uses.push(r::CandidateUse{fit:999,fit_blake3:"unknown".into(),device:Some(4),validation_seen:true,validation_influenced:true,draft_blake3:s.draft.fingerprint()});
    assert!(s.validate().unwrap_err().contains("refinement_evidence.candidate_uses.0.fit"));
}
#[test]
fn device_coordinates_require_archive_and_selected_trial_applicability() {
    use super::refinement as r;
    use crate::controller_refinement::calibration::Coordinate;
    let mut s=fixture();let absent=(1..=253).find(|d|s.archive.trials.iter().all(|t|t.device!=*d)).unwrap();
    let coordinate=Coordinate{path:"condition.load_inertia".into(),device:Some(absent),lower:-0.00001,upper:0.00001};
    let before=serde_json::to_value(&s).unwrap();
    assert!(r::apply(&mut s,r::Command::SetCoordinates(vec![coordinate])).unwrap_err().contains("coordinates.0.device"));
    assert_eq!(before,serde_json::to_value(&s).unwrap());
    let first=&s.archive.trials[0];let other=s.archive.trials.iter().find(|t|t.device!=first.device).unwrap();
    let selected=vec![first.id.clone()];let device=other.device;
    r::apply(&mut s,r::Command::SetCoordinates(vec![Coordinate{path:"condition.load_inertia".into(),device:Some(device),lower:-0.00001,upper:0.00001}])).unwrap();
    assert!(r::prepare(&mut s,r::Operation::Sensitivity{selected}).unwrap_err().contains("coordinates.0.device"));
}

#[test]
fn refinement_report_keeps_escaped_review_and_native_receipt_evidence() {
    let mut s=fixture();
    s.refinement_evidence.retained_fields.insert("source_note".into(),serde_json::json!("<script>unexecuted</script>"));
    s.retained_fields.insert("native_publication_captures".into(),serde_json::json!([{"destination":"new immutable review","status":"prepared capture"}]));
    let html=s.render_html().unwrap();
    assert!(html.contains("refinement_evidence"));assert!(html.contains("native_publication_captures"));
    assert!(html.contains("&lt;script&gt;unexecuted&lt;/script&gt;"));assert!(!html.contains("<script>unexecuted</script>"));
}

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

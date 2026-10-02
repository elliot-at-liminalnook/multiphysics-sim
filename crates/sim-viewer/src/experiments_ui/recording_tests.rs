//! Actual legacy consumers; written unexecuted source fixtures.
use super::*;
    // T50 actual compatibility consumers; written and deliberately unexecuted.
    #[test]
    fn legacy_import_and_both_prediction_actions_delegate_shared_capture() {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let archive=sim_runtime::experiment_comparison::hx_archive::load(&root.join("examples/actuators/hx30hm/pwm-identification"),&root).unwrap();
        let mut s=Study::new(archive).unwrap();
        let path=root.join("examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement/controller-id4/recording.json");
        let Outcome::Recording(r)=Action::Import(path.display().to_string()).run(s.clone(),&AtomicBool::new(false),|_,_|{}).unwrap() else {panic!("controller classification")};
        use sim_runtime::experiment_study::refinement::{self as shared,Command,Operation};
        shared::apply(&mut s,Command::ImportRecording{recording:r.clone()}).unwrap();
        shared::apply(&mut s,Command::ImportRecording{recording:r.clone()}).unwrap();
        assert_eq!(s.refinement.recordings.len(),1,"duplicate import preserves frozen source identity");
        s.limits=Some(sim_runtime::experiment_comparison::Limits{rmse:0.1,final_abs_error:0.1});
        for purpose in [recording::Purpose::RecordedCommandReplay,recording::Purpose::ClosedLoopPrediction] {
            let Outcome::Shared(outcome)=Action::Predict(0,purpose).run(s.clone(),&AtomicBool::new(true),|_,_|{}).unwrap() else {panic!("shared execution owner")};
            assert!(matches!(outcome.capture.operation,Operation::PredictRecording{purpose:p,..} if p==purpose));
            shared::apply_outcome(&mut s,outcome);
            assert!(s.refinement_evidence.receipts.last().unwrap().cancelled);
        }
    }
    #[test]
    fn legacy_configure_refuses_frozen_assignment_replacement_transactionally() {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let archive=sim_runtime::experiment_comparison::hx_archive::load(&root.join("examples/actuators/hx30hm/pwm-identification"),&root).unwrap();
        let mut s=Study::new(archive).unwrap();
        let r:recording::Recording=serde_json::from_slice(&std::fs::read(root.join("examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement/controller-id4/recording.json")).unwrap()).unwrap();
        use sim_runtime::experiment_study::refinement::{self as shared,Command};
        shared::apply(&mut s,Command::ImportRecording{recording:r.clone()}).unwrap();
        shared::apply(&mut s,Command::AssignRecording{assignment:data::Assignment{recording_hash:r.fingerprint(),role:data::Role::HeldOut,limits:sim_runtime::experiment_comparison::Limits{rmse:0.1,final_abs_error:0.1},rationale:"reserve whole run".into()}}).unwrap();
        let before=serde_json::to_value(&s).unwrap();
        let mut panel=super::super::ExperimentsPanel::default();
        panel.studies.push(s);panel.revisions.push(0);panel.saved_revisions.push(0);
        let outcome=panel.api_command(super::super::rest::Command::Configure{fields:serde_json::json!({"recording_assignments":[]})},&mut serde_json::Value::Null,&egui::Context::default());
        assert!(matches!(outcome,sim_api::Outcome::Done(Err(ref e)) if e.contains("configure.recording_assignments")));
        // Rejection metadata is retained; source assignments and context never change.
        assert_eq!(serde_json::to_value(&panel.studies[0].refinement.recording_assignments).unwrap(),before["refinement"]["recording_assignments"]);
        assert_eq!(serde_json::to_value(&panel.studies[0].refinement.capture_contexts).unwrap(),before["refinement"]["capture_contexts"]);
    }
#[test]
fn legacy_combined_requested_unknown_ids_are_refused_before_subset_changes(){
    let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let archive=sim_runtime::experiment_comparison::hx_archive::load(&root.join("examples/actuators/hx30hm/pwm-identification"),&root).unwrap();
    let s=Study::new(archive).unwrap();
    let error=match (Action::FitCombined{selected:vec![s.archive.trials[0].id.clone(),"unknown whole trial".into()],additional_study:String::new()}).run(s,&AtomicBool::new(true),|_,_|{}) {
        Err(error)=>error,Ok(_)=>panic!("unknown source identity cannot disappear"),
    };
    assert!(error.contains("refinement.fit_combined.selected")&&error.contains("unknown"));
}

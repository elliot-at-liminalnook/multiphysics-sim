//! Written source-review fixtures; never executed in the T52 repair batch.
use super::*;
use crate::controller_refinement::{control, power};

fn incomplete_run(s: &Study) -> control::Run {
    let mut model = s.draft.clone();
    model.conditions.voltage_v=None;
    let limits=power::Limits{minimum_voltage_v:Some(10.),..Default::default()};
    model.power=Some(power::Setup{source_component:"electrical.voltage_source".into(),source_parameters:BTreeMap::from([("voltage".into(),12.)]),auxiliary_current_a:0.,evidence:"Written ideal-source diagnostic fixture".into(),limits:limits.clone()});
    let electrical=power::Trace::new(vec![power::Sample{time_s:0.,supply_voltage_v:12.,supply_current_a:0.,winding_voltage_v:0.,winding_current_a:0.,supply_power_w:0.,winding_power_w:0.,state_of_charge:None}],limits).unwrap();
    assert_eq!(electrical.summary.passes,Some(true));
    control::Run{version:1,experiment:control::Experiment::default(),model,runtime:crate::physics_context::RuntimeIdentity::current(),frames:vec![],truth:vec![[0.,0.]],electrical:Some(electrical),score:None,failure:Some("Retained incomplete diagnostic interval".into()),cancelled:false,evidence_kind:"simulation_only".into()}
}
#[test]
fn incomplete_sampled_summary_is_diagnostic_not_passing_report(){
    let mut s=super::super::input_content_fixtures::study();
    let run=incomplete_run(&s);run.validate().unwrap();
    assert_eq!(electrical_acceptance(&run),"UNSCORED (cancelled, failed or incomplete)");
    s.refinement.controller_runs.push(run);
    let report=s.render_html().unwrap();
    assert!(report.contains("Electrical limits: UNSCORED"));
    assert!(report.contains("Cancelled: false; complete: false"));
    assert!(report.contains("Voltage 12.0000–12.0000 V"));
    assert!(!report.contains("Electrical limits: PASS"));
}
#[test]
fn cancelled_unapplied_terminal_reopens_with_exact_unscored_diagnostics(){
    use crate::experiment_study::refinement::{Capture,Operation,Outcome,ResultData};
    let mut s=super::super::input_content_fixtures::study();
    let mut run=incomplete_run(&s);run.cancelled=true;
    let exact=serde_json::to_vec(&ResultData::Controller(run.clone())).unwrap();
    let mut outcome=Outcome{capture:Capture{study:s.clone(),operation:Operation::Simulate,runtime:execution_identity(),captured_unix_ns:"123".into()},result:Ok(ResultData::Controller(run)),cancelled:true};
    terminal::capture_outcome(&mut outcome).unwrap();
    let inputs=outcome.capture.inputs();
    refinement::apply_outcome_with_inputs(&mut s,outcome,inputs);
    assert!(s.refinement.controller_runs.is_empty());
    let reference=&s.refinement_evidence.terminals[0];
    assert_eq!(s.input_contents.resolve(&reference.content_ref.blake3).unwrap(),exact);
    let dir=super::super::input_content_fixtures::directory();let path=dir.join("cancelled-terminal.json");
    s.save_new(&path).unwrap();
    let reopened=Study::load(&path).unwrap();
    assert_eq!(reopened.input_contents.resolve(&reference.content_ref.blake3).unwrap(),exact);
    let report=reopened.render_html().unwrap();
    assert!(report.contains("Retained unapplied electrical terminal — UNSCORED"));
    assert!(report.contains("Retained incomplete diagnostic interval"));
    assert!(report.contains("supply_voltage_v"));
    assert!(reopened.refinement.controller_runs.is_empty());
}

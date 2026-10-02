//! UNEXECUTED actual renderer fixtures. Parent ui_tests supplies the real panel,
//! Study owner, fonts and collector; these are not a parallel control catalogue.
use super::*;
use sim_runtime::experiment_study::refinement::{Command as RefineCommand,electrical};
pub(super) fn electrical_owner()->StudyOwner{
    let mut o=owner();let s=&mut o.studies[0].study;
    s.draft.power=Some(super::super::super::forms::electrical_forms::source_preset(s,true));
    s.refinement.experiment.electrical=Some(super::super::super::forms::electrical_forms::controller_preset());o
}
#[test]
fn actual_electrical_controls_and_scalar_fields_share_stamped_contract(){
    let mut w=world(electrical_owner());w.run_system_once(render).unwrap();w.run_system_once(collect).unwrap();
    let ui=w.resource::<StudyUi>();
    for id in["study:electrical:preset:false","study:electrical:preset:true","study:electrical:source:remove","study:electrical:controller:remove","study:electrical:simulate"]{assert!(ui.rendered[id].enabled,"actual rendered {id}");}
    let Hit::Action(StudyAction::RefineApply{stamp,command:RefineCommand::Electrical(electrical::Command::SetSource(Some(source)))})=super::super::super::forms::activate(ui,"study:electrical:preset:true").unwrap()else{panic!("shared source command")};
    assert_eq!(stamp,StudyStamp{id:11,revision:4});assert!(source.evidence.contains("Illustrative"));
    for path in["source:/source_parameters/nominal_voltage","source:/auxiliary_current_a","source:/evidence","source:/limits/maximum_return_power_w","controller:/sensing/voltage_quantum_v","controller:/sensing/supply_current_quantum_a","controller:/sensing/winding_current_quantum_a","controller:/sensing/evidence","controller:/nominal_voltage_for_compensation_v","controller:/limits/maximum_winding_current_a"]{
        assert!(ui.rendered.values().any(|c|matches!(&c.hit,Hit::Focus{field:Field::Electrical(p),..}if p==path)),"actual field {path}");
    }
}
#[test]
fn actual_electrical_raw_draft_refusal_and_newer_text_survive_ack(){
    let o=electrical_owner();let stamp=o.active().unwrap().stamp();let mut ui=StudyUi::default();
    let field=Field::Electrical("source:/auxiliary_current_a".into());
    let action=super::super::super::forms::text_submission(&mut ui,&o,Hit::Focus{stamp:Some(stamp),field:field.clone(),text:String::new()},"0.125").unwrap();
    let key=(Some((stamp.id,stamp.revision)),field);ui.drafts.insert(key.clone(),"later unsubmitted exact text".into());
    super::super::super::forms::acknowledge(&mut ui,&action,&sim_api::Outcome::Done(Err("draft.power: rejected".into())));
    assert_eq!(ui.drafts[&key],"later unsubmitted exact text");assert!(ui.blocking_reason().is_some());
    let mut w=world(o);w.insert_resource(ui);w.run_system_once(render).unwrap();w.run_system_once(collect).unwrap();
    assert!(w.resource::<StudyUi>().rendered.values().any(|c|matches!(c.hit,Hit::Discard{..})));
}
#[test]
fn stale_and_switched_electrical_submissions_cannot_retarget(){
    let mut o=electrical_owner();let stamp=o.active().unwrap().stamp();let field=Field::Electrical("controller:/sensing/evidence".into());
    o.studies[0].revision+=1;
    assert!(super::super::super::forms::submission(&o,Some(stamp),&field,"late raw evidence").is_err());
    o.studies[0].revision=stamp.revision;o.studies[0].id=22;o.active=Some(22);
    assert!(super::super::super::forms::submission(&o,Some(stamp),&field,"late raw evidence").is_err());
    assert!(o.pending.is_empty());assert!(!o.studies[0].study.refinement.experiment.electrical.as_ref().unwrap().sensing.evidence.contains("late"));
}
#[test]
fn busy_and_displaced_actual_electrical_execution_controls_are_disabled(){
    for displaced in[false,true]{let mut o=electrical_owner();if displaced{o.studies[0].displaced=Some("external replacement".into());}else{pending(&mut o);}
        let mut w=world(o);w.run_system_once(render).unwrap();w.run_system_once(collect).unwrap();
        assert!(!w.resource::<StudyUi>().rendered["study:electrical:simulate"].enabled);
        assert!(super::super::super::forms::activate(w.resource::<StudyUi>(),"study:electrical:simulate").is_err());
    }
}

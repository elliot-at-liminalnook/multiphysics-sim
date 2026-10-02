//! Written source-only fixtures; this repair does not execute them.
use super::*;
use crate::experiment_study::refinement::{self, electrical, Operation, ResultData};
use std::sync::atomic::{AtomicBool,Ordering};
fn study()->Study{
    let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut s=Study::new(crate::experiment_comparison::hx_archive::load(&root.join("examples/actuators/hx30hm/pwm-identification"),&root).unwrap()).unwrap();
    electrical::command(&mut s,electrical::Command::SetSource(Some(crate::controller_refinement::power::Setup{source_component:"electrical.voltage_source".into(),source_parameters:std::collections::BTreeMap::from([("voltage".into(),12.)]),auxiliary_current_a:0.,evidence:"Synthetic source-only terminal fixture".into(),limits:Default::default()}))).unwrap();s
}
fn roundtrip(s:&Study)->Study{
    let folder=std::env::temp_dir().join(format!("terminal-fixture-{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&folder).unwrap();let path=folder.join("study.json");s.save_new(&path).unwrap();Study::load(&path).unwrap()
}
#[test]
fn partial_cancelled_electrical_run_preserves_exact_companion_after_reopen(){
    let mut s=study();let capture=refinement::prepare(&mut s,Operation::Simulate).unwrap();let cancel=AtomicBool::new(false);
    let o=refinement::execute(capture,&cancel,|_,_|{cancel.store(true,Ordering::Relaxed);}).unwrap();
    let ResultData::Controller(r)=o.result.as_ref().unwrap() else{panic!("missing partial run")};assert!(r.cancelled);assert!(!run_complete(r));
    let exact=serde_json::to_vec(o.result.as_ref().unwrap()).unwrap();refinement::apply_outcome(&mut s,o);
    assert!(s.refinement.controller_runs.is_empty());let reopened=roundtrip(&s);let r=reopened.refinement_evidence.terminals.last().unwrap();
    assert!(r.cancelled&&r.unapplied&&r.unscored);assert_eq!(reopened.input_contents.resolve(&r.content_ref.blake3).unwrap(),exact);assert!(cached(&reopened,r).is_some());
}
#[test]
fn late_cancelled_complete_simulation_is_unapplied_exact_reopen_diagnostic(){
    let mut s=study();let capture=refinement::prepare(&mut s,Operation::Simulate).unwrap();let mut o=refinement::execute(capture,&AtomicBool::new(false),|_,_|{}).unwrap();
    let exact=serde_json::to_vec(o.result.as_ref().unwrap()).unwrap();o.cancelled=true;refinement::apply_outcome(&mut s,o);
    assert!(s.refinement.controller_runs.is_empty());let reopened=roundtrip(&s);let r=reopened.refinement_evidence.terminals.last().unwrap();
    assert!(r.cancelled&&r.unscored&&r.unapplied);assert_eq!(reopened.input_contents.resolve(&r.content_ref.blake3).unwrap(),exact);
}

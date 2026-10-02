//! Electrical text intent is retained by StudyUi; only stamped shared commands
//! can change Study. Parsing here performs no file access or physical validation.
use super::super::{StudyAction, StudyStamp};
use sim_runtime::{controller_refinement::power, experiment_study::{Study, refinement::{Command, electrical}}};
use serde_json::Value;

pub(crate) fn source_preset(s: &Study, battery: bool) -> power::Setup {
    power::Setup {
        source_component: if battery { "robot.battery" } else { "electrical.voltage_source" }.into(),
        source_parameters: if battery {
            [("nominal_voltage".into(),11.1),("internal_resistance".into(),0.1),("capacity_ah".into(),1.),("initial_soc".into(),1.)].into_iter().collect()
        } else { [("voltage".into(), s.refinement.experiment.voltage_v)].into_iter().collect() },
        auxiliary_current_a: 0.,
        evidence: if battery { "Illustrative battery parameters and built-in discharge curve; not a measured pack. Auxiliary electronics consumption explicitly neglected." } else { "Constant source at declared experiment voltage; no measured voltage variation. Auxiliary electronics consumption explicitly neglected." }.into(),
        limits: Default::default(),
    }
}
pub(crate) fn controller_preset() -> power::Controller {
    power::Controller {
        sensing: power::Sensing { voltage_quantum_v:0.1, supply_current_quantum_a:None, winding_current_quantum_a:None, evidence:"Hypothetical 0.1 V voltage resolution; sample age unestablished. No calibrated current channels connected.".into() },
        nominal_voltage_for_compensation_v:None, limits:Default::default(),
    }
}
fn leaf(value: &mut Value, pointer: &str, text: &str) -> Result<(), String> {
    let slot=value.pointer_mut(pointer).ok_or_else(||format!("electrical{pointer}: field absent"))?;
    *slot=match slot {
        Value::String(_)=>Value::String(text.into()),
        Value::Number(_) | Value::Null=>if text.trim().is_empty(){Value::Null}else{
            let n=text.trim().parse::<f64>().map_err(|e|format!("electrical{pointer}: {e}"))?;
            serde_json::Number::from_f64(n).map(Value::Number).ok_or_else(||format!("electrical{pointer}: finite number required"))?
        },
        _=>return Err(format!("electrical{pointer}: scalar field required")),
    };Ok(())
}
pub(crate) fn submission(s:&Study,stamp:StudyStamp,path:&str,text:&str)->Result<StudyAction,String>{
    if path=="import" {
        let prediction=s.refinement_evidence.electrical.selected_prediction.ok_or("electrical.import: select a captured prediction first")?;
        return Ok(StudyAction::ImportElectrical{stamp,path:crate::ui_kit::path_field::expand(text.trim()),prediction});
    }
    let command=if path=="voltage" {
        let voltage=if text.trim().is_empty(){None}else{let n=text.trim().parse::<f64>().map_err(|e|format!("draft.conditions.voltage_v: {e}"))?;if !n.is_finite(){return Err("draft.conditions.voltage_v: finite V required".into())}Some(n)};
        electrical::Command::SetVoltage(voltage)
    }else if path=="source_json" {
        electrical::Command::SetSource(electrical::parse_source(&serde_json::from_str(text).map_err(|e|format!("draft.power: {e}"))?)?)
    }else if path=="controller_json" {
        electrical::Command::SetController(electrical::parse_controller(&serde_json::from_str(text).map_err(|e|format!("refinement.experiment.electrical: {e}"))?)?)
    }else {
        let(group,pointer)=path.split_once(':').ok_or_else(||format!("electrical.{path}: missing group"))?;
        match group {
            "source"=>{
                let p=s.draft.power.as_ref().ok_or("draft.power: add a source first")?;
                let mut value=serde_json::to_value(p).map_err(|e|e.to_string())?;
                leaf(&mut value,pointer,text)?;
                electrical::Command::SetSource(Some(serde_json::from_value(value).map_err(|e|format!("draft.power{pointer}: {e}"))?))
            },
            "controller"=>{
                let c=s.refinement.experiment.electrical.as_ref().ok_or("refinement.experiment.electrical: enable feedback first")?;
                let mut value=serde_json::to_value(c).map_err(|e|e.to_string())?;
                leaf(&mut value,pointer,text)?;
                electrical::Command::SetController(Some(serde_json::from_value(value).map_err(|e|format!("refinement.experiment.electrical{pointer}: {e}"))?))
            },
            _=>return Err(format!("electrical.{group}: unknown group")),
        }
    };
    Ok(StudyAction::RefineApply{stamp,command:Command::Electrical(command)})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn invalid_scalar_keeps_exact_raw_intent_and_does_not_mutate_input(){
        let mut original=serde_json::json!({"limits":{"minimum_voltage_v":null},"evidence":"retained"});
        let before=original.clone();let raw=" NaN ";
        assert!(leaf(&mut original,"/limits/minimum_voltage_v",raw).unwrap_err().contains("/limits/minimum_voltage_v"));
        assert_eq!(original,before);assert_eq!(raw," NaN ");
    }
    #[test] fn absent_current_resolution_can_be_authored_and_cleared(){
        let mut v=serde_json::to_value(controller_preset()).unwrap();
        leaf(&mut v,"/sensing/supply_current_quantum_a","0.01").unwrap();
        assert_eq!(v.pointer("/sensing/supply_current_quantum_a"),Some(&serde_json::json!(0.01)));
        leaf(&mut v,"/sensing/supply_current_quantum_a","").unwrap();
        assert!(v.pointer("/sensing/supply_current_quantum_a").unwrap().is_null());
    }
}

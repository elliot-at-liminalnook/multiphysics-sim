//! Individual scalar fields preserve the complete typed draft before shared validation.
//! JSON pointers are internal identities, never a whole-workspace editor.
use super::super::{StudyAction,StudyStamp};
use sim_runtime::experiment_study::{Study,refinement::Command};
use serde_json::Value;

pub(crate) fn submission(study:&Study,stamp:StudyStamp,path:&str,text:&str)->Result<StudyAction,String> {
    let (group,pointer)=path.split_once(':').ok_or_else(||format!("refinement.{path}: missing field group"))?;
    if group=="parameter_new"||group=="parameter_value" {
        let (name,value)=if group=="parameter_new" {text.split_once('=').ok_or("refinement.experiment.controller.parameters: expected name = JSON value")?} else {(pointer,text)};
        if name.trim().is_empty(){return Err("refinement.experiment.controller.parameters: name required".into())}
        let value:Value=serde_json::from_str(value.trim()).map_err(|e|format!("refinement.experiment.controller.parameters: {e}"))?;
        let mut experiment=study.refinement.experiment.clone();
        let sim_runtime::controller_refinement::control::Policy::Rhai{parameters,..}=&mut experiment.controller else{return Err("refinement.experiment.controller: select Rhai first".into())};
        parameters.as_object_mut().ok_or("refinement.experiment.controller.parameters: expected object")?.insert(name.trim().into(),value);
        return Ok(StudyAction::RefineApply{stamp,command:Command::SetExperiment(experiment)});
    }
    let mut value=match group {
        "experiment"=>serde_json::to_value(&study.refinement.experiment),
        "coordinates"=>serde_json::to_value(&study.refinement.coordinates),
        "scenarios"=>serde_json::to_value(&study.refinement.scenarios),
        _=>return Err(format!("refinement.{path}: unknown field group")),
    }.map_err(|e|format!("refinement.{path}: {e}"))?;
    let leaf=value.pointer_mut(pointer).ok_or_else(||format!("refinement.{path}: field no longer exists"))?;
    *leaf=if group=="scenarios"&&pointer.contains("/conditions/")&&(pointer.ends_with("/voltage_v")||pointer.ends_with("/temperature_c"))&&text.trim().is_empty(){Value::Null}
    else if pointer.ends_with("/component_id") {
        if text.trim().is_empty(){Value::Null}else{Value::String(text.into())}
    } else if pointer.ends_with("/device") && group=="coordinates" {
        if text.trim().is_empty(){Value::Null}else{serde_json::from_str(text).map_err(|e|format!("refinement.{path}: {e}"))?}
    } else {match leaf {
        Value::String(_)=>Value::String(text.into()),
        Value::Number(_)|Value::Bool(_)|Value::Null=>serde_json::from_str(text).map_err(|e|format!("refinement.{path}: {e}"))?,
        _=>return Err(format!("refinement.{path}: edit an individual scalar row")),
    }};
    let command=match group {
        "experiment"=>Command::SetExperiment(serde_json::from_value(value).map_err(|e|format!("refinement.{path}: {e}"))?),
        "coordinates"=>Command::SetCoordinates(serde_json::from_value(value).map_err(|e|format!("refinement.{path}: {e}"))?),
        "scenarios"=>Command::SetScenarios(serde_json::from_value(value).map_err(|e|format!("refinement.{path}: {e}"))?),
        _=>unreachable!(),
    };
    Ok(StudyAction::RefineApply{stamp,command})
}

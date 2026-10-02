//! Structured recording drafts use the existing typed text identity. StudyUi
//! retains raw intent; only StudyAction applies durable immutable revisions.
use super::{Field,StudyUi};
use super::super::{StudyAction,StudyStamp};
use sim_runtime::{experiment_study::{Study,refinement::Command},controller_refinement::{context::CaptureContext,calibration_data::{Assignment,Role}}};
use serde_json::Value;
pub(crate) fn context(s:&Study,hash:&str)->Result<CaptureContext,String>{
    s.refinement.capture_contexts.iter().rev().find(|c|c.recording_hash==hash).cloned().or_else(||sim_runtime::experiment_study::refinement::recordings::cached_source(s,hash).map(|r|CaptureContext::unknown_with_identity(r,hash))).ok_or_else(||format!("recordings.{hash}: missing capture"))
}
pub(crate) fn submission(s:&Study,stamp:StudyStamp,path:&str,text:&str)->Result<StudyAction,String>{
    if path=="import" {return Ok(StudyAction::ImportRecording{stamp,path:crate::ui_kit::path_field::expand(text.trim())})}
    if path=="additional" {return Ok(StudyAction::FitCombined{stamp,additional_path:(!text.trim().is_empty()).then(||crate::ui_kit::path_field::expand(text.trim()))})}
    let(hash,pointer)=path.split_once(':').ok_or_else(||format!("recordings.{path}: missing identity"))?;
    if pointer.starts_with("new/"){return Err(format!("capture_contexts.{hash}.{pointer}: draft retained; use Append row after completing all fields"))}
    if pointer.starts_with("assignment/"){return Err(format!("recordings.{hash}.{pointer}: draft retained; use Freeze whole run after editing role, limits and rationale"))}
    let mut value=serde_json::to_value(context(s,hash)?).map_err(|e|e.to_string())?;
    if let Some(prefix)=pointer.strip_suffix("/uncertainty_bounds") {
        let bounds=if text.trim().is_empty(){Value::Null}else{
            let(lo,hi)=text.split_once(',').ok_or_else(||format!("capture_contexts.{hash}{pointer}: expected lower, upper"))?;
            let parse=|v:&str|v.trim().parse::<f64>().map_err(|e|format!("capture_contexts.{hash}{pointer}: {e}"));
            serde_json::json!([parse(lo)?,parse(hi)?])
        };
        *value.pointer_mut(pointer).ok_or("capture_contexts.uncertainty_bounds: missing property")?=bounds;
        let _=prefix;
        return Ok(StudyAction::RefineApply{stamp,command:Command::AppendContext{context:serde_json::from_value(value).map_err(|e|e.to_string())?}});
    }
    let leaf=value.pointer_mut(pointer).ok_or_else(||format!("capture_contexts.{hash}{pointer}: missing field"))?;
    *leaf=match leaf {
        Value::String(_) if pointer.ends_with("/joint_id")&&text.trim().is_empty()=>Value::Null,
        Value::String(_)=>Value::String(text.into()),
        Value::Number(_) if pointer.ends_with("/value")&&text.trim().is_empty()=>Value::Null,
        Value::Number(_)=>serde_json::from_str(text).map_err(|e|format!("capture_contexts.{hash}{pointer}: {e}"))?,
        Value::Null if pointer.ends_with("/joint_id")=>if text.trim().is_empty(){Value::Null}else{Value::String(text.into())},
        Value::Null=>if text.trim().is_empty(){Value::Null}else{serde_json::from_str(text).map_err(|e|format!("capture_contexts.{hash}{pointer}: {e}"))?},
        _=>return Err(format!("capture_contexts.{hash}{pointer}: scalar required")),
    };
    if let Some(prefix)=pointer.strip_suffix("/value").filter(|p|p.starts_with("/properties/")) {
        let known=value.pointer(pointer).is_some_and(|v|!v.is_null());
        *value.pointer_mut(&format!("{prefix}/origin")).ok_or("capture_contexts.origin: missing property")?=Value::String(if known{"estimated"}else{"unknown"}.into());
        if !known { *value.pointer_mut(&format!("{prefix}/uncertainty_bounds")).ok_or("capture_contexts.uncertainty_bounds: missing property")?=Value::Null; }
    }
    let context=serde_json::from_value(value).map_err(|e|format!("capture_contexts.{hash}{pointer}: {e}"))?;
    Ok(StudyAction::RefineApply{stamp,command:Command::AppendContext{context}})
}
pub(crate) fn freeze(s:&Study,ui:&StudyUi,stamp:StudyStamp,hash:&str)->Result<StudyAction,String>{
    let r=sim_runtime::experiment_study::refinement::recordings::cached_source(s,hash).ok_or("recordings.assignment: capture missing")?;
    let shown=|name:&str,fallback:String|ui.shown(Some(stamp),&Field::Recording(format!("{hash}:assignment/{name}")),fallback);
    let role=match shown("role","held_out".into()).trim(){"train"=>Role::Train,"held_out"=>Role::HeldOut,_=>return Err(format!("recording_assignments.{hash}.role: expected train or held_out"))};
    let number=|name:&str,fallback:f64|->Result<f64,String>{let raw=shown(name,fallback.to_string());let value=raw.parse::<f64>().map_err(|e|format!("recording_assignments.{hash}.limits.{name}: {e}"))?;if !value.is_finite(){return Err(format!("recording_assignments.{hash}.limits.{name}: finite value required"))}Ok(value)};
    let assignment=Assignment{recording_hash:hash.into(),role,limits:sim_runtime::experiment_comparison::Limits{rmse:number("rmse",r.experiment.limits.rms_rad)?,final_abs_error:number("final_abs_error",r.experiment.limits.settled_rad)?},rationale:shown("rationale",String::new())};
    Ok(StudyAction::RefineApply{stamp,command:Command::AssignRecording{assignment}})
}

pub(crate) fn add_row(s:&Study,ui:&StudyUi,stamp:StudyStamp,hash:&str,kind:&str)->Result<StudyAction,String>{
    let mut context=context(s,hash)?;
    let text=|name:&str|ui.shown(Some(stamp),&Field::Recording(format!("{hash}:new/{kind}/{name}")),String::new());
    match kind {
        "artifact"=>context.artifacts.push(sim_runtime::controller_refinement::context::Artifact{role:text("role"),location:text("location"),blake3:text("blake3")}),
        "binding"=>context.bindings.push(sim_runtime::controller_refinement::context::Binding{hardware_id:text("hardware_id").parse().map_err(|e|format!("capture_contexts.{hash}.bindings.hardware_id: {e}"))?,cad_component_id:text("cad_component_id"),joint_id:(!text("joint_id").trim().is_empty()).then(||text("joint_id")),source:text("source")}),
        "limitation"=>{let value=text("text");if value.trim().is_empty(){return Err(format!("capture_contexts.{hash}.limitations: text required"))}context.limitations.push(value);},
        _=>return Err(format!("capture_contexts.{hash}.{kind}: unknown row kind")),
    }
    Ok(StudyAction::RefineApply{stamp,command:Command::AppendContext{context}})
}

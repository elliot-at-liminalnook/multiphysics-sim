//! One typed apply owner for window, system_ui and REST offline study actions.
use super::{jobs, state::{DocumentCapture, StudyOwner, StudyStamp}};
use crate::{app::actions::{self, Act, Action, Call, InFlight, Replies, Spec}, document::DocumentRegistry};
use bevy::{ecs::message::Messages, prelude::*};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sim_runtime::experiment_study::commands::{self, Command, EvaluationSelection};
use sim_api::Outcome;
use sim_runtime::experiment_study::refinement;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag="op",rename_all="snake_case",deny_unknown_fields)]
pub enum StudyAction {
    OpenArchive { path:String },
    OpenReview { path:String },
    Choose { id:u64 },
    Apply { stamp:StudyStamp, command:Command },
    Evaluate { stamp:StudyStamp, set:EvaluationSelection },
    RefineApply { stamp:StudyStamp, command:refinement::Command },
    RefineRun { stamp:StudyStamp, operation:refinement::Operation },
    ImportRecording { stamp:StudyStamp, path:String },
    FitCombined { stamp:StudyStamp, additional_path:Option<String> },
    Cancel { job:u64 },
    Save { stamp:StudyStamp, path:String },
    Export { stamp:StudyStamp, path:String },
    SystemUi { id:String, #[serde(default)] text:Option<String> },
    Status,
}
impl Action for StudyAction {
    fn commands()->Vec<Spec> { vec![actions::spec("system_measured_study",actions::BUILDER,json!({"op":"status"}),"Offline retained measured-PWM authoring. Typed operations open_archive/open_review/choose/apply/evaluate/refine_apply/refine_run/import_recording/fit_combined/cancel/save/export/status. Apply commands use shared experiment_study validation; all scoped operations require stamp {id,revision}. Publication creates new destinations only; accepted jobs return an id and retain receipts.")] }
    fn parse(c:&sim_api::Command)->Result<Self,String> {
        if c.command=="system_ui" {
            let a=&c.args["action"];
            if a.get("operation").and_then(|v|v.as_str())!=Some("activate") { return Err("study.system_ui: expected activate".into()); }
            let text=match a.get("text") {None=>None,Some(serde_json::Value::String(text))=>Some(text.clone()),Some(_)=>return Err("study.system_ui.text: expected string".into())};
            return Ok(Self::SystemUi {id:a.get("id").and_then(|v|v.as_str()).ok_or("study.system_ui.id: expected control ID")?.into(),text});
        }
        if c.command!="system_measured_study" { return Err("Expected system_measured_study".into()); }
        serde_json::from_value(c.args.clone()).map_err(|e|format!("system_measured_study.args: {e}"))
    }
    fn accepts()->Vec<&'static str> { vec!["system_measured_study"] }
    fn controls()->&'static [&'static str] { &["study:<op>","study:<op>:<id>","study:<op>:<id>:<revision>","study:field:<id>:<field>"] }
}

pub fn apply(mut owner:ResMut<StudyOwner>, registry:Res<DocumentRegistry>, mut ui:ResMut<super::forms::StudyUi>, mut messages:ResMut<Messages<Act<StudyAction>>>, mut inflight:ResMut<InFlight<StudyAction>>, mut replies:ResMut<Replies>) {
    let before=owner.changed;
    let ui_before=ui.epoch;
    actions::apply(&mut messages,&mut inflight,&mut replies,|action,call| {
        if let StudyAction::SystemUi{id,text}=action {
            if call.cancelled { return Outcome::Done(Err("Study control activation cancelled".into())); }
            return match super::forms::activate(&ui,id) {
                Ok(hit) if text.is_some()=>match super::forms::text_submission(&mut ui,&owner,hit,text.as_deref().unwrap_or_default()) {
                    Ok(action)=>{
                        let outcome=handle_with_inputs(owner.bypass_change_detection(),&registry,&action,call,publication_inputs(&ui,&action));
                        if !ui.awaiting.is_empty() {super::forms::acknowledge(ui.bypass_change_detection(),&action,&outcome);}
                        outcome
                    }
                    Err(e)=>{ui.error=Some(e.clone());Outcome::Done(Err(e))},
                },
                Ok(super::forms::Hit::Action(action))=>{
                    let outcome=handle_with_inputs(owner.bypass_change_detection(),&registry,&action,call,publication_inputs(&ui,&action));
                    if !ui.awaiting.is_empty() {super::forms::acknowledge(ui.bypass_change_detection(),&action,&outcome);}
                    outcome
                },
                Ok(hit)=>{ui.pending.push(hit);ui.epoch+=1;Outcome::Done(Ok(json!({"message":"Study form control activated"})))},
                Err(e)=>{owner.status=e.clone();Outcome::Done(Err(e))},
            };
        }
        let outcome=handle_with_inputs(owner.bypass_change_detection(),&registry,action,call,publication_inputs(&ui,action));
        if !ui.awaiting.is_empty() {super::forms::acknowledge(ui.bypass_change_detection(),action,&outcome);}
        outcome
    });
    if owner.changed!=before {owner.set_changed();}
    if ui.epoch!=ui_before {ui.set_changed();}
}
fn publication_inputs(ui:&super::forms::StudyUi,action:&StudyAction)->Option<serde_json::Value> {
    match action {StudyAction::Save{stamp,..}|StudyAction::Export{stamp,..}=>Some(ui.publication_inputs(stamp.id)),_=>None}
}
pub fn handle(owner:&mut StudyOwner, registry:&DocumentRegistry, action:&StudyAction, call:&mut Call)->Outcome {
    handle_with_inputs(owner,registry,action,call,None)
}
pub fn handle_with_inputs(owner:&mut StudyOwner, registry:&DocumentRegistry, action:&StudyAction, call:&mut Call,inputs:Option<serde_json::Value>)->Outcome {
    if call.cancelled { return Outcome::Done(Err("Measured study request cancelled before application; retained work is unchanged".into())); }
    if matches!(action,StudyAction::Status) { return Outcome::Done(apply_action(owner,registry,action)); }
    let result=apply_action_with_inputs(owner,registry,action,inputs);
    if let Err(error)=&result {
        let stamp=match action {StudyAction::RefineApply{stamp,..}|StudyAction::RefineRun{stamp,..}|StudyAction::ImportRecording{stamp,..}|StudyAction::FitCombined{stamp,..}=>Some(*stamp),_=>None};
        if let Some(stamp)=stamp {
            if let Some(retained)=owner.get_mut(stamp.id) {
                jobs::retain_durable(&mut retained.study,"native_refinement_rejections",json!({"stamp":stamp,"action":action,"error":error}));
                retained.revision+=1;
            }
        }
    }
    match &result { Ok(v)=>owner.status=v.get("message").and_then(|v|v.as_str()).unwrap_or("Measured study action accepted").into(), Err(e)=>owner.status=e.clone() }
    owner.changed+=1;
    Outcome::Done(result)
}

fn checked(owner:&StudyOwner,registry:&DocumentRegistry,stamp:StudyStamp)->Result<(),String> {
    let retained=owner.validate_stamp(stamp)?;
    if retained.document.as_ref().is_some_and(|d|!d.matches(registry)) {
        return Err("study.document: captured Build document identity or revision changed; evidence remains retained".into());
    }
    Ok(())
}

pub fn apply_action(owner:&mut StudyOwner,registry:&DocumentRegistry,action:&StudyAction)->Result<serde_json::Value,String> {
    apply_action_with_inputs(owner,registry,action,None)
}
fn apply_action_with_inputs(owner:&mut StudyOwner,registry:&DocumentRegistry,action:&StudyAction,inputs:Option<serde_json::Value>)->Result<serde_json::Value,String> {
    match action {
        StudyAction::Status=>{
            // Full status includes traces from every retained archive, irrespective
            // of its current filter. Record that held-out exposure before returning.
            for retained in &mut owner.studies {
                let exposure=super::state::ReviewExposure::capture(&retained.study);
                let ids=retained.study.archive.trials.iter().map(|t|t.id.clone()).collect::<Vec<_>>();
                commands::expose(&mut retained.study,&ids);
                refinement::expose_review(&mut retained.study);
                if super::state::ReviewExposure::capture(&retained.study)!=exposure {retained.revision+=1;owner.changed+=1;}
            }
            return Ok(owner.snapshot());
        }
        StudyAction::OpenArchive{path}|StudyAction::OpenReview{path}=>{
            if owner.pending.iter().any(|p|p.kind.is_load()) { return Err("study.load: another archive/review is loading; cancel or wait".into()); }
            let review=matches!(action,StudyAction::OpenReview{..});
            let id=jobs::start_load(owner,path,review,DocumentCapture::current(registry))?;
            return Ok(json!({"job":id,"message":"Opening retained evidence; existing studies are preserved"}));
        }
        StudyAction::Choose{id}=>{
            let s=owner.get_mut(*id).ok_or("study.id: unknown retained study")?;
            let exposure=super::state::ReviewExposure::capture(&s.study);
            let ids=commands::filtered_ids(&s.study);
            commands::expose(&mut s.study,&ids);
            refinement::expose_review(&mut s.study);
            if super::state::ReviewExposure::capture(&s.study)!=exposure { s.revision+=1; }
            owner.active=Some(*id);owner.selection_epoch+=1;
        }
        StudyAction::Apply{stamp,command}=>{
            checked(owner,registry,*stamp)?;
            let retained=owner.get_mut(stamp.id).ok_or("study.id: missing retained study")?;
            commands::apply(&mut retained.study,command.clone())?;
            retained.revision+=1;
        }
        StudyAction::RefineApply{stamp,command}=>{
            checked(owner,registry,*stamp)?;
            // Sample/source/dataset validation belongs in jobs for every recording
            // authoring consumer, including typed REST payloads and rendered controls.
            if matches!(command,refinement::Command::ImportRecording{..}|refinement::Command::SelectRecording{..}|refinement::Command::SelectFitCase{..}|refinement::Command::SetPredictionPurpose(_)|refinement::Command::AppendContext{..}|refinement::Command::AssignRecording{..}|refinement::Command::UseRecordingFit{..}) {
                let id=super::recording_jobs::start_command(owner,*stamp,command.clone())?;
                return Ok(json!({"job":id,"message":"Validating immutable recording authoring inputs in a retained job"}));
            }
            let retained=owner.get_mut(stamp.id).ok_or("study.id: missing retained study")?;
            refinement::apply(&mut retained.study,command.clone())?;
            retained.revision+=1;
        }
        StudyAction::RefineRun{stamp,operation}=>{
            checked(owner,registry,*stamp)?;
            let id=jobs::start_refinement(owner,*stamp,operation.clone())?;
            return Ok(json!({"job":id,"message":"Running shared refinement from immutable captured inputs; results never adopt candidates automatically"}));
        }
        StudyAction::ImportRecording{stamp,path}=>{
            checked(owner,registry,*stamp)?;
            let id=super::recording_jobs::start_import(owner,*stamp,path)?;
            return Ok(json!({"job":id,"message":"Reading and validating captured recording in a retained job; FPGA workflows remain deferred"}));
        }
        StudyAction::FitCombined{stamp,additional_path}=>{
            checked(owner,registry,*stamp)?;
            let id=super::recording_jobs::start_combined(owner,*stamp,additional_path.clone())?;
            return Ok(json!({"job":id,"message":"Loading optional saved-study provenance and fitting frozen captured datasets in a retained job"}));
        }
        StudyAction::Evaluate{stamp,set}=>{
            checked(owner,registry,*stamp)?;
            let id=jobs::start_evaluation(owner,*stamp,set.clone())?;
            return Ok(json!({"job":id,"message":"Evaluating immutable captured inputs; progress and cancellation remain available"}));
        }
        StudyAction::Cancel{job}=>{
            let pending=owner.pending.iter_mut().find(|p|p.id==*job).ok_or("study.job: job is already terminal or missing; retained receipt remains available")?;
            let message=pending.cancel();
            return Ok(json!({"job":job,"message":message}));
        }
        StudyAction::Save{stamp,path}|StudyAction::Export{stamp,path}=>{
            // Publication remains possible after unexpected document displacement:
            // this publishes captured evidence, not a mutation of that document.
            owner.validate_stamp(*stamp)?;
            let export=matches!(action,StudyAction::Export{..});
            let id=jobs::start_publication_with_inputs(owner,*stamp,path,export,inputs)?;
            return Ok(json!({"job":id,"message":"Publishing a new immutable captured review revision"}));
        }
        StudyAction::SystemUi{..}=>return Err("study.system_ui: rendered controls must be resolved by the apply owner".into()),
    }
    Ok(json!({"message":"Retained measured study updated","active":owner.active,"stamp":owner.active().map(|s|s.stamp())}))
}
